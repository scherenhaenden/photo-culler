"""Unit tests for FastAPI JSON API endpoints."""

import time
from datetime import datetime, timedelta
from threading import Event

import pytest
from fastapi.testclient import TestClient
from PIL import Image

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import FileDB
from photo_culler.core.enums import DecisionState, FileRole
from photo_culler.core.models import FileRecord, MetadataRecord, Photo
from photo_culler.scanner.directory_scanner import DirectoryScanner
from photo_culler.web.app import create_app


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


def test_native_frontend_contracts_use_application_services(web_client, tmp_path):
    """The egui client receives catalog/decision/analysis JSON without reading SQLite."""
    image_path = tmp_path / "native.jpg"
    Image.new("RGB", (48, 32), color=(90, 120, 160)).save(image_path)
    gallery_id = web_client.app.state.gallery_imports.create_gallery("Native")
    with web_client.app.state.db_engine.session() as session:
        photo = PhotoRepository(session).save_photo(
            Photo(
                "native-photo",
                "native-frame",
                files=[FileRecord(image_path, FileRole.JPEG, image_path.stat().st_size, image_path.stat().st_mtime)],
            )
        )
        photo.gallery_id = gallery_id

    catalog = web_client.get(f"/api/v1/catalog?gallery_id={gallery_id}")
    assert catalog.status_code == 200
    assert catalog.json()["contract_version"] == 1
    assert catalog.json()["items"] == [
        {
            "id": "native-photo",
            "name": "native-frame",
            "decision": "UNPROCESSED",
            "score": 0.0,
            "quality_tier": "fair",
            "thumbnail_url": "/thumbnails/native-photo/800",
        }
    ]

    decision = web_client.put("/api/v1/photos/native-photo/decision", json={"decision": "keep"})
    assert decision.status_code == 200
    assert decision.json()["decision"] == "KEEP"
    assert web_client.put("/api/v1/photos/native-photo/decision", json={"decision": "kepp"}).status_code == 422
    assert web_client.put("/api/v1/photos/unknown/decision", json={"decision": "keep"}).status_code == 404
    assert web_client.get("/api/v1/analysis/progress").status_code == 200
    assert web_client.post("/api/v1/analysis/start", json={"profile": "missing"}).status_code == 422
    assert web_client.post("/api/v1/analysis/unknown").status_code == 404
    assert web_client.get("/api/v1/sessions").status_code == 200
    assert web_client.get("/api/v1/groups").status_code == 200


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


def test_system_usage_api(web_client):
    response = web_client.get("/api/v1/system-usage")
    assert response.status_code == 200
    data = response.json()
    assert data["contract_version"] == 1
    assert "cpu_system" in data
    assert "cpu_app" in data
    assert "cpu_app_capacity" in data
    assert "cpu_core_count" in data
    assert "gpu_system" in data
    assert "gpu_name" in data
    assert isinstance(data["cpu_system"], (int, float))
    assert isinstance(data["cpu_app"], (int, float))
    assert isinstance(data["gpu_system"], (int, float))
    assert isinstance(data["gpu_name"], str)


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
    assert result.json()["status"] == "ok"
    deadline = time.monotonic() + 3
    manager = web_client.app.state.similarity_grouping_jobs
    while manager.is_running and time.monotonic() < deadline:
        time.sleep(0.01)
    assert manager.snapshot()["status"] == "completed"
    assert manager.snapshot()["groups"] == 1
    page = web_client.get("/groups")
    assert "similar-one" in page.text
    with web_client.app.state.db_engine.session() as session:
        group_id = PhotoRepository(session).get_by_id("similar-one").burst_id
    assert group_id is not None
    comparison = web_client.get(f"/groups/{group_id}")
    assert comparison.status_code == 200
    assert "similar-one" in comparison.text
    assert "similar-two" in comparison.text
    assert "/previews/similar-one" in comparison.text
    assert web_client.get("/previews/similar-one").status_code == 200


def test_similarity_grouping_progress_and_photo_inspector_context(web_client, tmp_path):
    image = tmp_path / "group-progress.jpg"
    Image.new("RGB", (80, 60), (40, 100, 180)).save(image)
    stat = image.stat()
    captured = datetime(2026, 7, 31, 12, 0, 0)
    with web_client.app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        for index in range(2):
            repository.save_photo(
                Photo(
                    f"progress-{index}", f"progress-{index}",
                    files=[FileRecord(image, FileRole.JPEG, stat.st_size, stat.st_mtime)],
                    metadata=MetadataRecord(capture_time=captured + timedelta(seconds=index)), score=0.4 + index,
                )
            )

    assert web_client.post("/analysis/group-similar").status_code == 200
    deadline = time.monotonic() + 3
    manager = web_client.app.state.similarity_grouping_jobs
    while manager.is_running and time.monotonic() < deadline:
        time.sleep(0.01)
    snapshot = manager.snapshot()
    assert snapshot["status"] == "completed"
    assert snapshot["progress"] == 100
    inspector = web_client.get("/photos/progress-0")
    assert inspector.status_code == 200
    assert "Grupo de fotos parecidas" in inspector.text
    assert "progress-1" in inspector.text


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
    with web_client.app.state.db_engine.session() as session:
        assert PhotoRepository(session).get_by_id("profile-check").decision is not DecisionState.UNPROCESSED
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
    assert (technical["executed_metrics"], technical["cached_metrics"]) == (6, 4)
    assert (technical_again["executed_metrics"], technical_again["cached_metrics"]) == (0, 10)
    assert (concert["executed_metrics"], concert["cached_metrics"]) == (0, 10)

    detail = web_client.get("/photos/profile-check")
    assert detail.status_code == 200
    assert "Por qué esta puntuación" in detail.text
    assert "Perfil Concert Stage" in detail.text
    assert "peso" in detail.text
    assert "Ver original" in detail.text
    assert "const syncControls = (editDocument)" in detail.text
