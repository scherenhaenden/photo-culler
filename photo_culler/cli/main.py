"""Command Line Interface entry point module."""

import sys
from pathlib import Path

# Ensure photo_culler package root is on path when executed directly
project_root = Path(__file__).resolve().parent.parent.parent
if str(project_root) not in sys.path:
    sys.path.insert(0, str(project_root))

from photo_culler.cli.app import app, main

if __name__ == "__main__":
    main()
