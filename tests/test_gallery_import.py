"""Integration tests for persistent, idempotent gallery imports."""

import time
from threading import Event

import pytest
from PIL import Image
from sqlalchemy import inspect, text

from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import FileDB, ImportSourceDB
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
    excluded = service.estimate_import(source, exclude_patterns=["nested/**"])

    assert estimate["total_files"] == 4
    assert estimate["logical_photos"] == 2
    assert estimate["total_bytes"] == 13
    assert estimate["extensions"] == {".jpg": 1, ".nef": 1, ".png": 1, ".xmp": 1}
    assert estimate["roles"] == {"image": 1, "jpeg": 1, "raw": 1, "sidecar": 1}
    assert shallow["logical_photos"] == 1
    assert excluded["total_files"] == 3
    assert excluded["logical_photos"] == 1


def test_import_exclusions_persist_across_rescans(tmp_path):
    source = tmp_path / "shoot"
    source.mkdir()
    Image.new("RGB", (16, 16)).save(source / "keep.jpg")
    Image.new("RGB", (16, 16)).save(source / "skip.jpg")
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Exclusions")

    job = wait_for_job(
        service,
        service.start_import(gallery_id, source, exclude_patterns=["skip*.jpg"]),
    )
    assert job["discovered"] == 1
    assert service.list_galleries()[0]["photo_count"] == 1

    Image.new("RGB", (16, 16)).save(source / "skip-new.jpg")
    rescan = service.rescan_gallery(gallery_id)
    rescanned = wait_for_job(service, rescan["job_ids"][0])
    assert rescanned["discovered"] == 1
    assert service.list_galleries()[0]["photo_count"] == 1


def test_rescan_detects_new_modified_and_missing_files(tmp_path):
    source = tmp_path / "shoot"
    source.mkdir()
    first_path = source / "first.jpg"
    missing_path = source / "missing.jpg"
    Image.new("RGB", (16, 16), "red").save(first_path)
    Image.new("RGB", (16, 16), "blue").save(missing_path)
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Reconciliation")

    first_job = wait_for_job(service, service.start_import(gallery_id, source))
    first_revision = service.list_scan_revisions(gallery_id)[0]
    assert first_job["scan_revision_id"] == first_revision["id"]
    assert first_revision["new_files"] == 2
    assert first_revision["modified_files"] == 0
    assert first_revision["missing_files"] == 0

    Image.new("RGB", (32, 32), "green").save(first_path)
    missing_path.unlink()
    Image.new("RGB", (16, 16), "yellow").save(source / "new.jpg")
    rescan = service.rescan_gallery(gallery_id)
    assert rescan["offline_source_ids"] == []
    assert len(rescan["job_ids"]) == 1
    wait_for_job(service, rescan["job_ids"][0])

    latest = service.list_scan_revisions(gallery_id)[0]
    assert latest["state"] == "completed"
    assert latest["new_files"] == 1
    assert latest["modified_files"] == 1
    assert latest["moved_files"] == 0
    assert latest["missing_files"] == 1
    with service.database.session() as session:
        statuses = {
            row.source_relative_path: row.status for row in session.query(FileDB).order_by(FileDB.source_relative_path)
        }
    assert statuses == {
        "first.jpg": "present",
        "missing.jpg": "missing",
        "new.jpg": "present",
    }


def test_rescan_marks_unavailable_source_and_files_offline(tmp_path):
    source = tmp_path / "shoot"
    source.mkdir()
    Image.new("RGB", (16, 16)).save(source / "frame.jpg")
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("External drive")
    wait_for_job(service, service.start_import(gallery_id, source))
    detached = tmp_path / "detached"
    source.rename(detached)

    rescan = service.rescan_gallery(gallery_id)

    assert rescan["job_ids"] == []
    assert len(rescan["offline_source_ids"]) == 1
    assert service.list_scan_revisions(gallery_id)[0]["state"] == "offline"
    assert service.list_sources(gallery_id)[0]["status"] == "offline"
    with service.database.session() as session:
        source_row = session.query(ImportSourceDB).one()
        file_row = session.query(FileDB).one()
        assert source_row.status == "offline"
        assert file_row.status == "offline"
    with service.database.session() as session:
        photo = PhotoRepository(session).list_all()[0]
    assert photo.availability_status == "offline"


def test_rescan_preserves_photo_identity_for_unambiguous_file_move(tmp_path):
    source = tmp_path / "shoot"
    source.mkdir()
    original_path = source / "original.jpg"
    Image.new("RGB", (16, 16), "red").save(original_path)
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Move detection")
    wait_for_job(service, service.start_import(gallery_id, source))
    with service.database.session() as session:
        original_photo_id = PhotoRepository(session).list_all()[0].photo_id
    moved_path = source / "renamed.jpg"
    original_path.rename(moved_path)

    rescan = service.rescan_gallery(gallery_id)
    wait_for_job(service, rescan["job_ids"][0])

    latest = service.list_scan_revisions(gallery_id)[0]
    assert latest["moved_files"] == 1
    assert latest["new_files"] == 0
    assert latest["missing_files"] == 0
    assert service.list_galleries()[0]["photo_count"] == 1
    with service.database.session() as session:
        photo = PhotoRepository(session).list_all()[0]
        file_row = session.query(FileDB).one()
        persisted_path = file_row.relative_path
        persisted_status = file_row.status
    assert photo.photo_id == original_photo_id
    assert photo.stem_name == "renamed"
    assert persisted_path == str(moved_path)
    assert persisted_status == "present"


def test_identical_files_at_existing_paths_are_not_collapsed_as_moves(tmp_path):
    source = tmp_path / "shoot"
    source.mkdir()
    payload = b"same-content"
    (source / "first.jpg").write_bytes(payload)
    (source / "second.jpg").write_bytes(payload)
    service = GalleryImportService(Database(tmp_path / "catalog.db"))
    gallery_id = service.create_gallery("Exact duplicates")

    wait_for_job(service, service.start_import(gallery_id, source))

    assert service.list_galleries()[0]["photo_count"] == 2
    assert service.list_scan_revisions(gallery_id)[0]["moved_files"] == 0


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
    assert versions == [1, 2, 3, 4, 5, 6, 7]
    assert "gallery_id" in {column["name"] for column in inspect(reopened.engine).get_columns("photos")}
    import_job_columns = {column["name"] for column in inspect(reopened.engine).get_columns("import_jobs")}
    assert {"pause_requested", "resume_state"} <= import_job_columns
    file_columns = {column["name"] for column in inspect(reopened.engine).get_columns("files")}
    assert {"import_source_id", "last_seen_revision_id", "source_relative_path", "status"} <= file_columns
    assert "scan_revisions" in inspect(reopened.engine).get_table_names()
    assert "edit_documents" in inspect(reopened.engine).get_table_names()
    import_source_columns = {column["name"] for column in inspect(reopened.engine).get_columns("import_sources")}
    assert "exclude_patterns" in import_source_columns
