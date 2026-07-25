"""Full file streaming hash computation."""

import hashlib
from pathlib import Path


def compute_full_hash(path: Path, block_size: int = 1048576) -> str:
    """Compute complete SHA256 hash by streaming full file contents in 1MB chunks."""
    try:
        hasher = hashlib.sha256()
        with open(path, "rb") as f:
            while chunk := f.read(block_size):
                hasher.update(chunk)
        return hasher.hexdigest()
    except OSError:
        return ""
