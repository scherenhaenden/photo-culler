#!/usr/bin/env python3
"""Accept the normal packaged Sprint 02 import and reopen flow on Linux/X11."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import subprocess
import time
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
NATIVE_GUI = SCRIPT_DIR.parent / "native-gui" / "run.py"
SPEC = importlib.util.spec_from_file_location("native_gui_smoke", NATIVE_GUI)
assert SPEC and SPEC.loader
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)


def screenshot(output: Path) -> list[dict]:
    subprocess.run(["import", "-window", "root", str(output)], check=True)
    result = subprocess.run(
        ["tesseract", str(output), "stdout", "tsv"], check=True, capture_output=True, text=True
    )
    rows = parse_tesseract_tsv(result.stdout)
    output.with_suffix(".ocr.json").write_text(json.dumps(rows, indent=2) + "\n")
    return rows


def parse_tesseract_tsv(tsv: str) -> list[dict]:
    lines: dict[tuple[str, str, str], list[dict]] = {}
    for line in tsv.splitlines()[1:]:
        fields = line.split("\t")
        if len(fields) >= 12 and fields[11].strip():
            # TSV word_num is different for each token; group on line_num to
            # recover visible phrases such as "Tu biblioteca empieza...".
            key = tuple(fields[index] for index in (2, 3, 4))
            lines.setdefault(key, []).append({
                "text": fields[11].strip(), "x": int(fields[6]), "y": int(fields[7]),
                "width": int(fields[8]), "height": int(fields[9]), "confidence": float(fields[10]),
            })
    rows = []
    for words in lines.values():
        rows.append({
            "text": " ".join(word["text"] for word in words),
            "x": min(word["x"] for word in words), "y": min(word["y"] for word in words),
            "width": max(word["x"] + word["width"] for word in words) - min(word["x"] for word in words),
            "height": max(word["y"] + word["height"] for word in words) - min(word["y"] for word in words),
            "confidence": sum(word["confidence"] for word in words) / len(words),
        })
    return rows


def click_text(rows: list[dict], text: str, *, near: tuple[int, int] | None = None) -> None:
    candidates = [row for row in rows if row["text"].casefold() == text.casefold() and row["confidence"] >= 35]
    if near is not None:
        candidates.sort(key=lambda row: abs(row["x"] - near[0]) + abs(row["y"] - near[1]))
        if candidates:
            row = candidates[0]
            if abs(row["x"] - near[0]) + abs(row["y"] - near[1]) > 240:
                candidates = []
            else:
                candidates = [row]
    if len(candidates) != 1:
        raise RuntimeError(f"cannot uniquely locate {text!r}: {candidates}")
    row = candidates[0]
    subprocess.run([
        "xdotool", "mousemove", str(row["x"] + row["width"] // 2),
        str(row["y"] + row["height"] // 2), "click", "1",
    ], check=True)
    time.sleep(0.5)


def choose_folder_button_point(width: int, height: int) -> tuple[int, int]:
    # The first-run call to action is centered in the gallery panel, near the
    # bottom of the maximized 1440x900 runner window.
    return (width // 2 + width // 40, height - max(20, height // 40))


def click_choose_folder(rows: list[dict]) -> None:
    if any(row["text"].casefold() == "elegir carpeta" for row in rows):
        click_text(rows, "Elegir carpeta")
        return

    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = choose_folder_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def open_source_picker(source: Path, output: Path) -> None:
    rows = screenshot(output)
    click_choose_folder(rows)
    time.sleep(1.0)
    subprocess.run(["xdotool", "key", "ctrl+l"], check=True)
    subprocess.run(["xdotool", "type", "--clearmodifiers", str(source)], check=True)
    subprocess.run(["xdotool", "key", "Return"], check=True)
    time.sleep(1.0)
    rows = screenshot(output.with_name("folder-picker.png"))
    picker_button = next(
        (row["text"] for row in rows if row["text"].casefold() in {"open", "select", "choose"}),
        None,
    )
    if picker_button:
        click_text(rows, picker_button)
    else:
        subprocess.run(["xdotool", "key", "Return"], check=True)
    time.sleep(2.0)


def wait_text(text: str, output: Path, timeout: float = 30) -> list[dict]:
    deadline = time.monotonic() + timeout
    latest = []
    while time.monotonic() < deadline:
        latest = screenshot(output)
        if any(text.casefold() in row["text"].casefold() for row in latest):
            return latest
        time.sleep(0.5)
    raise TimeoutError(f"UI did not show {text!r} before timeout; last OCR={latest}")


def database_snapshot(database: Path) -> tuple[int, list[tuple[str, str, str, str]]]:
    import sqlite3

    with sqlite3.connect(database) as connection:
        rows = connection.execute(
            "SELECT p.stem_name, p.photo_id, f.source_relative_path, f.full_hash "
            "FROM photos p JOIN files f ON f.photo_id = p.id "
            "WHERE lower(f.role) = 'jpeg' ORDER BY p.stem_name"
        ).fetchall()
    return len(rows), rows


def assert_source_hashes(source: Path, manifest: Path) -> None:
    expected = json.loads(manifest.read_text())
    for entry in expected["photos"]:
        digest = hashlib.sha256((source / entry["filename"]).read_bytes()).hexdigest()
        if digest != entry["sha256"]:
            raise RuntimeError(f"source JPEG changed: {entry['filename']}")


def assert_catalog_hashes(rows: list[tuple[str, str, str, str]], expected: dict[str, str]) -> None:
    actual = {stem + ".jpg": digest for stem, _photo_id, _source_path, digest in rows}
    if len(rows) != len(expected) or actual != expected:
        raise RuntimeError(f"catalog full hashes differ from source manifest: {actual}")


def launch(desktop: Path, xdg: Path, evidence: Path, suffix: str) -> subprocess.Popen:
    env = os.environ.copy()
    env["XDG_DATA_HOME"] = str(xdg)
    env.pop("PHOTO_CULLER_API_ORIGIN", None)
    log = (evidence / f"desktop-{suffix}.log").open("wb")
    process = subprocess.Popen([str(desktop)], env=env, cwd=desktop.parent, stdout=log, stderr=subprocess.STDOUT)
    process._evidence_log = log
    return process


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    desktop, fixture, output = args.desktop.resolve(), args.fixture.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    source = fixture / "Fotos de selección"
    manifest = fixture / "manifest.json"
    xdg = output / "xdg"
    xdg.mkdir(parents=True, exist_ok=True)
    process = launch(desktop, xdg, output, "first")
    status = "PASS"
    try:
        window = native.wait_for_linux_window(process.pid, output)
        rows = screenshot(output / "first-run.png")
        if not any("Tu biblioteca empieza con una carpeta" in row["text"] for row in rows):
            raise RuntimeError(f"first-run empty-library state not visible: {rows}")
        open_source_picker(source, output / "choose-folder.png")
        time.sleep(1.0)
        rows = screenshot(output / "source-selected.png")
        click_text(rows, "Import")
        rows = wait_text("Estimate", output / "import-dialog.png")
        if not any("Estimate" in row["text"] for row in rows):
            raise RuntimeError("import dialog did not expose Estimate")
        click_text(rows, "Estimate")
        rows = wait_text("logical photos", output / "estimated.png")
        if not any("13 logical photos" in row["text"] for row in rows):
            raise RuntimeError(f"expected 13 logical photos: {rows}")
        click_text(rows, "Start import")
        wait_text("13 imported", output / "import-progress.png", timeout=90)
        rows = screenshot(output / "import-complete.png")
        click_text(rows, "Close")

        database = xdg / "photo-culler" / "desktop" / "catalog.sqlite3"
        deadline = time.monotonic() + 30
        first_rows = []
        while time.monotonic() < deadline:
            count, first_rows = database_snapshot(database)
            if count == 13:
                break
            time.sleep(0.5)
        if count != 13:
            raise RuntimeError(f"catalog has {count} photos, expected 13")
        ids_before = first_rows
        expected_hashes = {
            entry["filename"]: entry["sha256"] for entry in json.loads(manifest.read_text())["photos"]
        }
        assert_catalog_hashes(first_rows, expected_hashes)
        assert_source_hashes(source, manifest)
        rows = screenshot(output / "library-first-page.png")
        if not any("12 ON THIS PAGE" in row["text"] for row in rows):
            raise RuntimeError(f"expected twelve photos on the first page: {rows}")
        click_text(rows, "›")
        rows = wait_text("1 ON THIS PAGE", output / "library-second-page.png")
        if not any("photo-13.jpg" in row["text"].casefold() for row in rows):
            rows = wait_text("photo-13", output / "library-second-page-visible.png")
        if not any("photo-13.jpg" in row["text"].casefold() for row in rows):
            raise RuntimeError("second page did not show photo-13.jpg")

        native.close_window("linux", process.pid, window, output)
        process.wait(timeout=15)
        process._evidence_log.close()
        process = launch(desktop, xdg, output, "reopen")
        window = native.wait_for_linux_window(process.pid, output)
        rows = screenshot(output / "reopened.png")
        if not any("photo-13.jpg" in row["text"].casefold() for row in rows):
            click_text(rows, "›")
            rows = wait_text("photo-13.jpg", output / "reopened-second-page.png")
        if database_snapshot(database) != (13, ids_before):
            raise RuntimeError("reopened catalog IDs or counts differ")
        assert_source_hashes(source, manifest)

        rows = screenshot(output / "reopened-import.png")
        click_text(rows, "Import")
        rows = wait_text("Start import", output / "reimport-dialog.png")
        click_text(rows, "Start import")
        wait_text("13 imported", output / "reimport-progress.png", timeout=90)
        rows = screenshot(output / "reimport-complete.png")
        click_text(rows, "Close")
        if database_snapshot(database) != (13, ids_before):
            raise RuntimeError("reimport changed photo IDs or duplicated catalog rows")
        assert_source_hashes(source, manifest)
        assert_catalog_hashes(database_snapshot(database)[1], expected_hashes)
        (output / "s02-result.txt").write_text("PASS\n13 JPEG imported\n13 stable IDs after reopen and reimport\nsource and catalog SHA-256 verified\n")
        native.close_window("linux", process.pid, window, output)
        process.wait(timeout=15)
        process._evidence_log.close()
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
            process._evidence_log.close()


if __name__ == "__main__":
    main()
