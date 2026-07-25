"""Thumbnail and preview image generator."""

from PIL import Image
from pathlib import Path
from typing import Dict, Union


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
            with Image.open(image_path) as img:
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
