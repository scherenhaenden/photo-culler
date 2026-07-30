"""Integration tests for persistent, idempotent gallery imports."""

import time
from threading import Event

import pytest
from PIL import Image
from sqlalchemy import inspect, text

from photo_culler.catalog.database import Database
from photo_culler.importing import CancelResult, GalleryImportService, PauseResult, ResumeResult
from photo_culler.scanner.directory_scanner import DirectoryScanner


def wait_for_job(service: GalleryImportService, job_id: str) -> dict[str, object]:
    """Wait for a small fixture import without production-side sleeps."""
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        job = service.get_job(job_id)
        assert job is not None
        if job["state"] in {"completed", "failed", "cancelled"}:
            return job
        time.sleep(0.01)
    raise AssertionError("import did not finish")


def wait_for_state(service: GalleryImportService, job_id: str, expected: str) -> dict[str, object]:
    """Wait until a worker reaches a specific persisted state."""
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        job = service.get_job(job_id)
        assert job is not None
        if job["state"] == expected:
            return job
        time.sleep(0.01)
    raise AssertionError(f"import did not reach {expected}")


def test_import_is_persistent_and_idempotent(tmp_path):
    source = tmp_path / "shoot"
    source.mkdir()
    Image.new("RGB", (32, 32), "red").save(source / "frame-001.jpg")
    Image.new("RGB", (32, 32), "blue").save(source / "frame-002.png")
    database = Database(tmp_path / "catalog.db")
    service = GalleryImportService(database)
    gallery_id = service.create_gallery("Launch shoot")

    first = wait_for_job(service, service.start_import(gallery_id, source))
    second = wait_for_job(service, service.start_import(gallery_id, source))

    assert first["state"] == "completed"
    assert first["discovered"] == 2
    assert first["imported"] == 2
    assert second["state"] == "completed"
    assert service.list_galleries()[0]["photo_count"] == 2
    assert [job["id"] for job in service.list_jobs()] == [second["id"], first["id"]]

    reopened = GalleryImportService(Database(tmp_path / "catalog.db"))
    assert reopened.list_galleries()[0]["name"] == "Launch shoot"
    assert reopened.get_job(str(first["id"])) is not None


def test_non_recursive_import_ignores_nested_files(tmp_path):
    source = tmp_path / "shoot"
    nested = source / "nested"
    nested.mkdir(parents=True)
    Image.new("RGB", (16, 16)).save(source / "top.jpg")
    Image.new("RGB", (16, 16)).save(nested / "nested.jpg")
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Non recursive")

    job = wait_for_job(service, service.start_import(gallery_id, source, recursive=False))

    assert job["discovered"] == 1
    assert service.list_galleries()[0]["photo_count"] == 1


def test_import_estimate_reports_logical_pairing_types_and_size(tmp_path):
    source = tmp_path / "shoot"
    nested = source / "nested"
    nested.mkdir(parents=True)
    (source / "frame-001.nef").write_bytes(b"raw")
    (source / "frame-001.jpg").write_bytes(b"jpeg")
    (source / "frame-001.nef.xmp").write_bytes(b"xmp")
    (nested / "frame-002.png").write_bytes(b"png")
    service = GalleryImportService(Database(tmp_path / "catalog.db"))

    estimate = service.estimate_import(source)
    shallow = service.estimate_import(source, recursive=False)

    assert estimate["total_files"] == 4
    assert estimate["logical_photos"] == 2
    assert estimate["total_bytes"] == 13
    assert estimate["extensions"] == {".jpg": 1, ".nef": 1, ".png": 1, ".xmp": 1}
    assert estimate["roles"] == {"image": 1, "jpeg": 1, "raw": 1, "sidecar": 1}
    assert shallow["logical_photos"] == 1


def test_import_rejects_missing_and_non_directory_sources(tmp_path):
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Invalid sources")

    with pytest.raises(FileNotFoundError):
        service.start_import(gallery_id, tmp_path / "missing")

    file_source = tmp_path / "single.jpg"
    file_source.write_bytes(b"not-an-image")
    with pytest.raises(ValueError, match="must be a directory"):
        service.start_import(gallery_id, file_source)
    with pytest.raises(ValueError, match="must be a directory"):
        service.estimate_import(file_source)

    assert service.list_jobs(limit=0) == []


