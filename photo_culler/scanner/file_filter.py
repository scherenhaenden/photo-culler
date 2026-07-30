"""File filter and extension mapping for photo-culler."""

from pathlib import Path
from typing import Optional

from ..core.enums import FileRole

RAW_EXTENSIONS = {
    ".3fr",
    ".arw",
    ".cr2",
    ".cr3",
    ".dng",
    ".iiq",
    ".mos",
    ".nef",
    ".nrw",
    ".orf",
    ".pef",
    ".raf",
    ".rw2",
    ".rwl",
    ".srw",
}
JPEG_EXTENSIONS = {".jpg", ".jpeg"}
IMAGE_EXTENSIONS = {".heic", ".heif", ".png", ".tif", ".tiff"}
SIDECAR_EXTENSIONS = {".xmp", ".pp3", ".dop"}

IGNORED_DIRECTORIES = {".ds_store", ".git", ".idea", ".vscode", "__pycache__", "@eadir", ".photo-culler"}


class FileFilter:
    """Classifies files by extension and filters unwanted hidden files or directories."""

    def classify_role(self, path: Path) -> Optional[FileRole]:
        ext = path.suffix.lower()
        if ext in RAW_EXTENSIONS:
            return FileRole.RAW
        elif ext in JPEG_EXTENSIONS:
            return FileRole.JPEG
        elif ext in IMAGE_EXTENSIONS:
            return FileRole.IMAGE
        elif ext in SIDECAR_EXTENSIONS:
            return FileRole.SIDECAR
        return None

    def should_ignore_dir(self, dir_name: str) -> bool:
        return dir_name.lower() in IGNORED_DIRECTORIES or dir_name.startswith(".")
