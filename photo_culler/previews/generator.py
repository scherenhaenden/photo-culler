"""Thumbnail and preview image generator."""

import io
from pathlib import Path
from typing import Dict, Optional, Union

from PIL import Image

THUMBNAIL_SIZES = {
    "small": 256,
    "medium": 800,
    "large": 1600,
    "full": 3200,
}


def extract_embedded_jpeg(raw_path: Path) -> Optional[bytes]:
    """Scan a RAW file for embedded JPEGs and return the largest one."""
    import re
    from typing import Optional
    try:
        data = raw_path.read_bytes()
        # Find matches for JPEG start markers
        matches = [m.start() for m in re.finditer(b'\xff\xd8\xff', data)]
        if not matches:
            return None

        largest_jpeg = b''
        for start in matches:
            end = data.find(b'\xff\xd9', start)
            if end != -1 and end > start:
                jpeg_data = data[start:end+2]
                if len(jpeg_data) > len(largest_jpeg):
                    largest_jpeg = jpeg_data
        if len(largest_jpeg) > 5000:  # Must be at least 5KB to be a valid preview
            return largest_jpeg
    except Exception:
        pass
    return None


class PreviewGenerator:
    """Generates multi-resolution thumbnails and manages preview cache on disk."""

    def __init__(self, cache_dir: Union[str, Path] = "~/.cache/photo-culler/previews"):
        self.cache_dir = Path(cache_dir).expanduser().resolve()
        self.cache_dir.mkdir(parents=True, exist_ok=True)

    def generate_thumbnails(self, photo_id: str, image_path: Path) -> Dict[str, Path]:
        """Generate and save thumbnails for photo_id. Returns dict of size -> output_path."""
        results = {}
        try:
            img = None
            try:
                img = Image.open(image_path)
            except Exception:
                jpeg_bytes = extract_embedded_jpeg(image_path)
                if jpeg_bytes:
                    img = Image.open(io.BytesIO(jpeg_bytes))
                else:
                    raise

            with img:
                if img.mode != "RGB":
                    img = img.convert("RGB")

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
            pass

        return results

    @staticmethod
    def has_visible_content(image_path: Path) -> bool:
        """Reject decoder previews that are effectively a uniform black frame."""
        try:
            with Image.open(image_path) as image:
                low, high = image.convert("L").getextrema()
                return high > 8 and high - low > 4
        except Exception:
            return False
