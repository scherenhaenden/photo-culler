//! Read-only access to the single JPEG variant in the demo bundle.
//! Contract source: issue #83 (D01), #84 (D02); sprint base db9a00723da400c479c09f8381657874d620e845.

use axum::{
    Router,
    body::Body,
    extract::{Path, State},
    http::{HeaderValue, Response, StatusCode, header::CONTENT_TYPE},
    routing::get,
};
use std::sync::Arc;

pub const MAX_EDGE: u32 = 800;
pub const MAX_JPEG_BYTES: usize = 2 * 1024 * 1024;
const DEMO_PHOTO_ID: &str = "demo-001";

#[derive(Clone)]
pub struct ThumbnailAsset {
    bytes: Arc<[u8]>,
}
impl ThumbnailAsset {
    /// Accept only bytes loaded from an explicitly declared bundle fixture.
    pub fn from_declared_bundle(bytes: impl Into<Arc<[u8]>>) -> Result<Self, &'static str> {
        let bytes = bytes.into();
        if bytes.is_empty()
            || bytes.len() > MAX_JPEG_BYTES
            || !bytes.starts_with(&[0xff, 0xd8])
            || !bytes.ends_with(&[0xff, 0xd9])
        {
            return Err("invalid bundled JPEG");
        }
        Ok(Self { bytes })
    }
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

pub fn router(asset: ThumbnailAsset) -> Router {
    Router::new()
        .route("/api/v1/photos/:photo_id/thumbnail/:max_edge", get(serve))
        .with_state(asset)
}

async fn serve(
    Path((photo_id, edge)): Path<(String, String)>,
    State(asset): State<ThumbnailAsset>,
) -> Response<Body> {
    if photo_id != DEMO_PHOTO_ID || edge.parse::<u32>().ok() != Some(MAX_EDGE) {
        return not_found();
    }
    let mut response = Response::new(Body::from(asset.bytes().to_vec()));
    *response.status_mut() = StatusCode::OK;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("image/jpeg"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

fn not_found() -> Response<Body> {
    let mut response = Response::new(Body::from(
        r#"{"code":"not_found","message":"Thumbnail was not found","request_id":"req-example"}"#,
    ));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
        .headers_mut()
        .insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    response
        .headers_mut()
        .insert("cache-control", HeaderValue::from_static("no-store"));
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::to_bytes, http::Request};
    use tower::ServiceExt;
    const JPEG: &[u8] = &[0xff, 0xd8, 0xff, 0xdb, 0x00, 0x43, 0xff, 0xd9];

    #[tokio::test]
    async fn known_declared_variant_returns_exact_bundle_bytes_from_any_cwd() {
        let app = router(ThumbnailAsset::from_declared_bundle(Arc::<[u8]>::from(JPEG)).unwrap());
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/v1/photos/demo-001/thumbnail/800")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()[CONTENT_TYPE], "image/jpeg");
        assert_eq!(
            to_bytes(response.into_body(), MAX_JPEG_BYTES)
                .await
                .unwrap()
                .as_ref(),
            JPEG
        );
    }
    #[tokio::test]
    async fn unknown_id_or_size_returns_non_path_error() {
        let app = router(ThumbnailAsset::from_declared_bundle(Arc::<[u8]>::from(JPEG)).unwrap());
        for uri in [
            "/api/v1/photos/other/thumbnail/800",
            "/api/v1/photos/demo-001/thumbnail/1600",
        ] {
            let response = app
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            let body = to_bytes(response.into_body(), 1024).await.unwrap();
            assert!(!String::from_utf8_lossy(&body).contains('/'));
        }
    }
    #[test]
    fn rejects_unbounded_or_non_jpeg_bundle_bytes() {
        assert!(ThumbnailAsset::from_declared_bundle(vec![0xff, 0xd8, 0xff, 0xd9]).is_ok());
        assert!(ThumbnailAsset::from_declared_bundle(vec![0; 12]).is_err());
        assert!(
            ThumbnailAsset::from_declared_bundle(
                vec![0xff, 0xd8, 0xff, 0xd9].repeat(MAX_JPEG_BYTES)
            )
            .is_err()
        );
    }
}
