"""Analysis Web Route with Background Runner and SSE Event Channel."""

import asyncio
import json
import queue
import threading
import time
from typing import Optional

from fastapi import APIRouter, Form, Request
from fastapi.responses import HTMLResponse, StreamingResponse


class AnalysisJobManager:
    """Manages background photo analysis jobs and broadcasts progress to connected clients."""

    def __init__(self):
        self.is_running = False
        self.progress = 0  # 0 to 100
        self.processed = 0
        self.total = 0
        self.status = "idle"  # idle, running, completed, failed
        self.message = "No hay análisis activos."
        self.listeners = []
        self._lock = threading.Lock()

    def start_analysis(self, db_engine, profile: str = "fast") -> bool:
        """Start photo analysis in a background thread if not already running."""
        with self._lock:
            if self.is_running:
                return False

            self.is_running = True
            self.status = "running"
            self.progress = 0
            self.processed = 0
            self.total = 0
            self.message = "Iniciando análisis..."

        threading.Thread(target=self._run_analysis, args=(db_engine, profile), daemon=True).start()
        return True

    def _run_analysis(self, db_engine, profile: str):
        try:
            from photo_culler.analysis.engine.cache import MetricCache
            from photo_culler.analysis.engine.pipeline import AnalysisPipeline
            from photo_culler.catalog.repositories.photo_repository import PhotoRepository
            from photo_culler.cli.helpers.asset_resolver import AnalysisAssetResolver
            from photo_culler.scoring.technical_score import TechnicalScorer
            from photo_culler.selection.decisions.rules import SelectionRulesEngine

            with db_engine.session() as s:
                repo = PhotoRepository(s)
                photos = repo.list_all()

                with self._lock:
                    self.total = len(photos)

                if self.total == 0:
                    with self._lock:
                        self.is_running = False
                        self.status = "completed"
                        self.progress = 100
                        self.message = "No hay fotos en el catálogo para analizar."
                    self._notify_listeners()
                    return

                cache_path = str(db_engine.db_path) + ".metrics.db"
                cache = MetricCache(db_path=cache_path)
                pipeline = AnalysisPipeline(cache=cache, use_cache=True)
                asset_resolver = AnalysisAssetResolver()
                scorer = TechnicalScorer()
                rules_engine = SelectionRulesEngine()

                for i, p in enumerate(photos):
                    # Check if the process pool execution is simulated/enqueued
                    with self._lock:
                        self.processed = i + 1
                        self.progress = int((self.processed / self.total) * 100)
                        self.message = f"Analizando {p.stem_name} ({self.processed}/{self.total})"
                    self._notify_listeners()

                    img_asset = asset_resolver.resolve(p, prefer_jpeg=True)
                    if img_asset and img_asset.exists():
                        results = pipeline.run_image(image_path=img_asset, image_hash=p.photo_id)
                        tech_score = scorer.calculate_score(results)

                        p.score = tech_score["final_score"]
                        p.quality_tier = tech_score["quality_tier"]

                    repo.save_photo(p)
                    # Yield slightly to avoid blocking other threads entirely
                    time.sleep(0.01)

                rules_engine.apply_decisions(photos)
                for p in photos:
                    repo.save_photo(p)

                s.commit()

            with self._lock:
                self.status = "completed"
                self.progress = 100
                self.message = f"Análisis finalizado con éxito. {self.total} fotos analizadas."
            self._notify_listeners()

        except Exception as e:
            with self._lock:
                self.status = "failed"
                self.message = f"Error durante el análisis: {str(e)}"
            self._notify_listeners()
        finally:
            with self._lock:
                self.is_running = False

    def register_listener(self, q: queue.Queue):
        with self._lock:
            self.listeners.append(q)

    def unregister_listener(self, q: queue.Queue):
        with self._lock:
            if q in self.listeners:
                self.listeners.remove(q)

    def _notify_listeners(self):
        with self._lock:
            data = {
                "status": self.status,
                "progress": self.progress,
                "processed": self.processed,
                "total": self.total,
                "message": self.message,
            }
            payload = json.dumps(data)
            for q in self.listeners:
                try:
                    q.put_nowait(payload)
                except Exception:
                    pass


router = APIRouter()
job_manager = AnalysisJobManager()


@router.get("/analysis", response_class=HTMLResponse)
def get_analysis_page(request: Request):
    templates = request.app.state.templates
    return templates.TemplateResponse(
        request=request,
        name="analysis.html",
        context={"active_tab": "analysis", "job": job_manager},
    )


@router.post("/analysis/start")
def start_analysis(request: Request, profile: str = Form("fast")):
    db_engine = request.app.state.db_engine
    success = job_manager.start_analysis(db_engine, profile=profile)
    return {
        "status": "ok" if success else "error",
        "message": "Análisis iniciado" if success else "Análisis ya en ejecución",
    }


@router.get("/analysis/progress")
async def progress_events(request: Request):
    q = queue.Queue()
    job_manager.register_listener(q)

    async def event_generator():
        try:
            # Yield current state immediately
            initial_data = {
                "status": job_manager.status,
                "progress": job_manager.progress,
                "processed": job_manager.processed,
                "total": job_manager.total,
                "message": job_manager.message,
            }
            yield f"data: {json.dumps(initial_data)}\n\n"

            while True:
                if await request.is_disconnected():
                    break

                try:
                    # Non-blocking check for next message
                    loop = asyncio.get_running_loop()
                    data = await loop.run_in_executor(None, lambda: q.get(timeout=0.5))
                    yield f"data: {data}\n\n"

                    try:
                        parsed = json.loads(data)
                        if parsed.get("status") in ("completed", "failed"):
                            break
                    except Exception:
                        pass
                except queue.Empty:
                    if not job_manager.is_running:
                        break
                    continue
        finally:
            job_manager.unregister_listener(q)

    return StreamingResponse(event_generator(), media_type="text/event-stream")
