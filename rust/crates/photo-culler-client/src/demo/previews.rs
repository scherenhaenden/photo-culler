//! Typed REST access to cached, read-only JPEG previews.

use super::{
    API_PREFIX, ApiErrorBody, ClientError, DEFAULT_MAX_THUMBNAIL_BYTES, Response, Transport,
};
use serde::Deserialize;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PreviewSize {
    Small,
    Medium,
    Large,
    Full,
}

impl PreviewSize {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Small => "small",
            Self::Medium => "medium",
            Self::Large => "large",
            Self::Full => "full",
        }
    }
}

pub struct PreviewClient<T> {
    transport: T,
    max_response_bytes: usize,
}

impl<T: Transport> PreviewClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            max_response_bytes: DEFAULT_MAX_THUMBNAIL_BYTES,
        }
    }

    pub fn with_max_response_bytes(mut self, limit: usize) -> Self {
        self.max_response_bytes = limit.max(1);
        self
    }

    pub async fn get(&self, photo_id: &str, size: PreviewSize) -> Result<Vec<u8>, ClientError> {
        if !valid_preview_id(photo_id) {
            return Err(ClientError::InvalidBody("invalid photo ID".into()));
        }
        let path = format!(
            "{API_PREFIX}/photos/{photo_id}/preview?size={}",
            size.as_str()
        );
        let response = self
            .transport
            .get(&path, self.max_response_bytes)
            .await
            .map_err(|error| ClientError::Transport(error.0))?;
        validate_response(response, self.max_response_bytes)
    }
}

fn valid_preview_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn validate_response(response: Response, limit: usize) -> Result<Vec<u8>, ClientError> {
    if response.body.len() > limit {
        return Err(ClientError::BodyTooLarge {
            limit,
            actual: response.body.len(),
        });
    }
    if response.redirected {
        return Err(ClientError::Redirected);
    }
    if (300..400).contains(&response.status) {
        return Err(ClientError::Redirect {
            status: response.status,
        });
    }
    if !(200..300).contains(&response.status) {
        let body = serde_json::from_slice::<PreviewErrorBody>(&response.body)
            .ok()
            .map(|body| ApiErrorBody {
                code: body.code,
                message: body.message,
                request_id: body.request_id,
            });
        return Err(ClientError::Http {
            status: response.status,
            body,
        });
    }
    if !mime_is(&response.content_type, "image/jpeg") {
        return Err(ClientError::InvalidMime {
            expected: "image/jpeg",
            actual: response.content_type,
        });
    }
    Ok(response.body)
}

fn mime_is(actual: &Option<String>, expected: &str) -> bool {
    actual
        .as_deref()
        .and_then(|value| value.split(';').next())
        .is_some_and(|value| value.trim().eq_ignore_ascii_case(expected))
}

#[derive(Deserialize)]
struct PreviewErrorBody {
    code: String,
    message: String,
    #[serde(default)]
    request_id: Option<String>,
}
