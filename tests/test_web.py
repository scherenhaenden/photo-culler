"""Unit tests for FastAPI Web UI application and endpoints."""

import time
from datetime import datetime, timedelta
from threading import Event

import pytest
from fastapi.testclient import TestClient
from PIL import Image

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import FileRole
from photo_culler.core.models import FileRecord, MetadataRecord, Photo
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
    assert "Confirmar importación" not in response.text
    assert "/api/v1/import-estimates" not in response.text


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
                assert "profile" in data
                break


def test_import_then_immediate_analysis_waits_for_catalog(web_client, tmp_path, monkeypatch):
    """One import action followed immediately by analysis must include imported photos."""
    source = tmp_path / "photos"
    source.mkdir()
    Image.new("RGB", (48, 32), color=(120, 80, 40)).save(source / "frame.jpg")
    scan_started = Event()
    release_scan = Event()
    original_scan = DirectoryScanner.scan

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        assert release_scan.wait(timeout=2)
        yield from original_scan(self, directory, recursive=recursive)

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    gallery_id = web_client.post("/api/v1/galleries", json={"name": "Immediate"}).json()["id"]
    queued = web_client.post(
        f"/api/v1/galleries/{gallery_id}/imports",
        json={"path": str(source), "recursive": True},
    )
    assert queued.status_code == 202
    assert scan_started.wait(timeout=2)

    analysis = web_client.post("/analysis/start", data={"profile": "fast"})
    assert analysis.json()["status"] == "ok"
    deadline = time.monotonic() + 2
    while time.monotonic() < deadline:
        if "Importación en curso" in str(web_client.app.state.analysis_jobs.snapshot()["message"]):
            break
        time.sleep(0.01)
    assert "Importación en curso" in str(web_client.app.state.analysis_jobs.snapshot()["message"])

    release_scan.set()
    deadline = time.monotonic() + 8
    while time.monotonic() < deadline and web_client.app.state.analysis_jobs.is_running:
        time.sleep(0.02)

    job = web_client.get(f"/api/v1/import-jobs/{queued.json()['job_id']}").json()
    analysis_result = web_client.app.state.analysis_jobs.snapshot()
    assert job["state"] == "completed"
    assert job["imported"] == 1
    assert analysis_result["status"] == "completed"
    assert analysis_result["total"] == 1
    assert analysis_result["processed"] == 1
    assert analysis_result["profile"] == "fast"
    assert "1 fotos analizadas" in str(analysis_result["message"])
    assert len(web_client.get("/api/photos").json()) == 1
    thumbnail = web_client.get("/thumbnails/" + web_client.get("/api/photos").json()[0]["photo_id"] + "/800")
    assert thumbnail.status_code == 200
    assert thumbnail.headers["content-type"] == "image/jpeg"
    assert len(thumbnail.content) > 0


def test_analysis_rejects_unknown_profile(web_client):
    response = web_client.post("/analysis/start", data={"profile": "imaginary"})
    assert response.status_code == 422


def test_analysis_profiles_can_be_inspected_created_updated_and_deleted(web_client):
    profiles = web_client.get("/analysis/profiles")
    assert profiles.status_code == 200
    assert {profile["id"] for profile in profiles.json()["profiles"]} >= {"fast", "technical", "concert"}
    assert len(next(profile for profile in profiles.json()["profiles"] if profile["id"] == "fast")["analyzers"]) == 4

    payload = {
        "name": "Retrato nocturno",
        "description": "Prioriza foco y ruido.",
        "analyzers": ["corruption", "sharpness", "noise"],
        "weights": {"sharpness": 0.7, "exposure": 0, "clipping": 0, "noise": 0.3},
        "clipping_mode": "standard",
    }
    created = web_client.post("/analysis/profiles", json=payload)
    assert created.status_code == 201
    profile = created.json()["profile"]
    assert profile["id"] == "retrato-nocturno"
    assert profile["builtin"] is False

    payload["description"] = "Ajustado por el usuario."
    updated = web_client.put("/analysis/profiles/retrato-nocturno", json=payload)
    assert updated.status_code == 200
    assert updated.json()["profile"]["description"] == "Ajustado por el usuario."

    deleted = web_client.delete("/analysis/profiles/retrato-nocturno")
    assert deleted.status_code == 204
    assert web_client.post("/analysis/start", data={"profile": "retrato-nocturno"}).status_code == 422


