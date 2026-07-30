"""Unit tests for FastAPI Web UI application and endpoints."""

import pytest
from fastapi.testclient import TestClient

from photo_culler.web.app import create_app


@pytest.fixture
def web_client(tmp_path):
    cat_file = tmp_path / "catalog.db"
    app = create_app(catalog_path=cat_file)
    return TestClient(app)


def test_api_health(web_client):
    response = web_client.get("/api/health")
    assert response.status_code == 200
    assert response.json() == {"status": "ok", "app": "photo-culler", "version": "0.1.0"}


def test_dashboard_page(web_client):
    response = web_client.get("/")
    assert response.status_code == 200
    assert "Dashboard del Catálogo" in response.text


def test_library_page(web_client):
    response = web_client.get("/library")
    assert response.status_code == 200
    assert "Biblioteca de Fotografías" in response.text


def test_library_pagination_and_filters(web_client):
    # Test library route with query params
    response = web_client.get("/library?page=1&limit=5&sort=score_desc&decision=best")
    assert response.status_code == 200
    assert "Biblioteca de Fotografías" in response.text


def test_gallery_import_api_and_empty_state(web_client, tmp_path):
    source = tmp_path / "photos"
    source.mkdir()
    response = web_client.get("/library")
    assert "Importar galería" in response.text
    assert "nunca los modifica" in response.text

    created = web_client.post("/api/v1/galleries", json={"name": "Wedding"})
    assert created.status_code == 201
    gallery_id = created.json()["id"]
    queued = web_client.post(
        f"/api/v1/galleries/{gallery_id}/imports",
        json={"path": str(source), "recursive": True},
    )
    assert queued.status_code == 202
    job = web_client.get(f"/api/v1/import-jobs/{queued.json()['job_id']}")
    assert job.status_code == 200
    assert job.json()["contract_version"] == 1
    galleries = web_client.get("/api/v1/galleries").json()
    assert galleries["items"][0]["name"] == "Wedding"


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


def test_analysis_start_and_sse_progress(web_client):
    # Start analysis
    response = web_client.post("/analysis/start", data={"profile": "fast"})
    assert response.status_code == 200
    assert response.json()["status"] in ("ok", "error")

    # Connect to progress events
    with web_client.stream("GET", "/analysis/progress") as stream:
        # Check first line of SSE stream
        for line in stream.iter_lines():
            if line.startswith("data:"):
                import json

                data = json.loads(line[5:].strip())
                assert "status" in data
                assert "progress" in data
                break
