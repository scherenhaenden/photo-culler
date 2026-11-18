//! Read-only gallery projection for the bundled demo fixture.
//! Contract source: issue #83 (D01), #84 (D02); sprint base db9a00723da400c479c09f8381657874d620e845.

use std::{collections::BTreeMap, sync::Arc};

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DemoPhoto {
    pub photo_id: String,
    pub filename: String,
    pub decision: String,
    pub representation: String,
    pub thumbnail_url: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct DemoGalleryPage {
    pub gallery_id: String,
    pub items: Vec<DemoPhoto>,
    pub next_cursor: Option<String>,
    pub total: usize,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiError {
    pub code: String,
    pub message: String,
    pub request_id: String,
}

pub trait GalleryFixture: Send + Sync + 'static {
    fn photos(&self, gallery_id: &str) -> Option<Vec<DemoPhoto>>;
}

#[derive(Clone)]
pub struct FixtureMap(pub BTreeMap<String, Vec<DemoPhoto>>);
impl GalleryFixture for FixtureMap {
    fn photos(&self, id: &str) -> Option<Vec<DemoPhoto>> {
        self.0.get(id).cloned()
    }
}

pub fn router<F: GalleryFixture>(fixture: Arc<F>) -> Router {
    Router::new()
        .route("/api/v1/galleries/:id/photos", get(read_gallery::<F>))
        .with_state(fixture)
}

async fn read_gallery<F: GalleryFixture>(
    State(fixture): State<Arc<F>>,
    Path(id): Path<String>,
    Query(query): Query<BTreeMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    if !query.is_empty() {
        return error(
            StatusCode::BAD_REQUEST,
            "bad_request",
            "Query parameters are not supported",
        );
    }
    let Some(items) = fixture.photos(&id) else {
        return error(StatusCode::NOT_FOUND, "not_found", "Gallery was not found");
    };
    let page = DemoGalleryPage {
        gallery_id: id,
        total: items.len(),
        items,
        next_cursor: None,
    };
    (
        StatusCode::OK,
        Json(serde_json::to_value(page).expect("serializable DTO")),
    )
}

fn error(status: StatusCode, code: &str, message: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        status,
        Json(serde_json::json!({"code":code,"message":message,"request_id":"req-example"})),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    fn sample() -> DemoPhoto {
        DemoPhoto {
            photo_id: "demo-001".into(),
            filename: "demo-001.jpg".into(),
            decision: "undecided".into(),
            representation: "jpeg".into(),
            thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
        }
    }
    fn fixture() -> Arc<FixtureMap> {
        Arc::new(FixtureMap(BTreeMap::from([
            ("demo".into(), vec![sample()]),
            ("empty".into(), vec![]),
        ])))
    }
    async fn call(uri: &str) -> (StatusCode, serde_json::Value) {
        let response = router(fixture())
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 4096)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn fixture_row_matches_d01_fields_and_route() {
        let (status, body) = call("/api/v1/galleries/demo/photos").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["gallery_id"], "demo");
        assert_eq!(body["total"], 1);
        assert_eq!(body["next_cursor"], serde_json::Value::Null);
        assert_eq!(body["items"][0]["photo_id"], "demo-001");
        assert_eq!(
            body["items"][0]["thumbnail_url"],
            "/api/v1/photos/demo-001/thumbnail/800"
        );
    }
    #[tokio::test]
    async fn empty_and_unknown_galleries_are_distinct() {
        let (status, body) = call("/api/v1/galleries/empty/photos").await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["items"], serde_json::json!([]));
        assert_eq!(body["total"], 0);
        let (status, body) = call("/api/v1/galleries/other/photos").await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(body["code"], "not_found");
    }
    #[tokio::test]
    async fn unsupported_query_is_bad_request() {
        let (status, body) = call("/api/v1/galleries/demo/photos?limit=10").await;
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(body["code"], "bad_request");
    }
}
