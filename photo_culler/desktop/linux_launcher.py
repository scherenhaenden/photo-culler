"""Clickable Linux launcher using an isolated Chrome app window."""

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


def main() -> None:
    """Run the local server for the lifetime of a dedicated Chrome app window."""
    chrome = shutil.which("google-chrome") or shutil.which("chromium")
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
            subprocess.run(
                [
                    chrome,
                    f"--app={url}/?token={token}",
                    f"--user-data-dir={profile}",
                    "--no-first-run",
                    "--no-default-browser-check",
                ],
                check=False,
            )
    finally:
        server.should_exit = True
        server_thread.join(timeout=5)


if __name__ == "__main__":
    main()
