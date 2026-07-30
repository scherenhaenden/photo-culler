"""Integration tests for the complete catalog browsing and decision workflow."""

from fastapi.testclient import TestClient

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.core.enums import DecisionState, QualityTier
from photo_culler.core.models import Photo
from photo_culler.web.app import create_app


def seed_catalog(app) -> None:
    photos = [
        Photo("photo-best", "A_best", decision=DecisionState.BEST, score=0.95, quality_tier=QualityTier.EXCELLENT),
        Photo("photo-review", "B_review", decision=DecisionState.REVIEW, score=0.55, quality_tier=QualityTier.FAIR),
        Photo(
            "photo-reject",
            "C_reject",
            decision=DecisionState.REJECT_TECHNICAL,
            score=0.1,
            quality_tier=QualityTier.POOR,
        ),
    ]
    with app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        for photo in photos:
            repository.save_photo(photo)


def test_library_filters_pagination_navigation_and_decision_round_trip(tmp_path):
    app = create_app(catalog_path=tmp_path / "workflow.db")
    seed_catalog(app)
    client = TestClient(app)

    first_page = client.get("/library?limit=2&page=1")
    assert first_page.status_code == 200
    assert "Mostrando <span" in first_page.text
    assert "A_best" in first_page.text
    assert "B_review" in first_page.text
    assert "C_reject" not in first_page.text
    assert "Importar galería" in first_page.text
    assert "Página <strong>1</strong> de <strong>2</strong>" in first_page.text

    rejected = client.get("/library?decision=reject")
    assert rejected.status_code == 200
    assert "C_reject" in rejected.text
    assert "A_best" not in rejected.text

    detail = client.get("/photos/photo-review")
    assert detail.status_code == 200
    assert 'data-prev-photo-id="photo-best"' in detail.text
    assert 'data-next-photo-id="photo-reject"' in detail.text

    changed = client.post("/photos/photo-review/decision", data={"decision": "keep"})
    assert changed.status_code == 200
    assert "KEEP" in changed.text

    kept = client.get("/library?decision=keep")
    assert "B_review" in kept.text


def test_desktop_session_protects_html_static_and_api_routes(tmp_path):
    app = create_app(catalog_path=tmp_path / "desktop.db", desktop_token="one-time-token")
    client = TestClient(app, base_url="http://127.0.0.1")

    assert client.get("/").status_code == 403
    authenticated = client.get("/?token=one-time-token")
    assert authenticated.status_code == 200
    assert authenticated.cookies["desktop_token"] == "one-time-token"
    assert client.get("/static/css/app.css").status_code == 200
    assert client.get("/api/health").status_code == 200
