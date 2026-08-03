"""Real-process acceptance test for the packaged Tauri Linux desktop window."""

from __future__ import annotations

import os
import re
import shutil
import subprocess
import time
from pathlib import Path
from urllib.error import URLError
from urllib.request import urlopen

import pytest


@pytest.mark.e2e
def test_tauri_deb_starts_native_window_and_authenticated_sidecar(tmp_path: Path) -> None:
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
        while time.monotonic() < deadline:
            for process_dir in Path("/proc").glob("[0-9]*"):
                try:
                    command_line = (process_dir / "cmdline").read_text().replace("\0", " ")
                except OSError:
                    continue
                if str(root / "usr/bin/photo-culler-backend") in command_line:
                    break
            if "--token" in command_line:
                break
            time.sleep(0.1)
        match = re.search(r"--port\s+(\d+)\s+--token\s+([A-Za-z0-9_-]+)", command_line)
        assert match, process.stderr.read() if process.stderr else "Tauri sidecar did not start"
        health_url = f"http://127.0.0.1:{match.group(1)}/api/health?token={match.group(2)}"
        while time.monotonic() < deadline:
            try:
                with urlopen(health_url, timeout=1) as response:
                    assert response.status == 200
                    break
            except URLError:
                assert process.poll() is None, process.stderr.read() if process.stderr else "Tauri exited early"
                time.sleep(0.1)
        else:
            pytest.fail("Tauri sidecar did not expose its authenticated health endpoint")
    finally:
        process.terminate()
        process.wait(timeout=5)
