"""Process-level end-to-end test for the finished easy Linux desktop path."""

import os
import subprocess
import sys
import textwrap

import pytest


def _browser_standin() -> str:
    return textwrap.dedent(
        """\
        #!/usr/bin/env python3
        import os
        import sys
        import urllib.request

        url = next(arg.removeprefix('--app=') for arg in sys.argv if arg.startswith('--app='))
        with urllib.request.urlopen(url, timeout=5) as response:
            body = response.read().decode('utf-8')
            assert response.status == 200
            assert 'Dashboard del Catálogo' in body
        with open(os.environ['PHOTO_CULLER_E2E_CAPTURE'], 'w', encoding='utf-8') as output:
            output.write(url)
        """
    )


@pytest.mark.e2e
def test_linux_launcher_opens_authenticated_ui_and_stops_server(tmp_path):
    """A browser stand-in loads the real UI; launcher must then exit cleanly."""
    capture = tmp_path / "desktop-page.txt"
    browser = tmp_path / "browser.py"
    browser.write_text(_browser_standin())
    browser.chmod(0o755)
    env = os.environ.copy()
    env.update(
        {
            "PHOTO_CULLER_CHROME": str(browser),
            "PHOTO_CULLER_E2E_CAPTURE": str(capture),
            "XDG_DATA_HOME": str(tmp_path / "data"),
            "XDG_STATE_HOME": str(tmp_path / "state"),
        }
    )

    result = subprocess.run(
        [sys.executable, "-m", "photo_culler.desktop.linux_launcher"],
        cwd=os.getcwd(),
        env=env,
        capture_output=True,
        text=True,
        timeout=15,
    )

    assert result.returncode == 0, result.stderr
    launched_url = capture.read_text()
    assert launched_url.startswith("http://127.0.0.1:")
    assert "?token=" in launched_url
    assert (tmp_path / "data" / "photo-culler" / "catalog.db").exists()
    assert (tmp_path / "state" / "photo-culler" / "photo-culler.log").exists()
