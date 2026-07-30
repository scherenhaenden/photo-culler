"""Application-scoped technical analysis jobs with bounded SSE progress."""

import asyncio
import json
import logging
import queue
import threading
from typing import cast

from fastapi import APIRouter, Form, HTTPException, Request
from fastapi.responses import HTMLResponse, StreamingResponse

logger = logging.getLogger(__name__)


class AnalysisJobManager:
    """Run one cooperative analysis job for one application/catalog instance."""

    def __init__(self) -> None:
        self.is_running = False
        self.progress = 0
        self.processed = 0
        self.total = 0
        self.status = "idle"
        self.message = "No hay análisis activos."
        self._listeners: list[queue.Queue[str]] = []
        self._lock = threading.Lock()
        self._control = threading.Condition(self._lock)
        self._thread: threading.Thread | None = None
        self._pause_requested = False
        self._cancel_requested = False

    def start_analysis(self, db_engine, profile: str = "fast") -> bool:
        """Start analysis unless this application already owns an active job."""
        with self._control:
            if self.is_running:
                return False
            self.is_running = True
            self.status = "running"
            self.progress = 0
            self.processed = 0
            self.total = 0
            self.message = "Iniciando análisis..."
            self._pause_requested = False
            self._cancel_requested = False
            self._thread = threading.Thread(
                target=self._run_analysis,
                args=(db_engine, profile),
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

    def _run_analysis(self, db_engine, profile: str) -> None:
        try:
            from photo_culler.analysis.engine.cache import MetricCache
            from photo_culler.analysis.engine.pipeline import AnalysisPipeline
            from photo_culler.catalog.repositories.photo_repository import PhotoRepository
            from photo_culler.cli.helpers.asset_resolver import AnalysisAssetResolver
            from photo_culler.scoring.technical_score import TechnicalScorer
            from photo_culler.selection.decisions.rules import SelectionRulesEngine

            with db_engine.session() as session:
                photos = PhotoRepository(session).list_all()

            with self._lock:
                self.total = len(photos)

            if not photos:
                self._finish("completed", "No hay fotos en el catálogo para analizar.")
                return

            cache = MetricCache(db_path=str(db_engine.db_path) + ".metrics.db")
            pipeline = AnalysisPipeline(cache=cache, use_cache=True)
            asset_resolver = AnalysisAssetResolver()
            scorer = TechnicalScorer()

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
                    results = pipeline.run_image(
                        image_path=image_asset,
                        image_hash=photo.photo_id,
                    )
                    technical_score = scorer.calculate_score(results)
                    photo.score = technical_score["final_score"]
                    photo.quality_tier = technical_score["quality_tier"]
                with db_engine.session() as session:
                    PhotoRepository(session).save_photo(photo)

            SelectionRulesEngine().apply_decisions(photos)
            with db_engine.session() as session:
                repository = PhotoRepository(session)
                for photo in photos:
                    repository.save_photo(photo)

            self._finish(
                "completed",
                f"Análisis finalizado con éxito. {self.total} fotos analizadas.",
            )
        except Exception:
            logger.exception("Technical analysis job failed")
            self._finish("failed", "El análisis falló; revisa los logs locales.")
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
        context={"active_tab": "analysis", "job": _manager(request)},
    )


@router.post("/analysis/start")
def start_analysis(request: Request, profile: str = Form("fast")):
    success = _manager(request).start_analysis(
        request.app.state.db_engine,
        profile=profile,
    )
    return {
        "status": "ok" if success else "error",
        "message": "Análisis iniciado" if success else "Análisis ya en ejecución",
    }


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
