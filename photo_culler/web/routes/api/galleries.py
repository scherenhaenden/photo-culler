"""Gallery and import REST API routes."""

from pathlib import Path
from typing import cast

from fastapi import APIRouter, HTTPException, Request, status

from photo_culler.importing import CancelResult, GalleryImportService, PauseResult, ResumeResult
from photo_culler.web.routes.api.schemas import (
    GalleryCreateRequest,
    GalleryImportEstimateRequest,
    GalleryImportRequest,
)

router = APIRouter()


def _gallery_import_service(request: Request) -> GalleryImportService:
    """Return the application-scoped import service with its concrete type."""
    return cast(GalleryImportService, request.app.state.gallery_imports)


@router.get("/v1/galleries")
def list_galleries(request: Request) -> dict[str, object]:
    """List logical galleries without exposing ORM records."""
    return {"contract_version": 1, "items": _gallery_import_service(request).list_galleries()}


@router.post("/v1/galleries", status_code=status.HTTP_201_CREATED)
def create_gallery(request: Request, payload: GalleryCreateRequest) -> dict[str, object]:
    """Create a logical gallery."""
    gallery_id = _gallery_import_service(request).create_gallery(payload.name)
    return {"contract_version": 1, "id": gallery_id, "name": payload.name.strip()}


@router.get("/v1/galleries/{gallery_id}/sources")
def list_gallery_sources(gallery_id: str, request: Request) -> dict[str, object]:
    """List configured physical sources for one logical gallery."""
    galleries = _gallery_import_service(request).list_galleries()
    if not any(gallery["id"] == gallery_id for gallery in galleries):
        raise HTTPException(status_code=404, detail="Gallery not found")
    return {
        "contract_version": 1,
        "items": _gallery_import_service(request).list_sources(gallery_id),
    }


@router.post("/v1/galleries/{gallery_id}/imports", status_code=status.HTTP_202_ACCEPTED)
def import_gallery(gallery_id: str, request: Request, payload: GalleryImportRequest) -> dict[str, object]:
    """Queue a persistent import job."""
    try:
        job_id = _gallery_import_service(request).start_import(
            gallery_id,
            Path(payload.path),
            recursive=payload.recursive,
            exclude_patterns=payload.exclude_patterns,
        )
    except FileNotFoundError as exc:
        raise HTTPException(status_code=404, detail="Import source does not exist") from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    return {"contract_version": 1, "job_id": job_id, "state": "queued"}


@router.post("/v1/galleries/{gallery_id}/rescan", status_code=status.HTTP_202_ACCEPTED)
def rescan_gallery(gallery_id: str, request: Request) -> dict[str, object]:
    """Rescan all configured sources and report unavailable ones."""
    try:
        return _gallery_import_service(request).rescan_gallery(gallery_id)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc


@router.post("/v1/import-estimates")
def estimate_gallery_import(request: Request, payload: GalleryImportEstimateRequest) -> dict[str, object]:
    """Return a lightweight source estimate before import confirmation."""
    try:
        return _gallery_import_service(request).estimate_import(
            Path(payload.path),
            recursive=payload.recursive,
            exclude_patterns=payload.exclude_patterns,
        )
    except FileNotFoundError as exc:
        raise HTTPException(status_code=404, detail="Import source does not exist") from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.get("/v1/import-jobs/{job_id}")
def get_import_job(job_id: str, request: Request) -> dict[str, object]:
    """Return persisted progress for an import job."""
    job = _gallery_import_service(request).get_job(job_id)
    if job is None:
        raise HTTPException(status_code=404, detail="Import job not found")
    return job


@router.get("/v1/import-jobs")
def list_import_jobs(request: Request, limit: int = 20) -> dict[str, object]:
    """List recent persisted jobs for frontend recovery."""
    return {
        "contract_version": 1,
        "items": _gallery_import_service(request).list_jobs(limit=limit),
    }


@router.get("/v1/scan-revisions")
def list_scan_revisions(
    request: Request,
    gallery_id: str | None = None,
    limit: int = 20,
) -> dict[str, object]:
    """List persisted source reconciliation results."""
    return {
        "contract_version": 1,
        "items": _gallery_import_service(request).list_scan_revisions(
            gallery_id=gallery_id,
            limit=limit,
        ),
    }


@router.post("/v1/import-jobs/{job_id}/cancel", status_code=status.HTTP_202_ACCEPTED)
def cancel_import_job(job_id: str, request: Request) -> dict[str, object]:
    """Request cooperative cancellation."""
    result = _gallery_import_service(request).cancel(job_id)
    if result is CancelResult.NOT_FOUND:
        raise HTTPException(status_code=404, detail="Import job not found")
    if result is CancelResult.NOT_CANCELLABLE:
        raise HTTPException(status_code=409, detail="Import job cannot be cancelled")
    return {"contract_version": 1, "job_id": job_id, "cancel_requested": True}


@router.post("/v1/import-jobs/{job_id}/pause", status_code=status.HTTP_202_ACCEPTED)
def pause_import_job(job_id: str, request: Request) -> dict[str, object]:
    """Request a durable cooperative pause."""
    result = _gallery_import_service(request).pause(job_id)
    if result is PauseResult.NOT_FOUND:
        raise HTTPException(status_code=404, detail="Import job not found")
    if result is PauseResult.NOT_PAUSABLE:
        raise HTTPException(status_code=409, detail="Import job cannot be paused")
    return {"contract_version": 1, "job_id": job_id, "pause_requested": True}


@router.post("/v1/import-jobs/{job_id}/resume", status_code=status.HTTP_202_ACCEPTED)
def resume_import_job(job_id: str, request: Request) -> dict[str, object]:
    """Resume a paused import from its persisted source."""
    result = _gallery_import_service(request).resume(job_id)
    if result is ResumeResult.NOT_FOUND:
        raise HTTPException(status_code=404, detail="Import job not found")
    if result is ResumeResult.NOT_RESUMABLE:
        raise HTTPException(status_code=409, detail="Import job cannot be resumed")
    if result is ResumeResult.SOURCE_UNAVAILABLE:
        raise HTTPException(status_code=409, detail="Import source is unavailable")
    return {"contract_version": 1, "job_id": job_id, "state": "queued"}
