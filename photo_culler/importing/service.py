"""Persistent, idempotent gallery import orchestration."""

import hashlib
import json
import logging
import threading
import time
import uuid
from collections import Counter
from enum import Enum
from pathlib import Path
from typing import Iterable

from sqlalchemy import func, or_, select
from sqlalchemy.orm import Session

from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import (
    FileDB,
    GalleryDB,
    ImportJobDB,
    ImportSourceDB,
    PhotoDB,
    ScanRevisionDB,
    utc_now,
)
from photo_culler.core.models import FileRecord, Photo
from photo_culler.identity.quick_hash import compute_quick_hash
from photo_culler.pairing.raw_jpeg_pairer import RawJpegPairer
from photo_culler.scanner.directory_scanner import DirectoryScanner

logger = logging.getLogger(__name__)


class CancelResult(str, Enum):
    """Outcome of a cooperative cancellation request."""

    NOT_FOUND = "not_found"
    NOT_CANCELLABLE = "not_cancellable"
    CANCEL_REQUESTED = "cancel_requested"


class PauseResult(str, Enum):
    """Outcome of a cooperative pause request."""

    NOT_FOUND = "not_found"
    NOT_PAUSABLE = "not_pausable"
    PAUSE_REQUESTED = "pause_requested"


class ResumeResult(str, Enum):
    """Outcome of a persisted import resume request."""

    NOT_FOUND = "not_found"
    NOT_RESUMABLE = "not_resumable"
    SOURCE_UNAVAILABLE = "source_unavailable"
    RESUMED = "resumed"


