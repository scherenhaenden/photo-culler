"""Real Chrome end-to-end smoke test for the integrated web UI."""

import json
import shutil
import socket
import sqlite3
import subprocess
import tempfile
import threading
import time
import urllib.request
from datetime import datetime, timedelta

import pytest
import uvicorn
from PIL import Image
from websockets.sync.client import connect

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import DecisionState, QualityTier
from photo_culler.core.models import MetadataRecord, Photo
from photo_culler.web.app import create_app


def free_port() -> int:
    with socket.socket() as candidate:
        candidate.bind(("127.0.0.1", 0))
        return candidate.getsockname()[1]


def render_in_chrome(url: str) -> str:
    chrome = shutil.which("google-chrome")
    if not chrome:
        pytest.skip("google-chrome is not installed")
    result = subprocess.run(
        [chrome, "--headless", "--no-sandbox", "--disable-gpu", "--dump-dom", url],
        check=True,
        capture_output=True,
        text=True,
        timeout=15,
    )
    return result.stdout


class ChromeDevTools:
    """Minimal CDP client for real user-flow tests without browser test frameworks."""

    def __init__(self, url: str):
        chrome = shutil.which("google-chrome")
        if not chrome:
            pytest.skip("google-chrome is not installed")
        # Chromium may leave a short-lived helper writing into the profile just
        # after its main process exits. The test profile is disposable, so do
        # not turn that cleanup race into a product test failure.
        self._profile = tempfile.TemporaryDirectory(prefix="photo-culler-e2e-", ignore_cleanup_errors=True)
        self._port = free_port()
        self._process = subprocess.Popen(
            [
                chrome,
                "--headless",
                "--no-sandbox",
                "--disable-gpu",
                "--remote-allow-origins=*",
                f"--remote-debugging-port={self._port}",
                f"--user-data-dir={self._profile.name}",
                url,
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        deadline = time.monotonic() + 10
        targets = []
        while time.monotonic() < deadline:
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{self._port}/json", timeout=0.2) as response:
                    targets = json.load(response)
                if targets:
                    break
            except OSError:
                time.sleep(0.05)
        page = next((target for target in targets if target["type"] == "page"), None)
        if page is None:
            self.close()
            raise RuntimeError("Chrome did not expose a page target")
        self._socket = connect(page["webSocketDebuggerUrl"])
        self._command_id = 0

    def command(self, method: str, params: dict | None = None) -> dict:
        self._command_id += 1
        command_id = self._command_id
        self._socket.send(json.dumps({"id": command_id, "method": method, "params": params or {}}))
        while True:
            response = json.loads(self._socket.recv())
            if response.get("id") == command_id:
                if "error" in response:
                    raise RuntimeError(str(response["error"]))
                return response.get("result", {})

    def evaluate(self, expression: str):
        result = self.command(
            "Runtime.evaluate",
            {"expression": expression, "returnByValue": True, "awaitPromise": True},
        )
        if "exceptionDetails" in result:
            raise RuntimeError(str(result["exceptionDetails"]))
        return result["result"].get("value")

    def navigate(self, url: str) -> None:
        self.command("Page.navigate", {"url": url})
        self.wait_for("document.readyState === 'complete'")

    def wait_for(self, expression: str, timeout: float = 10) -> None:
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if self.evaluate(expression):
                return
            time.sleep(0.05)
        raise AssertionError(f"Browser condition timed out: {expression}")

    def close(self) -> None:
        socket_connection = getattr(self, "_socket", None)
        if socket_connection is not None:
            socket_connection.close()
        process = getattr(self, "_process", None)
        if process is not None:
            process.terminate()
            try:
                process.wait(timeout=3)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait(timeout=3)
        self._profile.cleanup()


@pytest.mark.e2e
def test_operator_pages_render_in_real_chrome(tmp_path):
    app = create_app(catalog_path=tmp_path / "browser.db")
    with app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        repository.save_photo(
            Photo(
                "browser-photo",
                "Browser_Photo",
                decision=DecisionState.REVIEW,
                score=0.8,
                quality_tier=QualityTier.GOOD,
            )
        )

    port = free_port()
    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="error"))
    thread = threading.Thread(target=server.run, daemon=True)
    thread.start()
    deadline = time.monotonic() + 5
    while not server.started and time.monotonic() < deadline:
        time.sleep(0.02)

    try:
        dashboard = render_in_chrome(f"http://127.0.0.1:{port}/?lang=es")
        assert "Dashboard del Catálogo" in dashboard

        library = render_in_chrome(f"http://127.0.0.1:{port}/library")
        assert "Browser_Photo" in library

        detail = render_in_chrome(f"http://127.0.0.1:{port}/photos/browser-photo")
        assert "[2] Mark as Keep" in detail
        assert 'data-current-photo-id="browser-photo"' in detail

        sessions = render_in_chrome(f"http://127.0.0.1:{port}/sessions")
        assert "Híbrido recomendado" in sessions
    finally:
        server.should_exit = True
        thread.join(timeout=5)


