"""Persistent, idempotent gallery import orchestration."""

import hashlib
import threading
import uuid
from pathlib import Path

from sqlalchemy import select

from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import GalleryDB, ImportJobDB, ImportSourceDB, PhotoDB, utc_now
from photo_culler.pairing.raw_jpeg_pairer import RawJpegPairer
from photo_culler.scanner.directory_scanner import DirectoryScanner


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
            rows = session.execute(select(GalleryDB).order_by(GalleryDB.created_at)).scalars().all()
            return [
                {
                    "contract_version": 1,
                    "id": row.id,
                    "name": row.name,
                    "photo_count": session.query(PhotoDB).filter_by(gallery_id=row.id).count(),
                    "created_at": row.created_at.isoformat(),
                }
                for row in rows
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

    def cancel(self, job_id: str) -> bool:
        """Request cooperative cancellation and persist the request."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is None or job.state in {"completed", "failed", "cancelled"}:
                return False
            job.cancel_requested = True
            job.updated_at = utc_now()
            return True

    def get_job(self, job_id: str) -> dict[str, object] | None:
        """Return a versioned import progress DTO."""
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is None:
                return None
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

    def _run(self, job_id: str, source: Path, recursive: bool) -> None:
        try:
            self._set_state(job_id, "discovering")
            records = []
            scanner = DirectoryScanner()
            for record in scanner.scan(source, recursive=recursive):
                records.append(record)
                with self.database.session() as session:
                    job = session.get(ImportJobDB, job_id)
                    if job is None or job.cancel_requested:
                        if job is not None:
                            job.state = "cancelled"
                            job.updated_at = utc_now()
                        return
                    job.discovered += 1
                    job.updated_at = utc_now()

            self._set_state(job_id, "previewing")
            photos = RawJpegPairer().pair_files(records)
            for photo in photos:
                with self.database.session() as session:
                    job = session.get(ImportJobDB, job_id)
                    if job is None or job.cancel_requested:
                        if job is not None:
                            job.state = "cancelled"
                            job.updated_at = utc_now()
                        return
                    # Logical identity is scoped by gallery and source-relative stem;
                    # it is stable across rescans and metadata/mtime changes.
                    primary = photo.primary_file
                    relative_stem = (
                        str(primary.path.relative_to(source).with_suffix(""))
                        if primary is not None
                        else photo.stem_name
                    )
                    identity = f"{job.gallery_id}\0{source}\0{relative_stem.lower()}"
                    photo.photo_id = hashlib.sha256(identity.encode()).hexdigest()[:32]
                    row = PhotoRepository(session).save_photo(photo)
                    row.gallery_id = job.gallery_id
                    job.imported += 1
                    job.updated_at = utc_now()
            self._set_state(job_id, "completed")
        except (OSError, ValueError) as exc:
            with self.database.session() as session:
                job = session.get(ImportJobDB, job_id)
                if job is not None:
                    job.state = "failed"
                    job.error = str(exc)
                    job.issues += 1
                    job.updated_at = utc_now()
        finally:
            with self._lock:
                self._threads.pop(job_id, None)

    def _set_state(self, job_id: str, state: str) -> None:
        with self.database.session() as session:
            job = session.get(ImportJobDB, job_id)
            if job is not None:
                job.state = state
                job.updated_at = utc_now()
