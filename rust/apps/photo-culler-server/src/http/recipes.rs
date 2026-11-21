//! HTTP contract for revision-checked, non-destructive edit recipes.
//!
//! This adapter stores recipe parameters only. It must never receive a source
//! path or write a photo original; render/export is a separate contract.

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
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub type RecipeApiFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct RecipeValues {
    pub schema_version: u32,
    pub exposure: f64,
    pub temperature: f64,
    pub tint: f64,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct RecipeView {
    pub contract_version: u8,
    pub request_id: String,
    pub photo_id: String,
    pub revision: u64,
    pub current: RecipeValues,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateRecipeRequest {
    pub expected_revision: u64,
    pub request_id: String,
    pub recipe: RecipeValues,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RecipeHistoryRequest {
    pub expected_revision: u64,
    pub request_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecipeApiError {
    InvalidRequest,
    InvalidRecipe,
    NotFound,
    VersionConflict { actual_revision: u64 },
    NothingToUndo,
    NothingToRedo,
    Unavailable,
}

/// Persistent service boundary. Implementations must perform revision checks
/// and snapshot writes atomically, including history changes.
pub trait RecipeApi: Send + Sync {
    fn load<'a>(
        &'a self,
        photo_id: &'a str,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>>;

    fn update<'a>(
        &'a self,
        photo_id: &'a str,
        expected_revision: u64,
        request_id: &'a str,
        recipe: &'a RecipeValues,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>>;

    fn undo<'a>(
        &'a self,
        photo_id: &'a str,
        expected_revision: u64,
        request_id: &'a str,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>>;

    fn redo<'a>(
        &'a self,
        photo_id: &'a str,
        expected_revision: u64,
        request_id: &'a str,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>>;
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Build recipe routes using one shared service adapter.
pub fn router<A: RecipeApi + 'static>() -> Router<Arc<A>> {
    Router::new()
        .route(
            "/api/v1/photos/:photo_id/recipe",
            get(load::<A>).put(update::<A>),
        )
        .route("/api/v1/photos/:photo_id/recipe/undo", post(undo::<A>))
        .route("/api/v1/photos/:photo_id/recipe/redo", post(redo::<A>))
}

async fn load<A: RecipeApi + 'static>(
    State(api): State<Arc<A>>,
    Path(photo_id): Path<String>,
) -> Response {
    let request_id = next_request_id();
    if !valid_photo_id(&photo_id) {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The recipe request is invalid.",
            request_id,
            None,
        );
    }
    match api.load(&photo_id).await {
        Ok(view) => success(view, request_id),
        Err(error) => map_error(error, request_id),
    }
}

async fn update<A: RecipeApi + 'static>(
    State(api): State<Arc<A>>,
    Path(photo_id): Path<String>,
    Json(request): Json<UpdateRecipeRequest>,
) -> Response {
    let request_id = match normalize_request_id(&request.request_id) {
        Some(value) => value,
        None => {
            return failure(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The recipe request is invalid.",
                next_request_id(),
                None,
            );
        }
    };
    if !valid_photo_id(&photo_id) {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The recipe request is invalid.",
            request_id,
            None,
        );
    }
    if !valid_recipe(&request.recipe) {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_recipe",
            "The recipe values are outside the supported range.",
            request_id,
            None,
        );
    }
    match api
        .update(
            &photo_id,
            request.expected_revision,
            &request_id,
            &request.recipe,
        )
        .await
    {
        Ok(view) => success(view, request_id),
        Err(error) => map_error(error, request_id),
    }
}

async fn undo<A: RecipeApi + 'static>(
    State(api): State<Arc<A>>,
    Path(photo_id): Path<String>,
    Json(request): Json<RecipeHistoryRequest>,
) -> Response {
    history_action(api, photo_id, request, true).await
}

async fn redo<A: RecipeApi + 'static>(
    State(api): State<Arc<A>>,
    Path(photo_id): Path<String>,
    Json(request): Json<RecipeHistoryRequest>,
) -> Response {
    history_action(api, photo_id, request, false).await
}

async fn history_action<A: RecipeApi + 'static>(
    api: Arc<A>,
    photo_id: String,
    request: RecipeHistoryRequest,
    undo: bool,
) -> Response {
    let request_id = match normalize_request_id(&request.request_id) {
        Some(value) => value,
        None => {
            return failure(
                StatusCode::BAD_REQUEST,
                "invalid_request",
                "The recipe request is invalid.",
                next_request_id(),
                None,
            );
        }
    };
    if !valid_photo_id(&photo_id) {
        return failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The recipe request is invalid.",
            request_id,
            None,
        );
    }
    let result = if undo {
        api.undo(&photo_id, request.expected_revision, &request_id)
            .await
    } else {
        api.redo(&photo_id, request.expected_revision, &request_id)
            .await
    };
    match result {
        Ok(view) => success(view, request_id),
        Err(error) => map_error(error, request_id),
    }
}

fn valid_photo_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 256
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_recipe(recipe: &RecipeValues) -> bool {
    recipe.schema_version == 1
        && recipe.exposure.is_finite()
        && (-5.0..=5.0).contains(&recipe.exposure)
        && recipe.temperature.is_finite()
        && (1000.0..=40000.0).contains(&recipe.temperature)
        && recipe.tint.is_finite()
        && (-100.0..=100.0).contains(&recipe.tint)
}

fn normalize_request_id(value: &str) -> Option<String> {
    let normalized = value.trim();
    if normalized.is_empty() || normalized.len() > 128 {
        None
    } else {
        Some(normalized.to_owned())
    }
}

fn success(mut view: RecipeView, request_id: String) -> Response {
    view.contract_version = 1;
    view.request_id = request_id.clone();
    (StatusCode::OK, [("x-request-id", request_id)], Json(view)).into_response()
}

fn map_error(error: RecipeApiError, request_id: String) -> Response {
    match error {
        RecipeApiError::InvalidRequest => failure(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "The recipe request is invalid.",
            request_id,
            None,
        ),
        RecipeApiError::InvalidRecipe => failure(
            StatusCode::BAD_REQUEST,
            "invalid_recipe",
            "The recipe values are outside the supported range.",
            request_id,
            None,
        ),
        RecipeApiError::NotFound => failure(
            StatusCode::NOT_FOUND,
            "not_found",
            "The photo was not found.",
            request_id,
            None,
        ),
        RecipeApiError::VersionConflict { actual_revision } => failure(
            StatusCode::CONFLICT,
            "version_conflict",
            "The recipe revision has changed.",
            request_id,
            Some(actual_revision),
        ),
        RecipeApiError::NothingToUndo | RecipeApiError::NothingToRedo => failure(
            StatusCode::CONFLICT,
            "history_unavailable",
            "No recipe history is available for that action.",
            request_id,
            None,
        ),
        RecipeApiError::Unavailable => failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "catalog_unavailable",
            "The catalog is temporarily unavailable.",
            request_id,
            None,
        ),
    }
}

#[derive(Serialize)]
struct ErrorBody {
    contract_version: u8,
    code: &'static str,
    message: &'static str,
    request_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    actual_revision: Option<u64>,
}

fn failure(
    status: StatusCode,
    code: &'static str,
    message: &'static str,
    request_id: String,
    actual_revision: Option<u64>,
) -> Response {
    (
        status,
        [("x-request-id", request_id.clone())],
        Json(ErrorBody {
            contract_version: 1,
            code,
            message,
            request_id,
            actual_revision,
        }),
    )
        .into_response()
}

fn next_request_id() -> String {
    format!(
        "recipe-{:x}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}
