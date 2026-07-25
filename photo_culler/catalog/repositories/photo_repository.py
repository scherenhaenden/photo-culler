"""Photo Repository for persisting and querying Photo domain objects."""

from typing import List, Optional
from sqlalchemy.orm import Session as SQLAlchemySession

from ..schema import PhotoDB, FileDB, MetadataDB, VolumeDB
from ...core.models import Photo, FileRecord, MetadataRecord
from ...core.enums import FileRole, DecisionState, QualityTier
from pathlib import Path


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
                quality_tier=photo.quality_tier.value if isinstance(photo.quality_tier, QualityTier) else str(photo.quality_tier),
            )
            self.session.add(db_photo)
            self.session.flush()
        else:
            db_photo.stem_name = photo.stem_name
            db_photo.perceptual_hash = photo.perceptual_hash
            db_photo.session_id = photo.session_id
            db_photo.burst_id = photo.burst_id
            db_photo.decision = photo.decision.value if isinstance(photo.decision, DecisionState) else str(photo.decision)
            db_photo.score = photo.score
            db_photo.quality_tier = photo.quality_tier.value if isinstance(photo.quality_tier, QualityTier) else str(photo.quality_tier)

        # Save files
        for f in photo.files:
            existing_file = self.session.query(FileDB).filter_by(
                photo_id=db_photo.id, relative_path=str(f.path)
            ).first()
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

    def list_all(self) -> List[Photo]:
        """Return all photos in catalog as domain objects."""
        db_photos = self.session.query(PhotoDB).all()
        return [self._to_domain(p) for p in db_photos]

    def count(self) -> int:
        """Return total photo count."""
        return self.session.query(PhotoDB).count()

    def _to_domain(self, db_photo: PhotoDB) -> Photo:
        files = []
        for f in db_photo.files:
            files.append(FileRecord(
                path=Path(f.relative_path),
                role=FileRole(f.role) if f.role in [r.value for r in FileRole] else FileRole.UNKNOWN,
                size_bytes=f.size_bytes,
                modified_time=f.modified_time,
                quick_hash=f.quick_hash,
                full_hash=f.full_hash,
                file_id=f.id,
            ))

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
            decision=DecisionState(db_photo.decision) if db_photo.decision in [d.value for d in DecisionState] else DecisionState.UNPROCESSED,
            score=db_photo.score,
            quality_tier=QualityTier(db_photo.quality_tier) if db_photo.quality_tier in [q.value for q in QualityTier] else QualityTier.FAIR,
        )
