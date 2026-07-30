"""Persistent, idempotent gallery import orchestration."""

import hashlib
import threading
import time
import uuid
from enum import Enum
from pathlib import Path
from typing import Iterable

from sqlalchemy import func, select

from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import GalleryDB, ImportJobDB, ImportSourceDB, PhotoDB, utc_now
from photo_culler.core.models import FileRecord, Photo
from photo_culler.pairing.raw_jpeg_pairer import RawJpegPairer
from photo_culler.scanner.directory_scanner import DirectoryScanner


class CancelResult(str, Enum):
    """Outcome of a cooperative cancellation request."""

    NOT_FOUND = "not_found"
    NOT_CANCELLABLE = "not_cancellable"
    CANCEL_REQUESTED = "cancel_requested"


class GalleryImportService:
    """Application-scoped coordinator for non-copying gallery imports."""

    def __init__(self, database: Database) -> None:
        self.database = database
        self._threads: dict[str, threading.Thread] = {}
        self._lock = threading.Lock()

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

    def start_import(self, gallery_id: str, source: Path, recursive: bool = True) -> str:
        """Persist and start an import without copying or modifying originals."""
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
                )
                session.add(source_row)
                session.flush()
            job_id = str(uuid.uuid4())
            session.add(
                ImportJobDB(
                    id=job_id,
                    gallery_id=gallery_id,
                    source_id=source_row.id,
                    state="queued",
                )
            )

        thread = threading.Thread(target=self._run, args=(job_id, resolved, recursive), daemon=True)
        with self._lock:
            self._threads[job_id] = thread
        thread.start()
        return job_id

    def cancel(self, job_id: str) -> CancelResult:
        """Request cooperative cancellation and persist the request."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is None:
                return CancelResult.NOT_FOUND
            if job.state in {"completed", "failed", "cancelled"}:
                return CancelResult.NOT_CANCELLABLE
            job.cancel_requested = True
            job.updated_at = utc_now()
            return CancelResult.CANCEL_REQUESTED

    def get_job(self, job_id: str) -> dict[str, object] | None:
        """Return a versioned import progress DTO."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            return None if job is None else self._job_to_dto(job)

    def shutdown(self, wait_timeout: float = 5.0) -> None:
        """Cancel active jobs and briefly wait for their worker threads to exit."""
        with self._lock:
            active_threads = list(self._threads.items())

        for job_id, _thread in active_threads:
            self.cancel(job_id)

        deadline = time.monotonic() + wait_timeout
        for _job_id, thread in active_threads:
            thread.join(timeout=max(0.0, deadline - time.monotonic()))

    def _run(self, job_id: str, source: Path, recursive: bool) -> None:
        try:
            records = self._discover_files(job_id, source, recursive)
            photos = RawJpegPairer().pair_files(records)
            if self._check_and_handle_cancellation(job_id):
                return
            self._set_state(job_id, "previewing")
            if self._import_photos(job_id, source, photos):
                self._complete_job(job_id)
        except (OSError, ValueError) as exc:
            self._mark_failed(job_id, str(exc))
        finally:
            with self._lock:
                self._threads.pop(job_id, None)

    def _discover_files(self, job_id: str, source: Path, recursive: bool) -> Iterable[FileRecord]:
        """Yield discovered records while persisting progress and cancellation."""
        self._set_state(job_id, "discovering")
        for record in DirectoryScanner().scan(source, recursive=recursive):
            if self._check_and_handle_cancellation(job_id):
                return
            self._increment_job_counter(job_id, "discovered")
            yield record

    def _import_photos(self, job_id: str, source: Path, photos: Iterable[Photo]) -> bool:
        """Persist paired photos, returning false when the job should stop."""
        for photo in photos:
            if self._check_and_handle_cancellation(job_id):
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
                row = PhotoRepository(session).save_photo(photo)
                row.gallery_id = job.gallery_id
                job.imported += 1
                job.updated_at = utc_now()
        return not self._check_and_handle_cancellation(job_id)

    def _check_and_handle_cancellation(self, job_id: str) -> bool:
        """Return true when processing must stop, persisting cancellation when requested."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is None:
                return True
            if job.cancel_requested:
                job.state = "cancelled"
                job.updated_at = utc_now()
                return True
            return False

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

    def _complete_job(self, job_id: str) -> None:
        """Complete a job unless a cancellation won the final race."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is not None:
                job.state = "cancelled" if job.cancel_requested else "completed"
                job.updated_at = utc_now()

    def _set_state(self, job_id: str, state: str) -> None:
        """Persist a job state transition."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is not None:
                job.state = state
                job.updated_at = utc_now()

    @staticmethod
    def _job_to_dto(job: ImportJobDB) -> dict[str, object]:
        """Convert a persisted job to the versioned public contract."""
        return {
            "contract_version": 1,
            "id": job.id,
            "gallery_id": job.gallery_id,
            "state": job.state,
            "discovered": job.discovered,
            "imported": job.imported,
            "issues": job.issues,
            "cancel_requested": job.cancel_requested,
            "error": job.error,
            "updated_at": job.updated_at.isoformat(),
        }
