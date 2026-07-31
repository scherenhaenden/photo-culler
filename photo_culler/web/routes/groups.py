"""Visible review surface for visually similar photo groups."""

from collections import defaultdict

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse

from photo_culler.catalog.repositories.photo_repository import PhotoRepository

router = APIRouter()


@router.get("/groups", response_class=HTMLResponse)
def get_similarity_groups_page(request: Request):
    """Show each detected group with its current best-scoring recommendation."""
    with request.app.state.db_engine.session() as session:
        photos = PhotoRepository(session).list_by_burst_prefix("similar-")

    grouped = defaultdict(list)
    for photo in photos:
        if (photo.burst_id or "").startswith("similar-"):
            grouped[photo.burst_id].append(photo)
    groups = []
    for group_id, members in grouped.items():
        members.sort(key=lambda photo: (-photo.score, photo.stem_name.lower()))
        groups.append({"id": group_id, "recommended": members[0], "photos": members})
    groups.sort(key=lambda group: group["recommended"].stem_name.lower())

    return request.app.state.templates.TemplateResponse(
        request=request,
        name="groups.html",
        context={
            "active_tab": "groups",
            "groups": groups,
            "grouped_photo_count": sum(len(group["photos"]) for group in groups),
        },
    )
