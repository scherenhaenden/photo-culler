//! Review-before-apply contract for derived-file exports.
//!
//! DTOs contain only opaque destination identifiers and destination-relative
//! output names. Source paths and rendered bytes stay inside the server.

use std::{collections::BTreeSet, future::Future, pin::Pin, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub type ExportApiFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

const MAX_EXPORT_ITEMS: usize = 100;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateExportPlanRequest {
    /// Catalog session used for provenance and ownership checks.
    pub session_id: String,
    /// Opaque server-configured destination. Filesystem paths are never sent.
    pub destination_id: String,
    pub photo_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExportPlanItemView {
    pub photo_id: String,
    /// Relative output name under the selected destination, never an absolute
    /// source or destination filesystem path.
    pub output_name: String,
    pub disposition: ExportDisposition,
    pub review_reasons: Vec<String>,
}

#[derive(Clone, Copy, Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExportDisposition {
    Ready,
    Collision,
    NeedsReview,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExportPlanView {
    pub contract_version: u8,
    pub plan_id: String,
    pub plan_revision: u64,
    pub plan_digest: String,
    pub gallery_id: String,
    pub session_id: String,
    pub destination_id: String,
    pub items: Vec<ExportPlanItemView>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplyExportPlanRequest {
    pub expected_plan_revision: u64,
    /// Exact server-issued digest that the user reviewed in the preceding GET.
    pub confirmed_plan_digest: String,
    pub request_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExportReceipt {
    pub contract_version: u8,
    pub plan_id: String,
    pub request_id: String,
    pub written: Vec<ExportedItemView>,
    pub collisions: Vec<ExportCollisionView>,
    pub skipped_for_review: Vec<ExportReviewView>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExportedItemView {
    pub photo_id: String,
    pub output_name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExportCollisionView {
    pub photo_id: String,
    pub output_name: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct ExportReviewView {
    pub photo_id: String,
    pub output_name: String,
    pub reasons: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportApiError {
    InvalidRequest,
    NotFound,
    PlanConflict,
    ConfirmationRequired,
    Unavailable,
}

/// Server implementation must resolve the opaque destination ID only from
/// trusted configuration, verify it does not overlap a configured source root,
/// and render each source into derived bytes internally. It must never accept
/// source paths or rendered bytes from an HTTP caller.
pub trait ExportApi: Send + Sync {
    fn create_plan<'a>(
        &'a self,
        gallery_id: &'a str,
        request: &'a CreateExportPlanRequest,
    ) -> ExportApiFuture<'a, Result<ExportPlanView, ExportApiError>>;

    fn get_plan<'a>(
        &'a self,
        plan_id: &'a str,
    ) -> ExportApiFuture<'a, Result<ExportPlanView, ExportApiError>>;

    fn apply_plan<'a>(
        &'a self,
        plan_id: &'a str,
        request: &'a ApplyExportPlanRequest,
    ) -> ExportApiFuture<'a, Result<ExportReceipt, ExportApiError>>;
}

pub fn router<A: ExportApi + 'static>() -> Router<Arc<A>> {
    Router::new()
        .route(
            "/api/v1/galleries/:gallery_id/export-plans",
            post(create_plan::<A>),
        )
        .route("/api/v1/export-plans/:plan_id", get(get_plan::<A>))
        .route("/api/v1/export-plans/:plan_id/apply", post(apply_plan::<A>))
}

async fn create_plan<A: ExportApi + 'static>(
    State(api): State<Arc<A>>,
    Path(gallery_id): Path<String>,
    Json(request): Json<CreateExportPlanRequest>,
) -> Response {
    if !valid_id(&gallery_id)
        || !valid_id(&request.session_id)
        || !valid_id(&request.destination_id)
        || request.photo_ids.is_empty()
        || request.photo_ids.len() > MAX_EXPORT_ITEMS
        || request.photo_ids.iter().any(|id| !valid_id(id))
        || request.photo_ids.iter().collect::<BTreeSet<_>>().len() != request.photo_ids.len()
    {
        return error_response(ExportApiError::InvalidRequest, String::new());
    }
    match api.create_plan(&gallery_id, &request).await {
        Ok(plan) => (StatusCode::CREATED, Json(plan)).into_response(),
        Err(error) => error_response(error, String::new()),
    }
}

async fn get_plan<A: ExportApi + 'static>(
    State(api): State<Arc<A>>,
    Path(plan_id): Path<String>,
) -> Response {
    if !valid_id(&plan_id) {
        return error_response(ExportApiError::InvalidRequest, String::new());
    }
    match api.get_plan(&plan_id).await {
        Ok(plan) => (StatusCode::OK, Json(plan)).into_response(),
        Err(error) => error_response(error, String::new()),
    }
}

async fn apply_plan<A: ExportApi + 'static>(
    State(api): State<Arc<A>>,
    Path(plan_id): Path<String>,
    Json(request): Json<ApplyExportPlanRequest>,
) -> Response {
    let request_id = request.request_id.trim().to_owned();
    if !valid_id(&plan_id)
        || !valid_id(&request_id)
        || request_id.len() > 128
        || request.confirmed_plan_digest.len() != 64
        || !request
            .confirmed_plan_digest
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return error_response(ExportApiError::InvalidRequest, request_id);
    }
    let request = ApplyExportPlanRequest {
        request_id: request_id.clone(),
        ..request
    };
    match api.apply_plan(&plan_id, &request).await {
        Ok(receipt) => (StatusCode::OK, Json(receipt)).into_response(),
        Err(error) => error_response(error, request_id),
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Serialize)]
struct ErrorBody {
    contract_version: u8,
    code: &'static str,
    message: &'static str,
    request_id: String,
}

fn error_response(error: ExportApiError, request_id: String) -> Response {
    let (status, code, message) = match error {
        ExportApiError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The export request is invalid.",
        ),
        ExportApiError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "The export plan or catalog item was not found.",
        ),
        ExportApiError::PlanConflict => (
            StatusCode::CONFLICT,
            "plan_conflict",
            "The export plan changed or its destination is no longer available.",
        ),
        ExportApiError::ConfirmationRequired => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "confirmation_required",
            "Review all export items and confirm the current plan before applying it.",
        ),
        ExportApiError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "export_unavailable",
            "The export service is temporarily unavailable.",
        ),
    };
    (
        status,
        Json(ErrorBody {
            contract_version: 1,
            code,
            message,
            request_id,
        }),
    )
        .into_response()
}
