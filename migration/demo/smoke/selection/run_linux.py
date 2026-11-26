#!/usr/bin/env python3
"""Exercise a seeded persistent library in a real Linux Slint window."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import shutil
import sqlite3
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


NATIVE_GUI = Path(__file__).parents[1] / "native-gui" / "run.py"
SPEC = importlib.util.spec_from_file_location("native_gui_smoke", NATIVE_GUI)
assert SPEC and SPEC.loader
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)


def request(origin: str, token: str, path: str, body: dict | None = None) -> dict:
    data = json.dumps(body).encode() if body is not None else None
    headers = {"Authorization": f"Bearer {token}"}
    if body is not None:
        headers["Content-Type"] = "application/json"
        # Native acceptance POSTs are writes too; the API's ingress policy
        # requires the exact loopback Origin even when no browser is involved.
        headers["Origin"] = origin
    req = urllib.request.Request(origin + path, data=data, headers=headers)
    with urllib.request.urlopen(req, timeout=20) as response:
        return json.loads(response.read())


def start_server(server: Path, data_dir: Path, source: Path) -> tuple[subprocess.Popen, str, str]:
    process = subprocess.Popen(
        [str(server), "--owned", "--port", "0", "--data-dir", str(data_dir),
         "--authorized-root", str(source)],
        cwd=server.parent,
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    assert process.stdout
    line = process.stdout.readline()
    if not line:
        stderr = process.stderr.read() if process.stderr else ""
        raise RuntimeError(f"fixture server exited before readiness: {stderr}")
    ready = json.loads(line)
    return process, ready["origin"], ready["session_token"]


def active_gallery_id(response: dict) -> str:
    """Validate and return the server-selected gallery from its read contract."""
    gallery_id = response.get("gallery_id")
    if response.get("contract_version") != 1 or not isinstance(gallery_id, str) or not gallery_id:
        raise RuntimeError(f"fixture server did not report one active gallery: {response}")
    return gallery_id


def import_fixture(server: Path, data_dir: Path, source: Path) -> str:
    process, origin, token = start_server(server, data_dir, source)
    try:
        gallery_id = active_gallery_id(request(origin, token, "/api/v1/galleries/active"))
        sources = request(origin, token, f"/api/v2/galleries/{gallery_id}/sources")
        if len(sources["items"]) != 1:
            raise RuntimeError(f"fixture server returned unexpected sources: {sources}")
        source_id = sources["items"][0]["id"]
        request(origin, token, "/api/v2/import-estimates", {
            "source_id": source_id, "recursive": True, "exclude_patterns": []
        })
        receipt = request(origin, token, f"/api/v2/galleries/{gallery_id}/imports", {
            "source_id": source_id, "recursive": True, "exclude_patterns": []
        })
        deadline = time.monotonic() + 60
        while time.monotonic() < deadline:
            job = request(origin, token, f"/api/v2/import-jobs/{receipt['job_id']}")
            if job["state"] == "completed":
                if job["imported"] != 13:
                    raise RuntimeError(f"expected 13 imported photos, received {job}")
                return gallery_id
            if job["state"] in {"failed", "cancelled"}:
                raise RuntimeError(f"fixture import failed: {job}")
            time.sleep(0.2)
        raise TimeoutError("fixture import did not finish within 60 seconds")
    finally:
        if process.stdin:
            process.stdin.close()
        code = process.wait(timeout=12)
        if code:
            raise RuntimeError(f"fixture server exited with {code}")


def page_ids(database: Path, gallery_id: str) -> list[str]:
    with sqlite3.connect(database) as connection:
        return [row[0] for row in connection.execute(
            "SELECT photo_id FROM photos WHERE gallery_id = ? ORDER BY photo_id",
            (gallery_id,),
        )]


def second_page_last_name(database: Path, gallery_id: str) -> str:
    with sqlite3.connect(database) as connection:
        row = connection.execute(
            "SELECT stem_name FROM photos WHERE gallery_id = ? ORDER BY photo_id ASC LIMIT 1 OFFSET 12",
            (gallery_id,),
        ).fetchone()
    if not row:
        raise RuntimeError("fixture catalog has no second page")
    return row[0] + ".jpg"


def screenshot_and_ocr(window_id: str, path: Path) -> list[dict]:
    subprocess.run(["import", "-window", window_id, str(path)], check=True)
    result = subprocess.run(
        ["tesseract", str(path), "stdout", "tsv"],
        check=True,
        capture_output=True,
        text=True,
    )
    rows = []
    for line in result.stdout.splitlines()[1:]:
        fields = line.split("\t")
        if len(fields) >= 12 and fields[11].strip():
            rows.append({
                "text": fields[11].strip(),
                "x": int(fields[6]), "y": int(fields[7]),
                "width": int(fields[8]), "height": int(fields[9]),
                "confidence": float(fields[10]),
            })
    path.with_suffix(".ocr.json").write_text(json.dumps(rows, indent=2) + "\n")
    return rows


def click_text(rows: list[dict], text: str, window_width: int, window_height: int,
               *, left: bool | None = None, below: float | None = None,
               above: float | None = None, origin_x: int = 0,
               origin_y: int = 0) -> None:
    candidates = [row for row in rows if row["text"].casefold() == text.casefold()
                  and row["confidence"] >= 35]
    if left is not None:
        candidates = [row for row in candidates if (row["x"] < window_width * 0.23) == left]
    if below is not None:
        candidates = [row for row in candidates if row["y"] > window_height * below]
    if above is not None:
        candidates = [row for row in candidates if row["y"] < window_height * above]
    if len(candidates) != 1:
        raise RuntimeError(f"cannot uniquely locate {text!r} in OCR output: {candidates}")
    row = candidates[0]
    x = origin_x + row["x"] + row["width"] // 2
    y = origin_y + row["y"] + row["height"] // 2
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def first_page_display_names(database: Path, gallery_id: str, limit: int = 12) -> list[str]:
    """Mirror the first API page and the UI's filename-descending grid order."""
    with sqlite3.connect(database) as connection:
        page = connection.execute(
            "SELECT photo_id, stem_name FROM photos WHERE gallery_id = ? "
            "ORDER BY photo_id ASC LIMIT ?",
            (gallery_id, limit),
        ).fetchall()
    # The deterministic JPEGs have no EXIF capture time. The desktop view sorts
    # the API page by filename descending before placing it row-major in the grid.
    page.sort(key=lambda row: row[1], reverse=True)
    return [f"{stem_name}.jpg" for _photo_id, stem_name in page]


