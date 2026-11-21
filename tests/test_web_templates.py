"""Unit tests for FastAPI HTML templates, view routes, and i18n localization."""

from io import BytesIO

import pytest
from fastapi.testclient import TestClient
from PIL import Image
from starlette.responses import StreamingResponse

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import FileRole
from photo_culler.core.models import FileRecord, Photo
from photo_culler.web.app import create_app


@pytest.fixture
def web_client(tmp_path):
    cat_file = tmp_path / "catalog.db"
    app = create_app(catalog_path=cat_file)
    with TestClient(app) as client:
        yield client


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


def test_groups_and_sessions_do_not_load_the_full_catalog(web_client, monkeypatch):
    def fail_if_called(*args, **kwargs):
        raise AssertionError("The full catalog must not be loaded for this page")

    monkeypatch.setattr(PhotoRepository, "list_all", fail_if_called)

    assert web_client.get("/groups").status_code == 200
    assert web_client.get("/sessions").status_code == 200


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
    library = web_client.get("/library?representation=jpeg")
    assert "frame.jpg" in library.text
    assert "JPEG" in library.text
    assert "RAW+JPEG" in library.text


def test_black_raw_preview_falls_back_to_its_jpeg_tandem(web_client, tmp_path):
    raw = tmp_path / "dark.nef"
    Image.new("RGB", (32, 32), color=(0, 0, 0)).save(raw, format="JPEG")
    jpeg = tmp_path / "dark.jpg"
    Image.new("RGB", (32, 32), color=(80, 130, 180)).save(jpeg)
    with web_client.app.state.db_engine.session() as session:
        PhotoRepository(session).save_photo(
            Photo(
                "dark-tandem",
                "dark",
                files=[
                    FileRecord(raw, FileRole.RAW, raw.stat().st_size, raw.stat().st_mtime),
                    FileRecord(jpeg, FileRole.JPEG, jpeg.stat().st_size, jpeg.stat().st_mtime),
                ],
            )
        )

    preview = web_client.get("/thumbnails/dark-tandem/800?representation=raw")

    assert preview.status_code == 200
    assert preview.headers["x-photo-culler-representation"] == "jpeg"
    assert Image.open(BytesIO(preview.content)).convert("RGB").getpixel((0, 0))[2] > 100
    library = web_client.get("/library?representation=raw")
    assert "dark.nef" in library.text
    assert "RAW" in library.text


def test_full_preview_sets_the_heic_content_type(web_client, tmp_path):
    image = tmp_path / "frame.heic"
    image.write_bytes(b"not-decoded-by-this-route")
    with web_client.app.state.db_engine.session() as session:
        PhotoRepository(session).save_photo(
            Photo(
                "heic-frame",
                "frame",
                files=[FileRecord(image, FileRole.IMAGE, image.stat().st_size, image.stat().st_mtime)],
            )
        )

    preview = web_client.get("/previews/heic-frame")

    assert preview.status_code == 200
    assert preview.headers["content-type"] == "image/heic"


def test_photo_inspector_shows_the_display_filename(web_client, tmp_path):
    source = tmp_path / "DSC_0068.NEF"
    source.write_bytes(b"raw source")
    with web_client.app.state.db_engine.session() as session:
        PhotoRepository(session).save_photo(
            Photo(
                "filename-frame",
                "DSC_0068",
                files=[FileRecord(source, FileRole.RAW, source.stat().st_size, source.stat().st_mtime)],
            )
        )

    inspector = web_client.get("/photos/filename-frame")

    assert inspector.status_code == 200
    assert "DSC_0068.NEF" in inspector.text


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


def test_photo_inspector_renders_filmstrip_and_toggles(web_client, tmp_path):
    """The photo inspector must render the filmstrip container and all adjacent photos, and preserve it during decision updates."""
    # Create 3 dummy photos
    photos = [
        Photo("photo-a", "DSC_0001", files=[]),
        Photo("photo-b", "DSC_0002", files=[]),
        Photo("photo-c", "DSC_0003", files=[]),
    ]

    with web_client.app.state.db_engine.session() as session:
        repo = PhotoRepository(session)
        for p in photos:
            repo.save_photo(p)

    # 1. GET inspect view for DSC_0002 (photo-b)
    response = web_client.get("/photos/photo-b")
    assert response.status_code == 200
    assert "filmstrip-container" in response.text
    assert "filmstrip-item-photo-a" in response.text
    assert "filmstrip-item-photo-b" in response.text
    assert "filmstrip-item-photo-c" in response.text
    assert "filmstrip-checkbox" in response.text

    # 2. Update decision via POST, should preserve the filmstrip and surrounding photos
    post_response = web_client.post("/photos/photo-b/decision", data={"decision": "best"})
    assert post_response.status_code == 200
    assert "filmstrip-container" in post_response.text
    assert "filmstrip-item-photo-a" in post_response.text
    assert "filmstrip-item-photo-b" in post_response.text
    assert "filmstrip-item-photo-c" in post_response.text
