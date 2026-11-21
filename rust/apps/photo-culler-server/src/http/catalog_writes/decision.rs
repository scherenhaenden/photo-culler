//! Explicit, revision-checked decision write route.

use std::{future::Future, pin::Pin, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::put,
};
use serde::{Deserialize, Serialize};

pub type DecisionFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Keep,
    Alt,
    Reject,
    Undecided,
}

impl Decision {
    pub const fn database_value(self) -> &'static str {
        match self {
            Self::Keep => "KEEP",
            Self::Alt => "ALTERNATE",
            Self::Reject => "REJECT_TECHNICAL",
            Self::Undecided => "UNPROCESSED",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRequest {
    pub decision: Decision,
    pub expected_revision: u64,
    pub request_id: String,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct DecisionReceipt {
    pub contract_version: u8,
    pub photo_id: String,
    pub decision: Decision,
    pub revision: u64,
    pub request_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecisionWriteError {
    InvalidRequest,
    InvalidState,
    VersionConflict,
    NotFound,
    Unavailable,
}

pub trait DecisionWriter: Send + Sync {
    fn write_decision<'a>(
        &'a self,
        photo_id: &'a str,
        request: &'a DecisionRequest,
    ) -> DecisionFuture<'a, Result<DecisionReceipt, DecisionWriteError>>;
}

/// Mount this router beside `catalog_reads::routes::router`. It expects the
/// same shared adapter instance through Axum state.
pub fn router<W: DecisionWriter + 'static>() -> Router<Arc<W>> {
    Router::new().route(
        "/api/v1/photos/:photo_id/decision",
        put(write_decision::<W>),
    )
}

async fn write_decision<W: DecisionWriter + 'static>(
    State(writer): State<Arc<W>>,
    Path(photo_id): Path<String>,
    Json(mut request): Json<DecisionRequest>,
) -> Response {
    let request_id = request.request_id.trim().to_owned();
    if photo_id.trim().is_empty()
        || request_id.is_empty()
        || request_id.len() > 128
        || photo_id.len() > 256
    {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The decision request is invalid.",
            request_id,
        );
    }
    request.request_id = request_id;

    match writer.write_decision(&photo_id, &request).await {
        Ok(receipt) => (StatusCode::OK, Json(receipt)).into_response(),
        Err(error) => {
            let (status, code, message) = match error {
                DecisionWriteError::InvalidRequest => (
                    StatusCode::BAD_REQUEST,
                    "invalid_request",
                    "The decision request is invalid.",
                ),
                DecisionWriteError::InvalidState => (
                    StatusCode::CONFLICT,
                    "invalid_state",
                    "The request ID was already used for a different write.",
                ),
                DecisionWriteError::VersionConflict => (
                    StatusCode::CONFLICT,
                    "version_conflict",
                    "The decision revision has changed.",
                ),
                DecisionWriteError::NotFound => (
                    StatusCode::NOT_FOUND,
                    "not_found",
                    "The photo was not found.",
                ),
                DecisionWriteError::Unavailable => (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "catalog_unavailable",
                    "The catalog is temporarily unavailable.",
                ),
            };
            failure(status, code, message, request.request_id)
        }
    }
}

#[derive(Serialize)]
struct ErrorBody {
    contract_version: u8,
    code: &'static str,
    message: &'static str,
    request_id: String,
}

fn failure(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    request_id: String,
) -> Response {
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
