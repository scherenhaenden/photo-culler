"""REST API routes for photos, catalog, analysis control, sessions and system metrics."""

import threading
from typing import cast

from fastapi import APIRouter, HTTPException, Query, Request

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.sessions import SessionManagementService
from photo_culler.web.services.decision_service import DecisionService
from photo_culler.web.routes.api.schemas import (
    NativeAnalysisStartRequest,
    NativeDecisionRequest,
)

router = APIRouter()


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
    return cast(dict[str, object], request.app.state.analysis_jobs.snapshot())


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
    return cast(dict[str, object], request.app.state.analysis_jobs.snapshot())


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
    return cast(dict[str, object], request.app.state.analysis_jobs.snapshot())


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


@router.get("/v1/summary")
def get_catalog_summary_api(request: Request) -> dict[str, object]:
    """Return high-level catalog statistics and summary metrics."""
    from photo_culler.web.services.library_service import LibraryService
    return LibraryService(request.app.state.db_engine).get_summary()


@router.get("/v1/photos/{photo_id}")
def get_photo_detail_api(photo_id: str, request: Request) -> dict[str, object]:
    """Return full JSON details for a single photo including metadata and analysis summary."""
    with request.app.state.db_engine.session() as session:
        repo = PhotoRepository(session)
        photo = repo.get_by_id(photo_id)
        if not photo:
            raise HTTPException(status_code=404, detail="Photo not found")

        analysis_summary = repo.get_analysis_summary(photo_id)

        metadata = None
        if photo.metadata_record:
            m = photo.metadata_record
            metadata = {
                "camera_model": m.camera_model,
                "lens": m.lens,
                "iso": m.iso,
                "aperture": m.aperture,
                "shutter_speed": m.shutter_speed,
                "focal_length": m.focal_length,
                "capture_time": m.capture_time.strftime('%Y-%m-%d %H:%M:%S') if m.capture_time else None,
            }

        return {
            "contract_version": 1,
            "id": photo.photo_id,
            "name": photo.stem_name,
            "decision": photo.decision.value if hasattr(photo.decision, "value") else str(photo.decision),
            "score": photo.score,
            "quality_tier": photo.quality_tier.value if hasattr(photo.quality_tier, "value") else str(photo.quality_tier),
            "analysis_summary": analysis_summary,
            "metadata": metadata,
        }
