"""Unit tests for the dependency-light clickable Linux launcher."""

import os

import pytest

from photo_culler.desktop import linux_launcher
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


def test_main_reports_a_server_thread_that_does_not_stop(monkeypatch):
    class StuckThread:
        def __init__(self, *args, **kwargs):
            pass

        def start(self):
            pass

        def join(self, timeout):
            assert timeout == 5

        def is_alive(self):
            return True

    class FakeServer:
        should_exit = False

        def __init__(self, config):
            pass

        def run(self):
            pass

    monkeypatch.setattr(linux_launcher, "find_chrome", lambda: "/usr/bin/chromium")
    monkeypatch.setattr(linux_launcher, "find_free_port", lambda: 9876)
    monkeypatch.setattr(linux_launcher, "create_app", lambda **kwargs: object())
    monkeypatch.setattr(linux_launcher, "wait_until_ready", lambda *args: None)
    monkeypatch.setattr(linux_launcher.threading, "Thread", StuckThread)
    monkeypatch.setattr(linux_launcher.uvicorn, "Server", FakeServer)
    monkeypatch.setattr(
        linux_launcher.subprocess,
        "run",
        lambda *args, **kwargs: type("Completed", (), {"returncode": 0})(),
    )

    with pytest.raises(RuntimeError, match="did not stop cleanly"):
        linux_launcher.main()


def test_main_stops_server_when_readiness_fails(monkeypatch):
    class FakeThread:
        instance = None

        def __init__(self, *args, **kwargs):
            type(self).instance = self
            self.joined = False

        def start(self):
            pass

        def join(self, timeout):
            assert timeout == 5
            self.joined = True

        def is_alive(self):
            return False

    class FakeServer:
        instance = None

        def __init__(self, config):
            type(self).instance = self
            self.should_exit = False

        def run(self):
            pass

    monkeypatch.setattr(linux_launcher, "find_chrome", lambda: "/usr/bin/chromium")
    monkeypatch.setattr(linux_launcher, "find_free_port", lambda: 9876)
    monkeypatch.setattr(linux_launcher, "create_app", lambda **kwargs: object())
    monkeypatch.setattr(
        linux_launcher, "wait_until_ready", lambda *args: (_ for _ in ()).throw(RuntimeError("not ready"))
    )
    monkeypatch.setattr(linux_launcher.threading, "Thread", FakeThread)
    monkeypatch.setattr(linux_launcher.uvicorn, "Server", FakeServer)

    with pytest.raises(RuntimeError, match="not ready"):
        linux_launcher.main()

    assert FakeServer.instance.should_exit is True
    assert FakeThread.instance.joined is True
