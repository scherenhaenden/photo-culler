"""Analysis Web Route."""

from fastapi import APIRouter, Request
from fastapi.responses import HTMLResponse

router = APIRouter()


@router.get("/analysis", response_class=HTMLResponse)
def get_analysis_page(request: Request):
    templates = request.app.state.templates
    return templates.TemplateResponse(
        request=request,
        name="analysis.html",
        context={"active_tab": "analysis"},
    )
