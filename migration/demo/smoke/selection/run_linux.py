#!/usr/bin/env python3
"""Exercise a seeded persistent library in a real Linux Slint window."""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import re
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


def decision_targets(display_names: list[str]) -> dict[str, str]:
    """Choose two distinct cards guaranteed to be present on the first page."""
    if len(display_names) < 2 or len(set(display_names)) != len(display_names):
        raise RuntimeError(f"first API page must contain at least two unique photos: {display_names}")
    return {"keep": display_names[0], "reject": display_names[1]}


def expected_decisions(targets: dict[str, str]) -> dict[str, str]:
    # The UI sends the categorical action "reject"; the write API persists
    # its canonical catalog enum as REJECT_TECHNICAL.
    return {
        Path(targets["keep"]).stem: "KEEP",
        Path(targets["reject"]).stem: "REJECT_TECHNICAL",
    }


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


def keep_chip_center_from_pixels(points: list[tuple[int, int]]) -> tuple[int, int]:
    """Find the teal KEEP chip in the screenshot's top toolbar crop."""
    by_x: dict[int, list[int]] = {}
    for x, y in points:
        by_x.setdefault(x, []).append(y)
    columns = sorted(x for x, ys in by_x.items() if len(ys) >= 4)
    runs: list[list[int]] = []
    for x in columns:
        if not runs or x - runs[-1][-1] > 2:
            runs.append([x])
        else:
            runs[-1].append(x)
    runs = [run for run in runs if 20 <= run[-1] - run[0] + 1 <= 48]
    if len(runs) != 1:
        raise RuntimeError(f"could not uniquely locate the teal KEEP toolbar chip: {runs}")
    run = runs[0]
    ys = [y for x in run for y in by_x[x]]
    return (run[0] + run[-1] + 1) // 2, (min(ys) + max(ys)) // 2


def is_keep_chip_color(rgb: tuple[int, int, int]) -> bool:
    # This is the stable fill color of the KEEP action chip, excluding nearby
    # antialiased text and unrelated teal UI accents.
    return all(abs(actual - expected) <= 5
               for actual, expected in zip(rgb, (35, 149, 132)))


def decision_button_center(geometry: dict, decision: str,
                           screenshot: Path) -> tuple[int, int]:
    """Anchor toolbar actions to the visible teal KEEP chip across sprint layouts."""
    offsets = {"KEEP": 0, "ALT": 38, "REJECT": 76, "CLEAR": 113}
    try:
        offset = offsets[decision.upper()]
    except KeyError as error:
        raise ValueError(f"unknown decision toolbar action: {decision}") from error
    from PIL import Image

    image = Image.open(screenshot).convert("RGB")
    left, right = int(image.width * 0.40), int(image.width * 0.78)
    top, bottom = int(image.height * 0.04), int(image.height * 0.10)
    points = []
    for y in range(top, bottom):
        for x in range(left, right):
            if is_keep_chip_color(image.getpixel((x, y))):
                points.append((x, y))
    anchor_x, anchor_y = keep_chip_center_from_pixels(points)
    return geometry["x"] + anchor_x + offset, geometry["y"] + anchor_y


def decision_filter_center(geometry: dict, decision: str) -> tuple[int, int]:
    """Return a decision-filter button center in the left filter sidebar."""
    positions = {
        "ALL": (0.202, 0.149),
        "KEEP": (0.279, 0.149),
        "ALT": (0.202, 0.186),
        "REJECT": (0.279, 0.186),
        "UNRATED": (0.202, 0.223),
    }
    try:
        x_fraction, y_fraction = positions[decision.upper()]
    except KeyError as error:
        raise ValueError(f"unknown decision filter: {decision}") from error
    return (
        geometry["x"] + round(geometry["width"] * x_fraction),
        geometry["y"] + round(geometry["height"] * y_fraction),
    )


def click_at(point: tuple[int, int]) -> None:
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


SUMMARY_ROW_CROPS = {
    "KEPT": (8, 396),
    "ALT": (8, 431),
    "REJECTED": (8, 466),
    "UNRATED": (8, 501),
}
SUMMARY_LABEL_ALIASES = {
    "KEPT": {"KEPT", "KEEP"},
    "ALT": {"ALT"},
    "REJECTED": {"REJECTED", "REJECT"},
    "UNRATED": {"UNRATED"},
}


