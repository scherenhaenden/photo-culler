//! HTTP endpoints for configured photo import sources.
//!
//! A browser request never supplies a filesystem path. It can select only an
//! opaque `source_id` already registered by the native application, and the
//! [`ImportApi`] implementation must resolve that ID inside configured roots.
//! This keeps the import API useful to the Python client's workflow without
//! exposing arbitrary server paths to remote callers.

use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub type ImportApiFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Serialize)]
pub struct SourceView {
    pub contract_version: u8,
    pub id: String,
    pub name: String,
    pub status: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct QueuedJob {
    pub job_id: String,
    pub state: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct JobView {
    pub contract_version: u8,
    pub id: String,
    pub gallery_id: String,
    pub scan_revision_id: Option<String>,
    pub source_id: String,
    pub state: String,
    pub discovered: u64,
    pub imported: u64,
    pub issues: u64,
    pub cancel_requested: bool,
    pub pause_requested: bool,
    pub updated_at: String,
    // Retained for the Rust client’s existing progress display.
    pub revision: u64,
    pub processed: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanRevisionView {
    pub contract_version: u8,
    pub id: String,
    pub gallery_id: String,
    pub source_id: String,
    pub state: String,
    pub discovered: u64,
    pub new_files: u64,
    pub modified_files: u64,
    pub moved_files: u64,
    pub missing_files: u64,
    pub started_at: String,
    pub completed_at: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ImportEstimateView {
    pub contract_version: u8,
    pub source_id: String,
    pub total_files: u64,
    pub total_bytes: u64,
    pub logical_photos: u64,
    pub extensions: std::collections::BTreeMap<String, u64>,
    pub roles: std::collections::BTreeMap<String, u64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RescanView {
    pub contract_version: u8,
    pub gallery_id: String,
    pub job_ids: Vec<String>,
    /// Sources known to be offline when the rescan was attempted.
    pub offline_source_ids: Vec<String>,
    /// Online sources that could not be processed after a service-unavailable error.
    pub deferred_source_ids: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControlAction {
    Cancel,
    Pause,
    Resume,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ImportError {
    InvalidRequest,
    NotFound,
    InvalidTransition,
    SourceUnavailable,
    Forbidden,
    Unavailable,
}

/// Persistent service boundary. Implementations must keep source path resolution
/// server-side, validate the source against configured roots, and persist job
/// transitions before reporting success.
pub trait ImportApi: Send + Sync {
    fn sources<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> ImportApiFuture<'a, Result<Vec<SourceView>, ImportError>>;

    fn start_import<'a>(
        &'a self,
        gallery_id: &'a str,
        source_id: &'a str,
        recursive: bool,
        exclude_patterns: &'a [String],
    ) -> ImportApiFuture<'a, Result<QueuedJob, ImportError>>;

    fn estimate<'a>(
        &'a self,
        source_id: &'a str,
        recursive: bool,
        exclude_patterns: &'a [String],
    ) -> ImportApiFuture<'a, Result<ImportEstimateView, ImportError>>;

    fn rescan<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> ImportApiFuture<'a, Result<RescanView, ImportError>>;

    fn job<'a>(&'a self, job_id: &'a str) -> ImportApiFuture<'a, Result<JobView, ImportError>>;

    fn jobs<'a>(
        &'a self,
        limit: usize,
        active_only: bool,
    ) -> ImportApiFuture<'a, Result<Vec<JobView>, ImportError>>;

    fn revisions<'a>(
        &'a self,
        gallery_id: Option<&'a str>,
        limit: usize,
    ) -> ImportApiFuture<'a, Result<Vec<ScanRevisionView>, ImportError>>;

    fn control<'a>(
        &'a self,
        job_id: &'a str,
        action: ControlAction,
    ) -> ImportApiFuture<'a, Result<(), ImportError>>;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartImportRequest {
    source_id: String,
    recursive: Option<bool>,
    #[serde(default)]
    exclude_patterns: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EstimateRequest {
    source_id: String,
    recursive: Option<bool>,
    #[serde(default)]
    exclude_patterns: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LimitQuery {
    limit: Option<usize>,
    active_only: Option<bool>,
}

#[derive(Deserialize)]
struct RevisionsQuery {
    gallery_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Serialize)]
struct Items<T> {
    contract_version: u8,
    items: Vec<T>,
}

#[derive(Serialize)]
struct JobQueuedResponse {
    contract_version: u8,
    job_id: String,
    state: String,
}

#[derive(Serialize)]
struct CancelResponse {
    contract_version: u8,
    job_id: String,
    cancel_requested: bool,
}

#[derive(Serialize)]
struct PauseResponse {
    contract_version: u8,
    job_id: String,
    pause_requested: bool,
}

#[derive(Serialize)]
struct ResumeResponse {
    contract_version: u8,
    job_id: String,
    state: &'static str,
}

#[derive(Serialize)]
struct ApiErrorBody {
    contract_version: u8,
    code: &'static str,
    message: &'static str,
    request_id: String,
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Build import routes backed by a persistent import service.
pub fn router<S: ImportApi + 'static>() -> Router<Arc<S>> {
    Router::new()
        .route("/api/v2/galleries/:gallery_id/sources", get(sources::<S>))
        .route(
            "/api/v2/galleries/:gallery_id/imports",
            post(start_import::<S>),
        )
        .route("/api/v2/galleries/:gallery_id/rescan", post(rescan::<S>))
        .route("/api/v2/import-estimates", post(estimate::<S>))
        .route("/api/v2/import-jobs", get(jobs::<S>))
        .route("/api/v2/import-jobs/:job_id", get(job::<S>))
        .route("/api/v2/import-jobs/:job_id/cancel", post(cancel::<S>))
        .route("/api/v2/import-jobs/:job_id/pause", post(pause::<S>))
        .route("/api/v2/import-jobs/:job_id/resume", post(resume::<S>))
        .route("/api/v2/scan-revisions", get(revisions::<S>))
}

async fn sources<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(gallery_id): Path<String>,
) -> Response {
    if !valid_opaque_id(&gallery_id) {
        return failure(ImportError::InvalidRequest);
    }
    match service.sources(&gallery_id).await {
        Ok(mut items) => {
            for item in &mut items {
                item.contract_version = 2;
            }
            (
                StatusCode::OK,
                Json(Items {
                    contract_version: 2,
                    items,
                }),
            )
                .into_response()
        }
        Err(error) => failure(error),
    }
}

async fn start_import<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(gallery_id): Path<String>,
    Json(request): Json<StartImportRequest>,
) -> Response {
    if !valid_opaque_id(&gallery_id) || !valid_opaque_id(&request.source_id) {
        return failure(ImportError::InvalidRequest);
    }
    let Some(exclude_patterns) = normalize_patterns(request.exclude_patterns) else {
        return failure(ImportError::InvalidRequest);
    };
    match service
        .start_import(
            &gallery_id,
            &request.source_id,
            request.recursive.unwrap_or(true),
            &exclude_patterns,
        )
        .await
    {
        Ok(job) => (
            StatusCode::ACCEPTED,
            Json(JobQueuedResponse {
                contract_version: 2,
                job_id: job.job_id,
                state: job.state,
            }),
        )
            .into_response(),
        Err(error) => failure(error),
    }
}

async fn estimate<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Json(request): Json<EstimateRequest>,
) -> Response {
    if !valid_opaque_id(&request.source_id) {
        return failure(ImportError::InvalidRequest);
    }
    let Some(exclude_patterns) = normalize_patterns(request.exclude_patterns) else {
        return failure(ImportError::InvalidRequest);
    };
    match service
        .estimate(
            &request.source_id,
            request.recursive.unwrap_or(true),
            &exclude_patterns,
        )
        .await
    {
        Ok(mut estimate) => {
            estimate.contract_version = 2;
            (StatusCode::OK, Json(estimate)).into_response()
        }
        Err(error) => failure(error),
    }
}

