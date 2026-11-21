#!/usr/bin/env python3
"""Run the staged Rust/Slint desktop on a hosted GUI runner and save evidence."""

import argparse
import hashlib
import json
import os
import platform
import queue
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import threading
import time
import urllib.error
import urllib.request
import zlib
from datetime import datetime, timezone
from pathlib import Path


class WindowNotVisibleError(RuntimeError):
    """No visible native window was discovered on this capture attempt."""

    def __init__(self, message, diagnostic=None):
        super().__init__(message)
        self.diagnostic = diagnostic or {}


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


def json_get(url, bearer=None):
    headers = {"Authorization": f"Bearer {bearer}"} if bearer else {}
    request = urllib.request.Request(url, headers=headers)
    with urllib.request.urlopen(request, timeout=3) as response:
        if response.status != 200:
            raise RuntimeError(f"GET {url} returned {response.status}")
        return json.loads(response.read())


def probe(origin, bearer=None):
    health = json_get(origin + "/api/v1/health")
    if health.get("status") != "ready":
        raise RuntimeError("health response did not satisfy S01")
    if not bearer:
        try:
            json_get(origin + "/api/v1/galleries/demo/photos")
        except urllib.error.HTTPError as error:
            if error.code != 401:
                raise
        else:
            raise RuntimeError("protected gallery route accepted an unauthenticated probe")
        return "health=ready; protected gallery rejects missing bearer with HTTP 401; UI loaded demo-001.jpg"

    gallery = json_get(origin + "/api/v1/galleries/demo/photos", bearer)
    request = urllib.request.Request(
        origin + "/api/v1/photos/demo-001/thumbnail/800",
        headers={"Authorization": f"Bearer {bearer}"},
    )
    with urllib.request.urlopen(request, timeout=3) as response:
        data = response.read()
        if response.status != 200 or not response.headers.get("Content-Type", "").lower().startswith("image/jpeg"):
            raise RuntimeError("demo thumbnail is not image/jpeg")
    if not any(x.get("photo_id") == "demo-001" for x in gallery.get("items", [])):
        raise RuntimeError("authenticated gallery response did not satisfy S01")
    if not data.startswith(b"\xff\xd8\xff"):
        raise RuntimeError("thumbnail is not a JPEG")
    return f"health=ready; gallery=demo-001.jpg; thumbnail=image/jpeg ({len(data)} bytes)"


