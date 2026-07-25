"""FastAPI Web Application Factory."""

from pathlib import Path
from typing import Optional, Union

from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates

from photo_culler.catalog.database import Database
from photo_culler.web.routes import api, dashboard, library, photos


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

    # Register Routes
    app.include_router(dashboard.router)
    app.include_router(library.router)
    app.include_router(photos.router)
    app.include_router(api.router)

    return app
