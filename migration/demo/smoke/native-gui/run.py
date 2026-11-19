#!/usr/bin/env python3
"""Run the staged Rust/Slint desktop on a hosted GUI runner and save evidence."""

import argparse
import hashlib
import json
import os
import platform
import queue
import shutil
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
from datetime import datetime, timezone
from pathlib import Path


def call(args, timeout=15, check=True):
    result = subprocess.run(args, check=False, capture_output=True, text=True, timeout=timeout)
    if check and result.returncode:
        raise RuntimeError(
            f"command {args!r} exited with {result.returncode}; "
            f"stdout={result.stdout!r}; stderr={result.stderr!r}"
        )
    return result


def events(path):
    if not path.exists():
        return []
    return [line.split("\t") for line in path.read_text(errors="replace").splitlines() if line]


def ready(process, status):
    deadline = time.monotonic() + 45
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise RuntimeError(f"desktop exited early ({process.returncode})")
        rows = events(status)
        error = next((r for r in rows if r[0] == "gallery-error"), None)
        if error:
            raise RuntimeError("gallery error: " + " ".join(error[1:]))
        row = next((r for r in reversed(rows) if r[0] == "gallery-ready"), None)
        if row:
            if len(row) < 5 or row[1:3] != ["demo-001", "demo-001.jpg"]:
                raise RuntimeError(f"wrong rendered photo: {row}")
            server = next((r for r in rows if r[0] == "server-ready"), None)
            if not server or len(server) < 3 or server[1] != row[3] or server[2] != row[4]:
                raise RuntimeError(f"server and rendered-gallery events disagree: {server}, {row}")
            return row[3], int(row[4]) if row[4].isdigit() else None
        time.sleep(.1)
    raise TimeoutError("Slint did not report the visible demo gallery")


def json_get(url):
    with urllib.request.urlopen(url, timeout=3) as response:
        if response.status != 200:
            raise RuntimeError(f"GET {url} returned {response.status}")
        return json.loads(response.read())


def probe(origin):
    health = json_get(origin + "/api/v1/health")
    gallery = json_get(origin + "/api/v1/galleries/demo/photos")
    with urllib.request.urlopen(origin + "/api/v1/photos/demo-001/thumbnail/800", timeout=3) as response:
        data = response.read()
        if response.status != 200 or not response.headers.get("Content-Type", "").lower().startswith("image/jpeg"):
            raise RuntimeError("demo thumbnail is not image/jpeg")
    if health.get("status") != "ready" or not any(x.get("photo_id") == "demo-001" for x in gallery.get("items", [])):
        raise RuntimeError("health/gallery response did not satisfy S01")
    if not data.startswith(b"\xff\xd8\xff"):
        raise RuntimeError("thumbnail is not a JPEG")
    return f"health=ready; gallery=demo-001.jpg; thumbnail=image/jpeg ({len(data)} bytes)"


def released(origin):
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline:
        try:
            json_get(origin + "/api/v1/health")
        except (OSError, urllib.error.URLError):
            return True
        time.sleep(.1)
    return False


def alive(pid):
    if not pid:
        return False
    if os.name == "nt":
        return subprocess.run(["powershell", "-NoProfile", "-Command",
            f"if (Get-Process -Id {pid} -ErrorAction SilentlyContinue) {{ exit 0 }} else {{ exit 1 }}"],
            capture_output=True, timeout=10).returncode == 0
    try:
        os.kill(pid, 0)
        return True
    except (ProcessLookupError, PermissionError) as e:
        return isinstance(e, PermissionError)


def stop_pid(pid):
    if alive(pid):
        subprocess.run(["taskkill", "/PID", str(pid), "/T", "/F"] if os.name == "nt" else ["kill", "-KILL", str(pid)],
                       check=False, capture_output=True, timeout=10)


