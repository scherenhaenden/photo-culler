"""FastAPI Web Application Factory."""

from pathlib import Path
from typing import Optional, Union

from fastapi import FastAPI
from fastapi.staticfiles import StaticFiles
from fastapi.templating import Jinja2Templates
from starlette.datastructures import MutableHeaders
from starlette.requests import Request
from starlette.types import ASGIApp, Message, Receive, Scope, Send

from photo_culler.analysis.profiles import AnalysisProfileStore
from photo_culler.catalog.database import Database
from photo_culler.editing import EditService
from photo_culler.importing import GalleryImportService
from photo_culler.web.i18n import SUPPORTED_LOCALES, language_selector, localize_html, resolve_locale
from photo_culler.web.routes import analysis, api, dashboard, editing, groups, library, photos, sessions


class InternationalizationMiddleware:
    """Localize complete HTML responses without consuming streaming response iterators."""

    def __init__(self, app: ASGIApp):
        self.app = app

    async def __call__(self, scope: Scope, receive: Receive, send: Send) -> None:
        if scope["type"] != "http":
            await self.app(scope, receive, send)
            return

        request = Request(scope)
        locale = resolve_locale(request)
        set_locale_cookie = request.query_params.get("lang") in SUPPORTED_LOCALES
        start_message: Message | None = None
        buffered_body = bytearray()
        localize_response = False

        async def send_wrapper(message: Message) -> None:
            nonlocal localize_response, start_message
            if message["type"] == "http.response.start":
                headers = MutableHeaders(raw=message["headers"])
                content_type = headers.get("content-type", "")
                localize_response = "text/html" in content_type and "content-length" in headers
                if set_locale_cookie:
                    headers.append(
                        "set-cookie", f"photo_culler_locale={locale}; Max-Age=31536000; Path=/; SameSite=lax"
                    )
                if localize_response:
                    start_message = message
                    return
                await send(message)
                return

            if message["type"] == "http.response.body" and localize_response:
                buffered_body.extend(message.get("body", b""))
                if message.get("more_body", False):
                    return
                localized_body = localize_html(buffered_body.decode("utf-8"), locale).encode("utf-8")
                assert start_message is not None
                MutableHeaders(raw=start_message["headers"])["content-length"] = str(len(localized_body))
                await send(start_message)
                await send({"type": "http.response.body", "body": localized_body, "more_body": False})
                return

            await send(message)

        await self.app(scope, receive, send_wrapper)


def create_app(
    catalog_path: Optional[Union[str, Path]] = None,
    database_url: Optional[str] = None,
    desktop_token: Optional[str] = None,
) -> FastAPI:
    """Create and configure FastAPI application instance."""
    app = FastAPI(title="Photo Culler", version="0.1.0")
    app.add_middleware(InternationalizationMiddleware)

    if desktop_token:
        app.state.desktop_token = desktop_token

        from fastapi import Response

        @app.middleware("http")
        async def validate_desktop_token(request, call_next):
            # Strict Host validation
            host_header = request.headers.get("host", "")
            is_local = any(host_header.startswith(prefix) for prefix in ("127.0.0.1", "localhost"))
            if not is_local:
                return Response(content="Forbidden: Access only allowed via localhost/127.0.0.1", status_code=403)

            token_param = request.query_params.get("token")
            token_cookie = request.cookies.get("desktop_token")
            expected_token = request.app.state.desktop_token

            if token_param == expected_token or token_cookie == expected_token:
                response = await call_next(request)
                if token_param == expected_token and token_cookie != expected_token:
                    response.set_cookie(
                        key="desktop_token",
                        value=expected_token,
                        httponly=True,
                        samesite="strict",
                        secure=False,
                    )
                # Inject security headers
                response.headers["Content-Security-Policy"] = (
                    "default-src 'self' 'unsafe-inline' 'unsafe-eval' data: blob:"
                )
                response.headers["X-Frame-Options"] = "DENY"
                response.headers["Referrer-Policy"] = "no-referrer"
                return response

            return Response(content="Forbidden: Invalid desktop session token", status_code=403)

    # Catalog DB setup
    cat_path = Path(catalog_path) if catalog_path else Path("catalog.db")
    db_engine = Database(cat_path, db_url=database_url)
    db_engine.create_tables()
    app.state.db_engine = db_engine
    app.state.gallery_imports = GalleryImportService(db_engine)
    app.state.analysis_jobs = analysis.AnalysisJobManager()
    app.state.analysis_profiles = AnalysisProfileStore(str(db_engine.db_path) + ".analysis-profiles.json")
    app.state.edit_service = EditService(db_engine)

    def shutdown_services() -> None:
        app.state.analysis_jobs.shutdown()
        app.state.gallery_imports.shutdown()
        db_engine.close()

    app.router.add_event_handler("shutdown", shutdown_services)

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
                "original_summary": summary,
            }
        except Exception:
            return {
                "total_photos": 0,
                "total_files": 0,
                "kept": 0,
                "alt": 0,
                "rejected": 0,
                "unrated": 0,
                "original_summary": {},
            }

    app.state.templates.env.globals["get_sidebar_stats"] = get_sidebar_stats
    app.state.templates.env.globals["language_selector"] = language_selector
    app.state.templates.env.globals["resolve_locale"] = resolve_locale

    # Register Routes
    app.include_router(dashboard.router)
    app.include_router(library.router)
    app.include_router(photos.router)
    app.include_router(analysis.router)
    app.include_router(groups.router)
    app.include_router(editing.router)
    app.include_router(sessions.router)
    app.include_router(api.router)

    return app
