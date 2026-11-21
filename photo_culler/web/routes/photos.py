"""Single Photo Inspector & Decision Toggle Route."""

import mimetypes

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
        display_file = photo.display_file("jpeg")
        analysis_summary = repo.get_analysis_summary(photo_id)
        similarity_group_id = photo.burst_id if (photo.burst_id or "").startswith("similar-") else None
        similarity_group_photos = repo.list_by_burst_ids([similarity_group_id]) if similarity_group_id else []
        similarity_group_photos.sort(key=lambda item: (-item.score, item.stem_name.lower()))
        recommended_group_photo = similarity_group_photos[0] if similarity_group_photos else None

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
            .limit(10)
            .all()
        ]
        next_ids = [
            row[0]
            for row in navigation_query.filter(next_filter)
            .order_by(PhotoDB.stem_name.asc(), PhotoDB.id.asc())
            .limit(10)
            .all()
        ]

        prev_photo_id = previous_ids[0] if previous_ids else None
        next_photo_id = next_ids[0] if next_ids else None
        prefetch_ids = list(reversed(previous_ids[:2])) + next_ids[:2]

        # Fetch filmstrip photo objects
        filmstrip_ids = list(reversed(previous_ids)) + [photo_id] + next_ids
        db_filmstrip_photos = session.query(PhotoDB).filter(PhotoDB.photo_id.in_(filmstrip_ids)).all()
        db_photo_map = {p.photo_id: p for p in db_filmstrip_photos}
        filmstrip_photos = []
        for fid in filmstrip_ids:
            if fid in db_photo_map:
                filmstrip_photos.append(repo._to_domain(db_photo_map[fid]))

    return templates.TemplateResponse(
        request=request,
        name="photo_detail.html",
        context={
            "active_tab": "library",
            "photo": photo,
            "display_file": display_file,
            "analysis_summary": analysis_summary,
            "prev_photo_id": prev_photo_id,
            "next_photo_id": next_photo_id,
            "prefetch_ids": prefetch_ids,
            "active_group_id": active_group_id,
            "similarity_group_id": similarity_group_id,
            "similarity_group_photos": similarity_group_photos,
            "recommended_group_photo": recommended_group_photo,
            "filmstrip_photos": filmstrip_photos,
        },
    )


@router.get("/thumbnails/{photo_id}/{size}")
def get_thumbnail(photo_id: str, size: str, request: Request, representation: str = Query(default="jpeg")):
    db_engine = request.app.state.db_engine
    thumb_service = ThumbnailService(db_engine)

    if representation not in {"jpeg", "raw"}:
        raise HTTPException(status_code=422, detail="Unknown preview representation")
    thumbnail = thumb_service.get_thumbnail(photo_id, size=size, representation=representation)
    if not thumbnail or not thumbnail.path.exists():
        raise HTTPException(status_code=404, detail="Thumbnail not found")

    return FileResponse(
        thumbnail.path,
        media_type="image/jpeg",
        headers={
            "Cache-Control": "private, max-age=31536000, immutable",
            "X-Photo-Culler-Representation": thumbnail.representation,
        },
    )


@router.get("/previews/{photo_id}")
def get_full_preview(photo_id: str, request: Request):
    """Serve the original JPEG/image for focused comparison without thumbnailing it."""
    from fastapi.responses import RedirectResponse

    with request.app.state.db_engine.session() as session:
        photo = PhotoRepository(session).get_by_id(photo_id)
        display_file = photo.display_file("jpeg") if photo else None
    if not display_file or not display_file.path.exists():
        raise HTTPException(status_code=404, detail="Preview not found")
    if display_file.role not in {FileRole.JPEG, FileRole.IMAGE}:
        # Redirect standalone RAW previews to generated high-res thumbnails so they are viewable
        return RedirectResponse(url=f"/thumbnails/{photo_id}/1600")
    media_type = {
        ".heic": "image/heic",
        ".heif": "image/heif",
    }.get(
        display_file.path.suffix.lower(), mimetypes.guess_type(display_file.path.name)[0] or "application/octet-stream"
    )
    return FileResponse(
        display_file.path,
        media_type=media_type,
        headers={"Cache-Control": "private, max-age=3600"},
    )


@router.post("/photos/{photo_id}/decision", response_class=HTMLResponse)
def update_photo_decision(
    photo_id: str, request: Request, decision: str = Form(...), group: str | None = Query(default=None)
):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    decision_service = DecisionService(db_engine)
    photo = decision_service.set_decision(photo_id, decision)

    if not photo:
        raise HTTPException(status_code=404, detail="Photo not found")

    with db_engine.session() as session:
        repo = PhotoRepository(session)
        display_file = photo.display_file("jpeg")
        analysis_summary = repo.get_analysis_summary(photo_id)

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
            .limit(10)
            .all()
        ]
        next_ids = [
            row[0]
            for row in navigation_query.filter(next_filter)
            .order_by(PhotoDB.stem_name.asc(), PhotoDB.id.asc())
            .limit(10)
            .all()
        ]

        prev_photo_id = previous_ids[0] if previous_ids else None
        next_photo_id = next_ids[0] if next_ids else None
        prefetch_ids = list(reversed(previous_ids[:2])) + next_ids[:2]

        # Fetch filmstrip photo objects
        filmstrip_ids = list(reversed(previous_ids)) + [photo_id] + next_ids
        db_filmstrip_photos = session.query(PhotoDB).filter(PhotoDB.photo_id.in_(filmstrip_ids)).all()
        db_photo_map = {p.photo_id: p for p in db_filmstrip_photos}
        filmstrip_photos = []
        for fid in filmstrip_ids:
            if fid in db_photo_map:
                filmstrip_photos.append(repo._to_domain(db_photo_map[fid]))

    return templates.TemplateResponse(
        request=request,
        name="photo_detail.html",
        context={
            "active_tab": "library",
            "photo": photo,
            "display_file": display_file,
            "analysis_summary": analysis_summary,
            "prev_photo_id": prev_photo_id,
            "next_photo_id": next_photo_id,
            "prefetch_ids": prefetch_ids,
            "active_group_id": active_group_id,
            "filmstrip_photos": filmstrip_photos,
        },
    )
