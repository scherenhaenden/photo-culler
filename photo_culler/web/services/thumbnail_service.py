"""Thumbnail Delivery Service for resolving and caching multi-resolution preview thumbnails."""

from pathlib import Path
from typing import Optional

from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.previews.generator import PreviewGenerator


class ThumbnailService:
    """Delivers multi-resolution preview thumbnail paths (256px, 800px, 1600px, 3200px)."""

    def __init__(self, db_engine: Database, cache_dir: Optional[Path] = None):
        self.db = db_engine
        self.generator = PreviewGenerator() if cache_dir is None else PreviewGenerator(cache_dir=cache_dir)

    def get_thumbnail_path(self, photo_id: str, size: str = "800", representation: str = "jpeg") -> Optional[Path]:
        """Resolve or generate thumbnail image file for photo_id."""
        size_map = {"256": "small", "800": "medium", "1600": "large", "3200": "full"}
        preset = size_map.get(size, "medium")

        with self.db.session() as session:
            repo = PhotoRepository(session)
            photo = repo.get_by_id(photo_id)
            if not photo:
                return None

            display_file = photo.display_file(representation)
            if not display_file:
                return None
            img_path = display_file.path
            if not img_path.exists():
                return None

            thumbnails = self.generator.generate_thumbnails(
                photo_id=f"{photo_id}-{representation}-{display_file.modified_time:.6f}", image_path=img_path
            )
            thumbnail = thumbnails.get(preset)
            if (thumbnail is None or not self.generator.has_visible_content(thumbnail)) and display_file.role.value == "raw":
                jpeg_file = photo.display_file("jpeg")
                if jpeg_file and jpeg_file.path != img_path and jpeg_file.path.exists():
                    thumbnails = self.generator.generate_thumbnails(
                        photo_id=f"{photo_id}-jpeg-{jpeg_file.modified_time:.6f}", image_path=jpeg_file.path
                    )
                    thumbnail = thumbnails.get(preset)
            return thumbnail if isinstance(thumbnail, Path) else None
