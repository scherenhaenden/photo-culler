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
import unicodedata
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


def second_page_last_photo(database: Path, gallery_id: str) -> tuple[str, str]:
    with sqlite3.connect(database) as connection:
        row = connection.execute(
            "SELECT photo_id, stem_name FROM photos WHERE gallery_id = ? "
            "ORDER BY photo_id ASC LIMIT 1 OFFSET 12",
            (gallery_id,),
        ).fetchone()
    if not row:
        raise RuntimeError("fixture catalog has no second page")
    return row[0], row[1]


def second_page_last_name(database: Path, gallery_id: str) -> str:
    return second_page_last_photo(database, gallery_id)[1] + ".jpg"


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


def photo_id_for_stem(database: Path, gallery_id: str, stem_name: str) -> str:
    with sqlite3.connect(database) as connection:
        rows = connection.execute(
            "SELECT photo_id FROM photos WHERE gallery_id = ? AND stem_name = ?",
            (gallery_id, stem_name),
        ).fetchall()
    if len(rows) != 1:
        raise RuntimeError(f"catalog did not uniquely resolve photo {stem_name!r}: {rows}")
    return rows[0][0]


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


def enlarged_crop_ocr(screenshot: Path, box: tuple[int, int, int, int], scale: int = 5) -> str:
    from PIL import Image

    with Image.open(screenshot) as image:
        x, y, width, height = box
        crop = image.crop((x, y, x + width, y + height))
        enlarged = crop.resize(
            (crop.width * scale, crop.height * scale), Image.Resampling.LANCZOS
        )
        with tempfile.NamedTemporaryFile(suffix=".png") as temporary:
            enlarged.save(temporary.name)
            result = subprocess.run(
                ["tesseract", temporary.name, "stdout", "--psm", "7"],
                check=True, capture_output=True, text=True,
            )
    return result.stdout


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
    expected = {"KEPT": kept, "ALT": alt, "REJECTED": rejected, "UNRATED": unrated}
    observed = {}
    for label, count in expected.items():
        x, y = SUMMARY_ROW_CROPS[label]
        text = enlarged_crop_ocr(screenshot, (x, y, 204, 30))
        actual = summary_count_from_ocr(text, label)
        if actual != count:
            raise RuntimeError(
                f"visible {label} summary showed {actual}, expected {count}: {text!r}"
            )
        observed[label.lower()] = actual
    return observed


def visible_page_count_from_ocr(text: str) -> int:
    match = re.search(r"\b([0-9]+)\s+ON\s+THIS\s+PAGE\b", text, re.IGNORECASE)
    if not match:
        raise RuntimeError(f"could not read visible page count from enlarged OCR: {text!r}")
    return int(match.group(1))


def photo_counter_from_ocr(text: str) -> tuple[int, int]:
    compact = re.sub(r"[^a-z0-9]", "", text.lower())
    match = re.search(r"foto(\d+)de(\d+)", compact)
    if not match:
        raise RuntimeError(f"could not read visible photo counter from enlarged OCR: {text!r}")
    return int(match.group(1)), int(match.group(2))


def normalized_photo_id_ocr(text: str) -> str:
    normalized = text.lower().replace("o", "0").replace("i", "1").replace("l", "1")
    return re.sub(r"[^0-9a-f]", "", normalized)


def _bounded_edit_distance(left: str, right: str, limit: int) -> int:
    """Return Levenshtein distance, stopping once it exceeds a small limit."""
    if abs(len(left) - len(right)) > limit:
        return limit + 1
    previous = list(range(len(right) + 1))
    for left_index, left_char in enumerate(left, start=1):
        current = [left_index]
        row_minimum = left_index
        for right_index, right_char in enumerate(right, start=1):
            value = min(
                current[right_index - 1] + 1,
                previous[right_index] + 1,
                previous[right_index - 1] + (left_char != right_char),
            )
            current.append(value)
            row_minimum = min(row_minimum, value)
        if row_minimum > limit:
            return limit + 1
        previous = current
    return previous[-1]


