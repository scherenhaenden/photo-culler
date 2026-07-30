"""Unit tests for Typer CLI commands and helpers."""

import pytest
from PIL import Image
from typer.testing import CliRunner

from photo_culler.cli.app import app

runner = CliRunner()


@pytest.fixture
def cli_test_env(tmp_path):
    catalog_file = tmp_path / "test_catalog.db"
    media_dir = tmp_path / "media"
    media_dir.mkdir()

    img = Image.new("RGB", (400, 300), color=(60, 60, 60))
    img.save(media_dir / "DSC_500.JPG")
    with open(media_dir / "DSC_500.NEF", "wb") as f:
        f.write(b"HEADER" * 100)

    return {
        "catalog": catalog_file,
        "media": media_dir,
    }


def test_cli_init_command(cli_test_env):
    cat_path = str(cli_test_env["catalog"])
    result = runner.invoke(app, ["init", "--catalog", cat_path])
    assert result.exit_code == 0
    assert "Initialized" in result.stdout or "Catalog" in result.stdout


def test_cli_doctor_command():
    result = runner.invoke(app, ["doctor"])
    assert result.exit_code == 0
    assert "Diagnostics" in result.stdout


def test_cli_full_workflow(cli_test_env):
    cat_path = str(cli_test_env["catalog"])
    media_path = str(cli_test_env["media"])

    # 1. Scan
    res_scan = runner.invoke(app, ["--catalog", cat_path, "scan", media_path])
    assert res_scan.exit_code == 0
    assert "Indexed" in res_scan.stdout or "Discovered" in res_scan.stdout

    # 2. Photos List
    res_photos = runner.invoke(app, ["--catalog", cat_path, "photos"])
    assert res_photos.exit_code == 0

    # 3. Analyze
    res_analyze = runner.invoke(app, ["--catalog", cat_path, "analyze", media_path])
    assert res_analyze.exit_code == 0

    # 4. Evaluate
    res_eval = runner.invoke(app, ["--catalog", cat_path, "evaluate"])
    assert res_eval.exit_code == 0

    # 5. Report
    res_report = runner.invoke(app, ["--catalog", cat_path, "report"])
    assert res_report.exit_code == 0
    assert "Report" in res_report.stdout or "Culling" in res_report.stdout
