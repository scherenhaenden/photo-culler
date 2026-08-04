"""Tests for persistent non-destructive recipes and real preview rendering."""

import io
import sys
from types import SimpleNamespace

import numpy as np
from fastapi.testclient import TestClient
from PIL import Image

from photo_culler.catalog.database import Database
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import FileRole
from photo_culler.core.models import FileRecord, Photo
from photo_culler.editing import EditService
from photo_culler.identity.full_hash import compute_full_hash
from photo_culler.web.app import create_app


def seed_editable_photo(database: Database, image_path) -> str:
    """Persist one logical JPEG backed by a real source image."""
    stat = image_path.stat()
    photo = Photo(
        photo_id="editable-photo",
        stem_name="editable",
        files=[
            FileRecord(
                path=image_path,
                role=FileRole.JPEG,
                size_bytes=stat.st_size,
                modified_time=stat.st_mtime,
            )
        ],
    )
    with database.session() as session:
        PhotoRepository(session).save_photo(photo)
    return photo.photo_id


def decoded_mean(payload: bytes) -> np.ndarray:
    """Return mean RGB values from a rendered JPEG."""
    with Image.open(io.BytesIO(payload)) as image:
        return np.asarray(image.convert("RGB"), dtype=np.float32).mean(axis=(0, 1))


def test_edit_recipe_persists_undo_redo_and_preserves_original(tmp_path):
    source = tmp_path / "original.jpg"
    Image.new("RGB", (64, 64), (60, 60, 60)).save(source, quality=95)
    original_hash = compute_full_hash(source)
    database = Database(tmp_path / "catalog.db")
    photo_id = seed_editable_photo(database, source)
    service = EditService(database)

    initial = service.get_document(photo_id)
    baseline = service.render_preview(photo_id, max_size=128)
    changed = service.update_document(
        photo_id,
        exposure=1.0,
        temperature=12000,
        tint=20,
    )
    edited = service.render_preview(photo_id, max_size=128)

    assert initial["recipe"] == {"exposure": 0.0, "temperature": 6500, "tint": 0.0}
    assert changed["revision"] == 1
    assert changed["can_undo"] is True
    assert decoded_mean(edited).mean() > decoded_mean(baseline).mean()
    assert decoded_mean(edited)[0] > decoded_mean(edited)[2]
    undone = service.undo(photo_id)
    assert undone["recipe"] == initial["recipe"]
    assert undone["can_redo"] is True
    redone = service.redo(photo_id)
    assert redone["recipe"] == changed["recipe"]
    assert compute_full_hash(source) == original_hash

    reopened = EditService(Database(tmp_path / "catalog.db"))
    assert reopened.get_document(photo_id)["recipe"] == changed["recipe"]


def test_edit_api_returns_real_preview_and_validates_history(tmp_path):
    app = create_app(catalog_path=tmp_path / "web.db")
    source = tmp_path / "source.jpg"
    Image.new("RGB", (32, 32), (80, 80, 80)).save(source)
    photo_id = seed_editable_photo(app.state.db_engine, source)

    with TestClient(app) as client:
        document = client.get(f"/api/v1/photos/{photo_id}/edit")
        assert document.status_code == 200
        updated = client.patch(
            f"/api/v1/photos/{photo_id}/edit",
            json={"exposure": 0.5, "temperature": 7000, "tint": -10},
        )
        assert updated.status_code == 200
        assert updated.json()["revision"] == 1
        preview = client.get(f"/api/v1/photos/{photo_id}/edit-preview?max_size=128")
        assert preview.status_code == 200
        assert preview.headers["content-type"] == "image/jpeg"
        assert preview.headers["cache-control"] == "private, no-store"
        assert client.post(f"/api/v1/photos/{photo_id}/edit/undo").status_code == 200
        assert client.post(f"/api/v1/photos/{photo_id}/edit/redo").status_code == 200

        unknown = client.get("/api/v1/photos/unknown/edit")
        assert unknown.status_code == 404
        no_history = client.post("/api/v1/photos/unknown/edit/undo")
        assert no_history.status_code == 404


def test_edit_preview_decodes_raw_sources_with_rawpy(tmp_path, monkeypatch):
    class FakeRaw:
        def __enter__(self):
            return self

        def __exit__(self, *args):
            return False

        def postprocess(self, **kwargs):
            assert kwargs == {"use_camera_wb": True, "output_bps": 8, "half_size": True}
            return np.full((16, 24, 3), (90, 120, 150), dtype=np.uint8)

    source = tmp_path / "source.nef"
    source.write_bytes(b"camera raw data")
    database = Database(tmp_path / "catalog.db")
    photo_id = "raw-editable"
    with database.session() as session:
        PhotoRepository(session).save_photo(
            Photo(
                photo_id=photo_id,
                stem_name="raw-editable",
                files=[FileRecord(source, FileRole.RAW, source.stat().st_size, source.stat().st_mtime)],
            )
        )
    monkeypatch.setitem(sys.modules, "rawpy", SimpleNamespace(imread=lambda path: FakeRaw()))

    preview = EditService(database).render_preview(photo_id, max_size=128)

    with Image.open(io.BytesIO(preview)) as image:
        assert image.size == (24, 16)