@pytest.mark.e2e
def test_single_click_import_and_every_analysis_profile_in_real_chrome(tmp_path):
    """Exercise the exact Import -> catalog -> all analysis profiles journey."""
    catalog_path = tmp_path / "complete-browser.db"
    source = tmp_path / "photos"
    source.mkdir()
    Image.new("RGB", (96, 64), color=(90, 120, 160)).save(source / "complete-flow.jpg")
    app = create_app(catalog_path=catalog_path)
    port = free_port()
    base_url = f"http://127.0.0.1:{port}"
    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="error"))
    server_thread = threading.Thread(target=server.run, daemon=True)
    server_thread.start()
    deadline = time.monotonic() + 5
    while not server.started and time.monotonic() < deadline:
        time.sleep(0.02)
    browser = ChromeDevTools(f"{base_url}/library")

    try:
        source_json = json.dumps(str(source))
        browser.wait_for(
            "document.readyState === 'complete' && document.querySelector('#gallery-import-form') !== null"
        )
        browser.evaluate(
            f"""
            (() => {{
              document.querySelector('input[name="name"]').value = 'Chrome Complete Flow';
              document.querySelector('input[name="path"]').value = {source_json};
              document.querySelector('#gallery-import-form').requestSubmit();
            }})()
            """
        )

        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            galleries = app.state.gallery_imports.list_galleries()
            jobs = app.state.gallery_imports.list_jobs()
            if galleries and galleries[0]["photo_count"] == 1 and jobs and jobs[0]["state"] == "completed":
                break
            time.sleep(0.05)
        assert galleries[0]["photo_count"] == 1
        assert jobs[0]["imported"] == 1
        assert len(jobs) == 1

        browser.navigate(f"{base_url}/library")
        assert browser.evaluate("document.body.innerText.includes('complete-flow')") is True
        browser.wait_for(
            "document.querySelector('.photo-card img').complete "
            "&& document.querySelector('.photo-card img').naturalWidth > 0"
        )

        for profile in ("fast", "technical", "concert"):
            browser.navigate(f"{base_url}/analysis")
            browser.evaluate(
                f"""
                (() => {{
                  document.querySelector(`[data-profile-id="{profile}"]`).click();
                  document.querySelector('#btn-start').click();
                }})()
                """
            )
            deadline = time.monotonic() + 10
            while time.monotonic() < deadline:
                snapshot = app.state.analysis_jobs.snapshot()
                if (
                    not app.state.analysis_jobs.is_running
                    and snapshot["status"] == "completed"
                    and snapshot["profile"] == profile
                ):
                    break
                time.sleep(0.05)
            assert snapshot["status"] == "completed"
            assert snapshot["profile"] == profile
            assert snapshot["processed"] == 1
            assert snapshot["total"] == 1

        with sqlite3.connect(str(catalog_path) + ".metrics.db") as connection:
            analyzers = {row[0] for row in connection.execute("SELECT DISTINCT analyzer_name FROM analyzer_metrics")}
        assert analyzers == {
            "clipping",
            "corruption",
            "dimensions",
            "exposure",
            "histogram",
            "motion_blur",
            "noise",
            "sharpness",
        }
    finally:
        browser.close()
        server.should_exit = True
        server_thread.join(timeout=5)


@pytest.mark.e2e
def test_hybrid_session_administration_in_real_chrome(tmp_path):
    app = create_app(catalog_path=tmp_path / "session-browser.db")
    captured = datetime(2026, 7, 30, 12, 0)
    with app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        repository.save_photo(Photo("take-1", "Take_1", metadata=MetadataRecord(capture_time=captured)))
        repository.save_photo(
            Photo("take-2", "Take_2", metadata=MetadataRecord(capture_time=captured + timedelta(seconds=1)))
        )
    port = free_port()
    base_url = f"http://127.0.0.1:{port}"
    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="error"))
    server_thread = threading.Thread(target=server.run, daemon=True)
    server_thread.start()
    deadline = time.monotonic() + 5
    while not server.started and time.monotonic() < deadline:
        time.sleep(0.02)
    browser = ChromeDevTools(f"{base_url}/sessions")
    try:
        browser.wait_for("document.querySelector('#session-group-form') !== null")
        browser.evaluate("document.querySelector('#session-group-form').requestSubmit()")
        browser.wait_for("document.body.innerText.includes('1 sesiones y 1 ráfagas')")
        assert browser.evaluate("document.querySelectorAll('.session-row').length") == 1
        browser.evaluate(
            "document.querySelector('.session-rename input').value='Sesión Chrome';"
            "document.querySelector('.session-rename').requestSubmit()"
        )
        browser.wait_for("document.body.innerText.includes('Sesión Chrome')")
        browser.evaluate("window.confirm = () => true;document.querySelector('.session-delete').requestSubmit()")
        browser.wait_for("document.body.innerText.includes('Aún no hay sesiones')")
        assert browser.evaluate("document.querySelectorAll('.session-row').length") == 0
    finally:
        browser.close()
        server.should_exit = True
        server_thread.join(timeout=5)
