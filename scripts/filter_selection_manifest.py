#!/usr/bin/env python3
"""Make a selected-only RAW queue manifest from exact JPEG stems.

This keeps the automatic cull as the source of truth while allowing the
reviewer to classify a contact-sheet block (Fans or one scheduled artist)
without accidentally including the surrounding burst candidates.
"""

from __future__ import annotations

import argparse
import csv
from pathlib import Path


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("source", type=Path)
    parser.add_argument("output", type=Path)
    parser.add_argument("--stems", nargs="+", required=True)
    parser.add_argument(
        "--allow-unselected",
        action="store_true",
        help="Use exact stems from the source even when the automatic cull omitted them.",
    )
    args = parser.parse_args()

    wanted = {stem.upper() for stem in args.stems}
    with args.source.open(newline="", encoding="utf-8") as source:
        rows = list(csv.DictReader(source))
        fields = source.seek(0) or next(csv.reader(source))
    selected = [
        row
        for row in rows
        if (args.allow_unselected or row.get("selected", "").lower() == "yes")
        and Path(row["jpeg_path"]).stem.upper() in wanted
    ]
    missing = wanted - {Path(row["jpeg_path"]).stem.upper() for row in selected}
    if missing:
        raise SystemExit(f"Selected stems not found: {', '.join(sorted(missing))}")

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", newline="", encoding="utf-8") as output:
        writer = csv.DictWriter(output, fieldnames=fields)
        writer.writeheader()
        for row in selected:
            row["selected"] = "yes"
            writer.writerow(row)
    print(f"Wrote {len(selected)} selections to {args.output}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
