//! Bounded typed client for the persistent recipe API.
//!
//! Kept beside the shared demo REST adapter so browser and native transports
//! can use the same revision checked recipe contract.

use photo_culler_client::demo::{Response, Transport, TransportError};
use serde::{Deserialize, Serialize};

const JSON_LIMIT: usize = 64 * 1024;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct RecipeValues {
    pub schema_version: u32,
    pub exposure: f64,
    pub temperature: f64,
    pub tint: f64,
}

impl Default for RecipeValues {
    fn default() -> Self {
        Self {
            schema_version: 1,
            exposure: 0.0,
            temperature: 6500.0,
            tint: 0.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct RecipeDocument {
    pub contract_version: u8,
    pub request_id: String,
    pub photo_id: String,
    pub revision: u64,
    pub current: RecipeValues,
    pub can_undo: bool,
    pub can_redo: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecipeClientError {
    InvalidPhotoId,
    Http {
        status: u16,
        code: Option<String>,
        message: Option<String>,
    },
    Redirect {
        status: u16,
    },
    Redirected,
    InvalidMime,
    BodyTooLarge,
    InvalidBody(String),
    Transport(String),
}

pub struct RecipeRestClient<T> {
    transport: T,
}

impl<T: Transport> RecipeRestClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub async fn load(&self, photo_id: &str) -> Result<RecipeDocument, RecipeClientError> {
        let path = recipe_path(photo_id)?;
        let response = self
            .transport
            .get(&path, JSON_LIMIT)
            .await
            .map_err(map_transport)?;
        decode(response)
    }

    pub async fn save(
        &self,
        photo_id: &str,
        expected_revision: u64,
        recipe: &RecipeValues,
        request_id: &str,
    ) -> Result<RecipeDocument, RecipeClientError> {
        let path = recipe_path(photo_id)?;
        let body = serde_json::to_vec(&UpdateRequest {
            expected_revision,
            request_id,
            recipe,
        })
        .map_err(|error| RecipeClientError::InvalidBody(error.to_string()))?;
        let response = self
            .transport
            .put_json(&path, body, JSON_LIMIT)
            .await
            .map_err(map_transport)?;
        decode(response)
    }

    pub async fn undo(
        &self,
        photo_id: &str,
        expected_revision: u64,
        request_id: &str,
    ) -> Result<RecipeDocument, RecipeClientError> {
        self.history_action(photo_id, expected_revision, request_id, "undo")
            .await
    }

    pub async fn redo(
        &self,
        photo_id: &str,
        expected_revision: u64,
        request_id: &str,
    ) -> Result<RecipeDocument, RecipeClientError> {
        self.history_action(photo_id, expected_revision, request_id, "redo")
            .await
    }

    async fn history_action(
        &self,
        photo_id: &str,
        expected_revision: u64,
        request_id: &str,
        action: &str,
    ) -> Result<RecipeDocument, RecipeClientError> {
        let path = format!("{}/{}", recipe_path(photo_id)?, action);
        let body = serde_json::to_vec(&HistoryRequest {
            expected_revision,
            request_id,
        })
        .map_err(|error| RecipeClientError::InvalidBody(error.to_string()))?;
        let response = self
            .transport
            .post_json(&path, body, JSON_LIMIT)
            .await
            .map_err(map_transport)?;
        decode(response)
    }
}

#[derive(Serialize)]
struct UpdateRequest<'a> {
    expected_revision: u64,
    request_id: &'a str,
    recipe: &'a RecipeValues,
}

#[derive(Serialize)]
struct HistoryRequest<'a> {
    expected_revision: u64,
    request_id: &'a str,
}

fn recipe_path(photo_id: &str) -> Result<String, RecipeClientError> {
    if photo_id.is_empty()
        || photo_id.len() > 256
        || photo_id.contains("..")
        || !photo_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err(RecipeClientError::InvalidPhotoId);
    }
    Ok(format!("/api/v1/photos/{photo_id}/recipe"))
}

fn decode(response: Response) -> Result<RecipeDocument, RecipeClientError> {
    if (300..400).contains(&response.status) {
        return Err(RecipeClientError::Redirect {
            status: response.status,
        });
    }
    if response.redirected {
        return Err(RecipeClientError::Redirected);
    }
    if response.body.len() > JSON_LIMIT {
        return Err(RecipeClientError::BodyTooLarge);
    }
    if !(200..300).contains(&response.status) {
        let error: Option<ErrorBody> = serde_json::from_slice(&response.body).ok();
        return Err(RecipeClientError::Http {
            status: response.status,
            code: error.as_ref().map(|body| body.code.clone()),
            message: error.map(|body| body.message),
        });
    }
    if !response.content_type.as_deref().is_some_and(is_json_mime) {
        return Err(RecipeClientError::InvalidMime);
    }
    let document: RecipeDocument = serde_json::from_slice(&response.body)
        .map_err(|error| RecipeClientError::InvalidBody(error.to_string()))?;
    if document.contract_version != 1
        || document.current.schema_version != 1
        || !document.current.exposure.is_finite()
        || !(-5.0..=5.0).contains(&document.current.exposure)
        || !document.current.temperature.is_finite()
        || !(1000.0..=40000.0).contains(&document.current.temperature)
        || !document.current.tint.is_finite()
        || !(-100.0..=100.0).contains(&document.current.tint)
    {
        return Err(RecipeClientError::InvalidBody(
            "the recipe response violates the supported contract".into(),
        ));
    }
    Ok(document)
}

fn is_json_mime(value: &str) -> bool {
    value
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    message: String,
}

fn map_transport(error: TransportError) -> RecipeClientError {
    RecipeClientError::Transport(error.0)
}
