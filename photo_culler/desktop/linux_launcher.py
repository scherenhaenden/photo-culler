"""Clickable Linux launcher using an isolated Chrome app window."""

import logging
import os
import secrets
import shutil
import subprocess
import tempfile
import threading

import uvicorn

from photo_culler.desktop.app import (
    configure_desktop_logging,
    default_desktop_catalog_path,
    find_free_port,
    wait_until_ready,
)
from photo_culler.web.app import create_app


def find_chrome() -> str | None:
    """Locate a supported browser, allowing an explicit packaged-install override."""
    override = os.environ.get("PHOTO_CULLER_CHROME")
    if override:
        candidate = os.path.abspath(os.path.expanduser(override))
        if os.path.isfile(candidate) and os.access(candidate, os.X_OK):
            return candidate
        raise RuntimeError(f"PHOTO_CULLER_CHROME is not an executable file: {candidate}")
    return shutil.which("google-chrome") or shutil.which("chromium") or shutil.which("chromium-browser")


def chrome_command(chrome: str, url: str, profile: str, extra_args: list[str] | None = None) -> list[str]:
    """Build the isolated app-window command without inheriting a browser profile."""
    command = [
        chrome,
        f"--app={url}",
        f"--user-data-dir={profile}",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-sync",
    ]
    return command + (extra_args or [])


def main() -> None:
    """Run the local server for exactly the lifetime of a dedicated Chrome app window."""
    chrome = find_chrome()
    if not chrome:
        raise RuntimeError("Photo Culler requires Google Chrome or Chromium on Linux")

    configure_desktop_logging()
    port = find_free_port()
    token = secrets.token_urlsafe(16)
    app = create_app(
        catalog_path=default_desktop_catalog_path(),
        desktop_token=token,
    )
    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning"))
    server_thread = threading.Thread(target=server.run, daemon=True)
    server_thread.start()

    url = f"http://127.0.0.1:{port}"
    wait_until_ready(url, token)

    try:
        with tempfile.TemporaryDirectory(prefix="photo-culler-chrome-") as profile:
            result = subprocess.run(chrome_command(chrome, f"{url}/?token={token}", profile), check=False)
            if result.returncode != 0:
                raise RuntimeError(f"Chrome exited unexpectedly (status {result.returncode})")
    finally:
        server.should_exit = True
        server_thread.join(timeout=5)
        if server_thread.is_alive():
            logging.warning("Photo Culler local server thread did not stop cleanly after shutdown signal")


if __name__ == "__main__":
    main()
