"""SQLAlchemy ORM database schema for photo-culler catalog."""

from datetime import datetime, timezone

from sqlalchemy import Boolean, Column, DateTime, Float, ForeignKey, Integer, String, Text, UniqueConstraint
from sqlalchemy.orm import declarative_base, relationship

Base = declarative_base()


def utc_now() -> datetime:
    """Return an aware UTC timestamp for persisted application state."""
    return datetime.now(timezone.utc)


class GalleryDB(Base):
    __tablename__ = "galleries"

    id = Column(String(36), primary_key=True)
    name = Column(String(255), nullable=False)
    created_at = Column(DateTime(timezone=True), default=utc_now, nullable=False)
    updated_at = Column(DateTime(timezone=True), default=utc_now, onupdate=utc_now, nullable=False)


class ImportSourceDB(Base):
    __tablename__ = "import_sources"
    __table_args__ = (UniqueConstraint("gallery_id", "normalized_path", name="uq_gallery_source_path"),)

    id = Column(String(36), primary_key=True)
    gallery_id = Column(String(36), ForeignKey("galleries.id"), nullable=False, index=True)
    path = Column(String(2048), nullable=False)
    normalized_path = Column(String(2048), nullable=False)
    recursive = Column(Boolean, default=True, nullable=False)
    exclude_patterns = Column(Text, default="[]", nullable=False)
    status = Column(String(32), default="online", nullable=False, index=True)
    last_seen_at = Column(DateTime(timezone=True), nullable=True)
    created_at = Column(DateTime(timezone=True), default=utc_now, nullable=False)


class ScanRevisionDB(Base):
    __tablename__ = "scan_revisions"

    id = Column(String(36), primary_key=True)
    gallery_id = Column(String(36), ForeignKey("galleries.id"), nullable=False, index=True)
    source_id = Column(String(36), ForeignKey("import_sources.id"), nullable=False, index=True)
    state = Column(String(32), nullable=False, index=True)
    discovered = Column(Integer, default=0, nullable=False)
    new_files = Column(Integer, default=0, nullable=False)
    modified_files = Column(Integer, default=0, nullable=False)
    moved_files = Column(Integer, default=0, nullable=False)
    missing_files = Column(Integer, default=0, nullable=False)
    started_at = Column(DateTime(timezone=True), default=utc_now, nullable=False)
    completed_at = Column(DateTime(timezone=True), nullable=True)


class ImportJobDB(Base):
    __tablename__ = "import_jobs"

    id = Column(String(36), primary_key=True)
    gallery_id = Column(String(36), ForeignKey("galleries.id"), nullable=False, index=True)
    source_id = Column(String(36), ForeignKey("import_sources.id"), nullable=False, index=True)
    scan_revision_id = Column(String(36), ForeignKey("scan_revisions.id"), nullable=True, index=True)
    state = Column(String(32), nullable=False, index=True)
    discovered = Column(Integer, default=0, nullable=False)
    imported = Column(Integer, default=0, nullable=False)
    issues = Column(Integer, default=0, nullable=False)
    cancel_requested = Column(Boolean, default=False, nullable=False)
    pause_requested = Column(Boolean, default=False, nullable=False)
    resume_state = Column(String(32), nullable=True)
    error = Column(Text, nullable=True)
    created_at = Column(DateTime(timezone=True), default=utc_now, nullable=False)
    updated_at = Column(DateTime(timezone=True), default=utc_now, onupdate=utc_now, nullable=False)


class VolumeDB(Base):
    __tablename__ = "volumes"

    id = Column(Integer, primary_key=True, autoincrement=True)
    volume_id = Column(String(64), unique=True, nullable=False, index=True)
    label = Column(String(255), nullable=False)
    mount_point = Column(String(1024), nullable=False)
    total_bytes = Column(Integer, default=0)
    available_bytes = Column(Integer, default=0)
    is_online = Column(Boolean, default=True)

    files = relationship("FileDB", back_populates="volume")