class GalleryImportService:
    """Application-scoped coordinator for non-copying gallery imports."""

    def __init__(self, database: Database) -> None:
        self.database = database
        self._threads: dict[str, threading.Thread] = {}
        self._lock = threading.Lock()
        self._control = threading.Condition()
        self._shutdown_jobs: set[str] = set()
        self._recover_interrupted_jobs()

    def create_gallery(self, name: str) -> str:
        """Create a logical gallery and return its stable identifier."""
        clean_name = name.strip()
        if not clean_name:
            raise ValueError("Gallery name cannot be empty")
        gallery_id = str(uuid.uuid4())
        with self.database.session() as session:
            session.add(GalleryDB(id=gallery_id, name=clean_name))
        return gallery_id

    def list_galleries(self) -> list[dict[str, object]]:
        """Return versioned presentation-neutral gallery summaries."""
        with self.database.session() as session:
            rows = session.execute(
                select(GalleryDB, func.count(PhotoDB.id))
                .outerjoin(PhotoDB, PhotoDB.gallery_id == GalleryDB.id)
                .group_by(GalleryDB.id)
                .order_by(GalleryDB.created_at)
            ).all()
            return [
                {
                    "contract_version": 1,
                    "id": row.id,
                    "name": row.name,
                    "photo_count": photo_count,
                    "created_at": row.created_at.isoformat(),
                }
                for row, photo_count in rows
            ]

    def list_sources(self, gallery_id: str) -> list[dict[str, object]]:
        """Return configured physical sources and their availability."""
        with self.database.session() as session:
            sources = (
                session.execute(
                    select(ImportSourceDB)
                    .where(ImportSourceDB.gallery_id == gallery_id)
                    .order_by(ImportSourceDB.created_at)
                )
                .scalars()
                .all()
            )
            return [
                {
                    "contract_version": 1,
                    "id": source.id,
                    "gallery_id": source.gallery_id,
                    "path": source.path,
                    "normalized_path": source.normalized_path,
                    "recursive": source.recursive,
                    "exclude_patterns": json.loads(source.exclude_patterns or "[]"),
                    "status": source.status,
                    "last_seen_at": (source.last_seen_at.isoformat() if source.last_seen_at is not None else None),
                }
                for source in sources
            ]

    def start_import(
        self,
        gallery_id: str,
        source: Path,
        recursive: bool = True,
        exclude_patterns: Iterable[str] = (),
    ) -> str:
        """Persist and start an import without copying or modifying originals."""
        exclusions = self._normalize_exclusions(exclude_patterns)
        resolved = source.expanduser().resolve(strict=True)
        if not resolved.is_dir():
            raise ValueError("Import source must be a directory")
        normalized = str(resolved)
        with self.database.session() as session:
            gallery = session.get(GalleryDB, gallery_id)
            if gallery is None:
                raise LookupError("Gallery not found")
            source_row = session.execute(
                select(ImportSourceDB).where(
                    ImportSourceDB.gallery_id == gallery_id,
                    ImportSourceDB.normalized_path == normalized,
                )
            ).scalar_one_or_none()
            if source_row is None:
                source_row = ImportSourceDB(
                    id=str(uuid.uuid4()),
                    gallery_id=gallery_id,
                    path=str(source),
                    normalized_path=normalized,
                    recursive=recursive,
                    exclude_patterns=json.dumps(exclusions),
                )
                session.add(source_row)
                session.flush()
            else:
                source_row.path = str(source)
                source_row.recursive = recursive
                source_row.exclude_patterns = json.dumps(exclusions)
            source_row.status = "online"
            source_row.last_seen_at = utc_now()
            revision_id = str(uuid.uuid4())
            session.add(
                ScanRevisionDB(
                    id=revision_id,
                    gallery_id=gallery_id,
                    source_id=source_row.id,
                    state="queued",
                )
            )
            job_id = str(uuid.uuid4())
            session.add(
                ImportJobDB(
                    id=job_id,
                    gallery_id=gallery_id,
                    source_id=source_row.id,
                    scan_revision_id=revision_id,
                    state="queued",
                )
            )

        thread = threading.Thread(
            target=self._run,
            args=(job_id, resolved, recursive, exclusions),
            daemon=True,
        )
        with self._lock:
            self._threads[job_id] = thread
        thread.start()
        return job_id

    def rescan_gallery(self, gallery_id: str) -> dict[str, object]:
        """Start idempotent rescans and mark unavailable sources explicitly offline."""
        with self.database.session() as session:
            if session.get(GalleryDB, gallery_id) is None:
                raise LookupError("Gallery not found")
            sources = (
                session.execute(
                    select(ImportSourceDB)
                    .where(ImportSourceDB.gallery_id == gallery_id)
                    .order_by(ImportSourceDB.created_at)
                )
                .scalars()
                .all()
            )
            source_specs = [
                (
                    source.id,
                    Path(source.normalized_path),
                    source.recursive,
                    tuple(json.loads(source.exclude_patterns or "[]")),
                )
                for source in sources
            ]

        jobs: list[str] = []
        offline_sources: list[str] = []
        for source_id, source_path, recursive, exclusions in source_specs:
            if source_path.is_dir():
                jobs.append(
                    self.start_import(
                        gallery_id,
                        source_path,
                        recursive=recursive,
                        exclude_patterns=exclusions,
                    )
                )
                continue
            offline_sources.append(source_id)
            self._mark_source_offline(gallery_id, source_id)
        return {
            "contract_version": 1,
            "gallery_id": gallery_id,
            "job_ids": jobs,
            "offline_source_ids": offline_sources,
        }

    def estimate_import(
        self,
        source: Path,
        recursive: bool = True,
        exclude_patterns: Iterable[str] = (),
    ) -> dict[str, object]:
        """Scan lightweight file facts without creating a gallery or import job."""
        exclusions = self._normalize_exclusions(exclude_patterns)
        resolved = source.expanduser().resolve(strict=True)
        if not resolved.is_dir():
            raise ValueError("Import source must be a directory")

        extensions: Counter[str] = Counter()
        roles: Counter[str] = Counter()
        logical_photos: set[str] = set()
        total_bytes = 0
        total_files = 0
        for record in DirectoryScanner().scan(
            resolved,
            recursive=recursive,
            exclude_patterns=exclusions,
        ):
            total_files += 1
            total_bytes += record.size_bytes
            extensions[record.path.suffix.lower() or "(none)"] += 1
            roles[record.role.value] += 1
            logical_photos.add(RawJpegPairer.group_key(record.path))

        return {
            "contract_version": 1,
            "total_files": total_files,
            "logical_photos": len(logical_photos),
            "total_bytes": total_bytes,
            "extensions": dict(sorted(extensions.items())),
            "roles": dict(sorted(roles.items())),
        }

    def cancel(self, job_id: str) -> CancelResult:
        """Request cooperative cancellation and persist the request."""
        with self._control:
            with self.database.session() as session:
                job = session.get(ImportJobDB, job_id)
                if job is None:
                    return CancelResult.NOT_FOUND
                if job.state in {"completed", "failed", "cancelled"}:
                    return CancelResult.NOT_CANCELLABLE
                job.cancel_requested = True
                job.pause_requested = False
                job.updated_at = utc_now()
                revision = (
                    session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
                )
                with self._lock:
                    has_worker = job_id in self._threads
                if not has_worker:
                    job.state = "cancelled"
                    if revision is not None:
                        revision.state = "cancelled"
                        revision.completed_at = utc_now()
            self._control.notify_all()
            return CancelResult.CANCEL_REQUESTED

    def pause(self, job_id: str) -> PauseResult:
        """Request a durable cooperative pause at the next item boundary."""
        with self._control:
            with self.database.session() as session:
                job = session.get(ImportJobDB, job_id)
                if job is None:
                    return PauseResult.NOT_FOUND
                if job.state in {"completed", "failed", "cancelled", "paused"}:
                    return PauseResult.NOT_PAUSABLE
                job.resume_state = job.state
                job.state = "paused"
                job.pause_requested = True
                job.updated_at = utc_now()
                revision = (
                    session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
                )
                if revision is not None:
                    revision.state = "paused"
            self._control.notify_all()
            return PauseResult.PAUSE_REQUESTED

    def resume(self, job_id: str) -> ResumeResult:
        """Resume a paused worker, recreating it from persisted source data if needed."""
        thread_to_start: threading.Thread | None = None
        with self._control:
            with self.database.session() as session:
                job = session.get(ImportJobDB, job_id)
                if job is None:
                    return ResumeResult.NOT_FOUND
                if job.state != "paused" or not job.pause_requested:
                    return ResumeResult.NOT_RESUMABLE
                source_row = session.get(ImportSourceDB, job.source_id)
                if source_row is None:
                    return ResumeResult.SOURCE_UNAVAILABLE
                source = Path(source_row.normalized_path)
                if not source.is_dir():
                    return ResumeResult.SOURCE_UNAVAILABLE
                job.state = job.resume_state or "queued"
                job.resume_state = None
                job.pause_requested = False
                job.updated_at = utc_now()
                revision = (
                    session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
                )
                if revision is not None:
                    revision.state = job.state
                with self._lock:
                    worker_alive = job_id in self._threads and self._threads[job_id].is_alive()
                    if not worker_alive:
                        # A recovered job restarts its idempotent scan. Counters describe
                        # the current attempt rather than double-counting prior work.
                        job.discovered = 0
                        job.imported = 0
                        job.issues = 0
                        job.error = None
                        thread_to_start = threading.Thread(
                            target=self._run,
                            args=(
                                job_id,
                                source,
                                source_row.recursive,
                                tuple(json.loads(source_row.exclude_patterns or "[]")),
                            ),
                            daemon=True,
                        )
                        self._threads[job_id] = thread_to_start
            self._control.notify_all()
        if thread_to_start is not None:
            thread_to_start.start()
        return ResumeResult.RESUMED

    def get_job(self, job_id: str) -> dict[str, object] | None:
        """Return a versioned import progress DTO."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            return None if job is None else self._job_to_dto(job)

    def list_jobs(self, limit: int = 20) -> list[dict[str, object]]:
        """Return recent import jobs so frontends can recover controls after reload."""
        safe_limit = min(max(limit, 1), 100)
        with self.database.session() as session:
            jobs = (
                session.execute(select(ImportJobDB).order_by(ImportJobDB.created_at.desc()).limit(safe_limit))
                .scalars()
                .all()
            )
            return [self._job_to_dto(job) for job in jobs]

    def list_scan_revisions(self, gallery_id: str | None = None, limit: int = 20) -> list[dict[str, object]]:
        """Return recent scan reconciliation results."""
        safe_limit = min(max(limit, 1), 100)
        with self.database.session() as session:
            query = select(ScanRevisionDB).order_by(ScanRevisionDB.started_at.desc()).limit(safe_limit)
            if gallery_id is not None:
                query = query.where(ScanRevisionDB.gallery_id == gallery_id)
            revisions = session.execute(query).scalars().all()
            return [self._revision_to_dto(revision) for revision in revisions]

    def shutdown(self, wait_timeout: float = 5.0) -> None:
        """Persist active jobs as paused and briefly wait for workers to exit."""
        with self._lock:
            active_threads = list(self._threads.items())

        with self._control:
            for job_id, _thread in active_threads:
                self.pause(job_id)
                self._shutdown_jobs.add(job_id)
            self._control.notify_all()

        deadline = time.monotonic() + wait_timeout
        for _job_id, thread in active_threads:
            thread.join(timeout=max(0.0, deadline - time.monotonic()))

    def _run(
        self,
        job_id: str,
        source: Path,
        recursive: bool,
        exclude_patterns: tuple[str, ...] = (),
    ) -> None:
        try:
            if self._wait_if_paused_or_cancelled(job_id):
                return
            records = list(self._discover_files(job_id, source, recursive, exclude_patterns))
            photos = RawJpegPairer().pair_files(records)
            if self._wait_if_paused_or_cancelled(job_id):
                return
            self._set_state(job_id, "previewing")
            current_paths = {str(record.path) for record in records}
            if self._import_photos(job_id, source, photos, current_paths):
                self._complete_job(job_id)
        except Exception as exc:
            logger.exception("Import job %s failed", job_id)
            self._mark_failed(job_id, str(exc))
        finally:
            with self._lock:
                self._threads.pop(job_id, None)

    def _discover_files(
        self,
        job_id: str,
        source: Path,
        recursive: bool,
        exclude_patterns: tuple[str, ...],
    ) -> Iterable[FileRecord]:
        """Yield discovered records while persisting progress and cancellation."""
        self._set_state(job_id, "discovering")
        scanner = DirectoryScanner()
        records = (
            scanner.scan(source, recursive=recursive, exclude_patterns=exclude_patterns)
            if exclude_patterns
            else scanner.scan(source, recursive=recursive)
        )
        for record in records:
            if self._wait_if_paused_or_cancelled(job_id):
                return
            record.quick_hash = compute_quick_hash(record.path)
            self._increment_job_counter(job_id, "discovered")
            self._increment_revision_counter(job_id, "discovered")
            yield record

    def _import_photos(
        self,
        job_id: str,
        source: Path,
        photos: Iterable[Photo],
        current_paths: set[str],
    ) -> bool:
        """Persist paired photos, returning false when the job should stop."""
        for photo in photos:
            if self._wait_if_paused_or_cancelled(job_id):
                return False
            with self.database.session() as session:
                job = session.get(ImportJobDB, job_id)
                if job is None:
                    return False
                # Logical identity is scoped by gallery and source-relative stem;
                # it is stable across rescans and metadata/mtime changes.
                primary = photo.primary_file
                relative_stem = (
                    str(primary.path.relative_to(source).with_suffix("")) if primary is not None else photo.stem_name
                )
                identity = f"{job.gallery_id}\0{source}\0{relative_stem.lower()}"
                photo.photo_id = hashlib.sha256(identity.encode()).hexdigest()[:32]
                moved_files = self._reconcile_moved_files(
                    session,
                    job,
                    photo,
                    source,
                    current_paths,
                )
                session.flush()
                existing_photo = session.execute(
                    select(PhotoDB).where(PhotoDB.photo_id == photo.photo_id)
                ).scalar_one_or_none()
                existing_files = (
                    {
                        file_row.relative_path: file_row
                        for file_row in session.execute(
                            select(FileDB).where(FileDB.photo_id == existing_photo.id)
                        ).scalars()
                    }
                    if existing_photo is not None
                    else {}
                )
                new_files = 0
                modified_files = 0
                for record in photo.files:
                    existing_file = existing_files.get(str(record.path))
                    if existing_file is None:
                        new_files += 1
                    elif (
                        existing_file.size_bytes != record.size_bytes
                        or existing_file.modified_time != record.modified_time
                    ):
                        modified_files += 1
                row = PhotoRepository(session).save_photo(photo)
                row.gallery_id = job.gallery_id
                session.flush()
                persisted_files = {
                    file_row.relative_path: file_row
                    for file_row in session.execute(select(FileDB).where(FileDB.photo_id == row.id)).scalars()
                }
                for record in photo.files:
                    file_row = persisted_files[str(record.path)]
                    file_row.import_source_id = job.source_id
                    file_row.last_seen_revision_id = job.scan_revision_id
                    file_row.source_relative_path = str(record.path.relative_to(source))
                    file_row.status = "present"
                    file_row.size_bytes = record.size_bytes
                    file_row.modified_time = record.modified_time
                    file_row.quick_hash = record.quick_hash
                revision = (
                    session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
                )
                if revision is not None:
                    revision.new_files += new_files
                    revision.modified_files += modified_files
                    revision.moved_files += moved_files
                job.imported += 1
                job.updated_at = utc_now()
        return not self._wait_if_paused_or_cancelled(job_id)

    @staticmethod
    def _reconcile_moved_files(
        session: Session,
        job: ImportJobDB,
        photo: Photo,
        source: Path,
        current_paths: set[str],
    ) -> int:
        """Reuse unambiguous prior file identities whose old path disappeared."""
        if job.scan_revision_id is None:
            return 0
        moved_files = 0
        primary = photo.primary_file
        for record in photo.files:
            if not record.quick_hash:
                continue
            candidates = (
                session.execute(
                    select(FileDB).where(
                        FileDB.import_source_id == job.source_id,
                        FileDB.quick_hash == record.quick_hash,
                        FileDB.size_bytes == record.size_bytes,
                        or_(
                            FileDB.last_seen_revision_id.is_(None),
                            FileDB.last_seen_revision_id != job.scan_revision_id,
                        ),
                    )
                )
                .scalars()
                .all()
            )
            candidates = [candidate for candidate in candidates if candidate.relative_path not in current_paths]
            if len(candidates) != 1:
                continue
            candidate = candidates[0]
            candidate.relative_path = str(record.path)
            candidate.source_relative_path = str(record.path.relative_to(source))
            candidate.status = "present"
            moved_files += 1
            if record is primary:
                prior_photo = session.get(PhotoDB, candidate.photo_id)
                if prior_photo is not None:
                    photo.photo_id = prior_photo.photo_id
        return moved_files

    def _wait_if_paused_or_cancelled(self, job_id: str) -> bool:
        """Block cooperatively while paused or return true when processing must stop."""
        with self._control:
            while True:
                with self.database.session() as session:
                    job = session.get(ImportJobDB, job_id)
                    if job is None:
                        return True
                    if job.cancel_requested:
                        job.state = "cancelled"
                        job.updated_at = utc_now()
                        revision = (
                            session.get(ScanRevisionDB, job.scan_revision_id)
                            if job.scan_revision_id is not None
                            else None
                        )
                        if revision is not None:
                            revision.state = "cancelled"
                            revision.completed_at = utc_now()
                        return True
                    if job_id in self._shutdown_jobs:
                        return True
                    if not job.pause_requested:
                        return False
                    if job.state != "paused":
                        job.resume_state = job.state
                        job.state = "paused"
                        job.updated_at = utc_now()
                self._control.wait()

    def _increment_job_counter(self, job_id: str, field: str) -> None:
        """Increment a persisted integer progress counter."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is not None:
                setattr(job, field, getattr(job, field) + 1)
                job.updated_at = utc_now()

    def _mark_failed(self, job_id: str, error: str) -> None:
        """Persist an import failure without leaking ORM state."""
        with self.database.session() as session:
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

    def _complete_job(self, job_id: str) -> None:
        """Complete a job unless a cancellation won the final race."""
        with self.database.session() as session:
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

    def _set_state(self, job_id: str, state: str) -> None:
        """Persist a job state transition."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is not None and not job.pause_requested:
                job.state = state
                job.updated_at = utc_now()
                revision = (
                    session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
                )
                if revision is not None and revision.state not in {"offline", "completed"}:
                    revision.state = state

    def _increment_revision_counter(self, job_id: str, field: str) -> None:
        """Increment a counter on the scan revision owned by a job."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is None:
                return
            revision = session.get(ScanRevisionDB, job.scan_revision_id) if job.scan_revision_id is not None else None
            if revision is not None:
                setattr(revision, field, getattr(revision, field) + 1)

    def _mark_source_offline(self, gallery_id: str, source_id: str) -> None:
        """Persist offline source/file state without treating files as deleted."""
        with self.database.session() as session:
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

    @staticmethod
    def _normalize_exclusions(patterns: Iterable[str]) -> tuple[str, ...]:
        """Validate and normalize repeatable source-relative glob patterns."""
        normalized = tuple(pattern.strip().replace("\\", "/") for pattern in patterns if pattern.strip())
        if len(normalized) > 50:
            raise ValueError("At most 50 exclusion patterns are allowed")
        if any(len(pattern) > 256 for pattern in normalized):
            raise ValueError("Exclusion patterns must be at most 256 characters")
        return normalized

    def _recover_interrupted_jobs(self) -> None:
        """Expose jobs interrupted by a prior process as explicitly resumable."""
        active_states = {"queued", "discovering", "previewing", "analyzing"}
        with self.database.session() as session:
            jobs = session.execute(select(ImportJobDB).where(ImportJobDB.state.in_(active_states))).scalars()
            for job in jobs:
                job.resume_state = job.state
                job.state = "paused"
                job.pause_requested = True
                job.updated_at = utc_now()

    @staticmethod
    def _job_to_dto(job: ImportJobDB) -> dict[str, object]:
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

    @staticmethod
    def _revision_to_dto(revision: ScanRevisionDB) -> dict[str, object]:
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