def gallery_fixture_pixels(path):
    """Count the fixture thumbnail's yellow sun and blue sky in the gallery area."""
    data = path.read_bytes()
    if data[:8] != b"\x89PNG\r\n\x1a\n":
        raise RuntimeError(f"invalid screenshot PNG: {path}")
    position = 8
    width = height = bit_depth = color_type = interlace = None
    compressed = bytearray()
    while position + 12 <= len(data):
        length = int.from_bytes(data[position:position + 4], "big")
        chunk_type = data[position + 4:position + 8]
        chunk_data = data[position + 8:position + 8 + length]
        if len(chunk_data) != length:
            raise RuntimeError(f"truncated PNG chunk in screenshot: {path}")
        if chunk_type == b"IHDR":
            width, height, bit_depth, color_type, compression, filter_method, interlace = struct.unpack(
                ">IIBBBBB", chunk_data
            )
            if compression != 0 or filter_method != 0:
                raise RuntimeError(f"unsupported PNG compression/filter in screenshot: {path}")
        elif chunk_type == b"IDAT":
            compressed.extend(chunk_data)
        elif chunk_type == b"IEND":
            break
        position += length + 12
    if not width or not height or bit_depth != 8 or color_type not in (2, 6) or interlace != 0:
        raise RuntimeError(
            f"unsupported screenshot PNG format: {path} ({width}x{height}, depth={bit_depth}, "
            f"color_type={color_type}, interlace={interlace})"
        )

    channels = 4 if color_type == 6 else 3
    stride = width * channels
    pixels = zlib.decompress(compressed)
    if len(pixels) != height * (stride + 1):
        raise RuntimeError(f"unexpected decoded PNG size in screenshot: {path}")
    # The first fixture thumbnail sits just to the right of the filter panel.
    # Its relative x-position changes with the desktop window width, so include
    # the panel edge while staying clear of the desktop strip and inspector.
    gallery_left, gallery_top = int(width * 0.30), int(height * 0.12)
    gallery_right, gallery_bottom = int(width * 0.75), int(height * 0.45)
    yellow_sun = blue_sky = 0
    yellow_bounds = [width, height, -1, -1]
    blue_bounds = [width, height, -1, -1]
    previous = bytearray(stride)
    cursor = 0

    def paeth(left, above, upper_left):
        estimate = left + above - upper_left
        distances = (abs(estimate - left), abs(estimate - above), abs(estimate - upper_left))
        return (left, above, upper_left)[distances.index(min(distances))]

    for y in range(height):
        filter_type = pixels[cursor]
        cursor += 1
        scanline = bytearray(pixels[cursor:cursor + stride])
        cursor += stride
        if filter_type == 1:
            for index in range(stride):
                scanline[index] = (scanline[index] + (scanline[index - channels] if index >= channels else 0)) & 255
        elif filter_type == 2:
            for index in range(stride):
                scanline[index] = (scanline[index] + previous[index]) & 255
        elif filter_type == 3:
            for index in range(stride):
                left = scanline[index - channels] if index >= channels else 0
                scanline[index] = (scanline[index] + ((left + previous[index]) // 2)) & 255
        elif filter_type == 4:
            for index in range(stride):
                left = scanline[index - channels] if index >= channels else 0
                upper_left = previous[index - channels] if index >= channels else 0
                scanline[index] = (scanline[index] + paeth(left, previous[index], upper_left)) & 255
        elif filter_type != 0:
            raise RuntimeError(f"unknown PNG row filter {filter_type} in screenshot: {path}")

        if gallery_top <= y < gallery_bottom:
            for x in range(gallery_left, gallery_right):
                index = x * channels
                red, green, blue = scanline[index:index + 3]
                alpha = scanline[index + 3] if channels == 4 else 255
                if alpha >= 200 and red >= 210 and green >= 155 and blue <= 145:
                    yellow_sun += 1
                    yellow_bounds = [min(yellow_bounds[0], x), min(yellow_bounds[1], y),
                                     max(yellow_bounds[2], x), max(yellow_bounds[3], y)]
                if alpha >= 200 and red <= 105 and green >= 85 and blue >= 145:
                    blue_sky += 1
                    blue_bounds = [min(blue_bounds[0], x), min(blue_bounds[1], y),
                                   max(blue_bounds[2], x), max(blue_bounds[3], y)]
        previous = scanline
    yellow_pixel_bounds = yellow_bounds if yellow_sun else None
    blue_pixel_bounds = blue_bounds if blue_sky else None
    sun_inside_thumbnail = bool(
        yellow_pixel_bounds and blue_pixel_bounds
        and blue_pixel_bounds[0] <= yellow_pixel_bounds[0] <= yellow_pixel_bounds[2] <= blue_pixel_bounds[2]
        and blue_pixel_bounds[1] <= yellow_pixel_bounds[1] <= yellow_pixel_bounds[3] <= blue_pixel_bounds[3]
    )
    return {
        "dimensions": [width, height],
        "gallery_region": [gallery_left, gallery_top, gallery_right, gallery_bottom],
        "yellow_sun_pixels": yellow_sun,
        "blue_sky_pixels": blue_sky,
        "yellow_pixel_bounds": yellow_pixel_bounds,
        "blue_pixel_bounds": blue_pixel_bounds,
        "sun_inside_thumbnail_region": sun_inside_thumbnail,
    }


def capture_visible_gallery(platform_name, pid, image, timeout=15):
    deadline = time.monotonic() + timeout
    attempts = 0
    latest = None
    latest_window = None
    last_window_error = None
    last_window_diagnostic = None
    minimum = (800, 500) if platform_name == "windows" else (1000, 650)
    while time.monotonic() < deadline:
        attempts += 1
        try:
            latest_window = capture(platform_name, pid, image)
            last_window_error = None
            last_window_diagnostic = None
        except WindowNotVisibleError as error:
            last_window_error = str(error)
            last_window_diagnostic = error.diagnostic
            time.sleep(.5)
            continue
        header = image.read_bytes()[:24]
        if (header[:8] != b"\x89PNG\r\n\x1a\n" or header[12:16] != b"IHDR"
                or image.stat().st_size < 10000):
            raise RuntimeError(f"empty or invalid screenshot: {image}")
        dimensions = (int.from_bytes(header[16:20], "big"), int.from_bytes(header[20:24], "big"))
        if dimensions[0] < minimum[0] or dimensions[1] < minimum[1]:
            raise RuntimeError(f"screenshot too small: {dimensions}")
        latest = gallery_fixture_pixels(image)
        latest["attempts"] = attempts
        if (latest["yellow_sun_pixels"] >= 10 and latest["blue_sky_pixels"] >= 100
                and latest["sun_inside_thumbnail_region"]):
            readiness = image.with_name(f"{image.stem}-content-readiness.json")
            readiness.write_text(json.dumps(latest, indent=2) + "\n")
            return latest_window, dimensions, latest
        time.sleep(.5)
    readiness = image.with_name(f"{image.stem}-content-readiness.json")
    linux_debug = image.parent / "linux-window-debug.txt"
    platform_diagnostics = {"process_alive_at_timeout": alive(pid)}
    if platform_name == "linux" and linux_debug.exists():
        platform_diagnostics.update({
            "linux_window_debug_path": str(linux_debug),
            "linux_window_debug": linux_debug.read_text(errors="replace"),
        })
    readiness.write_text(json.dumps({
        "attempts": attempts,
        "last_capture": latest,
        "last_window_error": last_window_error,
        "last_window_diagnostic": last_window_diagnostic,
        "platform_diagnostics": platform_diagnostics,
    }, indent=2) + "\n")
    raise RuntimeError(
        f"Slint window never painted the demo fixture thumbnail after {attempts} attempts; "
        f"last gallery pixels={latest}; last window discovery error={last_window_error!r} "
        f"(diagnostic: {readiness})"
    )


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


def discover_linux_window(pid, output_dir):
    output_dir.mkdir(parents=True, exist_ok=True)
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
    x11_windows = []
    window_pattern = re.compile(
        r'^\s*(0x[0-9a-fA-F]+)\s+"([^"]*)":\s+\("([^"]*)"\s+"([^"]*)"\)\s+'
        r'(\d+)x(\d+)\+(-?\d+)\+(-?\d+)'
    )
    for line in xwininfo.stdout.splitlines():
        match = window_pattern.match(line)
        if not match:
            continue
        xid, title, resource_name, class_name, width, height, x, y = match.groups()
        window_id = str(int(xid, 16))
        window_info = call(["xwininfo", "-id", xid], check=False)
        window_pid_text = call(["xdotool", "getwindowpid", window_id], check=False).stdout.strip()
        x11_windows.append({
            "window_id": window_id,
            "xid": xid,
            "title": title,
            "resource_name": resource_name,
            "class_name": class_name,
            "pid": int(window_pid_text) if window_pid_text.isdigit() else None,
            "viewable": "Map State: IsViewable" in window_info.stdout,
            "geometry": {"x": int(x), "y": int(y), "width": int(width), "height": int(height)},
        })
    debug_path = output_dir / "linux-window-debug.txt"
    debug_lines = [
        f"DISPLAY={os.environ.get('DISPLAY', '')}",
        f"WAYLAND_DISPLAY={os.environ.get('WAYLAND_DISPLAY', '')}",
        f"WAYLAND_SOCKET={os.environ.get('WAYLAND_SOCKET', '')}",
        f"desktop_pid={pid}",
        f"desktop_process_alive={alive(pid)}",
        f"pid_search={by_pid.stdout.strip()!r}",
        f"visible_search={all_visible.stdout.strip()!r}",
        f"visible_windows={windows!r}",
        f"xwininfo_windows={x11_windows!r}",
        f"xwininfo_exit={xwininfo.returncode}",
        xwininfo.stdout,
        xwininfo.stderr,
    ]
    debug_path.write_text("\n".join(debug_lines))
    candidates = [window for window in x11_windows
                  if "photo culler" in window["title"].casefold()
                  and "photo-culler-desktop" in window["resource_name"].casefold()
                  and window["viewable"]
                  and window["pid"] == pid
                  and window["geometry"]["width"] >= 1000
                  and window["geometry"]["height"] >= 650]
    if not candidates:
        raise WindowNotVisibleError(
            f"no visible titled X11 Photo Culler child for desktop PID {pid}; "
            f"pid-search={by_pid.stdout.strip()!r}; xwininfo-windows={x11_windows!r}",
            {
                "desktop_process_alive": alive(pid),
                "xwininfo_exit": xwininfo.returncode,
                "xwininfo_tree": xwininfo.stdout,
                "xwininfo_stderr": xwininfo.stderr,
                "linux_window_debug_path": str(debug_path),
            },
        )
    return max(candidates, key=lambda window: window["geometry"]["width"]
               * window["geometry"]["height"])


def wait_for_linux_window(pid, output_dir, timeout=20):
    deadline = time.monotonic() + timeout
    attempts = 0
    last_error = None
    while time.monotonic() < deadline:
        attempts += 1
        try:
            return discover_linux_window(pid, output_dir)
        except WindowNotVisibleError as error:
            last_error = error
            time.sleep(.5)
    debug_path = output_dir / "linux-window-debug.txt"
    debug_text = debug_path.read_text(errors="replace") if debug_path.exists() else ""
    diagnostics = {
        "attempts": attempts,
        "desktop_process_alive_at_timeout": alive(pid),
        "last_window_error": str(last_error) if last_error else None,
        "last_window_diagnostic": last_error.diagnostic if last_error else None,
        "linux_window_debug_path": str(debug_path),
        "linux_window_debug": debug_text,
    }
    (output_dir / "linux-window-close-discovery-timeout.json").write_text(
        json.dumps(diagnostics, indent=2) + "\n"
    )
    raise RuntimeError(
        f"no titled X11 Photo Culler window for desktop PID {pid} after {attempts} attempts; "
        f"process_alive={diagnostics['desktop_process_alive_at_timeout']}; "
        f"diagnostic={output_dir / 'linux-window-close-discovery-timeout.json'}"
    ) from last_error


def capture(platform_name, pid, output):
    if platform_name == "linux":
        wid = discover_linux_window(pid, output.parent)
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


def close_window(platform_name, pid, wid, diagnostics_dir=None):
    if platform_name == "linux":
        window_id = wid["window_id"] if isinstance(wid, dict) else wid
        if not window_id:
            diagnostics_dir = diagnostics_dir or (Path(tempfile.gettempdir()) / f"photo-culler-window-close-{pid}")
            wid = wait_for_linux_window(pid, diagnostics_dir)
            window_id = wid["window_id"]
        call(["xdotool", "windowactivate", "--sync", window_id])
        call(["xdotool", "key", "--clearmodifiers", "Alt+F4"])
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


def launch(desktop, status, log, external=None, gallery_smoke=False, bearer=None):
    status.unlink(missing_ok=True)
    env = os.environ.copy()
    env["PHOTO_CULLER_SMOKE_STATUS_FILE"] = str(status)
    env.pop("PHOTO_CULLER_SERVER", None)
    # Keep the live one-photo run deterministic even if the runner has this
    # selector in its environment; enable it only for the explicit S05 run.
    env.pop("PHOTO_CULLER_GALLERY_SMOKE", None)
    if gallery_smoke:
        env["PHOTO_CULLER_GALLERY_SMOKE"] = "1"
    if external:
        env["PHOTO_CULLER_API_ORIGIN"] = external
        if bearer:
            env["PHOTO_CULLER_BEARER"] = bearer
        else:
            env.pop("PHOTO_CULLER_BEARER", None)
    else:
        env.pop("PHOTO_CULLER_API_ORIGIN", None)
        env.pop("PHOTO_CULLER_BEARER", None)
    stream = log.open("wb")
    # Owned fixture runs are explicit so the normal desktop launch can open
    # the user's persistent library and first-run folder picker. External
    # server runs remain configured only through the API origin and bearer.
    command = [str(desktop)]
    if not external:
        command.append("--demo")
    p = subprocess.Popen(command, cwd=desktop.parent, env=env, stdout=stream, stderr=subprocess.STDOUT)
    p._log = stream
    return p


def close_app(p, platform_name, wid, child_pid, origin, release_expected=True, diagnostics_dir=None):
    close_window(platform_name, p.pid, wid, diagnostics_dir)
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


def owned_run(platform_name, desktop, out, label, screenshot=False, gallery_smoke=False):
    status, log = out / f"{label}-status.tsv", out / f"{label}-desktop.log"
    p = launch(desktop, status, log, gallery_smoke=gallery_smoke)
    child = None
    try:
        origin, child = ready(p, status)
        summary = probe(origin)
        image = out / f"{label}.png"
        wid = None
        dimensions = None
        screenshot_content = None
        if screenshot:
            wid, dimensions, screenshot_content = capture_visible_gallery(
                platform_name, p.pid, image, timeout=20
            )
            if platform_name == "windows":
                if wid.get("dpi_awareness") != "thread-per-monitor-v2":
                    raise RuntimeError(f"Windows capture did not report per-monitor DPI awareness: {wid}")
                physical_bounds = wid["window_bounds"]
                outer_bounds = wid.get("window_rect_bounds")
                screen_bounds = wid["physical_screen_bounds"]
                work_area_bounds = wid.get("work_area_bounds")
                expected_dimensions = wid["screenshot_dimensions"]
                if dimensions != tuple(expected_dimensions):
                    raise RuntimeError(f"Windows screenshot dimensions {dimensions} differ from window bounds {expected_dimensions}")
                if (physical_bounds[0] < screen_bounds[0] or physical_bounds[1] < screen_bounds[1]
                        or physical_bounds[2] > screen_bounds[2] or physical_bounds[3] > screen_bounds[3]):
                    raise RuntimeError(f"Windows window bounds exceed physical-screen bounds: {wid}")
                if (not work_area_bounds or physical_bounds[0] < work_area_bounds[0]
                        or physical_bounds[1] < work_area_bounds[1]
                        or physical_bounds[2] > work_area_bounds[2]
                        or physical_bounds[3] > work_area_bounds[3]):
                    raise RuntimeError(f"Windows window is not fully within the usable work area: {wid}")
                if (not outer_bounds or outer_bounds[0] < work_area_bounds[0]
                        or outer_bounds[1] < work_area_bounds[1]
                        or outer_bounds[2] > work_area_bounds[2]
                        or outer_bounds[3] > work_area_bounds[3]):
                    raise RuntimeError(f"Windows outer resize frame is not fully within the usable work area: {wid}")
            if platform_name == "linux":
                geometry = wid["geometry"]
                window_dimensions = (geometry.get("width"), geometry.get("height"))
                inset = (window_dimensions[0] - dimensions[0], window_dimensions[1] - dimensions[1])
                wid["screenshot_dimensions"] = list(dimensions)
                wid["screenshot_inset"] = list(inset)
                if (inset[0] < -2 or inset[1] < -2 or inset[0] > 32 or inset[1] > 32):
                    raise RuntimeError(
                        f"Linux screenshot dimensions {dimensions} exceed the selected X11 window "
                        f"or differ by more than the 32px frame allowance: geometry={geometry}, inset={inset}"
                    )
        exit_code = close_app(p, platform_name, wid, child, origin, diagnostics_dir=out)
        return {"desktop_pid": p.pid, "owned_server_pid": child, "origin": origin,
                "probe": summary, "exit_code": exit_code, "port_released": True,
                "screenshot": str(image) if screenshot else None, "dimensions": dimensions,
                "screenshot_content": screenshot_content,
                "gallery_smoke_fixture": gallery_smoke,
                "screenshot_sha256": sha(image) if screenshot else None,
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
    ready = json.loads(line)
    origin = ready.get("origin")
    bearer = ready.get("session_token")
    if not isinstance(origin, str) or not origin.startswith("http://127.0.0.1:"):
        p.kill()
        p.wait(timeout=5)
        stream.close()
        raise RuntimeError(f"invalid server origin: {origin}")
    if not isinstance(bearer, str) or not re.fullmatch(r"[0-9a-fA-F]{64}", bearer):
        p.kill()
        p.wait(timeout=5)
        stream.close()
        raise RuntimeError("owned server did not provide a valid private session bearer")
    p._log = stream
    return p, origin, bearer


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
    evidence["gallery_smoke"] = owned_run(
        args.platform, client, out, "gallery-smoke", True, gallery_smoke=True
    )

    external, origin, bearer = start_external(service, out / "external-server.log")
    try:
        external_probe = probe(origin, bearer)
        status, log = out / "external-status.tsv", out / "external-desktop.log"
        p = launch(client, status, log, origin, bearer=bearer)
        try:
            actual, child = ready(p, status)
            if actual != origin or child is not None:
                raise RuntimeError("external mode must reuse the specified server without owning it")
            code = close_app(p, args.platform, None, None, origin, release_expected=False,
                             diagnostics_dir=out)
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
        "Reopen: PASS; 10-card synthetic metadata capture: PASS; "
        "PHOTO_CULLER_API_ORIGIN survives close: PASS\n"
        f"Screenshot: {out / 'owned-first.png'} ({evidence['screenshot_sha256']})\n"
        f"10-card capture: {out / 'gallery-smoke.png'} "
        f"({evidence['gallery_smoke']['screenshot_sha256']})\n"
    )
    print(json.dumps(evidence, indent=2))


if __name__ == "__main__":
    try:
        main()
    except Exception as error:
        print(f"native desktop smoke failed: {error}", file=sys.stderr)
        raise
