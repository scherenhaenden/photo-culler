"""Desktop launch-path behavior."""

from photo_culler.desktop.app import default_desktop_catalog_path, default_desktop_log_path


def test_default_desktop_catalog_is_stable_outside_builds(monkeypatch, tmp_path):
    data_home = tmp_path / "user-data"
    monkeypatch.setenv("XDG_DATA_HOME", str(data_home))

    catalog = default_desktop_catalog_path()

    assert catalog == data_home / "photo-culler" / "catalog.db"
    assert "builds" not in catalog.parts
    assert catalog.is_absolute()


def test_default_desktop_log_is_stable_outside_builds(monkeypatch, tmp_path):
    state_home = tmp_path / "user-state"
    monkeypatch.setenv("XDG_STATE_HOME", str(state_home))

    log_path = default_desktop_log_path()

    assert log_path == state_home / "photo-culler" / "photo-culler.log"
    assert "builds" not in log_path.parts