def grid_cell_center(geometry: dict, index: int) -> tuple[int, int]:
    """Return a card center for the fixed 1440x900 Linux acceptance layout."""
    width, height = geometry["width"], geometry["height"]
    if width < 1280 or height < 700:
        raise RuntimeError(f"selection acceptance needs the 1440x900 grid layout: {geometry}")
    if not 0 <= index < 12:
        raise ValueError(f"first-page grid index is out of range: {index}")
    columns = 5  # Medium density at viewport-width >= 1280, as defined by LibraryGallery.
    row, column = divmod(index, columns)
    origin_x, origin_y = geometry["x"], geometry["y"]
    x = origin_x + 520 + column * 118
    y = origin_y + 160 + row * 92
    return x, y


def next_photo_button_center(geometry: dict) -> tuple[int, int]:
    """Return the center of the gallery header's Next Photo control."""
    return (
        geometry["x"] + round(geometry["width"] * 0.665),
        geometry["y"] + round(geometry["height"] * 0.114),
    )


def click_photo_at(point: tuple[int, int]) -> None:
    subprocess.run(["xdotool", "mousemove", str(point[0]), str(point[1]), "click", "1"], check=True)
    time.sleep(.4)


def check_catalog(database: Path, gallery_id: str, expected: dict[str, str]) -> None:
    with sqlite3.connect(database) as connection:
        actual = dict(connection.execute(
            "SELECT stem_name, decision FROM photos WHERE gallery_id = ?",
            (gallery_id,),
        ))
    for name, decision in expected.items():
        if actual.get(name) != decision:
            raise RuntimeError(f"decision for {name} is {actual.get(name)!r}, expected {decision!r}")


