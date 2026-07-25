"""Domain dataclasses for photo-culler."""

from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import List, Optional

from .enums import DecisionState, FileRole, QualityTier


@dataclass
class FileRecord:
    """Represents a physical file on disk."""

    path: Path
    role: FileRole
    size_bytes: int
    modified_time: float
    quick_hash: Optional[str] = None
    full_hash: Optional[str] = None
    file_id: Optional[int] = None


@dataclass
class MetadataRecord:
    """EXIF and camera settings extracted from photo."""

    capture_time: Optional[datetime] = None
    subsecond: Optional[str] = None
    camera_make: Optional[str] = None
    camera_model: Optional[str] = None
    serial_number: Optional[str] = None
    lens: Optional[str] = None
    iso: Optional[int] = None
    aperture: Optional[float] = None
    shutter_speed: Optional[str] = None
    focal_length: Optional[float] = None
    orientation: int = 1
    latitude: Optional[float] = None
    longitude: Optional[float] = None


@dataclass
class Photo:
    """Logical representation of a photo (unifying RAW, JPEG, and sidecars)."""

    photo_id: str  # Unique logical identifier (e.g. hash of capture_time + camera + stem)
    stem_name: str
    files: List[FileRecord] = field(default_factory=list)
    metadata: Optional[MetadataRecord] = None
    perceptual_hash: Optional[str] = None
    session_id: Optional[str] = None
    burst_id: Optional[str] = None
    decision: DecisionState = DecisionState.UNPROCESSED
    score: float = 0.0
    quality_tier: QualityTier = QualityTier.FAIR

    @property
    def primary_file(self) -> Optional[FileRecord]:
        """Return RAW file if available, otherwise JPEG, otherwise first file."""
        raws = [f for f in self.files if f.role == FileRole.RAW]
        if raws:
            return raws[0]
        jpegs = [f for f in self.files if f.role == FileRole.JPEG]
        if jpegs:
            return jpegs[0]
        return self.files[0] if self.files else None


@dataclass
class VolumeRecord:
    """Represents a mounted volume or storage device."""

    volume_id: str
    label: str
    mount_point: Path
    total_bytes: int
    available_bytes: int
    is_online: bool = True


@dataclass
class SessionRecord:
    """Represents a photo shoot session aggregated by time continuity."""

    session_id: str
    name: str
    start_time: datetime
    end_time: datetime
    photo_count: int = 0


@dataclass
class BurstGroup:
    """Represents a burst sequence of rapidly captured photos."""

    burst_id: str
    photos: List[Photo] = field(default_factory=list)
    representative_photo_id: Optional[str] = None
