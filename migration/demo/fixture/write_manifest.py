#!/usr/bin/env python3
"""Regenerate the deterministic fixture manifest using only the Python stdlib."""

import hashlib
import json
from pathlib import Path

root = Path(__file__).resolve().parent


def entry(filename: str, width: int, height: int) -> dict[str, object]:
    data = (root / filename).read_bytes()
    return {
        "filename": filename,
        "width": width,
        "height": height,
        "bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
    }


manifest = {
    "gallery_id": "demo",
    "items": [
        {
            "photo_id": "demo-001",
            "filename": "demo-001.jpg",
            "representation": "jpeg",
            "decision": "undecided",
            "thumbnail_url": "/api/v1/photos/demo-001/thumbnail/800",
            "source": "Generated synthetic landscape; contains no camera or personal data.",
            "dimensions": {"width": 1024, "height": 640},
            "sha256": hashlib.sha256((root / "demo-001.jpg").read_bytes()).hexdigest(),
            "thumbnail": entry("demo-001-thumbnail.jpg", 400, 250),
        }
    ],
}
(root / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
