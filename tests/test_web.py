"""Unit tests for FastAPI Web UI application and endpoints."""

import time
from threading import Event

import pytest
from fastapi.testclient import TestClient

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.models import Photo
from photo_culler.scanner.directory_scanner import DirectoryScanner
from photo_culler.web.app import create_app
from photo_culler.web.routes.analysis import AnalysisJobManager


@pytest.fixture
def web_client(tmp_path):
    cat_file = tmp_path / "catalog.db"
    app = create_app(catalog_path=cat_file)
    with TestClient(app) as client:
        yield client


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
    jobs = web_client.get("/api/v1/import-jobs?limit=10")
    assert jobs.status_code == 200
    assert jobs.json()["items"][0]["id"] == queued.json()["job_id"]
    galleries = web_client.get("/api/v1/galleries").json()
    assert galleries["items"][0]["name"] == "Wedding"
    sources = web_client.get(f"/api/v1/galleries/{gallery_id}/sources")
    assert sources.status_code == 200
    assert sources.json()["items"][0]["status"] == "online"
    assert web_client.get("/api/v1/galleries/unknown/sources").status_code == 404
    library = web_client.get("/library").text
    assert "Importaciones recientes" in library
    assert "Fuentes configuradas" in library
    assert 'name="gallery_id"' in library
    assert "Revisiones de escaneo" in library
    revisions = web_client.get(f"/api/v1/scan-revisions?gallery_id={gallery_id}")
    assert revisions.status_code == 200
    assert revisions.json()["items"][0]["gallery_id"] == gallery_id
    rescanned = web_client.post(f"/api/v1/galleries/{gallery_id}/rescan")
    assert rescanned.status_code == 202
    assert len(rescanned.json()["job_ids"]) == 1
    assert web_client.post("/api/v1/galleries/unknown/rescan").status_code == 404


def test_import_estimate_api(web_client, tmp_path):
    source = tmp_path / "photos"
    source.mkdir()
    (source / "frame.nef").write_bytes(b"raw")
    (source / "frame.jpg").write_bytes(b"jpeg")

    estimate = web_client.post(
        "/api/v1/import-estimates",
        json={"path": str(source), "recursive": True, "exclude_patterns": ["*.nef"]},
    )

    assert estimate.status_code == 200
    assert estimate.json()["logical_photos"] == 1
    assert estimate.json()["total_files"] == 1
    assert (
        web_client.post(
            "/api/v1/import-estimates",
            json={"path": str(tmp_path / "missing"), "recursive": True},
        ).status_code
        == 404
    )


def test_library_filters_by_active_gallery(web_client):
    app = web_client.app
    first_gallery = app.state.gallery_imports.create_gallery("First shoot")
    second_gallery = app.state.gallery_imports.create_gallery("Second shoot")
    with app.state.db_engine.session() as session:
        first = PhotoRepository(session).save_photo(Photo("first-photo", "First_frame"))
        first.gallery_id = first_gallery
        second = PhotoRepository(session).save_photo(Photo("second-photo", "Second_frame"))
        second.gallery_id = second_gallery

    first_page = web_client.get(f"/library?gallery_id={first_gallery}")
    second_page = web_client.get(f"/library?gallery_id={second_gallery}")

    assert "Galería activa: <strong>First shoot</strong>" in first_page.text
    assert "First_frame" in first_page.text
    assert "Second_frame" not in first_page.text
    assert "Second_frame" in second_page.text
    assert "First_frame" not in second_page.text


@pytest.mark.parametrize("name", ["", "   \t\n"])
def test_create_gallery_rejects_blank_names(web_client, name):
    response = web_client.post("/api/v1/galleries", json={"name": name})
    assert response.status_code == 422


def test_import_api_maps_invalid_sources(web_client, tmp_path):
    created = web_client.post("/api/v1/galleries", json={"name": "Events"})
    gallery_id = created.json()["id"]

    missing = web_client.post(
        f"/api/v1/galleries/{gallery_id}/imports",
        json={"path": str(tmp_path / "missing"), "recursive": True},
    )
    assert missing.status_code == 404

    file_source = tmp_path / "single.jpg"
    file_source.write_bytes(b"not-an-image")
    not_directory = web_client.post(
        f"/api/v1/galleries/{gallery_id}/imports",
        json={"path": str(file_source), "recursive": True},
    )
    assert not_directory.status_code == 422

    unknown_gallery = web_client.post(
        "/api/v1/galleries/unknown/imports",
        json={"path": str(tmp_path), "recursive": True},
    )
    assert unknown_gallery.status_code == 404


