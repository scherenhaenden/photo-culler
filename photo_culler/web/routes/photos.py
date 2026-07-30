"""Single Photo Inspector & Decision Toggle Route."""

from fastapi import APIRouter, Form, HTTPException, Request
from fastapi.responses import FileResponse, HTMLResponse
from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.web.services.decision_service import DecisionService
from photo_culler.web.services.thumbnail_service import ThumbnailService

router = APIRouter()


@router.get("/photos/{photo_id}", response_class=HTMLResponse)
def inspect_photo(photo_id: str, request: Request):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    with db_engine.session() as session:
        repo = PhotoRepository(session)
        photo = repo.get_by_id(photo_id)
        if not photo:
            raise HTTPException(status_code=404, detail="Photo not found")

        # Get all photos to calculate prev, next, and prefetch lists
        photos = repo.list_page(offset=0, limit=10000, sort="name_asc")
        idx = -1
        for i, p in enumerate(photos):
            if p.photo_id == photo_id:
                idx = i
                break

        prev_photo_id = None
        next_photo_id = None
        prefetch_ids = []

        if idx != -1:
            if idx > 0:
                prev_photo_id = photos[idx - 1].photo_id
            if idx < len(photos) - 1:
                next_photo_id = photos[idx + 1].photo_id

            # Prefetch 2 previous and 2 next photos
            for offset in (-2, -1, 1, 2):
                pos = idx + offset
                if 0 <= pos < len(photos):
                    prefetch_ids.append(photos[pos].photo_id)

    return templates.TemplateResponse(
        request=request,
        name="photo_detail.html",
        context={
            "active_tab": "library",
            "photo": photo,
            "prev_photo_id": prev_photo_id,
            "next_photo_id": next_photo_id,
            "prefetch_ids": prefetch_ids,
        },
    )


@router.get("/thumbnails/{photo_id}/{size}")
def get_thumbnail(photo_id: str, size: str, request: Request):
    db_engine = request.app.state.db_engine
    thumb_service = ThumbnailService(db_engine)

    thumb_path = thumb_service.get_thumbnail_path(photo_id, size=size)
    if not thumb_path or not thumb_path.exists():
        raise HTTPException(status_code=404, detail="Thumbnail not found")

    return FileResponse(
        thumb_path, media_type="image/jpeg", headers={"Cache-Control": "private, max-age=31536000, immutable"}
    )


@router.post("/photos/{photo_id}/decision", response_class=HTMLResponse)
def update_photo_decision(photo_id: str, request: Request, decision: str = Form(...)):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    decision_service = DecisionService(db_engine)
    photo = decision_service.set_decision(photo_id, decision)

    if not photo:
        raise HTTPException(status_code=404, detail="Photo not found")

    return templates.TemplateResponse(
        request=request,
        name="photo_detail.html",
        context={"active_tab": "library", "photo": photo},
    )
