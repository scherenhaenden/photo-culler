"""Database transaction helpers for managing import job state and progress counters."""

import uuid
from typing import Iterable
from sqlalchemy import select, func
from photo_culler.catalog.schema import ImportJobDB, ScanRevisionDB, FileDB, ImportSourceDB, utc_now


def increment_job_counter(service, job_id: str, field: str) -> None:
    """Increment a persisted integer progress counter."""
    with service.database.session() as session:
        job = session.get(ImportJobDB, job_id)
        if job is not None:
            setattr(job, field, getattr(job, field) + 1)
            job.updated_at = utc_now()


def increment_revision_counter(service, job_id: str, field: str) -> None:
    """Increment a counter on the scan revision owned by a job."""
    with service.database.session() as session:
        job = session.get(ImportJobDB, job_id)
        if job is None:
            return
        revision = session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
        if revision is not None:
            setattr(revision, field, getattr(revision, field) + 1)


def set_state(service, job_id: str, state: str) -> None:
    """Persist a job state transition."""
    with service.database.session() as session:
        job = session.get(ImportJobDB, job_id)
        if job is not None and not job.pause_requested:
            job.state = state
            job.updated_at = utc_now()
            revision = (
                session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
            )
            if revision is not None and revision.state not in {"offline", "completed"}:
                revision.state = state


def mark_failed(service, job_id: str, error: str) -> None:
    """Persist an import failure without leaking ORM state."""
    with service.database.session() as session:
        job = session.get(ImportJobDB, job_id)
        if job is not None:
            job.state = "failed"
            job.error = error
            job.issues += 1
            job.updated_at = utc_now()
            revision = (
                session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
            )
            if revision is not None:
                revision.state = "failed"
                revision.completed_at = utc_now()


def complete_job(service, job_id: str) -> None:
    """Complete a job unless a cancellation won the final race."""
    with service.database.session() as session:
        job = session.get(ImportJobDB, job_id)
        if job is not None:
            cancelled = job.cancel_requested
            job.state = "cancelled" if cancelled else "completed"
            job.updated_at = utc_now()
            revision = (
                session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
            )
            if revision is not None:
                if cancelled:
                    revision.state = "cancelled"
                else:
                    missing_files = (
                        session.execute(
                            select(FileDB).where(
                                FileDB.import_source_id == job.source_id,
                                FileDB.last_seen_revision_id != revision.id,
                                FileDB.status != "missing",
                            )
                        )
                        .scalars()
                        .all()
                    )
                    for file_row in missing_files:
                        file_row.status = "missing"
                    revision.missing_files = len(missing_files)
                    revision.state = "completed"
                revision.completed_at = utc_now()


def mark_source_offline(service, gallery_id: str, source_id: str) -> None:
    """Persist offline source/file state without treating files as deleted."""
    with service.database.session() as session:
        source = session.get(ImportSourceDB, source_id)
        if source is None:
            return
        source.status = "offline"
        revision = ScanRevisionDB(
            id=str(uuid.uuid4()),
            gallery_id=gallery_id,
            source_id=source_id,
            state="offline",
            completed_at=utc_now(),
        )
        session.add(revision)
        files = session.execute(select(FileDB).where(FileDB.import_source_id == source_id)).scalars()
        for file_row in files:
            file_row.status = "offline"


def job_to_dto(job: ImportJobDB) -> dict[str, object]:
    """Convert a persisted job to the versioned public contract."""
    return {
        "contract_version": 1,
        "id": job.id,
        "gallery_id": job.gallery_id,
        "scan_revision_id": job.scan_revision_id,
        "state": job.state,
        "discovered": job.discovered,
        "imported": job.imported,
        "issues": job.issues,
        "cancel_requested": job.cancel_requested,
        "pause_requested": job.pause_requested,
        "error": job.error,
        "updated_at": job.updated_at.isoformat(),
    }


def revision_to_dto(revision: ScanRevisionDB) -> dict[str, object]:
    """Convert a scan revision to its public versioned contract."""
    return {
        "contract_version": 1,
        "id": revision.id,
        "gallery_id": revision.gallery_id,
        "source_id": revision.source_id,
        "state": revision.state,
        "discovered": revision.discovered,
        "new_files": revision.new_files,
        "modified_files": revision.modified_files,
        "moved_files": revision.moved_files,
        "missing_files": revision.missing_files,
        "started_at": revision.started_at.isoformat(),
        "completed_at": (revision.completed_at.isoformat() if revision.completed_at is not None else None),
    }


def normalize_exclusions(patterns: Iterable[str]) -> tuple[str, ...]:
    """Validate and normalize repeatable source-relative glob patterns."""
    normalized = tuple(pattern.strip().replace("\\", "/") for pattern in patterns if pattern.strip())
    if len(normalized) > 50:
        raise ValueError("At most 50 exclusion patterns are allowed")
    if any(len(pattern) > 256 for pattern in normalized):
        raise ValueError("Exclusion patterns must be at most 256 characters")
    return normalized


def recover_interrupted_jobs(service) -> None:
    """Expose jobs interrupted by a prior process as explicitly resumable."""
    active_states = {"queued", "discovering", "previewing", "analyzing"}
    with service.database.session() as session:
        jobs = session.execute(select(ImportJobDB).where(ImportJobDB.state.in_(active_states))).scalars()
        for job in jobs:
            job.resume_state = job.state
            job.state = "paused"
            job.pause_requested = True
            job.updated_at = utc_now()
