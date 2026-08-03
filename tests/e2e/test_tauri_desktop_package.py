"""Real-process acceptance test for the packaged Tauri Linux desktop window."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen

import pytest


@pytest.mark.e2e
def test_tauri_deb_starts_native_window_and_authenticated_sidecar(tmp_path: Path) -> None:
    if not sys.platform.startswith("linux") or not Path("/proc").is_dir():
        pytest.skip("Linux /proc is required to inspect the Tauri backend process")
    package = os.environ.get("PHOTO_CULLER_TAURI_DEB")
    if not package:
        pytest.skip("set PHOTO_CULLER_TAURI_DEB to run the packaged Tauri desktop E2E")
    if not shutil.which("dpkg-deb"):
        pytest.skip("dpkg-deb is required for the Linux package E2E")

    root = tmp_path / "package"
    subprocess.run(["dpkg-deb", "-x", package, str(root)], check=True)
    app = root / "usr/bin/photo-culler-tauri"
    process = subprocess.Popen([str(app)], stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    try:
        deadline = time.monotonic() + 15
        command_line = ""
        token = ""
        while time.monotonic() < deadline:
            for process_dir in Path("/proc").glob("[0-9]*"):
                try:
                    command_line = (process_dir / "cmdline").read_text().replace("\0", " ")
                    environment = (process_dir / "environ").read_bytes().decode(errors="replace")
                except OSError:
                    continue
                if str(root / "usr/bin/photo-culler-backend") in command_line:
                    token_match = re.search(r"(?:^|\0)PHOTO_CULLER_TAURI_TOKEN=([^\0]+)", environment)
                    token = token_match.group(1) if token_match else ""
                    break
            if "--port" in command_line and token:
                break
            time.sleep(0.1)
        match = re.search(r"--port\s+(\d+)", command_line)
        if not match or not token:
            if process.poll() is None:
                process.terminate()
            try:
                _, stderr = process.communicate(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                _, stderr = process.communicate()
            pytest.fail(stderr or "Tauri sidecar did not start")
        assert "--token" not in command_line
        health_url = f"http://127.0.0.1:{match.group(1)}/api/health?token={token}"
        while time.monotonic() < deadline:
            try:
                with urlopen(health_url, timeout=1) as response:
                    assert response.status == 200
                    break
            except URLError:
                if process.poll() is not None:
                    _, stderr = process.communicate()
                    pytest.fail(stderr or "Tauri exited early")
                time.sleep(0.1)
        else:
            pytest.fail("Tauri sidecar did not expose its authenticated health endpoint")
    finally:
        if process.poll() is None:
            process.terminate()
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()