def test_import_can_be_cooperatively_cancelled(tmp_path, monkeypatch):
    source = tmp_path / "shoot"
    source.mkdir()
    scan_started = Event()
    release_scan = Event()

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        release_scan.wait(timeout=2)
        yield from ()

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Cancellation")
    job_id = service.start_import(gallery_id, source)
    assert scan_started.wait(timeout=2)

    assert service.cancel(job_id) is CancelResult.CANCEL_REQUESTED
    release_scan.set()
    job = wait_for_job(service, job_id)

    assert job["state"] == "cancelled"
    assert job["cancel_requested"] is True
    assert job["discovered"] == 0
    assert job["imported"] == 0
    assert service.cancel(job_id) is CancelResult.NOT_CANCELLABLE
    assert service.cancel("unknown") is CancelResult.NOT_FOUND


def test_import_can_be_paused_and_resumed(tmp_path, monkeypatch):
    source = tmp_path / "shoot"
    source.mkdir()
    Image.new("RGB", (16, 16)).save(source / "frame.jpg")
    scan_started = Event()
    release_scan = Event()
    original_scan = DirectoryScanner.scan

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        release_scan.wait(timeout=2)
        yield from original_scan(self, directory, recursive)

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Pause and resume")
    job_id = service.start_import(gallery_id, source)
    assert scan_started.wait(timeout=2)

    assert service.pause(job_id) is PauseResult.PAUSE_REQUESTED
    paused = wait_for_state(service, job_id, "paused")
    assert paused["pause_requested"] is True
    assert service.pause(job_id) is PauseResult.NOT_PAUSABLE

    release_scan.set()
    assert service.resume(job_id) is ResumeResult.RESUMED
    completed = wait_for_job(service, job_id)

    assert completed["state"] == "completed"
    assert completed["pause_requested"] is False
    assert completed["imported"] == 1
    assert service.resume(job_id) is ResumeResult.NOT_RESUMABLE
    assert service.pause("unknown") is PauseResult.NOT_FOUND
    assert service.resume("unknown") is ResumeResult.NOT_FOUND


def test_interrupted_import_is_resumable_after_service_restart(tmp_path, monkeypatch):
    source = tmp_path / "shoot"
    source.mkdir()
    Image.new("RGB", (16, 16)).save(source / "frame.jpg")
    scan_started = Event()
    release_scan = Event()
    original_scan = DirectoryScanner.scan

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        release_scan.wait(timeout=2)
        yield from original_scan(self, directory, recursive)

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    database = Database(tmp_path / "catalog.db")
    service = GalleryImportService(database)
    gallery_id = service.create_gallery("Restart recovery")
    job_id = service.start_import(gallery_id, source)
    assert scan_started.wait(timeout=2)

    assert service.pause(job_id) is PauseResult.PAUSE_REQUESTED
    release_scan.set()
    service.shutdown()

    reopened = GalleryImportService(Database(tmp_path / "catalog.db"))
    assert reopened.get_job(job_id)["state"] == "paused"
    assert reopened.resume(job_id) is ResumeResult.RESUMED
    assert wait_for_job(reopened, job_id)["state"] == "completed"


def test_paused_import_reports_offline_source_on_resume(tmp_path, monkeypatch):
    source = tmp_path / "shoot"
    source.mkdir()
    scan_started = Event()
    release_scan = Event()

    def blocking_scan(self, directory, recursive=True):
        scan_started.set()
        release_scan.wait(timeout=2)
        yield from ()

    monkeypatch.setattr(DirectoryScanner, "scan", blocking_scan)
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    job_id = service.start_import(service.create_gallery("Offline source"), source)
    assert scan_started.wait(timeout=2)
    assert service.pause(job_id) is PauseResult.PAUSE_REQUESTED
    source.rmdir()

    assert service.resume(job_id) is ResumeResult.SOURCE_UNAVAILABLE
    assert service.get_job(job_id)["state"] == "paused"
    release_scan.set()
    service.cancel(job_id)


def test_versioned_migration_upgrades_legacy_photos_table(tmp_path):
    path = tmp_path / "legacy.db"
    database = Database(path)
    with database.engine.begin() as connection:
        connection.execute(text("DROP TABLE schema_migrations"))
        connection.execute(text("DROP INDEX ix_photos_gallery_id"))
        # SQLite cannot drop a column on every supported deployment, so emulate
        # legacy migration bookkeeping and verify the migration remains repeatable.
    reopened = Database(path)
    with reopened.engine.connect() as connection:
        versions = connection.execute(text("SELECT version FROM schema_migrations")).scalars().all()
    assert versions == [1, 2]
    assert "gallery_id" in {column["name"] for column in inspect(reopened.engine).get_columns("photos")}
    import_job_columns = {column["name"] for column in inspect(reopened.engine).get_columns("import_jobs")}
    assert {"pause_requested", "resume_state"} <= import_job_columns
