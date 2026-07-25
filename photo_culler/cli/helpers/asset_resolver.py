"""AnalysisAssetResolver for selecting optimal image file for analysis."""

from pathlib import Path
from typing import Optional
from ...core.models import Photo
from ...core.enums import FileRole


class AnalysisAssetResolver:
    """Resolves optimal image source path for an analyzer based on availability and speed priority."""

    def resolve(
        self,
        photo: Photo,
        prefer_jpeg: bool = True
    ) -> Optional[Path]:
        """Return best available file path for image processing."""
        if not photo.files:
            return None

        # Priority 1: Camera JPEG if preferred
        if prefer_jpeg:
            jpegs = [f for f in photo.files if f.role == FileRole.JPEG and f.path.exists()]
            if jpegs:
                return jpegs[0].path

        # Priority 2: RAW file if available
        raws = [f for f in photo.files if f.role == FileRole.RAW and f.path.exists()]
        if raws:
            return raws[0].path

        # Priority 3: Any existing file
        existing = [f for f in photo.files if f.path.exists()]
        if existing:
            return existing[0].path

        return None
