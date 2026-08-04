#!/usr/bin/env python3
"""Render compact, chronological review sheets from selected JPEG manifest rows."""

from __future__ import annotations

import argparse
import csv
import math
from pathlib import Path

from PIL import Image, ImageDraw, ImageOps


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("manifest", type=Path, nargs="+")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--columns", type=int, default=6)
    parser.add_argument("--rows", type=int, default=8)
    parser.add_argument("--cell", type=int, default=200)
    args = parser.parse_args()

    paths: list[Path] = []
    for manifest in args.manifest:
        with manifest.open(newline="", encoding="utf-8") as stream:
            paths.extend(
                Path(row["jpeg_path"])
                for row in csv.DictReader(stream)
                if row.get("selected", "").lower() == "yes"
            )
    paths.sort(key=lambda path: path.name)
    per_sheet = args.columns * args.rows
    args.output.mkdir(parents=True, exist_ok=True)
    for number, offset in enumerate(range(0, len(paths), per_sheet), start=1):
        page = Image.new("RGB", (args.columns * args.cell, args.rows * args.cell), "black")
        draw = ImageDraw.Draw(page)
        for index, path in enumerate(paths[offset : offset + per_sheet]):
            with Image.open(path) as source:
                image = source.convert("RGB")
            image.thumbnail((args.cell, args.cell - 22))
            x = (index % args.columns) * args.cell
            y = (index // args.columns) * args.cell
            page.paste(image, (x + (args.cell - image.width) // 2, y))
            draw.text((x + 4, y + args.cell - 20), path.stem, fill="white")
        page.save(args.output / f"{number:02d}.jpg", quality=90)
        page.close()
    print(f"Wrote {math.ceil(len(paths) / per_sheet)} sheets for {len(paths)} selected JPEGs")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
