"""Sessions Web Route."""

from fastapi import APIRouter, Form, HTTPException, Request
from fastapi.responses import HTMLResponse, RedirectResponse

from photo_culler.catalog.repositories.photo_repository import PhotoRepository
from photo_culler.catalog.schema import PhotoDB
from photo_culler.sessions import SessionManagementService

router = APIRouter()


@router.get("/sessions", response_class=HTMLResponse)
def get_sessions_page(request: Request):
    db_engine = request.app.state.db_engine
    templates = request.app.state.templates

    with db_engine.session() as session:
        photos = PhotoRepository(session).list_all()
        session_service = SessionManagementService(session)
        sessions = session_service.list_sessions()
        burst_count = session.query(PhotoDB.burst_id).filter(PhotoDB.burst_id.is_not(None)).distinct().count()
        session.expunge_all()

    return templates.TemplateResponse(
        request=request,
        name="sessions.html",
        context={
            "active_tab": "sessions",
            "photo_count": len(photos),
            "sessions": sessions,
            "burst_count": burst_count,
            "message": request.query_params.get("message"),
        },
    )


@router.post("/sessions/group")
def group_sessions(
    request: Request,
    profile: str = Form(...),
    timeline_gap_minutes: float = Form(15.0),
    burst_gap_seconds: float = Form(1.5),
):
    try:
        with request.app.state.db_engine.session() as session:
            result = SessionManagementService(session).apply_profile(
                profile,  # type: ignore[arg-type]
                timeline_gap_minutes=timeline_gap_minutes,
                burst_gap_seconds=burst_gap_seconds,
            )
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    message = f"Procesadas: {result.sessions} sesiones y {result.bursts} ráfagas"
    return RedirectResponse(url=f"/sessions?message={message}", status_code=303)


@router.post("/sessions/{session_id}/rename")
def rename_session(request: Request, session_id: str, name: str = Form(...)):
    try:
        with request.app.state.db_engine.session() as session:
            SessionManagementService(session).rename(session_id, name)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    except ValueError as exc:
        raise HTTPException(status_code=422, detail=str(exc)) from exc
    return RedirectResponse(url="/sessions?message=Sesión renombrada", status_code=303)


@router.post("/sessions/{session_id}/delete")
def delete_session(request: Request, session_id: str):
    try:
        with request.app.state.db_engine.session() as session:
            SessionManagementService(session).delete(session_id)
    except LookupError as exc:
        raise HTTPException(status_code=404, detail=str(exc)) from exc
    return RedirectResponse(url="/sessions?message=Sesión eliminada", status_code=303)
