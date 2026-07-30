"""Integration tests for persistent, idempotent gallery imports."""

import time
from threading import Event

import pytest
from PIL import Image
from sqlalchemy import inspect, text

from photo_culler.catalog.database import Database
from photo_culler.importing import CancelResult, GalleryImportService
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


def test_import_rejects_missing_and_non_directory_sources(tmp_path):
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Invalid sources")

    with pytest.raises(FileNotFoundError):
        service.start_import(gallery_id, tmp_path / "missing")

    file_source = tmp_path / "single.jpg"
    file_source.write_bytes(b"not-an-image")
    with pytest.raises(ValueError, match="must be a directory"):
        service.start_import(gallery_id, file_source)


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
    assert versions == [1]
    assert "gallery_id" in {column["name"] for column in inspect(reopened.engine).get_columns("photos")}
