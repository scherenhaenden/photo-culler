"""Dashboard Web Route."""

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse

from photo_culler.web.services.library_service import LibraryService

router = APIRouter()


@router.get("/", response_class=HTMLResponse)
def get_dashboard(request: Request):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    lib_service = LibraryService(db_engine)
    summary = lib_service.get_summary()
    analysis = request.app.state.analysis_jobs.snapshot()
    import_jobs = request.app.state.gallery_imports.list_jobs(limit=5)

    return templates.TemplateResponse(
        request=request,
        name="dashboard.html",
        context={
            "active_tab": "dashboard",
            "summary": summary,
            "analysis": analysis,
            "import_jobs": import_jobs,
        },
    )
