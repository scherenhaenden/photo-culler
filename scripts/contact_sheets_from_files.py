#!/usr/bin/env python3
"""Build review sheets from a numeric DSC filename interval."""
from __future__ import annotations
import argparse
from pathlib import Path
from PIL import Image, ImageDraw

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path)
parser.add_argument("--start", type=int, required=True)
parser.add_argument("--end", type=int, required=True)
parser.add_argument("--output", type=Path, required=True)
args = parser.parse_args()
paths = [p for p in sorted(args.directory.glob("DSC_*.JPG")) if args.start <= int(p.stem.split("_")[1]) <= args.end]
args.output.mkdir(parents=True, exist_ok=True)
for sheet, offset in enumerate(range(0, len(paths), 48), 1):
    page = Image.new("RGB", (1200, 1200), "black")
    draw = ImageDraw.Draw(page)
    for i, path in enumerate(paths[offset:offset+48]):
        with Image.open(path) as src:
            image = src.convert("RGB")
        image.thumbnail((200, 178))
        x, y = (i % 6) * 200, (i // 6) * 150
        page.paste(image, (x + (200-image.width)//2, y))
        draw.text((x+3, y+130), path.stem, fill="white")
    page.save(args.output / f"{sheet:02d}.jpg", quality=88)
    page.close()
print(f"{len(paths)} JPEGs -> {sheet} sheets")
