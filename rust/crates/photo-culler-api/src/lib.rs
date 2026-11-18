//! Versioned, transport-neutral REST DTOs shared by the server and clients.
//!
//! Keep this crate free of native-only dependencies so it can also be compiled
//! for `wasm32-unknown-unknown`.

use serde::{Deserialize, Serialize};

pub const API_PREFIX: &str = "/api/v1";
pub const HEALTH_PATH: &str = "/api/v1/health";
pub const DEMO_GALLERY_PATH: &str = "/api/v1/galleries/demo/photos";
pub const DEFAULT_PAGE_SIZE: u32 = 100;
pub const MAX_PAGE_SIZE: u32 = 500;
pub const MAX_THUMBNAIL_EDGE: u32 = 2048;
/// Maximum encoded thumbnail response accepted by the REST demo transport.
pub const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;

/// Response for `GET /api/v1/health`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HealthResponse {
    pub status: HealthStatus,
    pub resources: Option<serde_json::Value>,
    pub request_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ready,
    Degraded,
}

/// Response for `GET /api/v1/galleries/{gallery_id}/photos`.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct GalleryPhotosResponse {
    pub gallery_id: String,
    pub items: Vec<GalleryPhoto>,
    pub next_cursor: Option<String>,
    pub total: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct GalleryPhoto {
    pub photo_id: String,
    pub filename: String,
    pub representation: PhotoRepresentation,
    pub decision: DecisionState,
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PhotoRepresentation {
    Jpeg,
    Raw,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Undecided,
    Keep,
    Reject,
}

/// Stable error envelope. `message` must never contain a local filesystem path.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub request_id: String,
}

/// Build the one canonical relative thumbnail route shared by server and client.
///
/// The identifier is encoded as a single path segment so identifiers containing
/// reserved URL characters cannot change the route structure.
pub fn thumbnail_path(photo_id: &str, max_edge: u32) -> Result<String, ThumbnailPathError> {
    if photo_id.is_empty() {
        return Err(ThumbnailPathError::EmptyPhotoId);
    }
    if max_edge == 0 || max_edge > MAX_THUMBNAIL_EDGE {
        return Err(ThumbnailPathError::InvalidMaxEdge {
            requested: max_edge,
            maximum: MAX_THUMBNAIL_EDGE,
        });
    }

    let mut encoded = String::with_capacity(photo_id.len());
    for byte in photo_id.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write as _;
            write!(&mut encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    Ok(format!(
        "{API_PREFIX}/photos/{encoded}/thumbnail/{max_edge}"
    ))
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ThumbnailPathError {
    EmptyPhotoId,
    InvalidMaxEdge { requested: u32, maximum: u32 },
}

impl std::fmt::Display for ThumbnailPathError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyPhotoId => formatter.write_str("photo id must not be empty"),
            Self::InvalidMaxEdge { requested, maximum } => {
                write!(
                    formatter,
                    "thumbnail edge {requested} must be between 1 and {maximum}"
                )
            }
        }
    }
}

impl std::error::Error for ThumbnailPathError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_round_trips_the_contract_json() {
        let json = r#"{"status":"ready","resources":null,"request_id":"req-example"}"#;
        let decoded: HealthResponse = serde_json::from_str(json).unwrap();
        assert_eq!(decoded.status, HealthStatus::Ready);
        assert_eq!(decoded.resources, None);
        assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
    }

    #[test]
    fn gallery_round_trips_empty_and_demo_responses() {
        for json in [
            r#"{"gallery_id":"empty","items":[],"next_cursor":null,"total":0}"#,
            r#"{"gallery_id":"demo","items":[{"photo_id":"demo-001","filename":"demo-001.jpg","representation":"jpeg","decision":"undecided","thumbnail_url":"/api/v1/photos/demo-001/thumbnail/800"}],"next_cursor":null,"total":1}"#,
        ] {
            let decoded: GalleryPhotosResponse = serde_json::from_str(json).unwrap();
            assert_eq!(
                serde_json::from_str::<GalleryPhotosResponse>(
                    &serde_json::to_string(&decoded).unwrap()
                )
                .unwrap(),
                decoded
            );
        }
    }

    #[test]
    fn error_round_trips_without_exposing_path_data() {
        let error = ApiError {
            code: "not_found".into(),
            message: "Photo is unavailable".into(),
            request_id: "req-example".into(),
        };
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(serde_json::from_str::<ApiError>(&json).unwrap(), error);
        assert!(!json.contains("/Users/"));
    }

    #[test]
    fn thumbnail_route_is_canonical_and_bounded() {
        assert_eq!(
            thumbnail_path("demo-001", 800).unwrap(),
            "/api/v1/photos/demo-001/thumbnail/800"
        );
        assert_eq!(
            thumbnail_path("raw/a", 800).unwrap(),
            "/api/v1/photos/raw%2Fa/thumbnail/800"
        );
        assert!(thumbnail_path("demo-001", MAX_THUMBNAIL_EDGE + 1).is_err());
        assert!(thumbnail_path("", 800).is_err());
    }
}
