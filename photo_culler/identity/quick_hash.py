"""Quick sparse hashing algorithm (first 64KB + file size)."""

import hashlib
from pathlib import Path


def compute_quick_hash(path: Path, chunk_size: int = 65536) -> str:
    """Compute fast sparse hash combining file size and initial byte block."""
    try:
        size = path.stat().st_size
        hasher = hashlib.sha256()
        hasher.update(str(size).encode("utf-8"))

        with open(path, "rb") as f:
            header = f.read(chunk_size)
            hasher.update(header)

        return hasher.hexdigest()
    except OSError:
        return ""
