"""Thumbnail and preview image generator."""

import logging
from pathlib import Path
from typing import Dict, Union

from PIL import Image

logger = logging.getLogger(__name__)

THUMBNAIL_SIZES = {
    "small": 256,
    "medium": 800,
    "large": 1600,
    "full": 3200,
}


class PreviewGenerator:
    """Generates multi-resolution thumbnails and manages preview cache on disk."""

    def __init__(self, cache_dir: Union[str, Path] = "~/.cache/photo-culler/previews"):
        self.cache_dir = Path(cache_dir).expanduser().resolve()
        self.cache_dir.mkdir(parents=True, exist_ok=True)

    def generate_thumbnails(self, photo_id: str, image_path: Path) -> Dict[str, Path]:
        """Generate and save thumbnails for photo_id. Returns dict of size -> output_path."""
        results = {}
        try:
            img = self._load_image(image_path)

            for size_name, max_dim in THUMBNAIL_SIZES.items():
                out_path = self.cache_dir / f"{photo_id}_{size_name}.jpg"
                if out_path.exists():
                    results[size_name] = out_path
                    continue

                # Create copy and resize while maintaining aspect ratio
                thumb = img.copy()
                thumb.thumbnail((max_dim, max_dim), Image.Resampling.BILINEAR)
                thumb.save(out_path, format="JPEG", quality=85)
                results[size_name] = out_path

        except Exception:
            logger.warning("Unable to generate previews for %s", image_path, exc_info=True)

        return results

    @staticmethod
    def _load_image(image_path: Path) -> Image.Image:
        """Load standard images with Pillow and camera RAW files with rawpy."""
        try:
            with Image.open(image_path) as image:
                return image.convert("RGB")
        except OSError, ValueError:
            import rawpy

            with rawpy.imread(str(image_path)) as raw:
                pixels = raw.postprocess(use_camera_wb=True, output_bps=8)
            return Image.fromarray(pixels).convert("RGB")

    @staticmethod
    def has_visible_content(image_path: Path) -> bool:
        """Reject decoder previews that are effectively a uniform black frame."""
        try:
            with Image.open(image_path) as image:
                low, high = image.convert("L").getextrema()
                return high > 8 and high - low > 4
        except Exception:
            return False
