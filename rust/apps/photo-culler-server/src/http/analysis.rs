//! HTTP control plane for durable gallery analysis jobs.
//!
//! Responses intentionally omit local source paths and per-analyzer error
//! strings. Jobs are advanced by a server-owned worker after `start` or
//! `resume`; clients only receive state and aggregate progress.

use std::{future::Future, pin::Pin, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub type AnalysisApiFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SubmitAnalysisRequest {
    pub job_id: String,
    pub profile_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct AnalysisJobView {
    pub contract_version: u8,
    pub catalog_id: String,
    pub job_id: String,
    pub gallery_id: String,
    pub profile_id: String,
    pub state: String,
    pub completed: usize,
    pub total: usize,
    pub failed: usize,
    pub percent: u8,
    pub created_at: u64,
    pub updated_at: u64,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ActiveAnalysisJobView {
    pub contract_version: u8,
    pub active_job: Option<AnalysisJobView>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisApiError {
    InvalidRequest,
    InvalidPlan,
    NotFound,
    QueueFull,
    InvalidTransition,
    Unavailable,
}

pub trait AnalysisApi: Send + Sync {
    fn submit<'a>(
        &'a self,
        gallery_id: &'a str,
        job_id: &'a str,
        profile_id: &'a str,
    ) -> AnalysisApiFuture<'a, Result<AnalysisJobView, AnalysisApiError>>;

    fn get<'a>(
        &'a self,
        catalog_id: &'a str,
        job_id: &'a str,
    ) -> AnalysisApiFuture<'a, Result<AnalysisJobView, AnalysisApiError>>;

    fn active_for_gallery<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> AnalysisApiFuture<'a, Result<Option<AnalysisJobView>, AnalysisApiError>>;

    fn control<'a>(
        &'a self,
        catalog_id: &'a str,
        job_id: &'a str,
        action: AnalysisAction,
    ) -> AnalysisApiFuture<'a, Result<AnalysisJobView, AnalysisApiError>>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalysisAction {
    Start,
    Pause,
    Resume,
    Cancel,
}

pub fn router<A: AnalysisApi + 'static>() -> Router<Arc<A>> {
    Router::new()
        .route(
            "/api/v1/galleries/:gallery_id/analysis-jobs",
            post(submit::<A>).get(active_for_gallery::<A>),
        )
        .route(
            "/api/v1/analysis-jobs/:catalog_id/:job_id",
            get(get_job::<A>),
        )
        .route(
            "/api/v1/analysis-jobs/:catalog_id/:job_id/start",
            post(start::<A>),
        )
        .route(
            "/api/v1/analysis-jobs/:catalog_id/:job_id/pause",
            post(pause::<A>),
        )
        .route(
            "/api/v1/analysis-jobs/:catalog_id/:job_id/resume",
            post(resume::<A>),
        )
        .route(
            "/api/v1/analysis-jobs/:catalog_id/:job_id/cancel",
            post(cancel::<A>),
        )
}

async fn active_for_gallery<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path(gallery_id): Path<String>,
) -> Response {
    if !valid_id(&gallery_id) {
        return error_response(AnalysisApiError::InvalidRequest);
    }
    match api.active_for_gallery(&gallery_id).await {
        Ok(job) => (
            StatusCode::OK,
            Json(ActiveAnalysisJobView {
                contract_version: 1,
                active_job: job,
            }),
        )
            .into_response(),
        Err(error) => error_response(error),
    }
}

async fn submit<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path(gallery_id): Path<String>,
    Json(request): Json<SubmitAnalysisRequest>,
) -> Response {
    if !valid_id(&gallery_id) || !valid_id(&request.job_id) || !valid_id(&request.profile_id) {
        return error_response(AnalysisApiError::InvalidRequest);
    }
    match api
        .submit(&gallery_id, &request.job_id, &request.profile_id)
        .await
    {
        Ok(view) => (StatusCode::CREATED, Json(view)).into_response(),
        Err(error) => error_response(error),
    }
}

async fn get_job<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path((catalog_id, job_id)): Path<(String, String)>,
) -> Response {
    if !valid_id(&catalog_id) || !valid_id(&job_id) {
        return error_response(AnalysisApiError::InvalidRequest);
    }
    match api.get(&catalog_id, &job_id).await {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(error) => error_response(error),
    }
}

async fn start<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path((catalog_id, job_id)): Path<(String, String)>,
) -> Response {
    control(api, catalog_id, job_id, AnalysisAction::Start).await
}

async fn pause<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path((catalog_id, job_id)): Path<(String, String)>,
) -> Response {
    control(api, catalog_id, job_id, AnalysisAction::Pause).await
}

async fn resume<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path((catalog_id, job_id)): Path<(String, String)>,
) -> Response {
    control(api, catalog_id, job_id, AnalysisAction::Resume).await
}

async fn cancel<A: AnalysisApi + 'static>(
    State(api): State<Arc<A>>,
    Path((catalog_id, job_id)): Path<(String, String)>,
) -> Response {
    control(api, catalog_id, job_id, AnalysisAction::Cancel).await
}

async fn control<A: AnalysisApi + 'static>(
    api: Arc<A>,
    catalog_id: String,
    job_id: String,
    action: AnalysisAction,
) -> Response {
    if !valid_id(&catalog_id) || !valid_id(&job_id) {
        return error_response(AnalysisApiError::InvalidRequest);
    }
    match api.control(&catalog_id, &job_id, action).await {
        Ok(view) => (StatusCode::OK, Json(view)).into_response(),
        Err(error) => error_response(error),
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn error_response(error: AnalysisApiError) -> Response {
    let (status, code, message) = match error {
        AnalysisApiError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The analysis request is invalid.",
        ),
        AnalysisApiError::InvalidPlan => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "invalid_plan",
            "The gallery does not have an analyzable plan.",
        ),
        AnalysisApiError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "The analysis job or gallery was not found.",
        ),
        AnalysisApiError::QueueFull => (
            StatusCode::TOO_MANY_REQUESTS,
            "queue_full",
            "The analysis queue is full.",
        ),
        AnalysisApiError::InvalidTransition => (
            StatusCode::CONFLICT,
            "invalid_transition",
            "The analysis job cannot perform that action in its current state.",
        ),
        AnalysisApiError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "The analysis service is temporarily unavailable.",
        ),
    };
    (
        status,
        Json(serde_json::json!({
            "error": { "code": code, "message": message }
        })),
    )
        .into_response()
}
