from photo_culler.desktop.tauri_backend import parser


def test_tauri_backend_requires_ephemeral_session_arguments():
    args = parser().parse_args(["--port", "42123", "--token", "secret"])

    assert args.host == "127.0.0.1"
    assert args.port == 42123
    assert args.token == "secret"