def check_counts(database: Path, gallery_id: str, keep: int, reject: int,
                 unrated: int, total: int) -> None:
    with sqlite3.connect(database) as connection:
        actual = connection.execute(
            "SELECT "
            "SUM(CASE WHEN UPPER(decision) IN ('KEEP','BEST','PROTECTED_BY_COVERAGE') THEN 1 ELSE 0 END), "
            "SUM(CASE WHEN UPPER(decision) IN ('REJECT','REJECT_TECHNICAL','REJECT_REDUNDANT') THEN 1 ELSE 0 END), "
            "SUM(CASE WHEN decision IS NULL OR UPPER(decision) NOT IN "
            "('KEEP','BEST','PROTECTED_BY_COVERAGE','ALT','ALTERNATE','REJECT','REJECT_TECHNICAL','REJECT_REDUNDANT') THEN 1 ELSE 0 END), "
            "COUNT(*) FROM photos WHERE gallery_id = ?",
            (gallery_id,),
        ).fetchone()
    expected = (keep, reject, unrated, total)
    if actual != expected:
        raise RuntimeError(f"catalog counts were {actual}, expected {expected}")


def assert_visible_summary(rows: list[dict], *, kept: int, rejected: int, unrated: int) -> None:
    for label, expected in (("KEPT", kept), ("REJECTED", rejected), ("UNRATED", unrated)):
        labels = [row for row in rows if row["text"].upper() == label and row["confidence"] >= 20]
        if len(labels) != 1:
            raise RuntimeError(f"could not uniquely read {label} in the visible selection summary: {labels}")
        label_row = labels[0]
        counts = [row for row in rows if row["text"] == str(expected)
                  and row["x"] >= label_row["x"] + label_row["width"]
                  and abs(row["y"] - label_row["y"]) <= 14 and row["confidence"] >= 20]
        if not counts:
            raise RuntimeError(f"visible {label} summary did not show count {expected}: {rows}")


def verify_fixture_hashes(manifest: dict, source: Path) -> dict[str, str]:
    actual = {}
    for photo in manifest["photos"]:
        path = source / photo["filename"]
        digest = hashlib.sha256(path.read_bytes()).hexdigest()
        if digest != photo["sha256"]:
            raise RuntimeError(f"fixture JPEG hash changed for {photo['filename']}: {digest}")
        actual[photo["filename"]] = digest
    return actual