def test_builtin_analysis_profile_can_be_edited_and_restored(web_client):
    fast = next(
        profile for profile in web_client.get("/analysis/profiles").json()["profiles"] if profile["id"] == "fast"
    )
    fast["description"] = "Mi ajuste temporal"
    updated = web_client.put("/analysis/profiles/fast", json=fast)
    assert updated.status_code == 200
    assert updated.json()["profile"]["description"] == "Mi ajuste temporal"
    assert web_client.delete("/analysis/profiles/fast").status_code == 422

    restored = web_client.post("/analysis/profiles/fast/restore")
    assert restored.status_code == 200
    assert restored.json()["profile"]["description"] != "Mi ajuste temporal"


def test_similarity_groups_page_shows_recommended_photo(web_client):
    photos = [
        Photo("group-low", "Group_low", burst_id="similar-example", score=0.4),
        Photo("group-high", "Group_high", burst_id="similar-example", score=0.9),
    ]
    with web_client.app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        for photo in photos:
            repository.save_photo(photo)

    page = web_client.get("/groups")
    assert page.status_code == 200
    assert "Grupos de fotos parecidas" in page.text
    assert "Group_high" in page.text
    assert "RECOMENDADA" in page.text

    inspector = web_client.get("/photos/group-high?group=similar-example")
    assert inspector.status_code == 200
    assert 'data-group-id="similar-example"' in inspector.text
    assert "Siguiente del grupo" in inspector.text
    assert "?group=similar-example" in inspector.text


def test_group_similar_endpoint_persists_detected_groups(web_client, tmp_path):
    image = tmp_path / "similar.jpg"
    Image.new("RGB", (80, 60), (90, 120, 160)).save(image)
    stat = image.stat()
    captured = datetime(2026, 7, 31, 12, 0, 0)
    photos = [
        Photo(
            "similar-one",
            "similar-one",
            files=[FileRecord(image, FileRole.JPEG, stat.st_size, stat.st_mtime)],
            metadata=MetadataRecord(capture_time=captured),
        ),
        Photo(
            "similar-two",
            "similar-two",
            files=[FileRecord(image, FileRole.JPEG, stat.st_size, stat.st_mtime)],
            metadata=MetadataRecord(capture_time=captured + timedelta(seconds=1)),
        ),
    ]
    with web_client.app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        for photo in photos:
            repository.save_photo(photo)

    result = web_client.post("/analysis/group-similar")
    assert result.status_code == 200
    assert result.json()["groups"] == 1
    page = web_client.get("/groups")
    assert "similar-one" in page.text
    assert "similar-two" in page.text


def test_profiles_run_distinct_analyzer_sets_and_report_cache_usage(web_client, tmp_path):
    image_path = tmp_path / "profile-check.jpg"
    Image.new("RGB", (96, 64), color=(90, 120, 160)).save(image_path)
    photo = Photo(
        "profile-check",
        "profile-check",
        files=[
            FileRecord(
                path=image_path,
                role=FileRole.JPEG,
                size_bytes=image_path.stat().st_size,
                modified_time=image_path.stat().st_mtime,
            )
        ],
    )
    with web_client.app.state.db_engine.session() as session:
        PhotoRepository(session).save_photo(photo)

    def run(profile_id):
        assert web_client.post("/analysis/start", data={"profile": profile_id}).json()["status"] == "ok"
        deadline = time.monotonic() + 8
        while web_client.app.state.analysis_jobs.is_running and time.monotonic() < deadline:
            time.sleep(0.02)
        result = web_client.app.state.analysis_jobs.snapshot()
        assert result["status"] == "completed"
        return result

    fast = run("fast")
    technical = run("technical")
    technical_again = run("technical")
    concert = run("concert")

    assert (fast["executed_metrics"], fast["cached_metrics"]) == (4, 0)
    assert (technical["executed_metrics"], technical["cached_metrics"]) == (8, 0)
    assert (technical_again["executed_metrics"], technical_again["cached_metrics"]) == (0, 8)
    assert (concert["executed_metrics"], concert["cached_metrics"]) == (8, 0)

    detail = web_client.get("/photos/profile-check")
    assert detail.status_code == 200
    assert "Por qué esta puntuación" in detail.text
    assert "Perfil Concert Stage" in detail.text
    assert "peso" in detail.text
    assert "Ver original" in detail.text
    assert "const syncControls = (editDocument)" in detail.text


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
