"""Versioned REST application API for frontends and local integrations."""

import threading
from pathlib import Path
from typing import cast

from fastapi import APIRouter, HTTPException, Query, Request, status
from pydantic import BaseModel, Field, field_validator

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.importing import CancelResult, GalleryImportService, PauseResult, ResumeResult
from photo_culler.sessions import SessionManagementService
from photo_culler.web.services.decision_service import DecisionService

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
    exclude_patterns: list[str] = Field(default_factory=list, max_length=50)


class GalleryImportEstimateRequest(BaseModel):
    """Read-only import preflight request."""

    path: str = Field(min_length=1, max_length=2048)
    recursive: bool = True
    exclude_patterns: list[str] = Field(default_factory=list, max_length=50)


class NativeDecisionRequest(BaseModel):
    """Decision mutation used by native delivery adapters."""

    decision: str = Field(min_length=1, max_length=32)


class NativeAnalysisStartRequest(BaseModel):
    """JSON equivalent of the analysis form used by native delivery adapters."""

    profile: str = Field(default="fast", min_length=1, max_length=128)
    scope: str = Field(default="remaining", pattern="^(remaining|all)$")


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


@router.get("/v1/catalog")
def list_catalog_for_native_clients(
    request: Request,
    gallery_id: str | None = None,
    offset: int = Query(default=0, ge=0),
    limit: int = Query(default=100, ge=1, le=200),
) -> dict[str, object]:
    """Return a paginated catalog DTO without exposing database records.

    Native frontends use the same application service boundary as the web UI;
    they never need direct SQLite access.
    """
    filters = {"gallery_id": gallery_id} if gallery_id else {}
    with request.app.state.db_engine.session() as session:
        repo = PhotoRepository(session)
        photos = repo.list_page(offset=offset, limit=limit, sort=None, filters=filters)
        total = repo.count_filtered(filters)
    return {
        "contract_version": 1,
        "items": [
            {
                "id": photo.photo_id,
                "name": photo.stem_name,
                "decision": photo.decision.value if hasattr(photo.decision, "value") else str(photo.decision),
                "score": photo.score,
                "quality_tier": (
                    photo.quality_tier.value if hasattr(photo.quality_tier, "value") else str(photo.quality_tier)
                ),
                "thumbnail_url": f"/thumbnails/{photo.photo_id}/800",
            }
            for photo in photos
        ],
        "offset": offset,
        "limit": limit,
        "total": total,
    }


@router.put("/v1/photos/{photo_id}/decision")
def set_photo_decision_for_native_clients(
    photo_id: str, request: Request, payload: NativeDecisionRequest
) -> dict[str, object]:
    """Persist a selection decision and return the normalized catalog value."""
    photo = DecisionService(request.app.state.db_engine).set_decision(photo_id, payload.decision)
    if not photo:
        raise HTTPException(status_code=404, detail="Photo not found")
    return {
        "contract_version": 1,
        "id": photo.photo_id,
        "decision": photo.decision.value if hasattr(photo.decision, "value") else str(photo.decision),
    }


@router.get("/v1/analysis/progress")
def native_analysis_progress(request: Request) -> dict[str, object]:
    """Expose a polling-friendly analysis snapshot for native applications."""
    return request.app.state.analysis_jobs.snapshot()


@router.post("/v1/analysis/start")
def native_start_analysis(request: Request, payload: NativeAnalysisStartRequest) -> dict[str, object]:
    """Start analysis using the same profile and worker manager as the web UI."""
    profile = request.app.state.analysis_profiles.get(payload.profile)
    if profile is None:
        raise HTTPException(status_code=422, detail="Unknown analysis profile")
    started = request.app.state.analysis_jobs.start_analysis(
        request.app.state.db_engine,
        profile=profile,
        import_service=request.app.state.gallery_imports,
        remaining_only=payload.scope == "remaining",
        legacy_cache_namespaces=[
            request.app.state.analysis_profiles.fingerprint(item) for item in request.app.state.analysis_profiles.list()
        ],
    )
    if not started:
        raise HTTPException(status_code=409, detail="Analysis already running")
    return request.app.state.analysis_jobs.snapshot()


