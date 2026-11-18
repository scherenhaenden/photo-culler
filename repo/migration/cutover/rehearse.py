#!/usr/bin/env python3
"""Synthetic cutover/rollback rehearsal. Uses only a fresh temporary directory."""

from __future__ import annotations

import hashlib
import json
import sqlite3
import tempfile
from pathlib import Path


def digest(path: Path) -> str:
    hasher = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(64 * 1024), b""):
            hasher.update(block)
    return hasher.hexdigest()


def make_catalog(path: Path, originals: list[Path]) -> None:
    with sqlite3.connect(path) as db:
        db.execute("PRAGMA journal_mode=WAL")
        db.executescript(
            """
            CREATE TABLE galleries(id TEXT PRIMARY KEY, name TEXT NOT NULL);
            CREATE TABLE photos(id TEXT PRIMARY KEY, gallery_id TEXT NOT NULL REFERENCES galleries(id), file_path TEXT NOT NULL, sha256 TEXT NOT NULL);
            CREATE TABLE decisions(photo_id TEXT PRIMARY KEY REFERENCES photos(id), decision TEXT NOT NULL, revision INTEGER NOT NULL);
            CREATE TABLE recipes(photo_id TEXT PRIMARY KEY REFERENCES photos(id), body TEXT NOT NULL, revision INTEGER NOT NULL);
            INSERT INTO galleries VALUES('gallery-001', 'synthetic');
            """
        )
        for index, original in enumerate(originals, start=1):
            photo_id = f"photo-{index:03}"
            db.execute(
                "INSERT INTO photos VALUES(?, 'gallery-001', ?, ?)",
                (photo_id, original.name, digest(original)),
            )
            db.execute("INSERT INTO decisions VALUES(?, ?, 1)", (photo_id, "keep" if index == 1 else "review"))
            db.execute("INSERT INTO recipes VALUES(?, ?, 1)", (photo_id, '{"exposure":0.0}'))
        db.commit()


def check_integrity(path: Path) -> None:
    with sqlite3.connect(path) as db:
        result = db.execute("PRAGMA integrity_check").fetchone()[0]
        if result != "ok":
            raise AssertionError(f"integrity_check failed: {result}")
        if db.execute("PRAGMA foreign_key_check").fetchall():
            raise AssertionError("foreign_key_check returned violations")


def inventory(path: Path) -> dict:
    with sqlite3.connect(path) as db:
        return {
            "galleries": db.execute("SELECT id, name FROM galleries ORDER BY id").fetchall(),
            "photos": db.execute("SELECT id, gallery_id, file_path, sha256 FROM photos ORDER BY id").fetchall(),
            "decisions": db.execute("SELECT photo_id, decision, revision FROM decisions ORDER BY photo_id").fetchall(),
            "recipes": db.execute("SELECT photo_id, body, revision FROM recipes ORDER BY photo_id").fetchall(),
        }


def write_pointer(pointer: Path, target: Path) -> None:
    pointer.write_text(json.dumps({"active_catalog": str(target)}) + "\n", encoding="utf-8")


def read_pointer(pointer: Path) -> Path:
    return Path(json.loads(pointer.read_text(encoding="utf-8"))["active_catalog"])


def main() -> None:
    with tempfile.TemporaryDirectory(prefix="photo-culler-cutover-") as temp:
        root = Path(temp)
        originals_dir = root / "originals"
        originals_dir.mkdir()
        originals = [originals_dir / "one.raw", originals_dir / "two.jpg"]
        originals[0].write_bytes(b"synthetic raw original\x00v1")
        originals[1].write_bytes(b"synthetic jpeg original\x00v1")
        original_hashes = {file.name: digest(file) for file in originals}

        legacy_dir = root / "legacy"
        legacy_dir.mkdir()
        legacy = legacy_dir / "catalog.db"
        backup_dir = root / "backup"
        backup_dir.mkdir()
        backup = backup_dir / "catalog-verified.db"
        candidate_dir = root / "candidate"
        candidate_dir.mkdir()
        candidate = candidate_dir / "catalog.db"
        pointer = root / "active.json"

        make_catalog(legacy, originals)
        check_integrity(legacy)
        source_inventory = inventory(legacy)
        with sqlite3.connect(legacy) as source, sqlite3.connect(backup) as destination:
            source.backup(destination)
        check_integrity(backup)
        if inventory(backup) != source_inventory:
            raise AssertionError("consistent backup inventory differs from legacy source")

        with sqlite3.connect(backup) as source, sqlite3.connect(candidate) as destination:
            source.backup(destination)
        with sqlite3.connect(candidate) as db:
            db.execute("CREATE TABLE candidate_marker(version TEXT NOT NULL)")
            db.execute("INSERT INTO candidate_marker VALUES('synthetic-candidate-1')")
            db.commit()
        check_integrity(candidate)
        if inventory(candidate) != source_inventory:
            raise AssertionError("candidate copy differs before cutover")

        write_pointer(pointer, candidate)
        writes_enabled = False
        candidate_health_ok = False  # Deliberate synthetic failure injection.
        if not candidate_health_ok:
            write_pointer(pointer, backup)

        active_after_rollback = read_pointer(pointer)
        check_integrity(active_after_rollback)
        if inventory(active_after_rollback) != source_inventory:
            raise AssertionError("rollback catalog does not match pre-cutover inventory")
        final_original_hashes = {file.name: digest(file) for file in originals}
        if final_original_hashes != original_hashes:
            raise AssertionError("synthetic original files changed")

        print("result=PASS")
        print("candidate=REJECTED")
        print(f"writes_enabled_during_cutover={str(writes_enabled).lower()}")
        print(f"rollback_catalog=VERIFIED ({active_after_rollback.name})")
        print("originals=UNCHANGED")
        print(f"synthetic_photos={len(source_inventory['photos'])}")
        print("scope=synthetic-only; no real catalog or originals accessed")


if __name__ == "__main__":
    main()
