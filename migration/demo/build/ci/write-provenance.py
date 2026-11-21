#!/usr/bin/env python3
"""Write checksums and compile-vs-runtime provenance for a completed CI artifact."""

from __future__ import annotations

import hashlib
import json
import platform
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


def run(*args: str, cwd: Path) -> str:
    return subprocess.check_output(args, cwd=cwd, text=True).strip()


def main() -> int:
    if len(sys.argv) != 6:
        raise SystemExit("usage: write-provenance.py WORKSPACE_ROOT ARTIFACT_DIR TARGET PHASE PACKAGE[,PACKAGE...]")
    root = Path(sys.argv[1]).resolve()
    artifact_dir = Path(sys.argv[2]).resolve()
    target, phase, package_csv = sys.argv[3:]
    package_names = package_csv.split(",")
    artifacts = sorted(
        path for path in artifact_dir.rglob("*")
        if path.is_file() and path.name not in {"artifact-manifest.json", "checksums.sha256"}
    )
    if not artifacts:
        raise SystemExit(f"refusing to publish an empty compile artifact: {artifact_dir}")

    metadata = json.loads(
        run("cargo", "metadata", "--locked", "--no-deps", "--format-version", "1", "--manifest-path", str(root / "rust/Cargo.toml"), cwd=root)
    )
    visible_version = next(package["version"] for package in metadata["packages"] if package["name"] == "photo-culler-slint")
    available_packages = {package["name"] for package in metadata["packages"]}
    missing_packages = sorted(set(package_names) - available_packages)
    if missing_packages:
        raise SystemExit(f"cannot record packages absent from Cargo metadata: {', '.join(missing_packages)}")
    record = {
        "schema_version": 1,
        "visible_version": visible_version,
        "sha": run("git", "rev-parse", "HEAD", cwd=root),
        "created_at_utc": datetime.now(timezone.utc).isoformat(),
        "toolchain": {"rustc": run("rustc", "--version", cwd=root), "cargo": run("cargo", "--version", cwd=root)},
        "runner": {"os": platform.system(), "architecture": platform.machine()},
        "target": target,
        "packages": package_names,
        "phase": phase,
        "runtime_smoke": "not_run",
        "artifacts": [str(path.relative_to(artifact_dir)) for path in artifacts],
    }
    (artifact_dir / "artifact-manifest.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    lines = []
    for path in artifacts:
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        lines.append(f"{digest}  {path.relative_to(artifact_dir).as_posix()}\n")
    (artifact_dir / "checksums.sha256").write_text("".join(lines), encoding="utf-8")
    print(f"provenance recorded: phase={phase}; runtime_smoke=not_run; files={len(artifacts)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
