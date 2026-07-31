"""Unit tests for FastAPI Web UI application and endpoints."""

import time
from datetime import datetime, timedelta
from threading import Event
from types import SimpleNamespace

import pytest
from fastapi.testclient import TestClient
from PIL import Image
from starlette.responses import StreamingResponse

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import FileDB, SessionDB
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
    assert 'id="language-picker"' in response.text


def test_language_query_localizes_html_and_persists_cookie(web_client):
    response = web_client.get("/?lang=de")

    assert response.status_code == 200
    assert '<html lang="de">' in response.text
    assert "Bibliothek" in response.text
    assert "photo_culler_locale=de" in response.headers["set-cookie"]


def test_i18n_middleware_does_not_buffer_streaming_html(web_client):
    async def stream():
        yield b'<html lang="es"><body>Biblioteca</body></html>'

    web_client.app.add_api_route(
        "/streaming-html", lambda: StreamingResponse(stream(), media_type="text/html"), methods=["GET"]
    )
    response = web_client.get("/streaming-html?lang=de")
    assert response.text == '<html lang="es"><body>Biblioteca</body></html>'
    assert "language-picker" not in response.text


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


def test_raw_jpeg_tandem_uses_jpeg_for_the_default_preview(web_client, tmp_path):
    raw = tmp_path / "frame.nef"
    raw.write_bytes(b"raw source is never modified")
    jpeg = tmp_path / "frame.jpg"
    Image.new("RGB", (48, 32), color=(90, 120, 160)).save(jpeg)
    with web_client.app.state.db_engine.session() as session:
        PhotoRepository(session).save_photo(
            Photo(
                "tandem",
                "frame",
                files=[
                    FileRecord(raw, FileRole.RAW, raw.stat().st_size, raw.stat().st_mtime),
                    FileRecord(jpeg, FileRole.JPEG, jpeg.stat().st_size, jpeg.stat().st_mtime),
                ],
            )
        )

    preview = web_client.get("/thumbnails/tandem/800")
    assert preview.status_code == 200
    assert preview.headers["content-type"] == "image/jpeg"
    assert raw.read_bytes() == b"raw source is never modified"


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


def test_sessions_web_workflow_combines_timeline_and_burst_engines(web_client):
    base_time = datetime(2026, 7, 25, 18, 0, 0)
    with web_client.app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        repository.save_photo(Photo("session-a", "A", metadata=MetadataRecord(capture_time=base_time)))
        repository.save_photo(
            Photo("session-b", "B", metadata=MetadataRecord(capture_time=base_time + timedelta(seconds=1)))
        )

    grouped = web_client.post(
        "/sessions/group",
        data={"profile": "hybrid", "timeline_gap_minutes": "15", "burst_gap_seconds": "1.5"},
        follow_redirects=True,
    )
    assert grouped.status_code == 200
    assert "Procesadas: 1 sesiones y 1 ráfagas" in grouped.text
    assert "Híbrido recomendado" in grouped.text

    with web_client.app.state.db_engine.session() as session:
        session_id = session.query(SessionDB).one().session_id
    renamed = web_client.post(
        f"/sessions/{session_id}/rename", data={"name": "Retratos familiares"}, follow_redirects=True
    )
    assert renamed.status_code == 200
    assert "Retratos familiares" in renamed.text

    deleted = web_client.post(f"/sessions/{session_id}/delete", follow_redirects=True)
    assert deleted.status_code == 200
    assert "Aún no hay sesiones" in deleted.text


def test_sessions_redirects_percent_encode_messages(web_client):
    response = web_client.post(
        "/sessions/group",
        data={"profile": "hybrid", "timeline_gap_minutes": "15", "burst_gap_seconds": "1.5"},
        follow_redirects=False,
    )
    assert response.status_code == 303
    assert "%C3%A1fagas" in response.headers["location"]


def test_sessions_web_workflow_reports_validation_and_missing_session_errors(web_client):
    invalid_group = web_client.post(
        "/sessions/group",
        data={"profile": "unknown", "timeline_gap_minutes": "15", "burst_gap_seconds": "1.5"},
    )
    assert invalid_group.status_code == 422
    assert invalid_group.json()["detail"] == "Unknown grouping profile"

    invalid_gap = web_client.post(
        "/sessions/group",
        data={"profile": "hybrid", "timeline_gap_minutes": "0", "burst_gap_seconds": "1.5"},
    )
    assert invalid_gap.status_code == 422
    assert "Timeline gap" in invalid_gap.json()["detail"]

    missing_rename = web_client.post("/sessions/missing/rename", data={"name": "Missing"})
    assert missing_rename.status_code == 404
    invalid_name = web_client.post("/sessions/missing/rename", data={"name": "   "})
    assert invalid_name.status_code == 422
    missing_delete = web_client.post("/sessions/missing/delete")
    assert missing_delete.status_code == 404


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

    def run(profile_id, scope="all"):
        assert web_client.post("/analysis/start", data={"profile": profile_id, "scope": scope}).json()["status"] == "ok"
        deadline = time.monotonic() + 8
        while web_client.app.state.analysis_jobs.is_running and time.monotonic() < deadline:
            time.sleep(0.02)
        result = web_client.app.state.analysis_jobs.snapshot()
        assert result["status"] == "completed"
        return result

    fast = run("fast")
    fast_remaining = run("fast", "remaining")
    with web_client.app.state.db_engine.session() as session:
        session.query(FileDB).filter(FileDB.relative_path == str(image_path)).update(
            {FileDB.modified_time: time.time() + 1}
        )
    changed_fast = run("fast", "remaining")
    technical = run("technical")
    technical_again = run("technical")
    concert = run("concert")

    assert (fast["executed_metrics"], fast["cached_metrics"]) == (4, 0)
    assert fast_remaining["total"] == 0
    assert "No quedan fotos pendientes" in str(fast_remaining["message"])
    assert (changed_fast["total"], changed_fast["executed_metrics"], changed_fast["cached_metrics"]) == (1, 0, 4)
    assert (technical["executed_metrics"], technical["cached_metrics"]) == (4, 4)
    assert (technical_again["executed_metrics"], technical_again["cached_metrics"]) == (0, 8)
    assert (concert["executed_metrics"], concert["cached_metrics"]) == (0, 8)

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


def test_system_usage_api(web_client):
    response = web_client.get("/api/v1/system-usage")
    assert response.status_code == 200
    data = response.json()
    assert data["contract_version"] == 1
    assert "cpu_system" in data
    assert "cpu_app" in data
    assert "gpu_system" in data
    assert "gpu_name" in data
    assert isinstance(data["cpu_system"], (int, float))
    assert isinstance(data["cpu_app"], (int, float))
    assert isinstance(data["gpu_system"], (int, float))
    assert isinstance(data["gpu_name"], str)


def test_system_usage_uses_first_gpu_line(web_client, monkeypatch):
    monkeypatch.setattr("shutil.which", lambda command: "/usr/bin/nvidia-smi")
    monkeypatch.setattr(
        "subprocess.run",
        lambda *args, **kwargs: SimpleNamespace(stdout="10, GPU0\n20, GPU1"),
    )

    data = web_client.get("/api/v1/system-usage").json()

    assert data["gpu_system"] == 10.0
    assert data["gpu_name"] == "GPU0"
