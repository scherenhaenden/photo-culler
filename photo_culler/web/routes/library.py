"""Library Web Route."""

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse
from photo_culler.catalog.repositories.photo_repository import PhotoRepository

router = APIRouter()


from typing import Optional

@router.get("/library", response_class=HTMLResponse)
def get_library(
    request: Request,
    page: int = 1,
    limit: int = 150,
    sort: Optional[str] = None,
    decision: Optional[str] = None,
    quality_tier: Optional[str] = None,
    session_id: Optional[str] = None,
):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    # Make sure we don't have negative pages/limits
    if page < 1:
        page = 1
    if limit < 1:
        limit = 150

    offset = (page - 1) * limit

    filters = {}
    if decision:
        filters["decision"] = decision
    if quality_tier:
        filters["quality_tier"] = quality_tier
    if session_id:
        filters["session_id"] = session_id

    with db_engine.session() as session:
        repo = PhotoRepository(session)
        photos = repo.list_page(offset=offset, limit=limit, sort=sort, filters=filters)
        total_photos = repo.count_filtered(filters)

    total_pages = (total_photos + limit - 1) // limit if total_photos > 0 else 1

    return templates.TemplateResponse(
        request=request,
        name="library.html",
        context={
            "active_tab": "library",
            "photos": photos,
            "page": page,
            "limit": limit,
            "total_photos": total_photos,
            "total_pages": total_pages,
            "sort": sort,
            "decision": decision,
            "quality_tier": quality_tier,
            "session_id": session_id,
        },
    )