def test_cancel_import_api_distinguishes_job_states(web_client, tmp_path, monkeypatch):
    source = tmp_path / "photos"
    source.mkdir()
    scan_started = Event()
    release_scan = Event()

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        release_scan.wait(timeout=2)
        yield from ()

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    created = web_client.post("/api/v1/galleries", json={"name": "Wedding"})
    gallery_id = created.json()["id"]
    queued = web_client.post(
        f"/api/v1/galleries/{gallery_id}/imports",
        json={"path": str(source), "recursive": True},
    )
    job_id = queued.json()["job_id"]
    assert scan_started.wait(timeout=2)

    cancelled = web_client.post(f"/api/v1/import-jobs/{job_id}/cancel")
    assert cancelled.status_code == 202
    release_scan.set()

    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        job = web_client.get(f"/api/v1/import-jobs/{job_id}").json()
        if job["state"] == "cancelled":
            break
        time.sleep(0.01)
    assert job["state"] == "cancelled"
    assert web_client.post(f"/api/v1/import-jobs/{job_id}/cancel").status_code == 409
    assert web_client.post("/api/v1/import-jobs/unknown/cancel").status_code == 404


def test_pause_and_resume_import_api(web_client, tmp_path, monkeypatch):
    source = tmp_path / "photos"
    source.mkdir()
    scan_started = Event()
    release_scan = Event()

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        release_scan.wait(timeout=2)
        yield from ()

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    gallery_id = web_client.post("/api/v1/galleries", json={"name": "Events"}).json()["id"]
    job_id = web_client.post(
        f"/api/v1/galleries/{gallery_id}/imports",
        json={"path": str(source), "recursive": True},
    ).json()["job_id"]
    assert scan_started.wait(timeout=2)

    paused = web_client.post(f"/api/v1/import-jobs/{job_id}/pause")
    assert paused.status_code == 202
    assert web_client.get(f"/api/v1/import-jobs/{job_id}").json()["state"] == "paused"
    assert web_client.post(f"/api/v1/import-jobs/{job_id}/pause").status_code == 409

    release_scan.set()
    resumed = web_client.post(f"/api/v1/import-jobs/{job_id}/resume")
    assert resumed.status_code == 202
    assert web_client.post(f"/api/v1/import-jobs/{job_id}/resume").status_code == 409
    assert web_client.post("/api/v1/import-jobs/unknown/pause").status_code == 404
    assert web_client.post("/api/v1/import-jobs/unknown/resume").status_code == 404


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


def test_analysis_manager_is_application_scoped_and_uses_bounded_listeners(tmp_path):
    first_app = create_app(catalog_path=tmp_path / "first.db")
    second_app = create_app(catalog_path=tmp_path / "second.db")
    assert first_app.state.analysis_jobs is not second_app.state.analysis_jobs

    manager = AnalysisJobManager()
    listener = manager.register_listener()
    for index in range(20):
        manager.message = str(index)
        manager._notify_listeners()
    assert listener.qsize() == 8
    manager.unregister_listener(listener)
    first_app.state.db_engine.close()
    second_app.state.db_engine.close()


def test_analysis_cooperative_controls_and_idle_conflicts(web_client):
    manager = web_client.app.state.analysis_jobs
    manager.is_running = True

    assert manager.pause() is True
    assert manager.snapshot()["status"] == "paused"
    assert manager.resume() is True
    assert manager.snapshot()["status"] == "running"
    assert manager.cancel() is True

    manager.is_running = False
    assert web_client.post("/analysis/pause").status_code == 409
    assert web_client.post("/analysis/resume").status_code == 409
    assert web_client.post("/analysis/cancel").status_code == 409
    page = web_client.get("/analysis")
    assert "Pausar" in page.text
    assert "Reanudar" in page.text
    assert "Cancelar" in page.text
