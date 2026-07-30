"""RAW, JPEG, and sidecar file pairer."""

import hashlib
from pathlib import Path
from typing import Dict, Iterable, List

from ..core.models import FileRecord, Photo
from ..metadata.extractor import MetadataExtractor
from ..scanner.file_filter import IMAGE_EXTENSIONS, JPEG_EXTENSIONS, RAW_EXTENSIONS

MEDIA_EXTENSIONS = RAW_EXTENSIONS | JPEG_EXTENSIONS | IMAGE_EXTENSIONS


class RawJpegPairer:
    """Pairs matching RAW, JPEG, and sidecar files into unified Photo entities."""

    def __init__(self, metadata_extractor: MetadataExtractor = None):
        self.meta_extractor = metadata_extractor or MetadataExtractor()

    @staticmethod
    def logical_stem(path: Path) -> str:
        """Return the shared logical stem used for media and compound sidecars."""
        stem = path.stem
        if Path(stem).suffix.lower() in MEDIA_EXTENSIONS:
            stem = Path(stem).stem
        return stem

    @classmethod
    def group_key(cls, path: Path) -> str:
        """Return the normalized grouping key without reading image metadata."""
        return f"{path.parent.resolve()}/{cls.logical_stem(path)}".lower()

    def pair_files(self, files: Iterable[FileRecord]) -> List[Photo]:
        """Group files by directory path and filename stem, creating unified Photo entities."""
        groups: Dict[str, List[FileRecord]] = {}

        for f in files:
            # Stem grouping key: directory + stem (e.g. /path/to/DSC_1234)
            key = self.group_key(f.path)
            if key not in groups:
                groups[key] = []
            groups[key].append(f)

        photos: List[Photo] = []

        for group_key, file_records in groups.items():
            stem_name = self.logical_stem(file_records[0].path)

            # Extract EXIF metadata from primary file (RAW preferred)
            primary = None
            for fr in file_records:
                if fr.role.value == "raw":
                    primary = fr
                    break
            if not primary and file_records:
                primary = file_records[0]

            meta = self.meta_extractor.extract(primary.path) if primary else None

            # Generate unique logical photo ID (hash of stem + capture_time + camera)
            time_str = meta.capture_time.isoformat() if meta and meta.capture_time else str(primary.modified_time)
            cam_str = f"{meta.camera_make}_{meta.camera_model}" if meta else "unknown"
            raw_id_str = f"{stem_name}_{time_str}_{cam_str}_{primary.size_bytes}"
            photo_id = hashlib.sha256(raw_id_str.encode("utf-8")).hexdigest()[:16]

            photos.append(
                Photo(
                    photo_id=photo_id,
                    stem_name=stem_name,
                    files=file_records,
                    metadata=meta,
                )
            )

        return photos
