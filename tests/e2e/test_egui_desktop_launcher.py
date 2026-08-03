"""Process-level verification of the native egui desktop launcher."""

import os
import subprocess
import sys
import textwrap

import pytest


def _native_standin() -> str:
    return textwrap.dedent(
        f"""\
        #!{sys.executable}
        import os
        import urllib.request

        url = os.environ['PHOTO_CULLER_SERVER'] + '/api/health?token=' + os.environ['PHOTO_CULLER_SERVER_TOKEN']
        with urllib.request.urlopen(url, timeout=5) as response:
            assert response.status == 200
            assert response.read().decode('utf-8').find('photo-culler') >= 0
        with open(os.environ['PHOTO_CULLER_E2E_CAPTURE'], 'w', encoding='utf-8') as output:
            output.write(url)
        """
    )


@pytest.mark.e2e
def test_egui_launcher_serves_native_client_and_stops_cleanly(tmp_path):
    client = tmp_path / "native-client.py"
    client.write_text(_native_standin())
    client.chmod(0o755)
    capture = tmp_path / "native-url.txt"
    environment = os.environ.copy()
    environment.update(
        {
            "PHOTO_CULLER_EGUI_BINARY": str(client),
            "PHOTO_CULLER_E2E_CAPTURE": str(capture),
            "XDG_DATA_HOME": str(tmp_path / "data"),
            "XDG_STATE_HOME": str(tmp_path / "state"),
        }
    )

    result = subprocess.run(
        [sys.executable, "-m", "photo_culler.desktop.egui_launcher"],
        cwd=os.getcwd(),
        env=environment,
        capture_output=True,
        text=True,
        timeout=15,
    )

    assert result.returncode == 0, result.stderr
    assert capture.read_text().startswith("http://127.0.0.1:")
    assert (tmp_path / "data" / "photo-culler" / "catalog.db").exists()
    assert (tmp_path / "state" / "photo-culler" / "photo-culler.log").exists()
