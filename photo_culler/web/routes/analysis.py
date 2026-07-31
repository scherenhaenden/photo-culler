"""Application-scoped technical analysis jobs with bounded SSE progress."""

import asyncio
import json
import logging
import queue
import threading
import time
from typing import cast

from fastapi import APIRouter, Body, Form, HTTPException, Request
from fastapi.responses import HTMLResponse, StreamingResponse

import photo_culler.analysis.analyzers.technical  # noqa: F401
from photo_culler.analysis.profiles import ANALYZER_CATALOG, DEFAULT_PROFILES

logger = logging.getLogger(__name__)


class AnalysisJobManager:
    """Run one cooperative analysis job for one application/catalog instance."""

    def __init__(self) -> None:
        self.is_running = False
        self.progress = 0
        self.processed = 0
        self.total = 0
        self.profile = "fast"
        self.profile_name = "Fast Scan"
        self.scope = "all"
        self.analyzers: list[str] = []
        self.executed_metrics = 0
        self.cached_metrics = 0
        self.status = "idle"
        self.message = "No hay análisis activos."
        self._listeners: list[queue.Queue[str]] = []
        self._lock = threading.Lock()
        self._control = threading.Condition(self._lock)
        self._thread: threading.Thread | None = None
        self._pause_requested = False
        self._cancel_requested = False

    def start_analysis(
        self,
        db_engine,
        profile: dict | str = "fast",
        import_service=None,
        remaining_only: bool = False,
        legacy_cache_namespaces: list[str] | None = None,
    ) -> bool:
        """Start analysis unless this application already owns an active job."""
        with self._control:
            if self.is_running:
                return False
            self.is_running = True
            self.status = "running"
            self.progress = 0
            self.processed = 0
            self.total = 0
            profile_config = (
                profile if isinstance(profile, dict) else DEFAULT_PROFILES.get(profile, DEFAULT_PROFILES["fast"])
            )
            self.profile = profile_config["id"]
            self.profile_name = profile_config["name"]
            self.scope = "remaining" if remaining_only else "all"
            self.analyzers = list(profile_config["analyzers"])
            self.executed_metrics = 0
            self.cached_metrics = 0
            self.message = "Iniciando análisis..."
            self._pause_requested = False
            self._cancel_requested = False
            self._thread = threading.Thread(
                target=self._run_analysis,
                args=(db_engine, profile_config, import_service, remaining_only, legacy_cache_namespaces or []),
                daemon=True,
            )
            self._thread.start()
        self._notify_listeners()
        return True

    def pause(self) -> bool:
        """Pause cooperatively at the next photo boundary."""
        with self._control:
            if not self.is_running or self.status == "paused":
                return False
            self._pause_requested = True
            self.status = "paused"
            self.message = "Análisis pausado."
            self._control.notify_all()
        self._notify_listeners()
        return True

    def resume(self) -> bool:
        """Resume a paused analysis job."""
        with self._control:
            if not self.is_running or not self._pause_requested:
                return False
            self._pause_requested = False
            self.status = "running"
            self.message = "Reanudando análisis..."
            self._control.notify_all()
        self._notify_listeners()
        return True

    def cancel(self) -> bool:
        """Cancel cooperatively at the next photo boundary."""
        with self._control:
            if not self.is_running:
                return False
            self._cancel_requested = True
            self._pause_requested = False
            self._control.notify_all()
        return True

    def shutdown(self, timeout: float = 5.0) -> None:
        """Stop this application's worker before its catalog is disposed."""
        self.cancel()
        with self._lock:
            thread = self._thread
        if thread is not None:
            thread.join(timeout=timeout)

    def snapshot(self) -> dict[str, object]:
        """Return the current versioned progress contract."""
        with self._lock:
            return {
                "contract_version": 1,
                "status": self.status,
                "progress": self.progress,
                "processed": self.processed,
                "total": self.total,
                "profile": self.profile,
                "profile_name": self.profile_name,
                "scope": self.scope,
                "analyzers": list(self.analyzers),
                "executed_metrics": self.executed_metrics,
                "cached_metrics": self.cached_metrics,
                "message": self.message,
            }

    def register_listener(self) -> queue.Queue[str]:
        """Register one bounded listener and return its event queue."""
        listener: queue.Queue[str] = queue.Queue(maxsize=8)
        with self._lock:
            self._listeners.append(listener)
        return listener

    def unregister_listener(self, listener: queue.Queue[str]) -> None:
        """Remove one SSE listener without affecting other application instances."""
        with self._lock:
            if listener in self._listeners:
                self._listeners.remove(listener)

    def _run_analysis(
        self, db_engine, profile: dict, import_service=None, remaining_only: bool = False, legacy_cache_namespaces=None
    ) -> None:
        try:
            from photo_culler.analysis.engine.cache import MetricCache
            from photo_culler.analysis.engine.pipeline import AnalysisPipeline
            from photo_culler.analysis.engine.registry import default_registry
            from photo_culler.analysis.explanation import build_score_explanation
            from photo_culler.analysis.profiles import AnalysisProfileStore
            from photo_culler.catalog.repositories.photo_repository import PhotoRepository
            from photo_culler.cli.helpers.asset_resolver import AnalysisAssetResolver
            from photo_culler.grouping import SimilarityGrouper
            from photo_culler.scoring.technical_score import TechnicalScorer
            from photo_culler.selection.decisions.rules import SelectionRulesEngine

            while import_service is not None and import_service.active_job_count() > 0:
                if not self._wait_for_control():
                    self._finish("cancelled", "Análisis cancelado.")
                    return
                with self._lock:
                    self.message = (
                        "Importación en curso. El análisis comenzará automáticamente "
                        "cuando las fotos estén en el catálogo…"
                    )
                self._notify_listeners()
                time.sleep(0.05)

            cache_namespace = AnalysisProfileStore.fingerprint(profile)
            with db_engine.session() as session:
                repository = PhotoRepository(session)
                photos = (
                    repository.list_needing_analysis(profile["id"], cache_namespace)
                    if remaining_only
                    else repository.list_all()
                )

            with self._lock:
                self.total = len(photos)

            if not photos:
                message = (
                    "No quedan fotos pendientes para este perfil."
                    if remaining_only
                    else "No hay fotos en el catálogo para analizar."
                )
                self._finish("completed", message)
                return

            cache = MetricCache(db_path=str(db_engine.db_path) + ".metrics.db")
            pipeline = AnalysisPipeline(cache=cache, use_cache=True)
            asset_resolver = AnalysisAssetResolver()
            analyzer_instances = []
            for analyzer_name in profile["analyzers"]:
                analyzer_class = default_registry.get(analyzer_name)
                if analyzer_class is None:
                    raise ValueError(f"Analyzer not registered: {analyzer_name}")
                analyzer_instances.append(analyzer_class())
            weights = profile["weights"]
            scorer = TechnicalScorer(
                profile=profile["clipping_mode"],
                weight_sharpness=weights["sharpness"],
                weight_exposure=weights["exposure"],
                weight_clipping=weights["clipping"],
                weight_noise=weights["noise"],
            )
            for index, photo in enumerate(photos):
                if not self._wait_for_control():
                    self._finish("cancelled", "Análisis cancelado.")
                    return
                with self._lock:
                    self.processed = index + 1
                    self.progress = int((self.processed / self.total) * 100)
                    self.message = f"Analizando {photo.stem_name} ({self.processed}/{self.total})"
                self._notify_listeners()

                image_asset = asset_resolver.resolve(photo, prefer_jpeg=True)
                if image_asset and image_asset.exists():
                    asset_stat = image_asset.stat()
                    results = pipeline.run_image(
                        image_path=image_asset,
                        image_hash=f"{photo.photo_id}:{asset_stat.st_size}:{asset_stat.st_mtime_ns}",
                        analyzers=analyzer_instances,
                        cache_fallback_hashes=[
                            f"{photo.photo_id}:{namespace}" for namespace in legacy_cache_namespaces
                        ],
                    )
                    with self._lock:
                        self.executed_metrics += pipeline.last_run_stats["executed"]
                        self.cached_metrics += pipeline.last_run_stats["cached"]
                    technical_score = scorer.calculate_score(results)
                    photo.score = technical_score["final_score"]
                    photo.quality_tier = technical_score["quality_tier"]
                with db_engine.session() as session:
                    repository = PhotoRepository(session)
                    repository.save_photo(photo)
                    if image_asset and image_asset.exists():
                        repository.save_analysis_summary(
                            photo.photo_id, build_score_explanation(profile, technical_score, results, cache_namespace)
                        )

            similarity_groups, _ = SimilarityGrouper().group(
                photos, lambda item: asset_resolver.resolve(item, prefer_jpeg=True)
            )
            SelectionRulesEngine().apply_decisions(photos, bursts=similarity_groups)
            with db_engine.session() as session:
                repository = PhotoRepository(session)
                for photo in photos:
                    repository.save_photo(photo)

            self._finish(
                "completed",
                f"Análisis finalizado con éxito. {self.total} fotos analizadas.",
            )
        except Exception as exc:
            logger.exception("Technical analysis job failed")
            self._finish(
                "failed",
                f"El análisis falló: {type(exc).__name__}: {exc}. Revisa el log local para más detalles.",
            )
        finally:
            with self._lock:
                self.is_running = False

    def _wait_for_control(self) -> bool:
        """Wait without polling while paused and report whether work may continue."""
        with self._control:
            while self._pause_requested and not self._cancel_requested:
                self._control.wait()
            return not self._cancel_requested

    def _finish(self, status: str, message: str) -> None:
        with self._lock:
            self.status = status
            self.progress = 100 if status == "completed" else self.progress
            self.message = message
        self._notify_listeners()

    def _notify_listeners(self) -> None:
        payload = json.dumps(self.snapshot())
        with self._lock:
            listeners = tuple(self._listeners)
        for listener in listeners:
            try:
                listener.put_nowait(payload)
            except queue.Full:
                try:
                    listener.get_nowait()
                except queue.Empty:
                    pass
                try:
                    listener.put_nowait(payload)
                except queue.Full:
                    pass


