"""SQLAlchemy ORM database schema for photo-culler catalog."""

from datetime import datetime

from sqlalchemy import Boolean, Column, DateTime, Float, ForeignKey, Integer, String
from sqlalchemy.orm import declarative_base, relationship

Base = declarative_base()


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
    photo_id = Column(String(64), unique=True, nullable=False, index=True)
    stem_name = Column(String(255), nullable=False, index=True)
    perceptual_hash = Column(String(64), nullable=True, index=True)
    session_id = Column(String(64), nullable=True, index=True)
    burst_id = Column(String(64), nullable=True, index=True)
    decision = Column(String(32), default="UNPROCESSED", index=True)
    score = Column(Float, default=0.0)
    quality_tier = Column(String(32), default="fair")
    created_at = Column(DateTime, default=datetime.utcnow)

    files = relationship("FileDB", back_populates="photo", cascade="all, delete-orphan")
    metadata_record = relationship("MetadataDB", uselist=False, back_populates="photo", cascade="all, delete-orphan")


class FileDB(Base):
    __tablename__ = "files"

    id = Column(Integer, primary_key=True, autoincrement=True)
    photo_id = Column(Integer, ForeignKey("photos.id"), nullable=False)
    volume_id = Column(Integer, ForeignKey("volumes.id"), nullable=True)
    relative_path = Column(String(1024), nullable=False)
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


class SessionDB(Base):
    __tablename__ = "sessions"

    id = Column(Integer, primary_key=True, autoincrement=True)
    session_id = Column(String(64), unique=True, nullable=False, index=True)
    name = Column(String(255), nullable=False)
    start_time = Column(DateTime, nullable=False)
    end_time = Column(DateTime, nullable=False)
    photo_count = Column(Integer, default=0)
