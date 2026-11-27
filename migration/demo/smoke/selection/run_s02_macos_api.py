#!/usr/bin/env python3
"""Exercise Sprint 02 package persistence/import APIs on macOS arm64.

This runner deliberately avoids UI scripting permissions. It verifies the
packaged server's import, catalog pagination and reimport contracts, then
opens and reopens the packaged desktop against that same catalog and records
window screenshots for visual review.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import select
import sqlite3
import subprocess
import time
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
NATIVE_GUI = SCRIPT_DIR.parent / "native-gui" / "run.py"
SPEC = importlib.util.spec_from_file_location("native_gui_smoke", NATIVE_GUI)
assert SPEC and SPEC.loader
native = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(native)


def request(origin: str, token: str, route: str, body: dict | None = None) -> dict:
    payload = None if body is None else json.dumps(body).encode()
    req = urllib.request.Request(
        origin + route,
        data=payload,
        headers={
            "Authorization": f"Bearer {token}",
            "Accept": "application/json",
            "Origin": origin,
            **({"Content-Type": "application/json"} if payload is not None else {}),
        },
        method="POST" if payload is not None else "GET",
    )
    try:
        with urllib.request.urlopen(req, timeout=10) as response:
            return json.loads(response.read())
    except urllib.error.HTTPError as error:
        raise RuntimeError(f"{req.method} {route} returned HTTP {error.code}: {error.read()!r}") from error


def start_server(server: Path, data_dir: Path, source: Path, evidence: Path, name: str):
    log = (evidence / f"server-{name}.log").open("wb")
    process = subprocess.Popen(
        [str(server), "--port", "0", "--data-dir", str(data_dir), "--authorized-root", str(source), "--owned"],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=log,
        cwd=server.parent,
    )
    assert process.stdout is not None
    if not select.select([process.stdout], [], [], 30)[0]:
        process.terminate()
        process.wait(timeout=10)
        log.close()
        raise TimeoutError(f"packaged server did not emit its ready event; see {evidence / f'server-{name}.log'}")
    line = process.stdout.readline()
    if not line:
        code = process.wait(timeout=5)
        log.close()
        raise RuntimeError(f"packaged server exited before ready with code {code}; see {evidence / f'server-{name}.log'}")
    try:
        ready = json.loads(line)
        if (
            ready.get("event") != "ready"
            or not isinstance(ready.get("origin"), str)
            or not ready["origin"].startswith("http://127.0.0.1:")
            or not isinstance(ready.get("session_token"), str)
            or not ready["session_token"]
        ):
            raise RuntimeError(f"packaged server returned invalid ready event: {ready!r}")
    except BaseException:
        if process.poll() is None:
            if process.stdin:
                process.stdin.close()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.terminate()
                process.wait(timeout=10)
        log.close()
        raise
    return process, ready["origin"], ready["session_token"], log


def stop_server(process, log) -> None:
    if process.poll() is None:
        if process.stdin:
            process.stdin.close()
        process.wait(timeout=20)
    log.close()


def catalog_rows(database: Path) -> list[tuple[str, str, str, str]]:
    with sqlite3.connect(database) as connection:
        return connection.execute(
            "SELECT p.stem_name, p.photo_id, f.source_relative_path, f.full_hash "
            "FROM photos p JOIN files f ON f.photo_id = p.id "
            "WHERE lower(f.role) = 'jpeg' ORDER BY p.stem_name"
        ).fetchall()


def job_snapshot(database: Path) -> list[tuple[str, str, int, int, int]]:
    with sqlite3.connect(database) as connection:
        return connection.execute(
            "SELECT id, state, discovered, imported, issues FROM import_jobs ORDER BY created_at, id"
        ).fetchall()


def import_source(origin: str, token: str, database: Path, gallery: str, source_id: str) -> str:
    estimate = request(origin, token, "/api/v2/import-estimates", {
        "source_id": source_id, "recursive": True, "exclude_patterns": [],
    })
    if estimate.get("total_files") != 13 or estimate.get("logical_photos") != 13:
        raise RuntimeError(f"expected 13 JPEGs in estimate, got {estimate!r}")
    receipt = request(origin, token, f"/api/v2/galleries/{gallery}/imports", {
        "source_id": source_id, "recursive": True, "exclude_patterns": [],
    })
    job_id = receipt["job_id"]
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        job = request(origin, token, f"/api/v2/import-jobs/{job_id}")
        if job.get("state") == "completed":
            if job.get("discovered") != 13 or job.get("imported") != 13 or job.get("issues") != 0:
                raise RuntimeError(f"import completed with unexpected counts: {job!r}")
            return job_id
        if job.get("state") in {"failed", "cancelled"}:
            raise RuntimeError(f"import did not complete: {job!r}")
        time.sleep(0.5)
    raise TimeoutError(f"import job {job_id} did not complete")


def catalog_pages(origin: str, token: str, gallery: str) -> tuple[list[dict], list[dict]]:
    filter_value = {"gallery_id": gallery, "decision": None}
    route = f"/api/v1/galleries/{gallery}/photos?limit=12"
    first = request(origin, token, route)
    if len(first["items"]) != 12 or not first.get("next_cursor"):
        raise RuntimeError(f"expected 12 entries and a next cursor, got {first!r}")
    cursor = first["next_cursor"]
    if cursor.get("filter") != filter_value:
        raise RuntimeError(f"catalog cursor filter differs from the requested gallery: {cursor!r}")
    query = urllib.parse.urlencode({"limit": "12", "cursor": json.dumps(cursor, separators=(",", ":"))})
    second = request(origin, token, f"/api/v1/galleries/{gallery}/photos?{query}")
    if len(second["items"]) != 1 or second.get("next_cursor") is not None:
        raise RuntimeError(f"expected one entry on the second catalog page, got {second!r}")
    if first["items"][-1]["id"] >= second["items"][0]["id"]:
        raise RuntimeError("catalog cursor did not advance to the second page")
    return first["items"], second["items"]


def wait_for_desktop_ready(status_path: Path, expected_photo_id: str, timeout: float = 30) -> str:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if status_path.exists():
            events = [line.split("\t") for line in status_path.read_text(errors="replace").splitlines()]
            errors = [event for event in events if event and event[0] == "gallery-error"]
            if errors:
                raise RuntimeError(f"packaged desktop reported a gallery error: {errors[-1]!r}")
            ready = [event for event in events if len(event) >= 3 and event[0] == "gallery-ready"]
            if ready:
                photo_id = ready[-1][1]
                if photo_id != expected_photo_id:
                    raise RuntimeError(
                        f"desktop profile loaded photo {photo_id!r}, expected {expected_photo_id!r}"
                    )
                return photo_id
        time.sleep(0.25)
    status = status_path.read_text(errors="replace") if status_path.exists() else "<no status events>"
    raise TimeoutError(f"desktop did not report gallery-ready for {expected_photo_id!r}: {status}")


def launch_desktop(desktop: Path, home: Path, evidence: Path, label: str, expected_photo_id: str):
    log = (evidence / f"desktop-{label}.log").open("wb")
    status_path = evidence / f"desktop-{label}.status.tsv"
    status_path.unlink(missing_ok=True)
    process = subprocess.Popen(
        [str(desktop)],
        stdout=log,
        stderr=subprocess.STDOUT,
        cwd=desktop.parent,
        env={**os.environ, "HOME": str(home), "PHOTO_CULLER_SMOKE_STATUS_FILE": str(status_path)},
    )
    time.sleep(3)
    if process.poll() is not None:
        log.close()
        raise RuntimeError(f"packaged desktop exited during {label} startup with {process.returncode}")
    screenshot = evidence / f"desktop-{label}.png"
    try:
        ready_photo_id = wait_for_desktop_ready(status_path, expected_photo_id)
        native.capture("macos", process.pid, screenshot)
    except BaseException:
        process.terminate()
        process.wait(timeout=10)
        log.close()
        raise
    return process, log, ready_photo_id


def close_desktop(process, log, evidence: Path, label: str) -> None:
    try:
        native.close_window("macos", process.pid, None, evidence)
        process.wait(timeout=20)
    finally:
        if process.poll() is None:
            process.terminate()
            process.wait(timeout=10)
        log.close()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--fixture", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    desktop, server = args.desktop.resolve(), args.server.resolve()
    fixture, output = args.fixture.resolve(), args.output.resolve()
    source = fixture / "Fotos de selección"
    output.mkdir(parents=True, exist_ok=True)
    home = output / "home"
    profile_dir = home / "Library" / "Application Support" / "Photo Culler" / "Desktop"
    profile_dir.mkdir(parents=True, exist_ok=True)
    data_dir = profile_dir
    database = data_dir / "catalog.sqlite3"
    manifest_path = fixture / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    expected_hashes = {row["filename"]: row["sha256"] for row in manifest["photos"]}

    process, origin, token, log = start_server(server, data_dir, source, output, "first")
    desktop_process = None
    desktop_log = None
    try:
        with sqlite3.connect(database) as connection:
            gallery = connection.execute("SELECT id FROM galleries ORDER BY created_at, id LIMIT 1").fetchone()[0]
        sources = request(origin, token, f"/api/v2/galleries/{gallery}/sources")["items"]
        if len(sources) != 1 or sources[0]["status"] != "online":
            raise RuntimeError(f"expected one registered online source, got {sources!r}")
        source_id = sources[0]["id"]
        (profile_dir / "desktop.json").write_text(json.dumps({
            "schema_version": 1,
            "authorized_root": str(source),
            "gallery_id": gallery,
        }, indent=2) + "\n")
        first_job = import_source(origin, token, database, gallery, source_id)
        first_page, second_page = catalog_pages(origin, token, gallery)
        before = catalog_rows(database)
        if len(before) != 13:
            raise RuntimeError(f"catalog has {len(before)} JPEGs, expected 13")
        actual_hashes = {stem + ".jpg": digest for stem, _id, _path, digest in before}
        if actual_hashes != expected_hashes:
            raise RuntimeError("catalog SHA-256 values differ from the fixture manifest")
        stop_server(process, log)

        process, origin, token, log = start_server(server, data_dir, source, output, "reopen")
        reopened_page, reopened_second = catalog_pages(origin, token, gallery)
        if reopened_page != first_page or reopened_second != second_page or catalog_rows(database) != before:
            raise RuntimeError("catalog entries or IDs changed after packaged server restart")
        jobs_before = {row[0] for row in job_snapshot(database)}
        second_job = import_source(origin, token, database, gallery, source_id)
        if second_job in jobs_before or catalog_rows(database) != before:
            raise RuntimeError("reimport duplicated catalog rows or photo IDs")
        if {stem + ".jpg": digest for stem, _id, _path, digest in catalog_rows(database)} != expected_hashes:
            raise RuntimeError("catalog SHA-256 values changed after reimport")
        stop_server(process, log)

        expected_first_photo_id = first_page[0]["id"]
        desktop_process, desktop_log, first_ready_photo_id = launch_desktop(
            desktop, home, output, "first", expected_first_photo_id
        )
        close_desktop(desktop_process, desktop_log, output, "first")
        desktop_process = desktop_log = None
        if catalog_rows(database) != before:
            raise RuntimeError("desktop close changed catalog rows or IDs")
        desktop_process, desktop_log, reopen_ready_photo_id = launch_desktop(
            desktop, home, output, "reopen", expected_first_photo_id
        )
        close_desktop(desktop_process, desktop_log, output, "reopen")
        desktop_process = desktop_log = None
        if catalog_rows(database) != before:
            raise RuntimeError("desktop reopen changed catalog rows or IDs")
        profile = json.loads((profile_dir / "desktop.json").read_text())
        if profile.get("gallery_id") != gallery or profile.get("authorized_root") != str(source):
            raise RuntimeError(f"desktop profile does not retain the expected gallery/source: {profile!r}")

        evidence = {
            "status": "PASS",
            "platform": "macOS arm64 packaged server and desktop",
            "fixture_photo_count": 13,
            "first_import_job": first_job,
            "reimport_job": second_job,
            "catalog_rows_after_reopen_and_reimport": len(before),
            "second_page_photo_id": second_page[0]["id"],
            "fixture_manifest_sha256": hashlib.sha256(manifest_path.read_bytes()).hexdigest(),
            "catalog_sha256_verified": True,
            "desktop_launched_and_closed_twice": True,
            "desktop_profile_gallery_id": profile["gallery_id"],
            "desktop_gallery_ready_photo_ids": [first_ready_photo_id, reopen_ready_photo_id],
            "desktop_window_screenshots": ["desktop-first.png", "desktop-reopen.png"],
            "ui_interaction_limit": "Selector clicks and visual control assertions are not automated on macOS; screenshots are retained for review.",
        }
        (output / "s02-macos-result.json").write_text(json.dumps(evidence, indent=2) + "\n")
        (output / "s02-macos-result.txt").write_text(
            "PASS: packaged macOS arm64 server imported 13 JPEGs, returned the second catalog page, "
            "reopened the same catalog, reimported without changing IDs/counts, and verified fixture hashes.\n"
            "Packaged desktop launched/closed/reopened twice against that catalog; screenshots are retained.\n"
            "UI selector clicks and visual control assertions remain a manual review item.\n"
        )
    finally:
        if desktop_process is not None:
            if desktop_process.poll() is None:
                desktop_process.terminate()
                desktop_process.wait(timeout=10)
            if desktop_log is not None:
                desktop_log.close()
        if process.poll() is None:
            stop_server(process, log)


if __name__ == "__main__":
    main()
