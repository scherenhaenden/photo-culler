"""Visible review surface for visually similar photo groups."""

from collections import defaultdict

from fastapi import APIRouter, HTTPException, Query, Request
from fastapi.responses import HTMLResponse

from photo_culler.catalog.repositories.photo_repository import PhotoRepository

router = APIRouter()


@router.get("/groups", response_class=HTMLResponse)
def get_similarity_groups_page(request: Request, page: int = Query(1, ge=1)):
    """Show each detected group with its current best-scoring recommendation."""
    with request.app.state.db_engine.session() as session:
        repository = PhotoRepository(session)
        page_size = 12
        total_groups = repository.count_bursts("similar-")
        total_pages = max(1, (total_groups + page_size - 1) // page_size)
        if page > total_pages:
            page = total_pages
        group_ids = repository.list_burst_ids("similar-", offset=(page - 1) * page_size, limit=page_size)
        photos = repository.list_by_burst_ids(group_ids)

    grouped = defaultdict(list)
    for photo in photos:
        if (photo.burst_id or "").startswith("similar-"):
            grouped[photo.burst_id].append(photo)
    groups = []
    for group_id, members in grouped.items():
        members.sort(key=lambda photo: (-photo.score, photo.stem_name.lower()))
        preview_members = members[:12]
        groups.append(
            {"id": group_id, "recommended": members[0], "photos": preview_members, "photo_count": len(members)}
        )
    groups.sort(key=lambda group: group["recommended"].stem_name.lower())

    return request.app.state.templates.TemplateResponse(
        request=request,
        name="groups.html",
        context={
            "active_tab": "groups",
            "groups": groups,
            "grouped_photo_count": sum(group["photo_count"] for group in groups),
            "page": page,
            "total_pages": total_pages,
            "total_groups": total_groups,
        },
    )


@router.get("/groups/{group_id}", response_class=HTMLResponse)
def compare_similarity_group(request: Request, group_id: str, page: int = Query(1, ge=1)):
    """Show a single similarity group in a large, comparison-oriented layout."""
    if not group_id.startswith("similar-"):
        raise HTTPException(status_code=404, detail="Similarity group not found")
    with request.app.state.db_engine.session() as session:
        photos = PhotoRepository(session).list_by_burst_ids([group_id])
    if not photos:
        raise HTTPException(status_code=404, detail="Similarity group not found")
    photos.sort(key=lambda photo: (-photo.score, photo.stem_name.lower()))
    page_size = 8
    total_pages = max(1, (len(photos) + page_size - 1) // page_size)
    if page > total_pages:
        page = total_pages
    start = (page - 1) * page_size
    return request.app.state.templates.TemplateResponse(
        request=request,
        name="group_compare.html",
        context={
            "active_tab": "groups",
            "group_id": group_id,
            "photos": photos[start : start + page_size],
            "photo_count": len(photos),
            "recommended_photo_id": photos[0].photo_id,
            "page": page,
            "total_pages": total_pages,
        },
    )
