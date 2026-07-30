"""Extended unit tests for CLI subcommands."""

import pytest
from PIL import Image
from typer.testing import CliRunner

from photo_culler.cli.app import app

runner = CliRunner()


@pytest.fixture
def cli_full_env(tmp_path):
    cat_file = tmp_path / "catalog.db"
    media_dir = tmp_path / "photos"
    media_dir.mkdir()

    img = Image.new("RGB", (300, 300), color=(80, 80, 80))
    img.save(media_dir / "DSC_001.JPG")

    return {"cat": str(cat_file), "media": str(media_dir)}


def test_cli_volumes_and_config(cli_full_env):
    res_vol = runner.invoke(app, ["--catalog", cli_full_env["cat"], "volumes"])
    assert res_vol.exit_code == 0

    res_cfg = runner.invoke(app, ["config"])
    assert res_cfg.exit_code == 0
    assert "catalog.path" in res_cfg.stdout


def test_cli_group_bursts_sessions_decisions(cli_full_env):
    # Scan first
    runner.invoke(app, ["--catalog", cli_full_env["cat"], "scan", cli_full_env["media"]])

    # Group
    res_group = runner.invoke(app, ["--catalog", cli_full_env["cat"], "group"])
    assert res_group.exit_code == 0

    # Bursts
    res_bursts = runner.invoke(app, ["--catalog", cli_full_env["cat"], "bursts"])
    assert res_bursts.exit_code == 0

    # Sessions
    res_sess = runner.invoke(app, ["--catalog", cli_full_env["cat"], "sessions"])
    assert res_sess.exit_code == 0

    # Decisions
    res_dec = runner.invoke(
        app, ["--catalog", cli_full_env["cat"], "decisions", "set", "--decision", "keep", "--confirm"]
    )
    assert res_dec.exit_code == 0
    assert "Updated" in res_dec.stdout


def test_cli_verify_command(cli_full_env):
    res_ver = runner.invoke(app, ["verify", cli_full_env["media"]])
    assert res_ver.exit_code == 0
    assert "Verification Report" in res_ver.stdout