async fn rescan<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(gallery_id): Path<String>,
) -> Response {
    if !valid_opaque_id(&gallery_id) {
        return failure(ImportError::InvalidRequest);
    }
    match service.rescan(&gallery_id).await {
        Ok(mut view) => {
            view.contract_version = 2;
            (StatusCode::ACCEPTED, Json(view)).into_response()
        }
        Err(error) => failure(error),
    }
}

async fn job<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(job_id): Path<String>,
) -> Response {
    if !valid_opaque_id(&job_id) {
        return failure(ImportError::InvalidRequest);
    }
    match service.job(&job_id).await {
        Ok(mut job) => {
            job.contract_version = 2;
            (StatusCode::OK, Json(job)).into_response()
        }
        Err(error) => failure(error),
    }
}

async fn jobs<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Query(query): Query<LimitQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(20);
    if !(1..=100).contains(&limit) {
        return failure(ImportError::InvalidRequest);
    }
    match service
        .jobs(limit, query.active_only.unwrap_or(false))
        .await
    {
        Ok(mut items) => {
            for item in &mut items {
                item.contract_version = 2;
            }
            (
                StatusCode::OK,
                Json(Items {
                    contract_version: 2,
                    items,
                }),
            )
                .into_response()
        }
        Err(error) => failure(error),
    }
}