class PhotoDB(Base):
    __tablename__ = "photos"

    id = Column(Integer, primary_key=True, autoincrement=True)
    gallery_id = Column(String(36), ForeignKey("galleries.id"), nullable=True, index=True)
    photo_id = Column(String(64), unique=True, nullable=False, index=True)
    stem_name = Column(String(255), nullable=False, index=True)
    perceptual_hash = Column(String(64), nullable=True, index=True)
    session_id = Column(String(64), nullable=True, index=True)
    burst_id = Column(String(64), nullable=True, index=True)
    decision = Column(String(32), default="UNPROCESSED", index=True)
    score = Column(Float, default=0.0)
    quality_tier = Column(String(32), default="fair")
    created_at = Column(DateTime(timezone=True), default=utc_now)

    files = relationship("FileDB", back_populates="photo", cascade="all, delete-orphan")
    metadata_record = relationship("MetadataDB", uselist=False, back_populates="photo", cascade="all, delete-orphan")


class FileDB(Base):
    __tablename__ = "files"
    __table_args__ = (UniqueConstraint("photo_id", "relative_path", name="uq_photo_file_path"),)

    id = Column(Integer, primary_key=True, autoincrement=True)
    photo_id = Column(Integer, ForeignKey("photos.id"), nullable=False)
    volume_id = Column(Integer, ForeignKey("volumes.id"), nullable=True)
    import_source_id = Column(String(36), ForeignKey("import_sources.id"), nullable=True, index=True)
    last_seen_revision_id = Column(String(36), ForeignKey("scan_revisions.id"), nullable=True, index=True)
    relative_path = Column(String(1024), nullable=False)
    source_relative_path = Column(String(1024), nullable=True)
    status = Column(String(32), default="present", nullable=False, index=True)
    role = Column(String(32), nullable=False)
    size_bytes = Column(Integer, nullable=False)
    modified_time = Column(Float, nullable=False)
    quick_hash = Column(String(64), nullable=True, index=True)
    full_hash = Column(String(64), nullable=True, index=True)

    photo = relationship("PhotoDB", back_populates="files")
    volume = relationship("VolumeDB", back_populates="files")


class MetadataDB(Base):
    __tablename__ = "metadata"

    id = Column(Integer, primary_key=True, autoincrement=True)
    photo_id = Column(Integer, ForeignKey("photos.id"), nullable=False, unique=True)
    capture_time = Column(DateTime, nullable=True, index=True)
    subsecond = Column(String(16), nullable=True)
    camera_make = Column(String(128), nullable=True)
    camera_model = Column(String(128), nullable=True)
    serial_number = Column(String(128), nullable=True)
    lens = Column(String(255), nullable=True)
    iso = Column(Integer, nullable=True)
    aperture = Column(Float, nullable=True)
    shutter_speed = Column(String(32), nullable=True)
    focal_length = Column(Float, nullable=True)
    orientation = Column(Integer, default=1)
    latitude = Column(Float, nullable=True)
    longitude = Column(Float, nullable=True)

    photo = relationship("PhotoDB", back_populates="metadata_record")


class EditDocumentDB(Base):
    __tablename__ = "edit_documents"

    id = Column(String(36), primary_key=True)
    photo_id = Column(Integer, ForeignKey("photos.id"), nullable=False, unique=True, index=True)
    contract_version = Column(Integer, default=1, nullable=False)
    revision = Column(Integer, default=0, nullable=False)
    recipe_json = Column(Text, default="{}", nullable=False)
    undo_stack_json = Column(Text, default="[]", nullable=False)
    redo_stack_json = Column(Text, default="[]", nullable=False)
    created_at = Column(DateTime(timezone=True), default=utc_now, nullable=False)
    updated_at = Column(DateTime(timezone=True), default=utc_now, onupdate=utc_now, nullable=False)


class SessionDB(Base):
    __tablename__ = "sessions"

    id = Column(Integer, primary_key=True, autoincrement=True)
    session_id = Column(String(64), unique=True, nullable=False, index=True)
    name = Column(String(255), nullable=False)
    start_time = Column(DateTime, nullable=False)
    end_time = Column(DateTime, nullable=False)
    photo_count = Column(Integer, default=0)
