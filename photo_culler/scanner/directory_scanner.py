"""Directory scanner for recursively indexing media files."""

import os
from fnmatch import fnmatchcase
from pathlib import Path
from typing import Generator, Iterable, Optional

from ..core.models import FileRecord
from .file_filter import FileFilter


class DirectoryScanner:
    """Crawls directory trees and yields FileRecord objects for recognized media files."""

    def __init__(self, file_filter: Optional[FileFilter] = None):
        self.filter = file_filter or FileFilter()

    def scan(
        self,
        directory: Path,
        recursive: bool = True,
        exclude_patterns: Iterable[str] = (),
    ) -> Generator[FileRecord, None, None]:
        """Yield FileRecord instances, descending into subdirectories when recursive."""
        root_path = directory.resolve()
        patterns = tuple(exclude_patterns)

        for root, dirs, files in os.walk(root_path):
            # Exclude ignored directories in-place
            root_relative = Path(root).relative_to(root_path)
            dirs[:] = [
                directory_name
                for directory_name in dirs
                if not self.filter.should_ignore_dir(directory_name)
                and not self._is_excluded(root_relative / directory_name, patterns)
            ]
            if not recursive:
                dirs.clear()

            for file_name in files:
                if file_name.startswith("."):
                    continue

                path = Path(root) / file_name
                if self._is_excluded(path.relative_to(root_path), patterns):
                    continue
                # Imported sources are trust boundaries. Do not follow file
                # symlinks that may escape the selected directory.
                if path.is_symlink():
                    continue
                role = self.filter.classify_role(path)
                if role is None:
                    continue

                try:
                    stat = path.stat()
                    yield FileRecord(
                        path=path,
                        role=role,
                        size_bytes=stat.st_size,
                        modified_time=stat.st_mtime,
                    )
                except OSError:
                    continue

    @staticmethod
    def _is_excluded(relative_path: Path, patterns: tuple[str, ...]) -> bool:
        """Match normalized source-relative paths against configured globs."""
        candidate = relative_path.as_posix()
        return any(fnmatchcase(candidate, pattern) or fnmatchcase(relative_path.name, pattern) for pattern in patterns)
