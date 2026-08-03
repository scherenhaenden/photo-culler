"""Thumbnail and preview image generator."""

import io
import logging
import re
from pathlib import Path
from typing import Dict, Optional, Union

from PIL import Image

logger = logging.getLogger(__name__)

THUMBNAIL_SIZES = {
    "small": 256,
    "medium": 800,
    "large": 1600,
    "full": 3200,
}


def extract_embedded_jpeg(raw_path: Path) -> Optional[bytes]:
    """Return the largest usable JPEG preview embedded in a RAW file."""
    try:
        data = raw_path.read_bytes()
    except OSError:
        return None

    largest_jpeg = b""
    for match in re.finditer(b"\xff\xd8\xff", data):
        end = data.find(b"\xff\xd9", match.start())
        if end != -1:
            candidate = data[match.start() : end + 2]
            if len(candidate) > len(largest_jpeg):
                largest_jpeg = candidate
    return largest_jpeg if len(largest_jpeg) > 5000 else None


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
                thumb = img.copy()
                thumb.thumbnail((max_dim, max_dim), Image.Resampling.BILINEAR)
                thumb.save(out_path, format="JPEG", quality=85)
                results[size_name] = out_path
        except Exception:
            logger.warning("Unable to generate previews for %s", image_path, exc_info=True)
        return results

    @staticmethod
    def _load_image(image_path: Path) -> Image.Image:
        """Load standard images, camera RAW files, or an embedded RAW JPEG preview."""
        try:
            with Image.open(image_path) as image:
                return image.convert("RGB")
        except (OSError, ValueError):
            pass

        try:
            import rawpy

            with rawpy.imread(str(image_path)) as raw:
                pixels = raw.postprocess(use_camera_wb=True, output_bps=8, half_size=True)
            return Image.fromarray(pixels).convert("RGB")
        except Exception:
            jpeg_bytes = extract_embedded_jpeg(image_path)
            if jpeg_bytes:
                with Image.open(io.BytesIO(jpeg_bytes)) as image:
                    return image.convert("RGB")
            raise

    @staticmethod
    def has_visible_content(image_path: Path) -> bool:
        """Reject decoder previews that are effectively a uniform black frame."""
        try:
            with Image.open(image_path) as image:
                low, high = image.convert("L").getextrema()
                return high > 8 and high - low > 4
        except Exception:
            return False
