"""Unit tests for FastAPI user sessions, technical analysis execution, and similarity grouping."""

import time
from datetime import datetime, timedelta
from threading import Event

import pytest
from fastapi.testclient import TestClient
from PIL import Image

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import FileDB, SessionDB
from photo_culler.core.enums import DecisionState, FileRole
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


def test_analysis_workers_can_be_changed_without_restarting(web_client):
    response = web_client.post("/analysis/workers", data={"workers": "1"})

    assert response.status_code == 200
    assert response.json()["workers"] == 1
    assert response.json()["max_workers"] >= 1


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


def test_similarity_groups_page_clamps_out_of_range_pages(web_client):
    with web_client.app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        for index in range(13):
            repository.save_photo(
                Photo(
                    f"group-{index}",
                    f"Group_{index}",
                    burst_id=f"similar-example-{index}",
                    score=0.5,
                )
            )

    page = web_client.get("/groups?page=999")

    assert page.status_code == 200
    assert "Página 2 de 2" in page.text
    assert "Group_9" in page.text


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
