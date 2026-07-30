"""Versioned REST application API for frontends and local integrations."""

from pathlib import Path
from typing import cast

from fastapi import APIRouter, HTTPException, Request, status
from pydantic import BaseModel, Field, field_validator

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.importing import CancelResult, GalleryImportService

router = APIRouter(prefix="/api")


def _gallery_import_service(request: Request) -> GalleryImportService:
    """Return the application-scoped import service with its concrete type."""
    return cast(GalleryImportService, request.app.state.gallery_imports)


class GalleryCreateRequest(BaseModel):
    """Create-gallery API request."""

    name: str = Field(min_length=1, max_length=255)

    @field_validator("name")
    @classmethod
    def name_must_not_be_blank(cls, value: str) -> str:
        """Reject names that contain only whitespace."""
        if not value.strip():
            raise ValueError("Gallery name cannot be empty")
        return value


class GalleryImportRequest(BaseModel):
    """Non-copying import request."""

    path: str = Field(min_length=1, max_length=2048)
    recursive: bool = True


@router.get("/health")
def health_check():
    """Health check endpoint."""
    return {"status": "ok", "app": "photo-culler", "version": "0.1.0"}


@router.get("/photos")
def list_photos_api(request: Request):
    """Return JSON list of indexed photos."""
    db_engine = request.app.state.db_engine
    with db_engine.session() as session:
        repo = PhotoRepository(session)
        photos = repo.list_all()

    return [
        {
            "photo_id": p.photo_id,
            "stem_name": p.stem_name,
            "decision": p.decision.value if hasattr(p.decision, "value") else str(p.decision),
            "score": p.score,
            "quality_tier": p.quality_tier.value if hasattr(p.quality_tier, "value") else str(p.quality_tier),
        }
        for p in photos
    ]


@router.get("/v1/galleries")
def list_galleries(request: Request) -> dict[str, object]:
    """List logical galleries without exposing ORM records."""
    return {"contract_version": 1, "items": _gallery_import_service(request).list_galleries()}


@router.post("/v1/galleries", status_code=status.HTTP_201_CREATED)
def create_gallery(request: Request, payload: GalleryCreateRequest) -> dict[str, object]:
    """Create a logical gallery."""
    gallery_id = _gallery_import_service(request).create_gallery(payload.name)
    return {"contract_version": 1, "id": gallery_id, "name": payload.name.strip()}


@router.post("/v1/galleries/{gallery_id}/imports", status_code=status.HTTP_202_ACCEPTED)
def import_gallery(gallery_id: str, request: Request, payload: GalleryImportRequest) -> dict[str, object]:
    """Queue a persistent import job."""
    try:
        job_id = _gallery_import_service(request).start_import(
            gallery_id, Path(payload.path), recursive=payload.recursive
        )
    except FileNotFoundError as exc:
        raise HTTPException(status_code=404, detail="Import source does not exist") from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    return {"contract_version": 1, "job_id": job_id, "state": "queued"}


@router.get("/v1/import-jobs/{job_id}")
def get_import_job(job_id: str, request: Request) -> dict[str, object]:
    """Return persisted progress for an import job."""
    job = _gallery_import_service(request).get_job(job_id)
    if job is None:
        raise HTTPException(status_code=404, detail="Import job not found")
    return job


@router.post("/v1/import-jobs/{job_id}/cancel", status_code=status.HTTP_202_ACCEPTED)
def cancel_import_job(job_id: str, request: Request) -> dict[str, object]:
    """Request cooperative cancellation."""
    result = _gallery_import_service(request).cancel(job_id)
    if result is CancelResult.NOT_FOUND:
        raise HTTPException(status_code=404, detail="Import job not found")
    if result is CancelResult.NOT_CANCELLABLE:
        raise HTTPException(status_code=409, detail="Import job cannot be cancelled")
    return {"contract_version": 1, "job_id": job_id, "cancel_requested": True}