def capture(platform_name, pid, output):
    if platform_name == "linux":
        by_pid = call(["xdotool", "search", "--onlyvisible", "--pid", str(pid)], check=False)
        all_visible = call(["xdotool", "search", "--onlyvisible", "--name", ".*"], check=False)
        windows = []
        for window_id in all_visible.stdout.split():
            title = call(["xdotool", "getwindowname", window_id], check=False).stdout.strip()
            window_pid_text = call(["xdotool", "getwindowpid", window_id], check=False).stdout.strip()
            geometry_text = call(["xdotool", "getwindowgeometry", "--shell", window_id], check=False).stdout
            geometry = {}
            for line in geometry_text.splitlines():
                key, separator, value = line.partition("=")
                if separator and value.lstrip("-").isdigit():
                    geometry[key.lower()] = int(value)
            windows.append({
                "window_id": window_id,
                "title": title,
                "pid": int(window_pid_text) if window_pid_text.isdigit() else None,
                "geometry": geometry,
            })
        xwininfo = call(["xwininfo", "-root", "-tree"], check=False)
        debug_lines = [
            f"DISPLAY={os.environ.get('DISPLAY', '')}",
            f"WAYLAND_DISPLAY={os.environ.get('WAYLAND_DISPLAY', '')}",
            f"WAYLAND_SOCKET={os.environ.get('WAYLAND_SOCKET', '')}",
            f"desktop_pid={pid}",
            f"pid_search={by_pid.stdout.strip()!r}",
            f"visible_search={all_visible.stdout.strip()!r}",
            f"visible_windows={windows!r}",
            f"xwininfo_exit={xwininfo.returncode}",
            xwininfo.stdout,
            xwininfo.stderr,
        ]
        (output.parent / "linux-window-debug.txt").write_text("\n".join(debug_lines))
        title_candidates = [window for window in windows
                            if "photo culler" in window["title"].casefold()]
        pid_candidates = [window for window in title_candidates if window["pid"] == pid]
        candidates = pid_candidates or title_candidates
        candidates = [window for window in candidates
                      if window["geometry"].get("width", 0) >= 1000
                      and window["geometry"].get("height", 0) >= 650]
        if not candidates:
            raise RuntimeError(
                f"no visible X11 Photo Culler window for desktop PID {pid}; "
                f"pid-search={by_pid.stdout.strip()!r}; visible-windows={windows!r}"
            )
        wid = max(candidates, key=lambda window: window["geometry"].get("width", 0)
                  * window["geometry"].get("height", 0))
        window_id = wid["window_id"]
        call(["xdotool", "windowactivate", "--sync", window_id])
        time.sleep(.75)
        call(["import", "-window", window_id, str(output)], timeout=20)
        return wid
    if platform_name == "macos":
        script = f'''tell application "System Events"
set p to first process whose unix id is {pid}
set frontmost of p to true
end tell'''
        call(["osascript", "-e", script])
        time.sleep(1)
        helper = Path(__file__).with_name("macos-window-id.swift")
        wid = call(["swift", str(helper), str(pid)], timeout=60).stdout.strip()
        if not wid.isdigit():
            raise RuntimeError(f"could not resolve the Slint window ID for PID {pid}: {wid!r}")
        call(["screencapture", "-x", "-l", wid, str(output)], timeout=20)
        return wid
    helper = Path(__file__).with_name("windows-capture-close.ps1")
    captured = call(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(helper),
                     "-Mode", "Capture", "-ProcessId", str(pid), "-OutputPath", str(output)], timeout=20)
    try:
        evidence = json.loads(captured.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Windows capture helper returned invalid bounds evidence: {captured.stdout!r}") from error
    return evidence


def close_window(platform_name, pid, wid):
    if platform_name == "linux":
        window_id = wid["window_id"] if isinstance(wid, dict) else wid
        if not window_id:
            ids = call(["xdotool", "search", "--onlyvisible", "--pid", str(pid)], check=False).stdout.split()
            window_id = ids[-1] if ids else None
        if not window_id:
            raise RuntimeError(f"no X11 window to close for PID {pid}")
        call(["xdotool", "windowclose", window_id])
    elif platform_name == "macos":
        script = f'''tell application "System Events"
set p to first process whose unix id is {pid}
set frontmost of p to true
tell p to click button 1 of window 1
end tell'''
        call(["osascript", "-e", script])
    else:
        helper = Path(__file__).with_name("windows-capture-close.ps1")
        call(["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", str(helper),
              "-Mode", "Close", "-ProcessId", str(pid)], timeout=15)


def launch(desktop, status, log, external=None):
    status.unlink(missing_ok=True)
    env = os.environ.copy()
    env["PHOTO_CULLER_SMOKE_STATUS_FILE"] = str(status)
    env.pop("PHOTO_CULLER_SERVER", None)
    if external:
        env["PHOTO_CULLER_API_ORIGIN"] = external
    else:
        env.pop("PHOTO_CULLER_API_ORIGIN", None)
    stream = log.open("wb")
    p = subprocess.Popen([str(desktop)], cwd=desktop.parent, env=env, stdout=stream, stderr=subprocess.STDOUT)
    p._log = stream
    return p


def close_app(p, platform_name, wid, child_pid, origin, release_expected=True):
    close_window(platform_name, p.pid, wid)
    try:
        code = p.wait(timeout=20)
    except subprocess.TimeoutExpired as e:
        p.terminate()
        try:
            p.wait(timeout=4)
        except subprocess.TimeoutExpired:
            p.kill()
            p.wait(timeout=4)
        stop_pid(child_pid)
        raise RuntimeError("window did not close normally within 20 seconds") from e
    port_released = released(origin) if release_expected else None
    if code or (release_expected and not port_released):
        stop_pid(child_pid)
        raise RuntimeError(f"desktop exit={code}; owned port released={port_released}")
    if child_pid and alive(child_pid):
        stop_pid(child_pid)
        raise RuntimeError(f"owned server PID {child_pid} remained alive")
    return code


def sha(path):
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def owned_run(platform_name, desktop, out, label, screenshot=False):
    status, log = out / f"{label}-status.tsv", out / f"{label}-desktop.log"
    p = launch(desktop, status, log)
    child = None
    try:
        origin, child = ready(p, status)
        summary = probe(origin)
        image = out / f"{label}.png"
        wid = None
        dimensions = None
        if screenshot:
            time.sleep(1)
            wid = capture(platform_name, p.pid, image)
            header = image.read_bytes()[:24]
            if header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR" or image.stat().st_size < 10000:
                raise RuntimeError(f"empty or invalid screenshot: {image}")
            dimensions = (int.from_bytes(header[16:20], "big"), int.from_bytes(header[20:24], "big"))
            minimum = (800, 500) if platform_name == "windows" else (1000, 650)
            if dimensions[0] < minimum[0] or dimensions[1] < minimum[1]:
                raise RuntimeError(f"screenshot too small: {dimensions}")
            if platform_name == "windows":
                if wid.get("dpi_awareness") != "thread-per-monitor-v2":
                    raise RuntimeError(f"Windows capture did not report per-monitor DPI awareness: {wid}")
                physical_bounds = wid["window_bounds"]
                screen_bounds = wid["virtual_screen_bounds"]
                expected_dimensions = wid["screenshot_dimensions"]
                if dimensions != tuple(expected_dimensions):
                    raise RuntimeError(f"Windows screenshot dimensions {dimensions} differ from window bounds {expected_dimensions}")
                if (physical_bounds[0] < screen_bounds[0] or physical_bounds[1] < screen_bounds[1]
                        or physical_bounds[2] > screen_bounds[2] or physical_bounds[3] > screen_bounds[3]):
                    raise RuntimeError(f"Windows window bounds exceed virtual-screen bounds: {wid}")
            if platform_name == "linux":
                geometry = wid["geometry"]
                if dimensions != (geometry.get("width"), geometry.get("height")):
                    raise RuntimeError(f"Linux screenshot dimensions {dimensions} differ from X11 window geometry {geometry}")
        exit_code = close_app(p, platform_name, wid, child, origin)
        return {"desktop_pid": p.pid, "owned_server_pid": child, "origin": origin,
                "probe": summary, "exit_code": exit_code, "port_released": True,
                "screenshot": str(image) if screenshot else None, "dimensions": dimensions,
                "window_capture": wid if isinstance(wid, dict) else None}
    except Exception:
        if p.poll() is None:
            p.terminate()
            try:
                p.wait(timeout=4)
            except subprocess.TimeoutExpired:
                p.kill()
                p.wait(timeout=4)
        stop_pid(child)
        raise
    finally:
        p._log.close()


def start_external(server, log):
    stream = log.open("wb")
    p = subprocess.Popen([str(server), "--owned", "--port", "0"], cwd=server.parent,
                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=stream, text=True)
    q = queue.Queue()
    threading.Thread(target=lambda: q.put(p.stdout.readline()), daemon=True).start()
    try:
        line = q.get(timeout=10)
    except queue.Empty as e:
        p.kill()
        p.wait(timeout=5)
        stream.close()
        raise RuntimeError("external server did not announce readiness") from e
    if not line:
        p.kill()
        p.wait(timeout=5)
        stream.close()
        raise RuntimeError("external server exited before readiness")
    origin = json.loads(line).get("origin")
    if not isinstance(origin, str) or not origin.startswith("http://127.0.0.1:"):
        p.kill()
        p.wait(timeout=5)
        stream.close()
        raise RuntimeError(f"invalid server origin: {origin}")
    p._log = stream
    return p, origin


def stop_external(p):
    if p.poll() is None and p.stdin:
        p.stdin.close()
    try:
        code = p.wait(timeout=10)
    except subprocess.TimeoutExpired:
        p.kill()
        code = p.wait(timeout=5)
        raise RuntimeError("external test server did not stop after stdin EOF")
    p._log.close()
    if code:
        raise RuntimeError(f"external server shutdown exited {code}")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--platform", choices=("linux", "macos", "windows"), required=True)
    parser.add_argument("--desktop", type=Path, required=True)
    parser.add_argument("--server", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    desktop, server, out = args.desktop.resolve(), args.server.resolve(), args.output.resolve()
    out.mkdir(parents=True, exist_ok=True)
    if not desktop.is_file() or not server.is_file():
        raise RuntimeError("staged desktop/server binary missing")

    spaced = Path(tempfile.mkdtemp(prefix="photo culler native smoke "))
    client, service = spaced / desktop.name, spaced / server.name
    shutil.copy2(desktop, client)
    shutil.copy2(server, service)
    if os.name != "nt":
        client.chmod(0o755)
        service.chmod(0o755)
    windows_backend = None
    if args.platform == "windows":
        windows_backend = os.environ.get("SLINT_BACKEND")
        if windows_backend != "winit-software":
            raise RuntimeError("the hosted Windows GUI smoke must select Slint's Winit software renderer")

    evidence = {
        "date_utc": datetime.now(timezone.utc).isoformat(),
        "platform": platform.platform(),
        "architecture": platform.machine(),
        "spaced_stage": str(spaced),
        "desktop_sha256": sha(client),
        "server_sha256": sha(service),
    }
    if windows_backend:
        evidence["windows_slint_backend"] = windows_backend
    evidence["owned_first"] = owned_run(args.platform, client, out, "owned-first", True)
    evidence["owned_reopen"] = owned_run(args.platform, client, out, "owned-reopen")

    external, origin = start_external(service, out / "external-server.log")
    try:
        external_probe = probe(origin)
        status, log = out / "external-status.tsv", out / "external-desktop.log"
        p = launch(client, status, log, origin)
        try:
            actual, child = ready(p, status)
            if actual != origin or child is not None:
                raise RuntimeError("external mode must reuse the specified server without owning it")
            code = close_app(p, args.platform, None, None, origin, release_expected=False)
            if external.poll() is not None or json_get(origin + "/api/v1/health").get("status") != "ready":
                raise RuntimeError("external service stopped when the desktop closed")
            evidence["external_survives_close"] = {
                "pid": external.pid, "origin": origin, "probe": external_probe,
                "desktop_exit": code, "health_after_close": "ready",
            }
        finally:
            if p.poll() is None:
                p.terminate()
                try:
                    p.wait(timeout=4)
                except subprocess.TimeoutExpired:
                    p.kill()
                    p.wait(timeout=4)
            p._log.close()
    finally:
        stop_external(external)

    evidence["screenshot_sha256"] = sha(out / "owned-first.png")
    (out / "evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")
    (out / "evidence.txt").write_text(
        "Native Slint desktop smoke: PASS\n"
        f"UTC: {evidence['date_utc']}\nPlatform: {evidence['platform']} ({evidence['architecture']})\n"
        f"Desktop SHA-256: {evidence['desktop_sha256']}\nServer SHA-256: {evidence['server_sha256']}\n"
        "Visible demo-001.jpg; owned launch/normal close/process and port release: PASS\n"
        "Reopen: PASS; PHOTO_CULLER_API_ORIGIN survives close: PASS\n"
        f"Screenshot: {out / 'owned-first.png'} ({evidence['screenshot_sha256']})\n"
    )
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"native desktop smoke failed: {error}", file=sys.stderr)
        raise