def summary_count_from_ocr(text: str, label: str) -> int:
    """Read one category count from enlarged row OCR, correcting 1l/O confusions."""
    tokens = [token.upper() for token in re.findall(r"[A-Za-z0-9]+", text)]
    aliases = SUMMARY_LABEL_ALIASES[label]
    label_index = next((index for index, token in enumerate(tokens) if token in aliases), None)
    if label_index is None:
        raise RuntimeError(f"could not read {label} label from enlarged summary row: {text!r}")
    for token in tokens[label_index + 1:]:
        digits = token.translate(str.maketrans({"I": "1", "L": "1", "O": "0"}))
        if digits.isdigit():
            return int(digits)
    raise RuntimeError(f"could not read {label} count from enlarged summary row: {text!r}")


def assert_visible_summary(screenshot: Path, *, kept: int, rejected: int,
                           unrated: int, alt: int = 0) -> dict[str, int]:
    from PIL import Image

    expected = {"KEPT": kept, "ALT": alt, "REJECTED": rejected, "UNRATED": unrated}
    observed = {}
    with Image.open(screenshot) as image:
        for label, count in expected.items():
            x, y = SUMMARY_ROW_CROPS[label]
            row = image.crop((x, y, x + 204, y + 30))
            row = row.resize((row.width * 5, row.height * 5), Image.Resampling.LANCZOS)
            with tempfile.NamedTemporaryFile(suffix=".png") as enlarged:
                row.save(enlarged.name)
                result = subprocess.run(
                    ["tesseract", enlarged.name, "stdout", "--psm", "7"],
                    check=True, capture_output=True, text=True,
                )
            actual = summary_count_from_ocr(result.stdout, label)
            if actual != count:
                raise RuntimeError(
                    f"visible {label} summary showed {actual}, expected {count}: {result.stdout!r}"
                )
            observed[label.lower()] = actual
    return observed


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
        targets = decision_targets(visible_names)
        expected_catalog = expected_decisions(targets)
        evidence["first_page_display_order"] = visible_names
        evidence["decision_targets"] = targets

        click_at(grid_cell_center(window["geometry"], visible_names.index(targets["keep"])))
        selected_first = output / "selected-first.png"
        rows = screenshot_and_ocr(wid, selected_first)
        click_at(decision_button_center(window["geometry"], "KEEP", selected_first))
        check_catalog(database, gallery_id, {Path(targets["keep"]).stem: "KEEP"})

        rows = screenshot_and_ocr(wid, output / "kept-first.png")
        click_at(grid_cell_center(window["geometry"], visible_names.index(targets["reject"])))
        selected_second = output / "selected-second.png"
        rows = screenshot_and_ocr(wid, selected_second)
        click_at(decision_button_center(window["geometry"], "REJECT", selected_second))
        check_catalog(database, gallery_id, expected_catalog)
        check_counts(database, gallery_id, keep=1, reject=1, unrated=11, total=13)

        reject_first = output / "reject-first.png"
        rows = screenshot_and_ocr(wid, reject_first)
        evidence["visible_summary"] = assert_visible_summary(
            reject_first, kept=1, rejected=1, unrated=11, alt=0
        )
        evidence["screenshots"].append(str(output / "reject-first.png"))
        click_at(decision_filter_center(window["geometry"], "KEEP"))
        rows = screenshot_and_ocr(wid, output / "keep-filter.png")
        evidence["screenshots"].append(str(output / "keep-filter.png"))
        visible_after_filter = [row["text"] for row in rows]
        if targets["keep"] not in visible_after_filter:
            raise RuntimeError(f"KEEP filter omitted the confirmed photo {targets['keep']}")
        if targets["reject"] in visible_after_filter:
            raise RuntimeError(f"KEEP filter retained the confirmed REJECT photo {targets['reject']}")
        check_catalog(database, gallery_id, expected_catalog)
        evidence["keep_filter"] = "only the KEEP photo is shown"

        rows = screenshot_and_ocr(wid, output / "keep-filter-before-next.png")
        click_at(decision_filter_center(window["geometry"], "ALL"))
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
        reopened_screenshot = output / "reopened.png"
        reopened_rows = screenshot_and_ocr(reopened_window["window_id"], reopened_screenshot)
        evidence["visible_summary_after_reopen"] = assert_visible_summary(
            reopened_screenshot, kept=1, rejected=1, unrated=11, alt=0
        )
        reopened_geometry = reopened_window["geometry"]
        click_at(decision_filter_center(reopened_geometry, "KEEP"))
        reopened_rows = screenshot_and_ocr(reopened_window["window_id"], output / "reopened-keep-filter.png")
        if targets["keep"] not in [row["text"] for row in reopened_rows]:
            raise RuntimeError(
                f"KEEP decision for {targets['keep']} was not visible after reopening"
            )
        check_catalog(database, gallery_id, expected_catalog)
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
