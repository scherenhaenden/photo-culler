"""Persistent, idempotent gallery import orchestration."""

import json
import logging
import threading
import time
import uuid
from pathlib import Path
from typing import Iterable

from sqlalchemy import func, select

from photo_culler.catalog.database import Database
from photo_culler.catalog.schema import (
    GalleryDB,
    ImportJobDB,
    ImportSourceDB,
    PhotoDB,
    ScanRevisionDB,
    utc_now,
)
from photo_culler.importing.types import CancelResult, PauseResult, ResumeResult
from photo_culler.importing.db_helpers import (
    mark_source_offline,
    job_to_dto,
    revision_to_dto,
    normalize_exclusions,
    recover_interrupted_jobs,
)
from photo_culler.importing.worker import run_import_worker, estimate_import

logger = logging.getLogger(__name__)


class GalleryImportService:
    """Application-scoped coordinator for non-copying gallery imports."""

    def __init__(self, database: Database) -> None:
        self.database = database
        self._threads: dict[str, threading.Thread] = {}
        self._lock = threading.Lock()
        self._control = threading.Condition()
        self._shutdown_jobs: set[str] = set()
        recover_interrupted_jobs(self)

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
        exclusions = normalize_exclusions(exclude_patterns)
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
            target=run_import_worker,
            args=(self, job_id, resolved, recursive, exclusions),
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
            mark_source_offline(self, gallery_id, source_id)
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
        return estimate_import(source, recursive=recursive, exclude_patterns=exclude_patterns)

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
                            target=run_import_worker,
                            args=(
                                self,
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
            return None if job is None else job_to_dto(job)

    def list_jobs(self, limit: int = 20) -> list[dict[str, object]]:
        """Return recent import jobs so frontends can recover controls after reload."""
        safe_limit = min(max(limit, 1), 100)
        with self.database.session() as session:
            jobs = (
                session.execute(select(ImportJobDB).order_by(ImportJobDB.created_at.desc()).limit(safe_limit))
                .scalars()
                .all()
            )
            return [job_to_dto(job) for job in jobs]

    def active_job_count(self) -> int:
        """Return imports that are still changing the catalog."""
        active_states = {"queued", "discovering", "previewing", "analyzing"}
        with self.database.session() as session:
            return session.scalar(select(func.count(ImportJobDB.id)).where(ImportJobDB.state.in_(active_states))) or 0

    def list_scan_revisions(self, gallery_id: str | None = None, limit: int = 20) -> list[dict[str, object]]:
        """Return recent scan reconciliation results."""
        safe_limit = min(max(limit, 1), 100)
        with self.database.session() as session:
            query = select(ScanRevisionDB).order_by(ScanRevisionDB.started_at.desc()).limit(safe_limit)
            if gallery_id is not None:
                query = query.where(ScanRevisionDB.gallery_id == gallery_id)
            revisions = session.execute(query).scalars().all()
            return [revision_to_dto(revision) for revision in revisions]

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
