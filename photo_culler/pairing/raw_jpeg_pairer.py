"""RAW, JPEG, and sidecar file pairer."""

import hashlib
from pathlib import Path
from typing import Dict, List

from ..core.models import FileRecord, Photo
from ..metadata.extractor import MetadataExtractor


class RawJpegPairer:
    """Pairs matching RAW, JPEG, and sidecar files into unified Photo entities."""

    def __init__(self, metadata_extractor: MetadataExtractor = None):
        self.meta_extractor = metadata_extractor or MetadataExtractor()

    def pair_files(self, files: List[FileRecord]) -> List[Photo]:
        """Group files by directory path and filename stem, creating unified Photo entities."""
        groups: Dict[str, List[FileRecord]] = {}

        for f in files:
            # Stem grouping key: directory + stem (e.g. /path/to/DSC_1234)
            parent_dir = str(f.path.parent.resolve())
            stem = f.path.stem

            # Normalize sidecar stems (e.g. DSC_1234.NEF.xmp -> DSC_1234)
            if stem.lower().endswith((".nef", ".cr2", ".cr3", ".arw", ".jpg", ".jpeg", ".dng")):
                stem = Path(stem).stem

            key = f"{parent_dir}/{stem}".lower()
            if key not in groups:
                groups[key] = []
            groups[key].append(f)

        photos: List[Photo] = []

        for group_key, file_records in groups.items():
            stem_name = file_records[0].path.stem
            if stem_name.lower().endswith((".nef", ".cr2", ".cr3", ".arw", ".jpg", ".jpeg", ".dng")):
                stem_name = Path(stem_name).stem

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
