#!/usr/bin/env python3
"""Build and stage only artifacts defined by targets.toml; missing apps stay pending."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import tomllib
from datetime import datetime, timezone
from pathlib import Path


HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
MANIFEST = HERE / "targets.toml"


def targets() -> tuple[dict, list[dict]]:
    data = tomllib.loads(MANIFEST.read_text(encoding="utf-8"))
    return data, data["target"]


def target_named(name: str) -> tuple[dict, dict]:
    config, all_targets = targets()
    for item in all_targets:
        if item["id"] == name:
            return config, item
    raise SystemExit(f"unknown target {name!r}; choose one of: {', '.join(t['id'] for t in all_targets)}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def readiness(item: dict) -> tuple[bool, str]:
    manifest_path = REPO / item["manifest"]
    if not manifest_path.is_file():
        return False, f"missing manifest {item['manifest']} ({item['status']})"
    with manifest_path.open("rb") as stream:
        package_manifest = tomllib.load(stream)
    package_root = manifest_path.parent.relative_to(REPO / "rust").as_posix()
    with (REPO / "rust/Cargo.toml").open("rb") as stream:
        workspace = tomllib.load(stream)["workspace"]
    if package_root not in workspace["members"]:
        return False, f"package is not registered in rust/Cargo.toml workspace members"
    if item["artifact_kind"] == "wasm-frontend":
        crate_types = package_manifest.get("lib", {}).get("crate-type", [])
        if "cdylib" not in crate_types:
            return False, "WASM frontend library must declare crate-type cdylib"
        if "wasm-bindgen" not in package_manifest.get("dependencies", {}):
            return False, "WASM frontend must declare the locked wasm-bindgen dependency"
    return True, "prototype crate present; production acceptance is separate"


def build(name: str, allow_pending: bool) -> int:
    config, item = target_named(name)
    ready, reason = readiness(item)
    print(f"target={name} status={item['status']} readiness={reason}")
    if not ready:
        return 0 if allow_pending else 2

    env = os.environ.copy()
    env["SOURCE_DATE_EPOCH"] = str(
        int(subprocess.check_output(["git", "-C", str(REPO), "show", "-s", "--format=%ct", "HEAD"], text=True).strip())
    )
    manifest_path = REPO / item["manifest"]
    if item["artifact_kind"] == "wasm-frontend":
        build_cmd = ["cargo", "build", "--locked", "--release", "--manifest-path", str(manifest_path), "--lib", "--target", item["triple"]]
    else:
        build_cmd = [
            "cargo", "build", "--locked", "--release", "--manifest-path", str(manifest_path),
            "--package", item["package"], "--bin", item["binary"], "--target", item["triple"],
        ]
    print("$", " ".join(build_cmd))
    subprocess.run(build_cmd, cwd=REPO, env=env, check=True)
    stage = HERE / "artifacts" / name
    stage.mkdir(parents=True, exist_ok=True)
    executable = REPO / "rust/target" / item["triple"] / "release" / item["binary"]
    if item["artifact_kind"] == "wasm-frontend":
        executable = REPO / "rust/target" / item["triple"] / "release" / f"{item['package'].replace('-', '_')}.wasm"
        if not executable.is_file():
            raise SystemExit(f"cargo succeeded but expected WASM input is missing: {executable}")
        subprocess.run(
            ["wasm-bindgen", "--target", "web", "--out-name", "photo_culler", "--out-dir", str(stage), str(executable)],
            cwd=REPO, env=env, check=True,
        )
        staged = stage / "photo_culler_bg.wasm"
        loader = stage / "photo_culler.js"
        if not staged.is_file() or not loader.is_file():
            raise SystemExit("wasm-bindgen completed without the expected loader and WASM files")
    else:
        if not executable.is_file():
            raise SystemExit(f"cargo succeeded but expected artifact is missing: {executable}")
        staged = stage / executable.name
        shutil.copy2(executable, staged)
    record = {
        "target": name,
        "triple": item["triple"],
        "source_commit": subprocess.check_output(["git", "-C", str(REPO), "rev-parse", "HEAD"], text=True).strip(),
        "built_at_utc": datetime.now(timezone.utc).isoformat(),
        "builder": {"os": platform.platform(), "rustc": subprocess.check_output(["rustc", "--version"], text=True).strip()},
        "artifact": staged.name,
        "sha256": sha256(staged),
        "smoke_test": "not-run",
    }
    (stage / "manifest.json").write_text(json.dumps(record, indent=2) + "\n", encoding="utf-8")
    checksum_files = sorted(path for path in stage.iterdir() if path.is_file() and path.name not in {"SHA256SUMS", "manifest.json"})
    (stage / "SHA256SUMS").write_text("".join(f"{sha256(path)}  {path.name}\n" for path in checksum_files), encoding="utf-8")
    print(f"built={staged} sha256={record['sha256']} smoke_test=not-run")
    return 0


def validate(name: str | None) -> int:
    _, items = targets()
    selected = [item for item in items if name is None or item["id"] == name]
    for item in selected:
        ready, reason = readiness(item)
        print(f"{item['id']}: {'ready' if ready else 'pending'}; triple={item['triple']}; {reason}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser()
    sub = parser.add_subparsers(dest="command", required=True)
    check = sub.add_parser("validate")
    check.add_argument("target", nargs="?")
    make = sub.add_parser("build")
    make.add_argument("target")
    make.add_argument("--allow-pending", action="store_true", help="report an unavailable target without claiming a build")
    args = parser.parse_args()
    if args.command == "validate":
        return validate(args.target)
    return build(args.target, args.allow_pending)


if __name__ == "__main__":
    raise SystemExit(main())
