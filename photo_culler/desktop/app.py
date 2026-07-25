"""Native pywebview Desktop Window Launcher."""

import socket
import threading
import time
from pathlib import Path
from typing import Optional, Union

import uvicorn

from photo_culler.web.app import create_app


def find_free_port() -> int:
    """Find an available local TCP port on 127.0.0.1."""
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as s:
        s.bind(("127.0.0.1", 0))
        return s.getsockname()[1]


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

    port = find_free_port()
    app = create_app(catalog_path=catalog_path)

    config = uvicorn.Config(app, host="127.0.0.1", port=port, log_level="warning")
    server = uvicorn.Server(config)

    # Start Uvicorn in background daemon thread
    server_thread = threading.Thread(target=server.run, daemon=True)
    server_thread.start()

    url = f"http://127.0.0.1:{port}"
    time.sleep(0.5)  # Wait for server bind

    webview.create_window(
        title="Photo Culler",
        url=url,
        width=width,
        height=height,
        min_size=(1000, 650),
        fullscreen=fullscreen,
    )

    webview.start()
