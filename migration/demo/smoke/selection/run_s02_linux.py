#!/usr/bin/env python3
"""Accept the normal packaged Sprint 02 import and reopen flow on Linux/X11."""

from __future__ import annotations

import argparse
import difflib
import hashlib
import importlib.util
import json
import os
import subprocess
import time
import unicodedata
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
    candidates = [row for row in rows if text_matches(row["text"], text) and row["confidence"] >= 35]
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


def normalize_text(text: str) -> str:
    decomposed = unicodedata.normalize("NFKD", text.casefold())
    unaccented = "".join(character for character in decomposed if not unicodedata.combining(character))
    return unaccented.strip().rstrip("/")


def text_matches(observed: str, expected: str) -> bool:
    observed_normalized = normalize_text(observed)
    expected_normalized = normalize_text(expected)
    return observed_normalized == expected_normalized or difflib.SequenceMatcher(
        None, observed_normalized, expected_normalized
    ).ratio() >= 0.9


def folder_name_matches_ocr(observed: str, expected: str) -> bool:
    if text_matches(observed, expected):
        return True
    # OCR can attach the folder icon or a checkbox-like glyph as a one- or
    # two-character prefix (for example, "Ml Fotos de seleccién"). Retry the
    # phrase after that short visual prefix while preserving the same fuzzy
    # threshold for the actual folder name.
    observed_words = normalize_text(observed).split()
    expected_normalized = normalize_text(expected)
    return bool(
        observed_words
        and len(observed_words[0]) <= 2
        and text_matches(" ".join(observed_words[1:]), expected_normalized)
    )


