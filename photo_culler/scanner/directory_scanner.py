"""Directory scanner for recursively indexing media files."""

import os
from pathlib import Path
from typing import List, Generator, Optional

from .file_filter import FileFilter
from ..core.models import FileRecord
from ..core.enums import FileRole


class DirectoryScanner:
    """Crawls directory trees and yields FileRecord objects for recognized media files."""

    def __init__(self, file_filter: Optional[FileFilter] = None):
        self.filter = file_filter or FileFilter()

    def scan(self, directory: Path) -> Generator[FileRecord, None, None]:
        """Walk directory recursively yielding FileRecord instances."""
        root_path = directory.resolve()
        
        for root, dirs, files in os.walk(root_path):
            # Exclude ignored directories in-place
            dirs[:] = [d for d in dirs if not self.filter.should_ignore_dir(d)]
            
            for file_name in files:
                if file_name.startswith("."):
                    continue
                
                path = Path(root) / file_name
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
