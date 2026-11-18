"""Unit and integration tests for NAS Thermal SDK, REST API, and CLI commands."""

import os
import pytest
from fastapi.testclient import TestClient
from typer.testing import CliRunner

from photo_culler.cli.app import app as cli_app
from photo_culler.nas import NASManager, ThermalSensor
from photo_culler.web.app import create_app
from photo_culler.web.routes.analysis import AnalysisJobManager


def test_thermal_sensor_mock():
    """Test that ThermalSensor correctly returns mocked temperatures."""
    sensor = ThermalSensor()
    sensor.set_mock_provider(lambda: 42.5)
    assert sensor.read_temperature() == 42.5


def test_thermal_sensor_fallback(tmp_path):
    """Test that ThermalSensor falls back gracefully when paths do not exist."""
    sensor = ThermalSensor()
    # Override standard search paths with empty temporary directories
    sensor.thermal_paths = [tmp_path / "thermal", tmp_path / "hwmon"]
    assert sensor.read_temperature() == 35.0


def test_nas_manager_polling_and_throttling():
    """Test that NASManager triggers pause/resume on AnalysisJobManager."""
    job_manager = AnalysisJobManager()
    # Mock analysis is running
    job_manager.is_running = True
    job_manager.status = "running"

    nas_manager = NASManager(
        analysis_jobs=job_manager,
        high_temp=70.0,
        low_temp=55.0,
        interval=0.1,
        enabled=False,  # Keep background polling disabled during unit test
    )

    temp_store = {"current": 40.0}
    nas_manager.sensor.set_mock_provider(lambda: temp_store["current"])

    # 1. Run step with normal temperature
    nas_manager._check_temperature_and_throttle()
    assert job_manager.status == "running"
    assert nas_manager.status == "normal"

    # 2. Trigger hot threshold (75°C >= 70°C)
    temp_store["current"] = 75.0
    nas_manager._check_temperature_and_throttle()
    assert job_manager.status == "paused"
    assert nas_manager.status == "throttled"
    assert nas_manager.snapshot()["paused_by_thermal"] is True

    # 3. Stay at high temp
    temp_store["current"] = 72.0
    nas_manager._check_temperature_and_throttle()
    assert job_manager.status == "paused"

    # 4. Return to normal temp (but not cooled down to low_temp yet)
    temp_store["current"] = 65.0
    nas_manager._check_temperature_and_throttle()
    assert job_manager.status == "paused"  # Should remain paused

    # 5. Cool down to safe range (50°C <= 55°C)
    temp_store["current"] = 50.0
    nas_manager._check_temperature_and_throttle()
    assert job_manager.status == "running"
    assert nas_manager.status == "normal"
    assert nas_manager.snapshot()["paused_by_thermal"] is False


def test_nas_manager_config_changes():
    """Test that setting config dynamically alters NASManager parameters."""
    job_manager = AnalysisJobManager()
    nas_manager = NASManager(analysis_jobs=job_manager)

    nas_manager.set_config(high_temp=80.0, low_temp=65.0, interval=10.0, enabled=False)
    assert nas_manager.high_temp == 80.0
    assert nas_manager.low_temp == 65.0
    assert nas_manager.interval == 10.0
    assert nas_manager.enabled is False


def test_nas_api_endpoints(tmp_path):
    """Test standard REST API endpoints for NAS temperature monitor."""
    cat_file = tmp_path / "catalog.db"
    app = create_app(catalog_path=cat_file)
    client = TestClient(app)

    # Injects mock provider into global application NAS Manager
    nas_manager = app.state.nas_manager
    nas_manager.sensor.set_mock_provider(lambda: 48.0)
    nas_manager._check_temperature_and_throttle()

    # 1. Get status
    resp = client.get("/api/v1/nas/status")
    assert resp.status_code == 200
    data = resp.json()
    assert data["temperature"] == 48.0
    assert data["status"] == "normal"
    assert data["monitoring_enabled"] is False

    # 2. Post invalid config (low_temp >= high_temp)
    resp = client.post(
        "/api/v1/nas/config",
        json={"high_temp_threshold": 60.0, "low_temp_threshold": 65.0},
    )
    assert resp.status_code == 422

    # 3. Post valid config
    resp = client.post(
        "/api/v1/nas/config",
        json={
            "high_temp_threshold": 80.0,
            "low_temp_threshold": 60.0,
            "interval": 2.5,
            "monitoring_enabled": True,
        },
    )
    assert resp.status_code == 200
    assert nas_manager.high_temp == 80.0
    assert nas_manager.low_temp == 60.0
    assert nas_manager.interval == 2.5
    assert nas_manager.enabled is True

    # Tear down
    nas_manager.stop()


def test_nas_cli_validation(tmp_path):
    """Test that photo-culler nas command validates parameters."""
    runner = CliRunner()
    catalog_path = tmp_path / "catalog.db"

    # Try starting the NAS server with invalid temperature thresholds
    result = runner.invoke(
        cli_app,
        [
            "-c",
            str(catalog_path),
            "nas",
            "--high-temp",
            "50.0",
            "--low-temp",
            "60.0",
        ],
    )
    assert result.exit_code == 1
    assert "Error: low-temp threshold must be strictly lower" in result.output
