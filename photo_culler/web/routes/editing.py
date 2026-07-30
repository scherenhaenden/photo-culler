"""Versioned non-destructive editing API."""

from typing import cast

from fastapi import APIRouter, HTTPException, Query, Request, Response
from pydantic import BaseModel, Field

from photo_culler.editing import EditService

router = APIRouter(prefix="/api/v1/photos")


class EditUpdateRequest(BaseModel):
    """Supported first-milestone global edit parameters."""

    exposure: float | None = Field(default=None, ge=-5.0, le=5.0)
    temperature: int | None = Field(default=None, ge=2000, le=12000)
    tint: float | None = Field(default=None, ge=-100.0, le=100.0)


def _service(request: Request) -> EditService:
    return cast(EditService, request.app.state.edit_service)


@router.get("/{photo_id}/edit")
def get_edit_document(photo_id: str, request: Request) -> dict[str, object]:
    """Return the persistent edit recipe for one photo."""
    try:
        return _service(request).get_document(photo_id)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc


@router.patch("/{photo_id}/edit")
def update_edit_document(
    photo_id: str,
    request: Request,
    payload: EditUpdateRequest,
) -> dict[str, object]:
    """Update supported edit operations and create an undo entry."""
    try:
        return _service(request).update_document(
            photo_id,
            exposure=payload.exposure,
            temperature=payload.temperature,
            tint=payload.tint,
        )
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.post("/{photo_id}/edit/undo")
def undo_edit(photo_id: str, request: Request) -> dict[str, object]:
    """Undo one persisted recipe change."""
    try:
        return _service(request).undo(photo_id)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc


@router.post("/{photo_id}/edit/redo")
def redo_edit(photo_id: str, request: Request) -> dict[str, object]:
    """Redo one persisted recipe change."""
    try:
        return _service(request).redo(photo_id)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc


@router.get("/{photo_id}/edit-preview")
def render_edit_preview(
    photo_id: str,
    request: Request,
    max_size: int = Query(default=1600, ge=64, le=4096),
) -> Response:
    """Render the current recipe into a disposable JPEG preview."""
    try:
        payload = _service(request).render_preview(photo_id, max_size=max_size)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=409, detail=str(exc)) from exc
    return Response(
        content=payload,
        media_type="image/jpeg",
        headers={"Cache-Control": "private, no-store"},
    )