def photo_id_prefix_matches_ocr(expected_photo_id: str, text: str,
                                prefix_length: int = 24, max_edits: int = 2) -> bool:
    """Match an inspector ID prefix despite a few localized OCR glyph errors."""
    expected_prefix = expected_photo_id[:prefix_length].lower()
    observed = normalized_photo_id_ocr(text)
    if not expected_prefix or not observed:
        return False
    # OCR can omit or invent a glyph. Compare short windows at the start of the
    # inspector row so errors elsewhere in the screenshot cannot create a hit.
    # Keep at least all but one expected glyph in the compared prefix. Allowing
    # a two-character-short candidate can make a different ID match by simply
    # discarding the distinguishing end of the prefix.
    minimum = max(1, len(expected_prefix) - 1)
    maximum = min(len(observed), len(expected_prefix) + 1)
    return any(
        _bounded_edit_distance(expected_prefix, observed[:length], max_edits) <= max_edits
        for length in range(minimum, maximum + 1)
    )


def normalized_ui_text(text: str) -> str:
    folded = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode().lower()
    return re.sub(r"[^a-z0-9%]+", " ", folded).strip()


def visible_text_center(rows: list[dict], phrase: str) -> tuple[int, int]:
    """Find a visible UI label in OCR rows and return its center."""
    expected = normalized_ui_text(phrase)
    matches = [row for row in rows if expected in normalized_ui_text(row["text"])]
    if len(matches) != 1:
        raise RuntimeError(f"expected one visible {phrase!r} label, found {matches}")
    row = matches[0]
    return row["x"] + row["width"] // 2, row["y"] + row["height"] // 2


def inspector_button_center(screenshot: Path, geometry: dict) -> tuple[int, int]:
    """Read the low-contrast inspector action from a focused header crop."""
    width, height = geometry["width"], geometry["height"]
    if width < 1280 or height < 700:
        raise RuntimeError(f"inspector acceptance needs the 1440x900 layout: {geometry}")
    box = (round(width * 0.5), round(height * 0.04), round(width * 0.15), round(height * 0.08))
    label = normalized_ui_text(enlarged_crop_ocr(screenshot, box))
    # The hosted 1440x900 renderer consistently OCRs only the first word from
    # this low-contrast Button. The crop is bounded to that control, so this
    # label uniquely anchors the click without requiring its second word.
    if "inspeccionar" not in label:
        raise RuntimeError(f"inspector action was not visible in its header crop: {label!r}")
    x, y, crop_width, crop_height = box
    return geometry["x"] + x + crop_width // 3, geometry["y"] + y + crop_height // 2


def assert_inspector_overlay(rows: list[dict], photo_name: str, decision: str) -> dict[str, str]:
    text = normalized_ui_text(" ".join(row["text"] for row in rows))
    expected_name = normalized_ui_text(photo_name)
    if "inspeccion de foco" not in text or expected_name not in text:
        raise RuntimeError(
            f"inspector did not identify {photo_name!r} in its header: {rows}"
        )
    expected_decision = normalized_ui_text(decision)
    if expected_decision not in text:
        raise RuntimeError(
            f"inspector did not show the {decision!r} decision for {photo_name!r}: {rows}"
        )
    if "zoom 100%" not in text or "restablecer" not in text or "recorte" not in text:
        raise RuntimeError(f"inspector focus controls were not visible at 100%: {rows}")
    return {"photo_name": photo_name, "decision": decision, "zoom": "100%"}


def assert_keep_filter(screenshot: Path, rows: list[dict], expected_photo_id: str) -> dict[str, object]:
    # Filenames are not rendered on these compact thumbnail cards. Confirm the
    # single filtered page count and match the inspector's visible ID prefix to
    # the database identity selected for KEEP.
    page_text = enlarged_crop_ocr(screenshot, (450, 58, 145, 24))
    visible_count = visible_page_count_from_ocr(page_text)
    if visible_count != 1:
        raise RuntimeError(f"KEEP filter showed {visible_count} cards, expected exactly one")
    expected_prefix = expected_photo_id[:24].lower()
    matching_ids = [
        normalized_photo_id_ocr(row["text"])
        for row in rows
        if row["x"] >= 1000 and row["y"] < 150
        and photo_id_prefix_matches_ocr(expected_photo_id, row["text"])
    ]
    if not matching_ids:
        raise RuntimeError(
            f"KEEP filter inspector did not show the expected photo ID prefix "
            f"{expected_prefix}: {rows}"
        )
    return {
        "visible_photo_count": visible_count,
        "photo_id_prefix": expected_prefix,
        "matching_inspector_id_rows": len(matching_ids),
    }


