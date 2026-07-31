"""Visible review surface for visually similar photo groups."""

from collections import defaultdict

from fastapi import APIRouter, Query, Request
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
            "total_pages": max(1, (total_groups + page_size - 1) // page_size),
            "total_groups": total_groups,
        },
    )
