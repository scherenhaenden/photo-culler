"""Unit tests for FastAPI security, desktop token, host middleware, and system metrics helpers."""

import time
from threading import Lock
from types import SimpleNamespace

import pytest
from fastapi.testclient import TestClient

from photo_culler.web.app import create_app


def test_desktop_token_middleware_blocked(tmp_path):
    # Create an app with a desktop token
    cat_file = tmp_path / "catalog.db"
    app = create_app(catalog_path=cat_file, desktop_token="secure_token")
    client = TestClient(app)

    # 1. Access without token should be Forbidden (403)
    response = client.get("/api/health", headers={"Host": "127.0.0.1"})
    assert response.status_code == 403
    assert "Forbidden: Invalid desktop session token" in response.text

    # 2. Access with external Host header should be Forbidden (403)
    response = client.get("/api/health?token=secure_token", headers={"Host": "malicious-domain.com"})
    assert response.status_code == 403
    assert "Forbidden: Access only allowed via localhost/127.0.0.1" in response.text

    # 3. Access with valid token and local Host should succeed (200)
    response = client.get("/api/health?token=secure_token", headers={"Host": "127.0.0.1"})
    assert response.status_code == 200
    assert response.json()["status"] == "ok"
    # Ensure security headers are injected
    assert "default-src 'self'" in response.headers["Content-Security-Policy"]
    assert response.headers["X-Frame-Options"] == "DENY"
    assert response.headers["Referrer-Policy"] == "no-referrer"

    # 4. Successive requests should work with the cookie set
    cookie = response.cookies.get("desktop_token")
    assert cookie == "secure_token"

    # Access without query param but with cookie should succeed
    response_cookie = client.get(
        "/api/health", cookies={"desktop_token": "secure_token"}, headers={"Host": "localhost"}
    )
    assert response_cookie.status_code == 200


def test_system_usage_uses_first_gpu_line(monkeypatch, tmp_path):
    cat_file = tmp_path / "catalog.db"
    app = create_app(catalog_path=cat_file)
    with TestClient(app) as web_client:
        monkeypatch.setattr("shutil.which", lambda command: "/usr/bin/nvidia-smi")
        monkeypatch.setattr(
            "subprocess.run",
            lambda *args, **kwargs: SimpleNamespace(stdout="10, GPU0\n20, GPU1"),
        )

        data = web_client.get("/api/v1/system-usage").json()

        assert data["gpu_system"] == 10.0
        assert data["gpu_name"] == "GPU0"


def test_system_usage_serializes_shared_cpu_sampling(monkeypatch):
    from concurrent.futures import ThreadPoolExecutor
    from types import ModuleType

    from photo_culler.web.routes.api import get_system_usage

    active = 0
    peak_active = 0
    counter_lock = Lock()

    def sample_cpu(*_args, **_kwargs):
        nonlocal active, peak_active
        with counter_lock:
            active += 1
            peak_active = max(peak_active, active)
        time.sleep(0.01)
        with counter_lock:
            active -= 1
        return 10.0

    fake_psutil = ModuleType("psutil")
    fake_psutil.cpu_percent = sample_cpu
    fake_psutil.cpu_count = lambda: 4
    fake_psutil.Process = lambda _pid: SimpleNamespace(cpu_percent=sample_cpu)
    monkeypatch.setitem(__import__("sys").modules, "psutil", fake_psutil)
    request = SimpleNamespace(app=SimpleNamespace(state=SimpleNamespace()))

    with ThreadPoolExecutor(max_workers=4) as executor:
        list(executor.map(lambda _index: get_system_usage(request), range(4)))

    assert peak_active == 1
