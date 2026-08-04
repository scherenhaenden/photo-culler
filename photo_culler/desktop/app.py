"""Native pywebview Desktop Window Launcher."""

import os
import secrets
import socket
import threading
import time
import urllib.error
import urllib.request
from logging.handlers import RotatingFileHandler
from pathlib import Path
from typing import Optional, Union

import uvicorn

from photo_culler.web.app import create_app


def default_desktop_catalog_path() -> Path:
    """Return a stable user-data path independent of the launch directory."""
    data_root = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local" / "share"))
    return data_root / "photo-culler" / "catalog.db"


def default_desktop_log_path() -> Path:
    """Return the persistent desktop diagnostic log path."""
    state_root = Path(os.environ.get("XDG_STATE_HOME", Path.home() / ".local" / "state"))
    return state_root / "photo-culler" / "photo-culler.log"


def configure_desktop_logging() -> Path:
    """Write background failures somewhere useful for a windowed executable."""
    import logging

    log_path = default_desktop_log_path()
    log_path.parent.mkdir(parents=True, exist_ok=True)
    root_logger = logging.getLogger()
    resolved_log = log_path.resolve()
    already_configured = any(
        isinstance(handler, RotatingFileHandler) and Path(handler.baseFilename).resolve() == resolved_log
        for handler in root_logger.handlers
    )
    if not already_configured:
        handler = RotatingFileHandler(log_path, maxBytes=2_000_000, backupCount=3, encoding="utf-8")
        handler.setFormatter(logging.Formatter("%(asctime)s %(levelname)s %(name)s: %(message)s"))
        root_logger.addHandler(handler)
    root_logger.setLevel(logging.INFO)
    return log_path


def find_free_port() -> int:
    """Find an available local TCP port on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def wait_until_ready(url: str, token: str, timeout: float = 10.0) -> None:
    """Wait for the local FastAPI server to become responsive by polling its health endpoint with the token."""
    deadline = time.monotonic() + timeout

    while time.monotonic() < deadline:
        try:
            health_url = f"{url}/api/health?token={token}"
            with urllib.request.urlopen(health_url, timeout=0.25) as response:
                if response.status == 200:
                    return
        except (OSError, urllib.error.URLError):
            time.sleep(0.05)

    raise RuntimeError("Photo Culler server did not start")


class DesktopApi:
    """pywebview JS API bridge exposing native OS-specific actions to the frontend."""

    def __init__(self) -> None:
        self._window = None

    def set_window(self, window) -> None:
        self._window = window

    def select_folder(self) -> Optional[str]:
        """Open native OS directory selection dialog."""
        from photo_culler.desktop.dialogs import select_folder_dialog

        selected = select_folder_dialog(self._window)
        return selected if isinstance(selected, str) else None

    def save_file(self) -> Optional[str]:
        """Open native OS save file dialog."""
        if self._window:
            try:
                import webview

                result = self._window.create_file_dialog(webview.SAVE_DIALOG)
                if result and isinstance(result, (list, tuple)) and len(result) > 0:
                    return result[0]
                elif result and isinstance(result, str):
                    return result
            except Exception:
                pass
        return None

    def toggle_fullscreen(self) -> None:
        """Toggle fullscreen mode of the desktop window."""
        if self._window:
            try:
                self._window.toggle_fullscreen()
            except Exception:
                pass

    def show_notification(self, title: str, message: str) -> None:
        """Display a system notification or window alert."""
        if self._window:
            try:
                self._window.evaluate_js(f"alert('{title}: {message}');")
            except Exception:
                pass

    def reveal_in_file_manager(self, path: str) -> bool:
        """Open file manager showing/highlighting the specified file or directory."""
        import subprocess
        import sys
        from pathlib import Path

        try:
            path_obj = Path(path).resolve()
            if not path_obj.exists():
                return False

            if sys.platform == "win32":
                subprocess.run(["explorer", "/select,", str(path_obj)], check=False)
            elif sys.platform == "darwin":
                subprocess.run(["open", "-R", str(path_obj)], check=False)
            else:
                # Linux: open parent folder via xdg-open
                parent = path_obj.parent if path_obj.is_file() else path_obj
                subprocess.run(["xdg-open", str(parent)], check=False)
            return True
        except Exception:
            return False

    def get_platform_info(self) -> dict:
        """Retrieve platform and operating system details."""
        import platform

        return {
            "system": platform.system(),
            "release": platform.release(),
            "version": platform.version(),
            "machine": platform.machine(),
        }


def run_desktop(
    catalog_path: Optional[Union[str, Path]] = None,
    fullscreen: bool = False,
    width: int = 1440,
    height: int = 900,
) -> None:
    """Launch local FastAPI server in background thread and open native pywebview window."""
    try:
        import webview
    except ImportError:
        raise RuntimeError("pywebview is not installed. Install with: pip install 'photo-culler[desktop]'")

    configure_desktop_logging()
    port = find_free_port()
    token = secrets.token_urlsafe(16)
    app = create_app(
        catalog_path=catalog_path or default_desktop_catalog_path(),
        desktop_token=token,
    )

    config = uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning")
    server = uvicorn.Server(config)

    # Start Uvicorn in background daemon thread
    server_thread = threading.Thread(target=server.run, daemon=True)
    server_thread.start()

    url = f"http://127.0.0.1:{port}"
    wait_until_ready(url, token=token)

    api = DesktopApi()
    window = webview.create_window(
        title="Photo Culler",
        url=f"{url}/?token={token}",
        width=width,
        height=height,
        min_size=(1000, 650),
        fullscreen=fullscreen,
        js_api=api,
    )
    api.set_window(window)

    webview.start()

    # Graceful shutdown after webview window exits
    server.should_exit = True
    server_thread.join(timeout=5.0)
