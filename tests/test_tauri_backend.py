import pytest

from photo_culler.desktop.tauri_backend import main, parser


def test_tauri_backend_requires_ephemeral_session_arguments():
    args = parser().parse_args(["--port", "42123"])

    assert args.host == "127.0.0.1"
    assert args.port == 42123


def test_tauri_backend_rejects_non_loopback_host(monkeypatch):
    monkeypatch.setenv("PHOTO_CULLER_TAURI_TOKEN", "secret")

    with pytest.raises(ValueError, match="127.0.0.1"):
        main(["--host", "0.0.0.0", "--port", "1"])


def test_tauri_backend_requires_token_environment_variable(monkeypatch):
    monkeypatch.delenv("PHOTO_CULLER_TAURI_TOKEN", raising=False)

    with pytest.raises(ValueError, match="session token"):
        main(["--port", "1"])
