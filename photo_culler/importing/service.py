"""Persistent, idempotent gallery import orchestration."""

import hashlib
import threading
import time
import uuid
from collections import Counter
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
            else:
                source_row.path = str(source)
                source_row.recursive = recursive
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

    def estimate_import(self, source: Path, recursive: bool = True) -> dict[str, object]:
        """Scan lightweight file facts without creating a gallery or import job."""
        resolved = source.expanduser().resolve(strict=True)
        if not resolved.is_dir():
            raise ValueError("Import source must be a directory")

        extensions: Counter[str] = Counter()
        roles: Counter[str] = Counter()
        logical_photos: set[str] = set()
        total_bytes = 0
        total_files = 0
        for record in DirectoryScanner().scan(resolved, recursive=recursive):
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
                with self._lock:
                    has_worker = job_id in self._threads
                if not has_worker:
                    job.state = "cancelled"
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
                            args=(job_id, source, source_row.recursive),
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

    def _run(self, job_id: str, source: Path, recursive: bool) -> None:
        try:
            if self._wait_if_paused_or_cancelled(job_id):
                return
            records = self._discover_files(job_id, source, recursive)
            photos = RawJpegPairer().pair_files(records)
            if self._wait_if_paused_or_cancelled(job_id):
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
            if self._wait_if_paused_or_cancelled(job_id):
                return
            self._increment_job_counter(job_id, "discovered")
            yield record

    def _import_photos(self, job_id: str, source: Path, photos: Iterable[Photo]) -> bool:
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
                row = PhotoRepository(session).save_photo(photo)
                row.gallery_id = job.gallery_id
                job.imported += 1
                job.updated_at = utc_now()
        return not self._wait_if_paused_or_cancelled(job_id)

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
            if job is not None and not job.pause_requested:
                job.state = state
                job.updated_at = utc_now()

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
            "state": job.state,
            "discovered": job.discovered,
            "imported": job.imported,
            "issues": job.issues,
            "cancel_requested": job.cancel_requested,
            "pause_requested": job.pause_requested,
            "error": job.error,
            "updated_at": job.updated_at.isoformat(),
        }
