#!/usr/bin/env python3
"""Custom linter for file lengths.

Warns for any Python or Rust file larger than 200 lines.
Errors for any file larger than 400 lines unless it contains an allow-large-file bypass comment.
"""

import sys
from pathlib import Path

BYPASS_TOKEN = "allow-large-file"


def lint_files() -> bool:
    root = Path(__file__).resolve().parent.parent
    paths_to_scan = []

    # We will search in photo_culler/, tests/, and rust/
    for folder_name in ["photo_culler", "tests", "rust"]:
        folder = root / folder_name
        if folder.exists():
            for p in folder.rglob("*"):
                if p.is_file() and p.suffix in {".py", ".rs"}:
                    paths_to_scan.append(p)

    has_errors = False
    warning_count = 0
    error_count = 0

    print("Running line length linter...")
    print("-" * 60)

    for path in sorted(paths_to_scan):
        # Read the file and count lines
        try:
            content = path.read_text(encoding="utf-8", errors="ignore")
        except Exception as e:
            print(f"Error reading file {path.relative_to(root)}: {e}")
            continue

        lines = content.splitlines()
        num_lines = len(lines)

        if num_lines > 400:
            # Check for bypass token
            if BYPASS_TOKEN in content:
                print(f"[WARNING (Bypassed)] {path.relative_to(root)} has {num_lines} lines (exceeds 400, but has bypass token)")
                warning_count += 1
            else:
                print(f"[ERROR] {path.relative_to(root)} has {num_lines} lines (exceeds 400!)")
                error_count += 1
                has_errors = True
        elif num_lines > 200:
            print(f"[WARNING] {path.relative_to(root)} has {num_lines} lines (exceeds 200)")
            warning_count += 1

    print("-" * 60)
    print(f"Linter finished. Warnings: {warning_count}, Errors: {error_count}")
    if has_errors:
        print("FAIL: One or more files exceed 400 lines without a bypass comment.")
        return False
    else:
        print("PASS: All files are within length limits.")
        return True


if __name__ == "__main__":
    success = lint_files()
    sys.exit(0 if success else 1)