@router.post("/v1/analysis/{action}")
def native_control_analysis(action: str, request: Request) -> dict[str, object]:
    """Pause, resume, or cancel the shared analysis job from a native UI."""
    operations = {
        "pause": request.app.state.analysis_jobs.pause,
        "resume": request.app.state.analysis_jobs.resume,
        "cancel": request.app.state.analysis_jobs.cancel,
    }
    operation = operations.get(action)
    if operation is None:
        raise HTTPException(status_code=404, detail="Unknown analysis action")
    if not operation():
        raise HTTPException(status_code=409, detail=f"Analysis cannot be {action}d")
    return request.app.state.analysis_jobs.snapshot()


@router.get("/v1/sessions")
def list_sessions_for_native_clients(request: Request) -> dict[str, object]:
    """Expose persisted sessions through the same service boundary as the web UI."""
    with request.app.state.db_engine.session() as session:
        sessions = SessionManagementService(session).list_sessions()
        return {
            "contract_version": 1,
            "items": [{"id": item.session_id, "name": item.name, "photo_count": item.photo_count} for item in sessions],
        }


@router.get("/v1/groups")
def list_similarity_groups_for_native_clients(request: Request) -> dict[str, object]:
    """List compact similarity-group DTOs for native review surfaces."""
    with request.app.state.db_engine.session() as session:
        repo = PhotoRepository(session)
        group_ids = repo.list_burst_ids("similar-", offset=0, limit=100)
        photos = repo.list_by_burst_ids(group_ids)
    groups = {
        group_id: sorted(
            (photo for photo in photos if photo.burst_id == group_id), key=lambda photo: (-photo.score, photo.stem_name)
        )
        for group_id in group_ids
    }
    return {
        "contract_version": 1,
        "items": [
            {"id": group_id, "photo_count": len(members), "recommended_photo_id": members[0].photo_id}
            for group_id, members in groups.items()
            if members
        ],
    }


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


@router.get("/v1/system-usage")
def get_system_usage(request: Request) -> dict[str, object]:
    """Retrieve system-wide and application-specific CPU and GPU utilization."""
    import os
    import shutil
    import subprocess

    cpu_sys = 0.0
    cpu_app = 0.0
    cpu_app_capacity = 0.0
    cpu_count = 1
    try:
        import psutil

        sampler_lock = getattr(request.app.state, "system_usage_lock", None)
        if sampler_lock is None:
            sampler_lock = threading.Lock()
            request.app.state.system_usage_lock = sampler_lock
        with sampler_lock:
            cpu_sys = psutil.cpu_percent(interval=None)
            proc = getattr(request.app.state, "system_usage_process", None)
            if proc is None:
                proc = psutil.Process(os.getpid())
                proc.cpu_percent(interval=None)  # Establish psutil's non-blocking baseline once.
                request.app.state.system_usage_process = proc
            cpu_app_raw = proc.cpu_percent(interval=None)
            cpu_count = psutil.cpu_count() or 1
            cpu_app = cpu_app_raw
            cpu_app_capacity = cpu_app_raw / cpu_count
    except Exception:
        cpu_sys = 5.0
        cpu_app = 1.0
        cpu_app_capacity = 1.0

    gpu_sys = 0.0
    gpu_name = "N/A"
    try:
        if shutil.which("nvidia-smi"):
            res = subprocess.run(
                ["nvidia-smi", "--query-gpu=utilization.gpu,name", "--format=csv,noheader,nounits"],
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                timeout=1.0,
                check=True,
            )
            first_gpu = next((line for line in res.stdout.splitlines() if line.strip()), "")
            parts = first_gpu.split(",")
            if parts and parts[0].strip():
                gpu_sys = float(parts[0].strip())
                gpu_name = parts[1].strip() if len(parts) > 1 else "NVIDIA GPU"
    except Exception:
        pass

    return {
        "contract_version": 1,
        "cpu_system": round(cpu_sys, 1),
        "cpu_app": round(cpu_app, 1),
        "cpu_app_capacity": round(cpu_app_capacity, 1),
        "cpu_core_count": cpu_count,
        "gpu_system": round(gpu_sys, 1),
        "gpu_name": gpu_name,
    }
