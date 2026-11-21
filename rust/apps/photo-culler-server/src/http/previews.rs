//! Bounded, read-only derivatives for photo review.

use std::{future::Future, pin::Pin, sync::Arc};

use axum::{
    Json, Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::{Deserialize, Serialize};

use crate::media::previews::PreviewPreset;

pub type PreviewApiFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PreviewAsset {
    pub bytes: Arc<Vec<u8>>,
    pub etag: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewApiError {
    InvalidRequest,
    NotFound,
    UnsupportedSource,
    TooLarge,
    Busy,
    Unavailable,
}

pub trait PreviewApi: Send + Sync {
    fn preview<'a>(
        &'a self,
        photo_id: &'a str,
        preset: PreviewPreset,
    ) -> PreviewApiFuture<'a, Result<PreviewAsset, PreviewApiError>>;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewQuery {
    size: Option<String>,
}

pub fn router<A: PreviewApi + 'static>() -> Router<Arc<A>> {
    Router::new().route("/api/v1/photos/:photo_id/preview", get(preview::<A>))
}

async fn preview<A: PreviewApi + 'static>(
    State(api): State<Arc<A>>,
    Path(photo_id): Path<String>,
    Query(query): Query<PreviewQuery>,
) -> Response {
    if !valid_id(&photo_id) {
        return error_response(PreviewApiError::InvalidRequest);
    }
    let preset = match query.size.as_deref().unwrap_or("medium") {
        "small" => PreviewPreset::Small,
        "medium" => PreviewPreset::Medium,
        "large" => PreviewPreset::Large,
        "full" => PreviewPreset::Full,
        _ => return error_response(PreviewApiError::InvalidRequest),
    };
    match api.preview(&photo_id, preset).await {
        Ok(asset) => {
            let mut response = Response::new(Body::from(asset.bytes.as_ref().clone()));
            *response.status_mut() = StatusCode::OK;
            response
                .headers_mut()
                .insert(header::CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static("private, max-age=3600"),
            );
            if let Ok(value) = HeaderValue::from_str(&format!("\"{}\"", asset.etag)) {
                response.headers_mut().insert(header::ETAG, value);
            }
            response
        }
        Err(error) => error_response(error),
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        && !value.contains("..")
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
}

fn error_response(error: PreviewApiError) -> Response {
    let (status, code, message) = match error {
        PreviewApiError::InvalidRequest => (
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The preview request is invalid.",
        ),
        PreviewApiError::NotFound => (
            StatusCode::NOT_FOUND,
            "not_found",
            "The photo or its source file was not found.",
        ),
        PreviewApiError::UnsupportedSource => (
            StatusCode::UNPROCESSABLE_ENTITY,
            "unsupported_source",
            "The source format cannot produce a raster preview.",
        ),
        PreviewApiError::TooLarge => (
            StatusCode::PAYLOAD_TOO_LARGE,
            "preview_too_large",
            "The source exceeds the preview processing limits.",
        ),
        PreviewApiError::Busy => (
            StatusCode::TOO_MANY_REQUESTS,
            "preview_busy",
            "The preview service is processing its request limit.",
        ),
        PreviewApiError::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            "unavailable",
            "The preview service is temporarily unavailable.",
        ),
    };
    (status, Json(ErrorBody { code, message })).into_response()
}
