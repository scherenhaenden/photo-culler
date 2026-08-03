"""Linux launcher for the packaged native egui/wgpu desktop client."""

import logging
import os
import secrets
import subprocess
import sys
import threading
from pathlib import Path

import uvicorn

from photo_culler.desktop.app import (
    configure_desktop_logging,
    default_desktop_catalog_path,
    find_free_port,
    wait_until_ready,
)
from photo_culler.web.app import create_app


def native_binary_path() -> Path:
    """Locate the Rust executable both from source and from a PyInstaller bundle."""
    override = os.environ.get("PHOTO_CULLER_EGUI_BINARY")
    if override:
        candidate = Path(override).expanduser().resolve()
        if candidate.is_file() and os.access(candidate, os.X_OK):
            return candidate
        raise RuntimeError(f"PHOTO_CULLER_EGUI_BINARY is not an executable file: {candidate}")
    bundle_root = Path(getattr(sys, "_MEIPASS", Path(__file__).resolve().parent))
    name = "photo-culler-egui-native.exe" if sys.platform == "win32" else "photo-culler-egui-native"
    candidate = bundle_root / name
    if not candidate.is_file():
        raise RuntimeError(f"Native egui executable is missing: {candidate}")
    return candidate


def main() -> None:
    """Run the private local service for the lifetime of the native window."""
    configure_desktop_logging()
    port = find_free_port()
    token = secrets.token_urlsafe(16)
    app = create_app(catalog_path=default_desktop_catalog_path(), desktop_token=token)
    server = uvicorn.Server(uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning"))
    server_thread = threading.Thread(target=server.run, daemon=True)
    server_thread.start()
    url = f"http://127.0.0.1:{port}"
    client_exit_code = 0
    primary_error: BaseException | None = None
    try:
        wait_until_ready(url, token=token)
        environment = os.environ.copy()
        environment["PHOTO_CULLER_SERVER"] = url
        environment["PHOTO_CULLER_SERVER_TOKEN"] = token
        result = subprocess.run([str(native_binary_path())], check=False, env=environment)
        if result.returncode != 0:
            logging.error("Native egui client exited unexpectedly with status %s", result.returncode)
            client_exit_code = result.returncode
    except BaseException as error:
        primary_error = error
        raise
    finally:
        server.should_exit = True
        server_thread.join(timeout=5)
        if server_thread.is_alive():
            logging.error("Photo Culler local server did not stop cleanly after native client shutdown")
            if primary_error is None and client_exit_code == 0:
                raise RuntimeError("Photo Culler local server did not stop cleanly")

    if client_exit_code:
        raise SystemExit(client_exit_code)


if __name__ == "__main__":
    main()
