"""Executable entry point for the native desktop application."""

from photo_culler.desktop.app import run_desktop


def main() -> None:
    """Launch Photo Culler in its native desktop window."""
    run_desktop()


if __name__ == "__main__":
    main()
