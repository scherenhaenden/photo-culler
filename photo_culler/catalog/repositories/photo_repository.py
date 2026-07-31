"""Photo Repository for persisting and querying Photo domain objects."""

import json
from datetime import datetime
from pathlib import Path
from typing import List, Optional

from sqlalchemy import func
from sqlalchemy.orm import Session as SQLAlchemySession

from ...core.enums import DecisionState, FileRole, QualityTier
from ...core.models import FileRecord, MetadataRecord, Photo
from ..schema import FileDB, MetadataDB, PhotoDB


class PhotoRepository:
    """Repository for managing Photo domain objects in the database catalog."""

    def __init__(self, db_session: SQLAlchemySession):
        self.session = db_session

    def save_photo(self, photo: Photo) -> PhotoDB:
        """Upsert a Photo domain model into database."""
        db_photo = self.session.query(PhotoDB).filter_by(photo_id=photo.photo_id).first()
        if not db_photo:
            db_photo = PhotoDB(
                photo_id=photo.photo_id,
                stem_name=photo.stem_name,
                perceptual_hash=photo.perceptual_hash,
                session_id=photo.session_id,
                burst_id=photo.burst_id,
                decision=photo.decision.value if isinstance(photo.decision, DecisionState) else str(photo.decision),
                score=photo.score,
                quality_tier=photo.quality_tier.value
                if isinstance(photo.quality_tier, QualityTier)
                else str(photo.quality_tier),
            )
            self.session.add(db_photo)
            self.session.flush()
        else:
            db_photo.stem_name = photo.stem_name
            db_photo.perceptual_hash = photo.perceptual_hash
            db_photo.session_id = photo.session_id
            db_photo.burst_id = photo.burst_id
            db_photo.decision = (
                photo.decision.value if isinstance(photo.decision, DecisionState) else str(photo.decision)
            )
            db_photo.score = photo.score
            db_photo.quality_tier = (
                photo.quality_tier.value if isinstance(photo.quality_tier, QualityTier) else str(photo.quality_tier)
            )

        # Save files
        for f in photo.files:
            existing_file = (
                self.session.query(FileDB).filter_by(photo_id=db_photo.id, relative_path=str(f.path)).first()
            )
            if not existing_file:
                db_file = FileDB(
                    photo_id=db_photo.id,
                    relative_path=str(f.path),
                    role=f.role.value if isinstance(f.role, FileRole) else str(f.role),
                    size_bytes=f.size_bytes,
                    modified_time=f.modified_time,
                    quick_hash=f.quick_hash,
                    full_hash=f.full_hash,
                )
                self.session.add(db_file)

        # Save metadata
        if photo.metadata:
            meta = photo.metadata
            db_meta = self.session.query(MetadataDB).filter_by(photo_id=db_photo.id).first()
            if not db_meta:
                db_meta = MetadataDB(photo_id=db_photo.id)
                self.session.add(db_meta)

            db_meta.capture_time = meta.capture_time
            db_meta.subsecond = meta.subsecond
            db_meta.camera_make = meta.camera_make
            db_meta.camera_model = meta.camera_model
            db_meta.serial_number = meta.serial_number
            db_meta.lens = meta.lens
            db_meta.iso = meta.iso
            db_meta.aperture = meta.aperture
            db_meta.shutter_speed = meta.shutter_speed
            db_meta.focal_length = meta.focal_length
            db_meta.orientation = meta.orientation

        return db_photo

    def get_by_id(self, photo_id: str) -> Optional[Photo]:
        """Fetch Photo domain object by photo_id."""
        db_photo = self.session.query(PhotoDB).filter_by(photo_id=photo_id).first()
        if not db_photo:
            return None
        return self._to_domain(db_photo)

    def get_analysis_summary(self, photo_id: str) -> dict:
        """Return the persisted score explanation, if this photo has been analyzed."""
        db_photo = self.session.query(PhotoDB).filter_by(photo_id=photo_id).first()
        if not db_photo:
            return {}
        try:
            return json.loads(db_photo.analysis_summary_json or "{}")
        except json.JSONDecodeError:
            return {}

    def save_analysis_summary(self, photo_id: str, summary: dict) -> None:
        """Persist the measurements and weighted calculation shown in the inspector."""
        db_photo = self.session.query(PhotoDB).filter_by(photo_id=photo_id).first()
        if not db_photo:
            raise LookupError(f"Photo not found: {photo_id}")
        db_photo.analysis_summary_json = json.dumps(summary, ensure_ascii=False, sort_keys=True)

    def list_all(self) -> List[Photo]:
        """Return all photos in catalog as domain objects."""
        db_photos = self.session.query(PhotoDB).all()
        return [self._to_domain(p) for p in db_photos]

    def list_needing_analysis(self, profile_id: str, profile_fingerprint: str) -> List[Photo]:
        """Return only photos missing, outdating, or changing since an analysis.

        The first lightweight query avoids constructing domain objects (and their file
        records) for the already-complete majority of a large catalog.
        """
        rows = (
            self.session.query(
                PhotoDB.photo_id,
                PhotoDB.analysis_summary_json,
                func.max(FileDB.modified_time),
            )
            .outerjoin(FileDB, FileDB.photo_id == PhotoDB.id)
            .group_by(PhotoDB.id)
            .all()
        )
        pending_ids = [
            photo_id
            for photo_id, summary_json, newest_file_mtime in rows
            if self._needs_analysis(summary_json, newest_file_mtime, profile_id, profile_fingerprint)
        ]
        if not pending_ids:
            return []
        db_photos = self.session.query(PhotoDB).filter(PhotoDB.photo_id.in_(pending_ids)).all()
        return [self._to_domain(photo) for photo in db_photos]

    def list_by_burst_prefix(self, prefix: str) -> List[Photo]:
        """Return only photos belonging to burst groups with the given prefix."""
        db_photos = self.session.query(PhotoDB).filter(PhotoDB.burst_id.like(f"{prefix}%")).all()
        return [self._to_domain(photo) for photo in db_photos]

    def list_burst_ids(self, prefix: str, offset: int, limit: int) -> List[str]:
        """Return one bounded page of burst IDs without loading their photos."""
        return [
            row[0]
            for row in self.session.query(PhotoDB.burst_id)
            .filter(PhotoDB.burst_id.like(f"{prefix}%"))
            .distinct()
            .order_by(PhotoDB.burst_id)
            .offset(offset)
            .limit(limit)
            .all()
        ]

    def count_bursts(self, prefix: str) -> int:
        """Count logical similarity groups in SQL."""
        return self.session.query(PhotoDB.burst_id).filter(PhotoDB.burst_id.like(f"{prefix}%")).distinct().count()

    def list_by_burst_ids(self, burst_ids: List[str]) -> List[Photo]:
        """Load photos only for the requested similarity groups."""
        if not burst_ids:
            return []
        db_photos = self.session.query(PhotoDB).filter(PhotoDB.burst_id.in_(burst_ids)).all()
        return [self._to_domain(photo) for photo in db_photos]

    @staticmethod
    def _needs_analysis(
        summary_json: str, newest_file_mtime: float | None, profile_id: str, profile_fingerprint: str
    ) -> bool:
        try:
            summary = json.loads(summary_json or "{}")
            analyzed_at = datetime.fromisoformat(str(summary.get("analyzed_at", "")).replace("Z", "+00:00"))
        except (TypeError, ValueError, json.JSONDecodeError):
            return True
        if summary.get("profile_id") != profile_id:
            return True
        stored_fingerprint = summary.get("profile_fingerprint")
        if stored_fingerprint and stored_fingerprint != profile_fingerprint:
            return True
        return newest_file_mtime is not None and newest_file_mtime > analyzed_at.timestamp()

    def list_page(
        self,
        offset: int = 0,
        limit: int = 150,
        sort: Optional[str] = None,
        filters: Optional[dict] = None,
    ) -> List[Photo]:
        """Return a page of photos from catalog as domain objects with optional sorting and filtering."""
        query = self.session.query(PhotoDB)

        # Apply filters if any
        if filters:
            if "decision" in filters and filters["decision"]:
                decision = str(filters["decision"]).upper()
                if decision == "REJECT":
                    query = query.filter(PhotoDB.decision.in_(["REJECT_TECHNICAL", "REJECT_REDUNDANT"]))
                else:
                    query = query.filter(PhotoDB.decision == decision)
            if "quality_tier" in filters and filters["quality_tier"]:
                query = query.filter(PhotoDB.quality_tier == filters["quality_tier"])
            if "session_id" in filters and filters["session_id"]:
                query = query.filter(PhotoDB.session_id == filters["session_id"])
            if "gallery_id" in filters and filters["gallery_id"]:
                query = query.filter(PhotoDB.gallery_id == filters["gallery_id"])

        # Apply sort
        if sort:
            if sort == "score_desc":
                query = query.order_by(PhotoDB.score.desc())
            elif sort == "score_asc":
                query = query.order_by(PhotoDB.score.asc())
            elif sort == "name_asc":
                query = query.order_by(PhotoDB.stem_name.asc())
            elif sort == "name_desc":
                query = query.order_by(PhotoDB.stem_name.desc())
        else:
            # Default sort by stem_name
            query = query.order_by(PhotoDB.stem_name.asc())

        db_photos = query.offset(offset).limit(limit).all()
        return [self._to_domain(p) for p in db_photos]

    def count(self) -> int:
        """Return total photo count."""
        return self.session.query(PhotoDB).count()

    def count_filtered(self, filters: Optional[dict] = None) -> int:
        """Return total photo count matching filters."""
        query = self.session.query(PhotoDB)
        if filters:
            if "decision" in filters and filters["decision"]:
                decision = str(filters["decision"]).upper()
                if decision == "REJECT":
                    query = query.filter(PhotoDB.decision.in_(["REJECT_TECHNICAL", "REJECT_REDUNDANT"]))
                else:
                    query = query.filter(PhotoDB.decision == decision)
            if "quality_tier" in filters and filters["quality_tier"]:
                query = query.filter(PhotoDB.quality_tier == filters["quality_tier"])
            if "session_id" in filters and filters["session_id"]:
                query = query.filter(PhotoDB.session_id == filters["session_id"])
            if "gallery_id" in filters and filters["gallery_id"]:
                query = query.filter(PhotoDB.gallery_id == filters["gallery_id"])
        return query.count()

    def _to_domain(self, db_photo: PhotoDB) -> Photo:
        files = []
        for f in db_photo.files:
            files.append(
                FileRecord(
                    path=Path(f.relative_path),
                    role=FileRole(f.role) if f.role in [r.value for r in FileRole] else FileRole.UNKNOWN,
                    size_bytes=f.size_bytes,
                    modified_time=f.modified_time,
                    quick_hash=f.quick_hash,
                    full_hash=f.full_hash,
                    file_id=f.id,
                    status=f.status,
                )
            )

        meta = None
        if db_photo.metadata_record:
            m = db_photo.metadata_record
            meta = MetadataRecord(
                capture_time=m.capture_time,
                subsecond=m.subsecond,
                camera_make=m.camera_make,
                camera_model=m.camera_model,
                serial_number=m.serial_number,
                lens=m.lens,
                iso=m.iso,
                aperture=m.aperture,
                shutter_speed=m.shutter_speed,
                focal_length=m.focal_length,
                orientation=m.orientation,
            )

        return Photo(
            photo_id=db_photo.photo_id,
            stem_name=db_photo.stem_name,
            files=files,
            metadata=meta,
            perceptual_hash=db_photo.perceptual_hash,
            session_id=db_photo.session_id,
            burst_id=db_photo.burst_id,
            decision=DecisionState(db_photo.decision)
            if db_photo.decision in [d.value for d in DecisionState]
            else DecisionState.UNPROCESSED,
            score=db_photo.score,
            quality_tier=QualityTier(db_photo.quality_tier)
            if db_photo.quality_tier in [q.value for q in QualityTier]
            else QualityTier.FAIR,
        )