def snapshot_catalog(database: Path, destination: Path) -> None:
    with sqlite3.connect(database) as source_db, sqlite3.connect(destination) as backup_db:
        source_db.backup(backup_db)


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    desktop, server = args.desktop.resolve(), args.server.resolve()
    fixture, output = args.fixture.resolve(), args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    manifest = json.loads((fixture / "manifest.json").read_text())
    source = fixture / "Fotos de selección"
    data_dir = output / "xdg" / "photo-culler" / "desktop"
    data_dir.mkdir(parents=True)
    shutil.copytree(source, data_dir / "source")
    source_copy = data_dir / "source"

    gallery_id = import_fixture(server, data_dir, source_copy)
    database = data_dir / "catalog.sqlite3"
    ids = page_ids(database, gallery_id)
    if len(ids) != 13:
        raise RuntimeError(f"catalog contains {len(ids)} photos, expected 13")
    fixture_hashes = verify_fixture_hashes(manifest, source_copy)
    snapshot_catalog(database, output / "catalog-seeded.sqlite3")

    status = output / "desktop-status.tsv"
    log = output / "desktop.log"
    env = os.environ.copy()
    env["PHOTO_CULLER_SMOKE_STATUS_FILE"] = str(status)
    env.pop("PHOTO_CULLER_API_ORIGIN", None)
    stream = log.open("wb")
    process = subprocess.Popen(
        [str(desktop), "--data-dir", str(data_dir), "--authorized-root", str(source_copy),
         "--gallery-id", gallery_id],
        cwd=desktop.parent, env=env, stdout=stream, stderr=subprocess.STDOUT,
    )
    process._smoke_log = stream
    evidence = {"source_sha": subprocess.check_output(["git", "rev-parse", "HEAD"], text=True).strip(),
                "gallery_id": gallery_id, "photo_ids": ids, "screenshots": []}
    try:
        wait_gallery_ready(process, status)
        window = native.wait_for_linux_window(process.pid, output)
        wid = window["window_id"]
        width, height = window["geometry"]["width"], window["geometry"]["height"]
        origin_x, origin_y = window["geometry"]["x"], window["geometry"]["y"]

        first = output / "page-one.png"
        rows = screenshot_and_ocr(wid, first)
        evidence["screenshots"].append(str(first))
        visible_names = first_page_display_names(database, gallery_id)
        required_names = {"photo-01.jpg", "photo-02.jpg"}
        if not required_names.issubset(visible_names):
            raise RuntimeError(
                f"decision targets are not on the first API page: {visible_names}"
            )
        evidence["first_page_display_order"] = visible_names

        click_photo_at(grid_cell_center(window["geometry"], visible_names.index("photo-01.jpg")))
        rows = screenshot_and_ocr(wid, output / "selected-first.png")
        click_text(rows, "KEEP", width, height, below=0.55, left=False,
                   origin_x=origin_x, origin_y=origin_y)
        check_catalog(database, gallery_id, {"photo-01": "KEEP"})

        rows = screenshot_and_ocr(wid, output / "kept-first.png")
        click_photo_at(grid_cell_center(window["geometry"], visible_names.index("photo-02.jpg")))
        rows = screenshot_and_ocr(wid, output / "selected-second.png")
        click_text(rows, "REJECT", width, height, below=0.55, left=False,
                   origin_x=origin_x, origin_y=origin_y)
        check_catalog(database, gallery_id, {"photo-01": "KEEP", "photo-02": "REJECT"})
        check_counts(database, gallery_id, keep=1, reject=1, unrated=11, total=13)

        rows = screenshot_and_ocr(wid, output / "reject-first.png")
        assert_visible_summary(rows, kept=1, rejected=1, unrated=11)
        evidence["visible_summary"] = {"kept": 1, "rejected": 1, "unrated": 11}
        evidence["screenshots"].append(str(output / "reject-first.png"))
        click_text(rows, "KEEP", width, height, left=True, above=0.40,
                   origin_x=origin_x, origin_y=origin_y)
        rows = screenshot_and_ocr(wid, output / "keep-filter.png")
        evidence["screenshots"].append(str(output / "keep-filter.png"))
        if "photo-01.jpg" not in [row["text"] for row in rows]:
            raise RuntimeError("KEEP filter omitted the confirmed photo-01 decision")
        if "photo-02.jpg" in [row["text"] for row in rows]:
            raise RuntimeError("KEEP filter retained the confirmed REJECT photo")
        check_catalog(database, gallery_id, {"photo-01": "KEEP", "photo-02": "REJECT"})
        evidence["keep_filter"] = "only the KEEP photo is shown"

        rows = screenshot_and_ocr(wid, output / "keep-filter-before-next.png")
        click_text(rows, "ALL", width, height, left=True, above=0.40,
                   origin_x=origin_x, origin_y=origin_y)
        for _ in range(12):
            click_next_photo(wid, origin_x, origin_y, width, height)
        rows = screenshot_and_ocr(wid, output / "page-two.png")
        evidence["screenshots"].append(str(output / "page-two.png"))
        expected_last = second_page_last_name(database, gallery_id)
        if expected_last not in [row["text"] for row in rows]:
            raise RuntimeError(f"last fixture photo did not appear after 12 Next Photo actions: {expected_last}")
        evidence["pagination"] = f"reached second-page item {expected_last} after 12 Next Photo actions"
        snapshot_catalog(database, output / "catalog-after-decisions.sqlite3")

        native.close_window("linux", process.pid, window, output)
        process.wait(timeout=20)
        if process._smoke_log and not process._smoke_log.closed:
            process._smoke_log.close()

        reopened_status, reopened_log = output / "reopened-status.tsv", output / "reopened.log"
        process = launch_desktop(desktop, data_dir, source_copy, reopened_status, reopened_log,
                                 gallery_id)
        wait_gallery_ready(process, reopened_status)
        reopened_window = native.wait_for_linux_window(process.pid, output)
        reopened_rows = screenshot_and_ocr(reopened_window["window_id"], output / "reopened.png")
        assert_visible_summary(reopened_rows, kept=1, rejected=1, unrated=11)
        reopened_geometry = reopened_window["geometry"]
        click_text(reopened_rows, "KEEP", reopened_geometry["width"],
                   reopened_geometry["height"], left=True, above=0.40,
                   origin_x=reopened_geometry["x"], origin_y=reopened_geometry["y"])
        reopened_rows = screenshot_and_ocr(reopened_window["window_id"], output / "reopened-keep-filter.png")
        if "photo-01.jpg" not in [row["text"] for row in reopened_rows]:
            raise RuntimeError("KEEP decision was not visible after reopening and applying KEEP filter")
        check_catalog(database, gallery_id, {"photo-01": "KEEP"})
        if page_ids(database, gallery_id) != ids:
            raise RuntimeError("photo IDs changed after closing and reopening the library")
        check_counts(database, gallery_id, keep=1, reject=1, unrated=11, total=13)
        if verify_fixture_hashes(manifest, source_copy) != fixture_hashes:
            raise RuntimeError("fixture JPEG hashes changed during the UI acceptance run")
        snapshot_catalog(database, output / "catalog-after-reopen.sqlite3")
        evidence["persistence_on_close_reopen"] = "PASS"
        evidence["photo_ids_stable_after_reopen"] = True
        evidence["fixture_sha256"] = fixture_hashes
        evidence["screenshots"].extend([str(output / "reopened.png"), str(output / "reopened-keep-filter.png")])
        (output / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
        native.close_window("linux", process.pid, reopened_window, output)
        process.wait(timeout=20)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=5)
        for stream in (getattr(process, "_smoke_log", None),):
            if stream and not stream.closed:
                stream.close()
    print(json.dumps(evidence, indent=2))