def choose_folder_button_point(width: int, height: int) -> tuple[int, int]:
    # The first-run call to action is centered in the gallery panel, near the
    # bottom of the maximized 1440x900 runner window.
    return (width // 2 + width // 40, height - max(20, height // 40))


def import_button_point(width: int, height: int) -> tuple[int, int]:
    return (round(width * 0.8), max(1, round(height * 0.04)))


def click_choose_folder(rows: list[dict]) -> None:
    if any(text_matches(row["text"], "Elegir carpeta") for row in rows):
        click_text(rows, "Elegir carpeta")
        return

    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = choose_folder_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def click_import_button(rows: list[dict]) -> None:
    candidates = [
        row for row in rows
        if row["confidence"] >= 35
        and (text_matches(row["text"], "Import") or row["text"].strip().casefold().startswith("importar"))
    ]
    if len(candidates) == 1:
        click_text(rows, candidates[0]["text"])
        return
    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = import_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def click_import_source(rows: list[dict], source_name: str) -> None:
    candidates = [
        row for row in rows
        if row["confidence"] >= 35 and folder_name_matches_ocr(row["text"], source_name)
    ]
    if len(candidates) != 1:
        raise RuntimeError(f"cannot uniquely locate import source {source_name!r}: {candidates}")
    row = candidates[0]
    subprocess.run([
        "xdotool", "mousemove", str(row["x"] + row["width"] // 2),
        str(row["y"] + row["height"] // 2), "click", "1",
    ], check=True)
    time.sleep(0.5)


def estimate_button_point(width: int, height: int) -> tuple[int, int]:
    return (round(width * 0.359), round(height * 0.827))


def click_estimate_button() -> None:
    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = estimate_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def start_import_button_point(width: int, height: int) -> tuple[int, int]:
    return (round(width * 0.357), round(height * 0.88))


def click_start_import_button(rows: list[dict]) -> None:
    candidates = [row for row in rows if row["confidence"] >= 35 and text_matches(row["text"], "Start import")]
    if len(candidates) == 1:
        click_text(rows, candidates[0]["text"])
        return
    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = start_import_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def close_import_button_point(width: int, height: int) -> tuple[int, int]:
    # The completed-import modal keeps its close action in the upper-right.
    return (round(width * 0.657), round(height * 0.161))


def click_close_import(rows: list[dict]) -> None:
    candidates = [row for row in rows if row["confidence"] >= 35 and text_matches(row["text"], "Close")]
    if len(candidates) == 1:
        click_text(rows, candidates[0]["text"])
        return
    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = close_import_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def import_completed_visible(rows: list[dict]) -> bool:
    return any(row["confidence"] >= 35 and text_matches(row["text"], "Import completed") for row in rows)


def wait_for_import_completion(
    database: Path, output: Path, expected_files: int, timeout: float = 90
) -> list[dict]:
    deadline = time.monotonic() + timeout
    latest_rows: list[dict] = []
    latest_count = 0
    while time.monotonic() < deadline:
        latest_rows = screenshot(output)
        if import_completed_visible(latest_rows):
            latest_count, _ = database_snapshot(database)
            if latest_count == expected_files:
                return latest_rows
        time.sleep(0.5)
    raise TimeoutError(
        f"import completion not confirmed: visible={import_completed_visible(latest_rows)}, "
        f"catalog JPEG count={latest_count}, expected={expected_files}; last OCR={latest_rows}"
    )


def wait_for_import_completion_dismissal(output: Path, timeout: float = 10) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if not import_completed_visible(screenshot(output)):
            return True
        time.sleep(0.25)
    return False


def next_page_button_point(width: int, height: int) -> tuple[int, int]:
    # The gallery pager sits above the photo grid, just right of its page label.
    return (round(width * 0.667), round(height * 0.137))


def click_next_page(rows: list[dict]) -> None:
    candidates = [row for row in rows if row["confidence"] >= 35 and row["text"].strip() in {"›", ">"}]
    if len(candidates) == 1:
        click_text(rows, candidates[0]["text"])
        return
    width, height = map(int, subprocess.check_output(["xdotool", "getdisplaygeometry"], text=True).split())
    x, y = next_page_button_point(width, height)
    subprocess.run(["xdotool", "mousemove", str(x), str(y), "click", "1"], check=True)
    time.sleep(0.5)


def estimate_result_visible(rows: list[dict], expected_files: int) -> bool:
    expected = f"Estimate: {expected_files} files"
    return any(row["confidence"] >= 35 and expected in row["text"] for row in rows)


def wait_for_estimate_result(output: Path, expected_files: int, timeout: float = 12) -> list[dict]:
    deadline = time.monotonic() + timeout
    latest_rows: list[dict] = []
    while time.monotonic() < deadline:
        latest_rows = screenshot(output)
        if estimate_result_visible(latest_rows, expected_files):
            return latest_rows
        time.sleep(0.25)
    raise TimeoutError(f"estimate did not report {expected_files} files; last OCR={latest_rows}")


def folder_dialog_select_point(width: int, height: int) -> tuple[int, int]:
    return (max(1, width - 50), max(1, height - 26))


def folder_dialog_project_bookmark_point(width: int, height: int) -> tuple[int, int]:
    return (max(1, round(width * 0.078)), max(1, round(height * 0.184)))


def visible_folder_picker_id() -> str | None:
    result = subprocess.run(
        ["xdotool", "search", "--onlyvisible", "--name", "Choose photo folder"],
        capture_output=True,
        text=True,
    )
    window_ids = result.stdout.split()
    return window_ids[-1] if window_ids else None


def click_folder_picker_select() -> None:
    window_id = visible_folder_picker_id()
    if not window_id:
        raise RuntimeError("folder picker window was not visible after selecting the source folder")
    geometry = subprocess.check_output(["xdotool", "getwindowgeometry", "--shell", window_id], text=True)
    values = dict(line.split("=", 1) for line in geometry.splitlines() if "=" in line)
    x, y = folder_dialog_select_point(int(values["WIDTH"]), int(values["HEIGHT"]))
    subprocess.run(["xdotool", "mousemove", "--window", window_id, str(x), str(y), "click", "1"], check=True)


def click_folder_picker_project_bookmark() -> None:
    window_id = visible_folder_picker_id()
    if not window_id:
        raise RuntimeError("folder picker window was not visible when opening project bookmark")
    geometry = subprocess.check_output(["xdotool", "getwindowgeometry", "--shell", window_id], text=True)
    values = dict(line.split("=", 1) for line in geometry.splitlines() if "=" in line)
    x, y = folder_dialog_project_bookmark_point(int(values["WIDTH"]), int(values["HEIGHT"]))
    subprocess.run(["xdotool", "mousemove", "--window", window_id, str(x), str(y), "click", "1"], check=True)


def wait_for_folder_picker_close(timeout: float = 8) -> bool:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if visible_folder_picker_id() is None:
            return True
        time.sleep(0.25)
    return False


def select_current_folder(output: Path) -> None:
    # The portal populates its footer controls asynchronously after the source
    # row is selected. Wait for it before clicking the lower-right Select action.
    time.sleep(2.5)
    click_folder_picker_select()
    if wait_for_folder_picker_close():
        return

    screenshot(output.with_name("folder-picker-after-select.png"))
    time.sleep(1.0)
    click_folder_picker_select()
    if not wait_for_folder_picker_close():
        raise RuntimeError("GTK Select action did not close the source folder picker")


def wait_for_folder_picker(timeout: float = 15) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = subprocess.run(
            ["xdotool", "search", "--onlyvisible", "--name", "Choose photo folder"],
            capture_output=True,
            text=True,
        )
        window_ids = result.stdout.split()
        if result.returncode == 0 and window_ids:
            window_id = window_ids[-1]
            subprocess.run(["xdotool", "windowactivate", "--sync", window_id], check=True)
            subprocess.run(["xdotool", "windowfocus", "--sync", window_id], check=True)
            # The portal window appears before its location/search widgets are
            # ready to receive shortcuts on hosted Xvfb.
            time.sleep(1.5)
            return window_id
        time.sleep(0.25)
    raise TimeoutError("GTK folder picker did not become visible")


def location_entry_visible(rows: list[dict]) -> bool:
    visible_text = normalize_text(" ".join(row["text"] for row in rows))
    return "home" in visible_text and "other locations" in visible_text and "no results found" not in visible_text


def folder_list_row_visible(rows: list[dict], folder_name: str) -> bool:
    # Autocomplete suggestions sit above the file list (around y=115) and can
    # look like a selectable directory even though the chooser is still in its
    # location entry. Only accept a matching row below the list header.
    return any(row["y"] >= 130 and folder_name_matches_ocr(row["text"], folder_name) for row in rows)


def source_folder_navigation_segments(source: Path) -> tuple[str, str]:
    return (source.parent.parent.name, source.parent.name)


def wait_for_folder_list_row(folder_name: str, output: Path, timeout: float = 12) -> list[dict]:
    deadline = time.monotonic() + timeout
    latest_rows: list[dict] = []
    while time.monotonic() < deadline:
        latest_rows = screenshot(output)
        if folder_list_row_visible(latest_rows, folder_name):
            return latest_rows
        time.sleep(0.25)
    raise TimeoutError(f"GTK folder list did not show {folder_name!r}; last OCR={latest_rows}")


def click_folder_list_row(rows: list[dict], folder_name: str, *, enter: bool = False) -> None:
    # Ignore same-named sidebar/bookmark/autocomplete text. Only click a row in
    # the actual file list below the header.
    candidates = [row for row in rows if row["y"] >= 130 and folder_name_matches_ocr(row["text"], folder_name)]
    if len(candidates) != 1:
        raise RuntimeError(f"cannot uniquely locate folder list row {folder_name!r}: {candidates}")
    row = candidates[0]
    command = [
        "xdotool", "mousemove", str(row["x"] + row["width"] // 2),
        str(row["y"] + row["height"] // 2), "click",
    ]
    if enter:
        command.extend(["--repeat", "2", "--delay", "150"])
    command.append("1")
    subprocess.run(command, check=True)
    time.sleep(0.5)


def open_picker_location_entry(output: Path) -> None:
    latest_rows: list[dict] = []
    for attempt in range(3):
        subprocess.run(["xdotool", "key", "ctrl+l"], check=True)
        time.sleep(0.25)
        latest_rows = screenshot(output.with_name(f"folder-picker-location-entry-{attempt + 1}.png"))
        if location_entry_visible(latest_rows):
            return
        # A late portal can initially treat the shortcut as search input.
        # Close that state and explicitly retry after the window has focus.
        subprocess.run(["xdotool", "key", "Escape"], check=True)
        time.sleep(0.25)
    raise RuntimeError(f"GTK location entry did not open; last OCR={latest_rows}")


def open_source_picker(source: Path, output: Path) -> None:
    rows = screenshot(output)
    click_choose_folder(rows)
    wait_for_folder_picker()
    # The hosted GTK location entry autocompletes this path but does not commit
    # navigation reliably. Start from the repository bookmark and traverse the
    # real directory rows so the source selection is an actual folder row.
    screenshot(output.with_name("folder-picker-bookmarks.png"))
    click_folder_picker_project_bookmark()
    time.sleep(0.5)
    wait_for_folder_list_row(source_folder_navigation_segments(source)[0], output.with_name("folder-picker-repository-root.png"))
    segments = source_folder_navigation_segments(source)
    for index, folder_name in enumerate(segments, start=1):
        rows = wait_for_folder_list_row(folder_name, output.with_name(f"folder-picker-list-{index}.png"))
        click_folder_list_row(rows, folder_name, enter=True)
    rows = wait_for_folder_list_row(source.name, output.with_name("folder-picker-source-row.png"))
    click_folder_list_row(rows, source.name)
    time.sleep(1.0)
    # Keep a screenshot of the selected row. Tesseract can miss its label once
    # the blue GTK selection highlight changes the text contrast; the Select
    # action closing the portal below is the reliable selection confirmation.
    screenshot(output.with_name("folder-picker-selected.png"))
    select_current_folder(output)
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
    database = xdg / "photo-culler" / "desktop" / "catalog.sqlite3"
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
        click_import_button(rows)
        rows = wait_text("Available", output / "import-dialog.png")
        click_import_source(rows, source.name)
        screenshot(output / "import-estimate-ready.png")
        click_estimate_button()
        wait_for_estimate_result(output / "estimated.png", expected_files=13)
        click_start_import_button(rows)
        rows = wait_for_import_completion(database, output / "import-complete.png", expected_files=13)
        click_close_import(rows)
        if not wait_for_import_completion_dismissal(output / "import-after-close.png"):
            raise RuntimeError("completed import modal remained visible after clicking Close")

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
        click_next_page(rows)
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
            click_next_page(rows)
            rows = wait_text("photo-13.jpg", output / "reopened-second-page.png")
        if database_snapshot(database) != (13, ids_before):
            raise RuntimeError("reopened catalog IDs or counts differ")
        assert_source_hashes(source, manifest)

        rows = screenshot(output / "reopened-import.png")
        click_text(rows, "Import")
        rows = wait_text("Start import", output / "reimport-dialog.png")
        click_text(rows, "Start import")
        rows = wait_for_import_completion(database, output / "reimport-complete.png", expected_files=13)
        click_close_import(rows)
        if not wait_for_import_completion_dismissal(output / "reimport-after-close.png"):
            raise RuntimeError("completed reimport modal remained visible after clicking Close")
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
