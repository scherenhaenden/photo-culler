"""Loopback-only FastAPI sidecar owned by the Tauri desktop shell."""

from __future__ import annotations

import argparse
import os

import uvicorn

from photo_culler.desktop.app import configure_desktop_logging, default_desktop_catalog_path
from photo_culler.web.app import create_app


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description="Photo Culler Tauri local backend")
    result.add_argument("--host", default="127.0.0.1")
    result.add_argument("--port", type=int, required=True)
    return result


def main(argv: list[str] | None = None) -> None:
    args = parser().parse_args(argv)
    if args.host != "127.0.0.1":
        raise ValueError("the Tauri backend must listen on 127.0.0.1 only")
    token = os.environ.get("PHOTO_CULLER_TAURI_TOKEN")
    if not token:
        raise ValueError("the Tauri backend requires a session token")
    configure_desktop_logging()
    app = create_app(catalog_path=default_desktop_catalog_path(), desktop_token=token)
    uvicorn.run(app, host=args.host, port=args.port, log_level="warning")


if __name__ == "__main__":
    main()