def wait_gallery_ready(process: subprocess.Popen, status: Path) -> None:
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"desktop exited early ({process.returncode})")
        rows = native.events(status)
        error = next((row for row in rows if row[0] == "gallery-error"), None)
        if error:
            raise RuntimeError("desktop gallery failed: " + " ".join(error[1:]))
        if any(row[0] == "gallery-ready" for row in rows):
            return
        time.sleep(0.1)
    raise TimeoutError("desktop did not report the persistent gallery ready")


def launch_desktop(desktop: Path, data_dir: Path, source: Path, status: Path,
                   log: Path, gallery_id: str) -> subprocess.Popen:
    status.unlink(missing_ok=True)
    env = os.environ.copy()
    env["PHOTO_CULLER_SMOKE_STATUS_FILE"] = str(status)
    env.pop("PHOTO_CULLER_API_ORIGIN", None)
    stream = log.open("wb")
    process = subprocess.Popen(
        [str(desktop), "--data-dir", str(data_dir), "--authorized-root", str(source),
         "--gallery-id", gallery_id],
        cwd=desktop.parent, env=env, stdout=stream, stderr=subprocess.STDOUT,
    )
    process._smoke_log = stream
    return process


def click_next_photo(window_id: str, origin_x: int, origin_y: int,
                     width: int, height: int) -> None:
    x, y = next_photo_button_center({
        "x": origin_x, "y": origin_y, "width": width, "height": height,
    })
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.35)


if __name__ == "__main__":
    main()
