"""FastAPI Web Application Factory."""

from pathlib import Path
from typing import Optional, Union

from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates

from photo_culler.catalog.database import Database
from photo_culler.web.routes import analysis, api, dashboard, library, photos, sessions


def create_app(catalog_path: Optional[Union[str, Path]] = None) -> FastAPI:
    """Create and configure FastAPI application instance."""
    app = FastAPI(title="Photo Culler", version="0.1.0")

    # Catalog DB setup
    cat_path = Path(catalog_path) if catalog_path else Path("catalog.db")
    db_engine = Database(cat_path)
    db_engine.create_tables()
    app.state.db_engine = db_engine

    # Templates & Static Files setup
    web_dir = Path(__file__).parent.resolve()
    static_dir = web_dir / "static"
    templates_dir = web_dir / "templates"

    app.mount("/static", StaticFiles(directory=static_dir), name="static")
    app.state.templates = Jinja2Templates(directory=templates_dir)

    def get_sidebar_stats():
        from photo_culler.web.services.library_service import LibraryService
        service = LibraryService(db_engine)
        try:
            summary = service.get_summary()

            # Map decisions to stats panel keys
            decisions = summary.get("decisions", {})
            kept = decisions.get("BEST", 0) + decisions.get("KEEP", 0)
            alt = decisions.get("ALTERNATE", 0)
            rejected = decisions.get("REJECT_TECHNICAL", 0) + decisions.get("REJECT_REDUNDANT", 0)
            unrated = decisions.get("UNPROCESSED", 0)

            return {
                "total_photos": summary.get("total_photos", 0),
                "total_files": summary.get("total_files", 0),
                "kept": kept,
                "alt": alt,
                "rejected": rejected,
                "unrated": unrated,
                "original_summary": summary
            }
        except Exception:
            return {
                "total_photos": 0,
                "total_files": 0,
                "kept": 0,
                "alt": 0,
                "rejected": 0,
                "unrated": 0,
                "original_summary": {}
            }

    app.state.templates.env.globals["get_sidebar_stats"] = get_sidebar_stats

    # Register Routes
    app.include_router(dashboard.router)
    app.include_router(library.router)
    app.include_router(photos.router)
    app.include_router(analysis.router)
    app.include_router(sessions.router)
    app.include_router(api.router)

    return app
