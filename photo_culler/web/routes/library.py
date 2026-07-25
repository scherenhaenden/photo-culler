"""Library Web Route."""

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse
from photo_culler.catalog.repositories.photo_repository import PhotoRepository

router = APIRouter()


@router.get("/library", response_class=HTMLResponse)
def get_library(request: Request):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    with db_engine.session() as session:
        repo = PhotoRepository(session)
        photos = repo.list_all()

    return templates.TemplateResponse(
        request=request,
        name="library.html",
        context={"active_tab": "library", "photos": photos},
    )
