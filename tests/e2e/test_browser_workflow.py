"""Real Chrome end-to-end smoke test for the integrated web UI."""

import shutil
import socket
import subprocess
import threading
import time

import pytest
import uvicorn

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import DecisionState, QualityTier
from photo_culler.core.models import Photo
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
        dashboard = render_in_chrome(f"http://127.0.0.1:{port}/")
        assert "Dashboard del Catálogo" in dashboard

        library = render_in_chrome(f"http://127.0.0.1:{port}/library")
        assert "Browser_Photo" in library

        detail = render_in_chrome(f"http://127.0.0.1:{port}/photos/browser-photo")
        assert "[2] Mark as Keep" in detail
        assert 'data-current-photo-id="browser-photo"' in detail
    finally:
        server.should_exit = True
        thread.join(timeout=5)
