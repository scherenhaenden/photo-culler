"""Unit tests for the dependency-light clickable Linux launcher."""

import os

import pytest

from photo_culler.desktop.linux_launcher import chrome_command, find_chrome


def test_find_chrome_accepts_an_explicit_executable(monkeypatch, tmp_path):
    browser = tmp_path / "custom browser"
    browser.write_text("#!/bin/sh\nexit 0\n")
    browser.chmod(0o755)
    monkeypatch.setenv("PHOTO_CULLER_CHROME", str(browser))

    assert find_chrome() == os.path.abspath(browser)


def test_find_chrome_rejects_an_invalid_override(monkeypatch, tmp_path):
    missing = tmp_path / "missing-browser"
    monkeypatch.setenv("PHOTO_CULLER_CHROME", str(missing))

    with pytest.raises(RuntimeError, match="not an executable file"):
        find_chrome()


def test_chrome_command_is_an_isolated_app_window(tmp_path):
    command = chrome_command(
        "/usr/bin/chromium", "http://127.0.0.1:1234/?token=secret", str(tmp_path), ["--disable-gpu"]
    )

    assert command[0] == "/usr/bin/chromium"
    assert "--app=http://127.0.0.1:1234/?token=secret" in command
    assert f"--user-data-dir={tmp_path}" in command
    assert "--disable-sync" in command
    assert command[-1] == "--disable-gpu"