router = APIRouter()


def _manager(request: Request) -> AnalysisJobManager:
    return cast(AnalysisJobManager, request.app.state.analysis_jobs)


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
            request.app.state.analysis_profiles.fingerprint(item)
            for item in request.app.state.analysis_profiles.list()
        ],
    )
    return {
        "status": "ok" if success else "error",
        "message": "Análisis de pendientes iniciado" if success and scope == "remaining" else "Análisis iniciado"
        if success
        else "Análisis ya en ejecución",
    }


@router.post("/analysis/group-similar")
def group_similar_photos(request: Request) -> dict[str, object]:
    """Group visually similar nearby photos without changing their keep/reject decision."""
    from photo_culler.catalog.repositories.photo_repository import PhotoRepository
    from photo_culler.cli.helpers.asset_resolver import AnalysisAssetResolver
    from photo_culler.grouping import SimilarityGrouper

    try:
        with request.app.state.db_engine.session() as session:
            repository = PhotoRepository(session)
            photos = repository.list_all()
            asset_resolver = AnalysisAssetResolver()
            groups, skipped = SimilarityGrouper().group(
                photos, lambda item: asset_resolver.resolve(item, prefer_jpeg=True)
            )
            for photo in photos:
                repository.save_photo(photo)
    except Exception as exc:
        logger.exception("Similarity grouping failed")
        raise HTTPException(status_code=500, detail="No se pudieron agrupar las fotos. Revisa el log local.") from exc
    return {
        "groups": len(groups),
        "grouped_photos": sum(len(group.photos) for group in groups),
        "skipped": skipped,
    }


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
