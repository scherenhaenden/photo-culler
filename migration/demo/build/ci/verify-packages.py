#!/usr/bin/env python3
"""Fail unless the demo's required Cargo packages are registered at expected manifests."""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path


EXPECTED = {
    "photo-culler-api": "rust/crates/photo-culler-api/Cargo.toml",
    "photo-culler-client": "rust/crates/photo-culler-client/Cargo.toml",
    "photo-culler-server": "rust/apps/photo-culler-server/Cargo.toml",
    "photo-culler-slint": "rust/apps/photo-culler-slint/Cargo.toml",
}


def main() -> int:
    root = Path(sys.argv[1] if len(sys.argv) > 1 else ".").resolve()
    for relative in ["rust/Cargo.toml", "rust/Cargo.lock", *EXPECTED.values()]:
        if not (root / relative).is_file():
            raise SystemExit(f"required CI input is missing: {relative}")

    result = subprocess.run(
        [
            "cargo", "metadata", "--locked", "--no-deps", "--format-version", "1",
            "--manifest-path", str(root / "rust/Cargo.toml"),
        ],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    metadata = json.loads(result.stdout)
    packages = {package["name"]: Path(package["manifest_path"]).resolve() for package in metadata["packages"]}
    for name, relative in EXPECTED.items():
        expected_manifest = (root / relative).resolve()
        actual_manifest = packages.get(name)
        if actual_manifest != expected_manifest:
            raise SystemExit(f"Cargo package {name!r} missing or registered at unexpected manifest: {actual_manifest}")
    print("required Cargo packages and manifest paths verified:")
    for name, relative in EXPECTED.items():
        print(f"  {name}: {relative}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
