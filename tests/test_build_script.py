"""Smoke tests for the selectable local build entrypoint."""

import subprocess
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parents[1]
BUILD_SCRIPT = PROJECT_ROOT / "scripts" / "build.sh"


def test_build_script_documents_targets_and_exclusions():
    result = subprocess.run([str(BUILD_SCRIPT), "--help"], cwd=PROJECT_ROOT, capture_output=True, text=True)

    assert result.returncode == 0
    assert "--all" in result.stdout
    assert "--no-linux" in result.stdout
    assert "--rust-cli" in result.stdout


def test_build_script_rejects_unknown_options():
    result = subprocess.run([str(BUILD_SCRIPT), "--unknown"], cwd=PROJECT_ROOT, capture_output=True, text=True)

    assert result.returncode == 2
    assert "Unknown option" in result.stderr