async fn revisions<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Query(query): Query<RevisionsQuery>,
) -> Response {
    let limit = query.limit.unwrap_or(20);
    if !(1..=100).contains(&limit)
        || query
            .gallery_id
            .as_ref()
            .is_some_and(|gallery_id| !valid_opaque_id(gallery_id))
    {
        return failure(ImportError::InvalidRequest);
    }
    match service.revisions(query.gallery_id.as_deref(), limit).await {
        Ok(mut items) => {
            for item in &mut items {
                item.contract_version = 2;
            }
            (
                StatusCode::OK,
                Json(Items {
                    contract_version: 2,
                    items,
                }),
            )
                .into_response()
        }
        Err(error) => failure(error),
    }
}

async fn cancel<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(job_id): Path<String>,
) -> Response {
    control(service, job_id, ControlAction::Cancel).await
}

async fn pause<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(job_id): Path<String>,
) -> Response {
    control(service, job_id, ControlAction::Pause).await
}

async fn resume<S: ImportApi + 'static>(
    State(service): State<Arc<S>>,
    Path(job_id): Path<String>,
) -> Response {
    control(service, job_id, ControlAction::Resume).await
}

async fn control<S: ImportApi + 'static>(
    service: Arc<S>,
    job_id: String,
    action: ControlAction,
) -> Response {
    if !valid_opaque_id(&job_id) {
        return failure(ImportError::InvalidRequest);
    }
    match service.control(&job_id, action).await {
        Ok(()) => match action {
            ControlAction::Cancel => (
                StatusCode::ACCEPTED,
                Json(CancelResponse {
                    contract_version: 2,
                    job_id,
                    cancel_requested: true,
                }),
            )
                .into_response(),
            ControlAction::Pause => (
                StatusCode::ACCEPTED,
                Json(PauseResponse {
                    contract_version: 2,
                    job_id,
                    pause_requested: true,
                }),
            )
                .into_response(),
            ControlAction::Resume => (
                StatusCode::ACCEPTED,
                Json(ResumeResponse {
                    contract_version: 2,
                    job_id,
                    state: "queued",
                }),
            )
                .into_response(),
        },
        Err(error) => failure(error),
    }
}

fn valid_opaque_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !value.contains("..")
}

fn normalize_patterns(patterns: Vec<String>) -> Option<Vec<String>> {
    let patterns = patterns
        .into_iter()
        .map(|pattern| pattern.trim().replace('\\', "/"))
        .filter(|pattern| !pattern.is_empty())
        .collect::<Vec<_>>();
    (patterns.len() <= 50
        && patterns
            .iter()
            .all(|pattern| pattern.chars().count() <= 256 && !pattern.contains('\0')))
    .then_some(patterns)
}

fn failure(error: ImportError) -> Response {
    let (status, code, message) = match error {
        ImportError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The import request is invalid.",
        ),
        ImportError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "The requested import resource was not found.",
        ),
        ImportError::InvalidTransition => (
            StatusCode::CONFLICT,
            "invalid_transition",
            "The import job cannot make that state transition.",
        ),
        ImportError::SourceUnavailable => (
            StatusCode::CONFLICT,
            "source_unavailable",
            "The configured import source is unavailable.",
        ),
        ImportError::Forbidden => (
            StatusCode::FORBIDDEN,
            "source_forbidden",
            "The configured import source is outside authorized roots.",
        ),
        ImportError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "import_unavailable",
            "The import service is temporarily unavailable.",
        ),
    };
    (
        status,
        Json(ApiErrorBody {
            contract_version: 2,
            code,
            message,
            request_id: format!(
                "req-import-{}",
                NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
            ),
        }),
    )
        .into_response()
}
