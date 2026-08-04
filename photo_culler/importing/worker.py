"""Background thread worker logic for idempotent gallery import jobs."""

import hashlib
import logging
import time
from collections import Counter
from pathlib import Path
from typing import Iterable

from sqlalchemy import select, or_

from photo_culler.catalog.schema import ImportJobDB, ScanRevisionDB, PhotoDB, FileDB, utc_now
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.models import FileRecord, Photo
from photo_culler.identity.quick_hash import compute_quick_hash
from photo_culler.pairing.raw_jpeg_pairer import RawJpegPairer
from photo_culler.scanner.directory_scanner import DirectoryScanner

from photo_culler.importing.db_helpers import (
    increment_job_counter,
    increment_revision_counter,
    set_state,
    complete_job,
    mark_failed,
    normalize_exclusions,
)

logger = logging.getLogger(__name__)


def run_import_worker(
    service,
    job_id: str,
    source: Path,
    recursive: bool,
    exclude_patterns: tuple[str, ...] = (),
) -> None:
    """Run the main background import thread loop."""
    try:
        if _wait_if_paused_or_cancelled(service, job_id):
            return
        records = list(_discover_files(service, job_id, source, recursive, exclude_patterns))
        photos = RawJpegPairer().pair_files(records)
        if _wait_if_paused_or_cancelled(service, job_id):
            return
        set_state(service, job_id, "previewing")
        current_paths = {str(record.path) for record in records}
        if _import_photos(service, job_id, source, photos, current_paths):
            complete_job(service, job_id)
    except Exception as exc:
        logger.exception("Import job %s failed", job_id)
        mark_failed(service, job_id, str(exc))
    finally:
        with service._lock:
            service._threads.pop(job_id, None)


def _discover_files(
    service,
    job_id: str,
    source: Path,
    recursive: bool,
    exclude_patterns: tuple[str, ...],
) -> Iterable[FileRecord]:
    """Yield discovered records while persisting progress and cancellation."""
    set_state(service, job_id, "discovering")
    scanner = DirectoryScanner()
    records = (
        scanner.scan(source, recursive=recursive, exclude_patterns=exclude_patterns)
        if exclude_patterns
        else scanner.scan(source, recursive=recursive)
    )
    for record in records:
        if _wait_if_paused_or_cancelled(service, job_id):
            return
        record.quick_hash = compute_quick_hash(record.path)
        increment_job_counter(service, job_id, "discovered")
        increment_revision_counter(service, job_id, "discovered")
        yield record


def _import_photos(
    service,
    job_id: str,
    source: Path,
    photos: Iterable[Photo],
    current_paths: set[str],
) -> bool:
    """Persist paired photos, returning false when the job should stop."""
    for photo in photos:
        if _wait_if_paused_or_cancelled(service, job_id):
            return False
        with service.database.session() as session:
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
            moved_files = _reconcile_moved_files(
                service,
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
    return not _wait_if_paused_or_cancelled(service, job_id)


def _reconcile_moved_files(
    service,
    session,
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


def _wait_if_paused_or_cancelled(service, job_id: str) -> bool:
    """Block cooperatively while paused or return true when processing must stop."""
    with service._control:
        while True:
            with service.database.session() as session:
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
                if job_id in service._shutdown_jobs:
                    return True
                if not job.pause_requested:
                    return False
                if job.state != "paused":
                    job.resume_state = job.state
                    job.state = "paused"
                    job.updated_at = utc_now()
            service._control.wait()


def estimate_import(
    source: Path,
    recursive: bool = True,
    exclude_patterns: Iterable[str] = (),
) -> dict[str, object]:
    """Scan lightweight file facts without creating a gallery or import job."""
    exclusions = normalize_exclusions(exclude_patterns)
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
