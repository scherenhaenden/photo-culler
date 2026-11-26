#!/usr/bin/env python3
"""Create the deterministic 13-photo JPEG fixture used by Sprint 03 acceptance."""

from __future__ import annotations

import argparse
import hashlib
import json
from pathlib import Path

from PIL import Image, ImageDraw


def write_fixture(destination: Path) -> dict[str, object]:
    source = destination / "Fotos de selección"
    empty = destination / "Carpeta vacía"
    source.mkdir(parents=True, exist_ok=False)
    empty.mkdir(parents=True, exist_ok=False)

    photos: list[dict[str, str | int]] = []
    for number in range(1, 14):
        filename = f"photo-{number:02}.jpg"
        image = Image.new(
            "RGB",
            (640, 480),
            ((53 * number) % 256, (97 * number) % 256, (149 * number) % 256),
        )
        draw = ImageDraw.Draw(image)
        for bit in range(4):
            color = (238, 238, 238) if number & (1 << bit) else (21, 21, 21)
            x = 44 + bit * 144
            draw.rectangle((x, 100, x + 120, 360), fill=color)
        draw.text((44, 408), filename, fill=(255, 255, 255))

        path = source / filename
        image.save(path, "JPEG", quality=90)
        photos.append(
            {
                "photo_id": filename.removesuffix(".jpg"),
                "filename": filename,
                "size_bytes": path.stat().st_size,
                "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            }
        )

    manifest = {
        "schema_version": 1,
        "photo_count": len(photos),
        "photos_per_page": 12,
        "photos": photos,
    }
    (destination / "manifest.json").write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8"
    )
    return manifest


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    print(json.dumps(write_fixture(args.destination.resolve()), indent=2))


if __name__ == "__main__":
    main()
