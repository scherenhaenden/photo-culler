"""Application-scoped technical analysis routing and views."""

import asyncio
import json
import queue
from typing import cast

from fastapi import APIRouter, Body, Form, HTTPException, Request
from fastapi.responses import HTMLResponse, StreamingResponse

import photo_culler.analysis.analyzers.technical  # noqa: F401
import photo_culler.analysis.analyzers.composition  # noqa: F401
import photo_culler.analysis.analyzers.geometry  # noqa: F401
from photo_culler.analysis.profiles import ANALYZER_CATALOG
from photo_culler.web.services.analysis_manager import (
    AnalysisJobManager,
    SimilarityGroupingJobManager,
)

# Re-export for app factory and test compatibility
__all__ = ["AnalysisJobManager", "SimilarityGroupingJobManager"]

router = APIRouter()


def _manager(request: Request) -> AnalysisJobManager:
    return cast(AnalysisJobManager, request.app.state.analysis_jobs)


def _grouping_manager(request: Request) -> SimilarityGroupingJobManager:
    return cast(SimilarityGroupingJobManager, request.app.state.similarity_grouping_jobs)


@router.get("/analysis", response_class=HTMLResponse)
def get_analysis_page(request: Request):
    templates = request.app.state.templates
    return templates.TemplateResponse(
        request=request,
        name="analysis.html",
        context={
            "active_tab": "analysis",
            "job": _manager(request),
            "profiles": request.app.state.analysis_profiles.list(),
            "analyzer_catalog": ANALYZER_CATALOG,
        },
    )


@router.post("/analysis/start")
def start_analysis(request: Request, profile: str = Form("fast"), scope: str = Form("remaining")):
    profile_config = request.app.state.analysis_profiles.get(profile)
    if profile_config is None:
        raise HTTPException(status_code=422, detail="Unknown analysis profile")
    if scope not in {"remaining", "all"}:
        raise HTTPException(status_code=422, detail="Unknown analysis scope")
    success = _manager(request).start_analysis(
        request.app.state.db_engine,
        profile=profile_config,
        import_service=request.app.state.gallery_imports,
        remaining_only=scope == "remaining",
        legacy_cache_namespaces=[
            request.app.state.analysis_profiles.fingerprint(item) for item in request.app.state.analysis_profiles.list()
        ],
    )
    return {
        "status": "ok" if success else "error",
        "message": "Análisis de pendientes iniciado"
        if success and scope == "remaining"
        else "Análisis iniciado"
        if success
        else "Análisis ya en ejecución",
    }


@router.post("/analysis/workers")
def set_analysis_workers(request: Request, workers: int = Form(...)) -> dict[str, int]:
    manager = _manager(request)
    return {"workers": manager.set_workers(workers), "max_workers": manager.max_workers}


@router.post("/analysis/group-similar")
def group_similar_photos(request: Request) -> dict[str, object]:
    """Start non-blocking similarity grouping, so the browser can show real progress."""
    manager = _grouping_manager(request)
    if not manager.start(request.app.state.db_engine):
        raise HTTPException(status_code=409, detail="Ya hay una agrupación en ejecución.")
    return {"status": "ok", "message": "Agrupación iniciada."}


@router.get("/analysis/group-similar/progress")
async def similarity_grouping_progress_events(request: Request):
    manager = _grouping_manager(request)
    listener = manager.register_listener()

    async def event_generator():
        try:
            yield f"data: {json.dumps(manager.snapshot())}\n\n"
            while True:
                if await request.is_disconnected():
                    break
                try:
                    data = await asyncio.get_running_loop().run_in_executor(None, lambda: listener.get(timeout=0.5))
                    yield f"data: {data}\n\n"
                    if json.loads(data).get("status") in {"completed", "failed"}:
                        break
                except queue.Empty:
                    if not manager.is_running:
                        break
        finally:
            manager.unregister_listener(listener)

    return StreamingResponse(event_generator(), media_type="text/event-stream")


@router.get("/analysis/profiles")
def list_profiles(request: Request) -> dict[str, object]:
    return {"profiles": request.app.state.analysis_profiles.list()}


@router.post("/analysis/profiles", status_code=201)
def create_profile(request: Request, payload: dict = Body(...)) -> dict[str, object]:
    try:
        return {"profile": request.app.state.analysis_profiles.save(payload)}
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.put("/analysis/profiles/{profile_id}")
def update_profile(request: Request, profile_id: str, payload: dict = Body(...)) -> dict[str, object]:
    try:
        return {"profile": request.app.state.analysis_profiles.save(payload, profile_id=profile_id)}
    except KeyError as exc:
        raise HTTPException(status_code=404, detail="Profile not found") from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.delete("/analysis/profiles/{profile_id}", status_code=204)
def delete_profile(request: Request, profile_id: str):
    try:
        request.app.state.analysis_profiles.delete(profile_id)
    except KeyError as exc:
        raise HTTPException(status_code=404, detail="Profile not found") from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.post("/analysis/profiles/{profile_id}/restore")
def restore_profile(request: Request, profile_id: str) -> dict[str, object]:
    try:
        return {"profile": request.app.state.analysis_profiles.restore(profile_id)}
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc


@router.post("/analysis/pause")
def pause_analysis(request: Request) -> dict[str, object]:
    if not _manager(request).pause():
        raise HTTPException(status_code=409, detail="Analysis cannot be paused")
    return _manager(request).snapshot()


@router.post("/analysis/resume")
def resume_analysis(request: Request) -> dict[str, object]:
    if not _manager(request).resume():
        raise HTTPException(status_code=409, detail="Analysis cannot be resumed")
    return _manager(request).snapshot()


@router.post("/analysis/cancel")
def cancel_analysis(request: Request) -> dict[str, object]:
    if not _manager(request).cancel():
        raise HTTPException(status_code=409, detail="Analysis cannot be cancelled")
    return _manager(request).snapshot()


@router.get("/analysis/progress")
async def progress_events(request: Request):
    manager = _manager(request)
    listener = manager.register_listener()

    async def event_generator():
        try:
            yield f"data: {json.dumps(manager.snapshot())}\n\n"
            while True:
                if await request.is_disconnected():
                    break
                try:
                    loop = asyncio.get_running_loop()
                    data = await loop.run_in_executor(
                        None,
                        lambda: listener.get(timeout=0.5),
                    )
                    yield f"data: {data}\n\n"
                    parsed = json.loads(data)
                    if parsed.get("status") in {
                        "completed",
                        "failed",
                        "cancelled",
                    }:
                        break
                except queue.Empty:
                    if not manager.is_running:
                        break
        finally:
            manager.unregister_listener(listener)

    return StreamingResponse(event_generator(), media_type="text/event-stream")
