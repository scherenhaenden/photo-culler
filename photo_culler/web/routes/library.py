"""Library Web Route."""

from typing import Optional

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse

from photo_culler.catalog.repositories.photo_repository import PhotoRepository

router = APIRouter()


@router.get("/library", response_class=HTMLResponse)
def get_library(
    request: Request,
    page: int = 1,
    limit: int = 150,
    sort: Optional[str] = None,
    decision: Optional[str] = None,
    quality_tier: Optional[str] = None,
    session_id: Optional[str] = None,
    gallery_id: Optional[str] = None,
):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates
    galleries = request.app.state.gallery_imports.list_galleries()
    gallery_by_id = {str(gallery["id"]): gallery for gallery in galleries}
    active_gallery_id = gallery_id if gallery_id in gallery_by_id else None
    if active_gallery_id is None and galleries:
        active_gallery_id = str(galleries[0]["id"])
    active_gallery = gallery_by_id.get(active_gallery_id) if active_gallery_id else None

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
    if active_gallery_id:
        filters["gallery_id"] = active_gallery_id

    with db_engine.session() as session:
        repo = PhotoRepository(session)
        photos = repo.list_page(offset=offset, limit=limit, sort=sort, filters=filters)
        total_photos = repo.count_filtered(filters)

    import_jobs = request.app.state.gallery_imports.list_jobs()

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
            "galleries": galleries,
            "active_gallery": active_gallery,
            "active_gallery_id": active_gallery_id,
            "import_jobs": import_jobs,
        },
    )
