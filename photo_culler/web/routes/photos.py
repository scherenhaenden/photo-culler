"""Single Photo Inspector & Decision Toggle Route."""

from fastapi import APIRouter, Form, HTTPException, Query, Request
from fastapi.responses import FileResponse, HTMLResponse
from sqlalchemy import and_, or_

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import PhotoDB
from photo_culler.core.enums import FileRole
from photo_culler.web.services.decision_service import DecisionService
from photo_culler.web.services.thumbnail_service import ThumbnailService

router = APIRouter()


@router.get("/photos/{photo_id}", response_class=HTMLResponse)
def inspect_photo(photo_id: str, request: Request, group: str | None = Query(default=None)):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    with db_engine.session() as session:
        repo = PhotoRepository(session)
        photo = repo.get_by_id(photo_id)
        if not photo:
            raise HTTPException(status_code=404, detail="Photo not found")
        analysis_summary = repo.get_analysis_summary(photo_id)

        # Query only adjacent records. Loading and converting the entire catalog made
        # opening an inspector page increasingly slow as the catalog grew.
        active_group_id = group if group and group.startswith("similar-") else None
        current = session.query(PhotoDB).filter(PhotoDB.photo_id == photo_id).one()
        navigation_query = session.query(PhotoDB.photo_id)
        if active_group_id:
            navigation_query = navigation_query.filter(PhotoDB.burst_id == active_group_id)

        previous_filter = or_(
            PhotoDB.stem_name < current.stem_name,
            and_(PhotoDB.stem_name == current.stem_name, PhotoDB.id < current.id),
        )
        next_filter = or_(
            PhotoDB.stem_name > current.stem_name,
            and_(PhotoDB.stem_name == current.stem_name, PhotoDB.id > current.id),
        )
        previous_ids = [
            row[0]
            for row in navigation_query.filter(previous_filter)
            .order_by(PhotoDB.stem_name.desc(), PhotoDB.id.desc())
            .limit(2)
            .all()
        ]
        next_ids = [
            row[0]
            for row in navigation_query.filter(next_filter)
            .order_by(PhotoDB.stem_name.asc(), PhotoDB.id.asc())
            .limit(2)
            .all()
        ]

        prev_photo_id = previous_ids[0] if previous_ids else None
        next_photo_id = next_ids[0] if next_ids else None
        prefetch_ids = list(reversed(previous_ids)) + next_ids

    return templates.TemplateResponse(
        request=request,
        name="photo_detail.html",
        context={
            "active_tab": "library",
            "photo": photo,
            "analysis_summary": analysis_summary,
            "prev_photo_id": prev_photo_id,
            "next_photo_id": next_photo_id,
            "prefetch_ids": prefetch_ids,
            "active_group_id": active_group_id,
        },
    )


@router.get("/thumbnails/{photo_id}/{size}")
def get_thumbnail(photo_id: str, size: str, request: Request, representation: str = Query(default="jpeg")):
    db_engine = request.app.state.db_engine
    thumb_service = ThumbnailService(db_engine)

    if representation not in {"jpeg", "raw"}:
        raise HTTPException(status_code=422, detail="Unknown preview representation")
    thumb_path = thumb_service.get_thumbnail_path(photo_id, size=size, representation=representation)
    if not thumb_path or not thumb_path.exists():
        raise HTTPException(status_code=404, detail="Thumbnail not found")

    return FileResponse(
        thumb_path, media_type="image/jpeg", headers={"Cache-Control": "private, max-age=31536000, immutable"}
    )


@router.get("/previews/{photo_id}")
def get_full_preview(photo_id: str, request: Request):
    """Serve the original JPEG/image for focused comparison without thumbnailing it."""
    with request.app.state.db_engine.session() as session:
        photo = PhotoRepository(session).get_by_id(photo_id)
        display_file = photo.display_file("jpeg") if photo else None
    if not display_file or not display_file.path.exists():
        raise HTTPException(status_code=404, detail="Preview not found")
    if display_file.role not in {FileRole.JPEG, FileRole.IMAGE}:
        raise HTTPException(status_code=404, detail="A browser-viewable original is not available")
    return FileResponse(display_file.path, headers={"Cache-Control": "private, max-age=3600"})


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