def assert_last_photo_visible(screenshot: Path, rows: list[dict],
                              expected_photo_id: str, total_photos: int) -> dict[str, object]:
    counter_text = enlarged_crop_ocr(screenshot, (850, 92, 125, 24))
    current, total = photo_counter_from_ocr(counter_text)
    if (current, total) != (total_photos, total_photos):
        raise RuntimeError(
            f"Next Photo navigation ended at {current} of {total}, "
            f"expected {total_photos} of {total_photos}"
        )
    expected_prefix = expected_photo_id[:24].lower()
    matching_ids = [
        normalized_photo_id_ocr(row["text"])
        for row in rows
        if row["x"] >= 1000 and row["y"] < 150
        and photo_id_prefix_matches_ocr(expected_photo_id, row["text"])
    ]
    if not matching_ids:
        raise RuntimeError(
            f"last-photo inspector did not show the expected photo ID prefix "
            f"{expected_prefix}: {rows}"
        )
    return {
        "visible_photo_index": current,
        "visible_photo_total": total,
        "photo_id_prefix": expected_prefix,
        "matching_inspector_id_rows": len(matching_ids),
    }


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
        keep_photo_id = photo_id_for_stem(database, gallery_id, Path(targets["keep"]).stem)
        reject_photo_id = photo_id_for_stem(database, gallery_id, Path(targets["reject"]).stem)
        evidence["first_page_display_order"] = visible_names
        evidence["decision_targets"] = targets
        evidence["decision_target_ids"] = {"keep": keep_photo_id, "reject": reject_photo_id}

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

        click_at(inspector_button_center(reject_first, window["geometry"]))
        inspector_path = output / "inspector-reject.png"
        inspector_rows = screenshot_and_ocr(wid, inspector_path)
        evidence["screenshots"].append(str(inspector_path))
        evidence["inspector"] = assert_inspector_overlay(
            inspector_rows, targets["reject"], "REJECTED"
        )
        close_x, close_y = visible_text_center(inspector_rows, "Volver a galería")
        click_at((origin_x + close_x, origin_y + close_y))
        returned = output / "inspector-return.png"
        returned_rows = screenshot_and_ocr(wid, returned)
        evidence["screenshots"].append(str(returned))
        returned_count = visible_page_count_from_ocr(
            enlarged_crop_ocr(returned, (450, 58, 145, 24))
        )
        if returned_count != 13:
            raise RuntimeError(
                f"returning from inspector showed {returned_count} library photos, expected 13"
            )
        evidence["inspector_return"] = {
            "visible_photo_count": returned_count,
            "decision_preserved": True,
        }
        click_at(decision_filter_center(window["geometry"], "KEEP"))
        keep_filter = output / "keep-filter.png"
        rows = screenshot_and_ocr(wid, keep_filter)
        evidence["screenshots"].append(str(keep_filter))
        evidence["keep_filter"] = assert_keep_filter(keep_filter, rows, keep_photo_id)
        check_catalog(database, gallery_id, expected_catalog)
        check_counts(database, gallery_id, keep=1, reject=1, unrated=11, total=13)

        rows = screenshot_and_ocr(wid, output / "keep-filter-before-next.png")
        click_at(decision_filter_center(window["geometry"], "ALL"))
        for _ in range(12):
            click_next_photo(wid, origin_x, origin_y, width, height)
        page_two = output / "page-two.png"
        rows = screenshot_and_ocr(wid, page_two)
        evidence["screenshots"].append(str(page_two))
        last_photo_id, _last_stem_name = second_page_last_photo(database, gallery_id)
        evidence["pagination"] = assert_last_photo_visible(
            page_two, rows, last_photo_id, total_photos=len(ids)
        )
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
        reopened_filter = output / "reopened-keep-filter.png"
        reopened_rows = screenshot_and_ocr(reopened_window["window_id"], reopened_filter)
        evidence["keep_filter_after_reopen"] = assert_keep_filter(
            reopened_filter, reopened_rows, keep_photo_id
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
