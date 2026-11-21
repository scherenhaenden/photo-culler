//! HTTP routes for the catalog read contract.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::{
    Router,
    body::Body,
    extract::{Path, Query, State},
    http::{HeaderValue, Response, StatusCode, header::CONTENT_TYPE},
    routing::get,
};
use serde::Deserialize;

use super::{Catalog, Cursor, Error, Filter, Reads};

#[derive(Deserialize)]
struct PhotoQuery {
    decision: Option<String>,
    cursor: Option<String>,
    limit: Option<usize>,
}

#[derive(serde::Serialize)]
struct ActiveGallery {
    contract_version: u8,
    gallery_id: Option<String>,
}

#[derive(serde::Serialize)]
struct ApiError {
    code: &'static str,
    message: &'static str,
    request_id: String,
}

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Build routes backed by one shared, thread-safe catalog read adapter.
pub fn router<C>() -> Router<Arc<Reads<C>>>
where
    C: Catalog + Send + Sync + 'static,
{
    Router::new()
        .route("/api/v1/galleries", get(galleries::<C>))
        .route("/api/v1/galleries/active", get(active_gallery::<C>))
        .route("/api/v1/galleries/:gallery_id/photos", get(photos::<C>))
        .route("/api/v1/galleries/:gallery_id/sessions", get(sessions::<C>))
        .route("/api/v1/galleries/:gallery_id/groups", get(groups::<C>))
        .route("/api/v1/galleries/:gallery_id/summary", get(summary::<C>))
        .route("/api/v1/profiles", get(profiles::<C>))
        .route("/api/v1/photos/:photo_id", get(photo::<C>))
        .route(
            "/api/v1/photos/:photo_id/thumbnail/:size",
            get(thumbnail::<C>),
        )
}

async fn galleries<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
) -> Response<Body> {
    json_result(reads.galleries().await)
}

async fn active_gallery<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
) -> Response<Body> {
    json_result(
        reads
            .active_gallery()
            .await
            .map(|gallery_id| ActiveGallery {
                contract_version: 1,
                gallery_id,
            }),
    )
}

async fn photos<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
    Path(gallery_id): Path<String>,
    Query(query): Query<PhotoQuery>,
) -> Response<Body> {
    let filter = Filter {
        gallery_id,
        decision: query.decision,
    };
    let cursor = match query.cursor {
        Some(value) => match serde_json::from_str::<Cursor>(&value) {
            Ok(cursor) => Some(cursor),
            Err(_) => return error_response(Error::InvalidPage),
        },
        None => None,
    };
    json_result(reads.page(filter, cursor, query.limit.unwrap_or(50)).await)
}

async fn photo<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
    Path(photo_id): Path<String>,
) -> Response<Body> {
    json_result(reads.detail(&photo_id).await)
}

async fn sessions<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
    Path(gallery_id): Path<String>,
) -> Response<Body> {
    json_result(reads.sessions(&gallery_id).await)
}

async fn groups<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
    Path(gallery_id): Path<String>,
) -> Response<Body> {
    json_result(reads.groups(&gallery_id).await)
}

async fn profiles<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
) -> Response<Body> {
    json_result(reads.profiles().await)
}

async fn summary<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
    Path(gallery_id): Path<String>,
) -> Response<Body> {
    json_result(reads.summary(&gallery_id).await)
}

async fn thumbnail<C: Catalog + Send + Sync + 'static>(
    State(reads): State<Arc<Reads<C>>>,
    Path((photo_id, size)): Path<(String, String)>,
) -> Response<Body> {
    let Ok(size) = size.parse::<u32>() else {
        return error_response(Error::InvalidPage);
    };
    match reads.thumbnail(&photo_id, size).await {
        Ok(thumbnail) => {
            let mut response = Response::new(Body::from(thumbnail.bytes));
            *response.status_mut() = StatusCode::OK;
            let Ok(content_type) = HeaderValue::from_str(&thumbnail.content_type) else {
                return error_response(Error::Unavailable);
            };
            response.headers_mut().insert(CONTENT_TYPE, content_type);
            response
        }
        Err(error) => error_response(error),
    }
}

fn json_result<T: serde::Serialize>(result: Result<T, Error>) -> Response<Body> {
    match result {
        Ok(value) => {
            let Ok(body) = serde_json::to_vec(&value) else {
                return error_response(Error::Unavailable);
            };
            let mut response = Response::new(Body::from(body));
            *response.status_mut() = StatusCode::OK;
            response.headers_mut().insert(
                CONTENT_TYPE,
                HeaderValue::from_static("application/json; charset=utf-8"),
            );
            response
        }
        Err(error) => error_response(error),
    }
}

fn error_response(error: Error) -> Response<Body> {
    let (status, body) = match error {
        Error::InvalidPage => (
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "invalid_page",
                message: "The page request is invalid.",
                request_id: next_request_id(),
            },
        ),
        Error::CursorFilterMismatch => (
            StatusCode::BAD_REQUEST,
            ApiError {
                code: "cursor_filter_mismatch",
                message: "The cursor belongs to a different filter.",
                request_id: next_request_id(),
            },
        ),
        Error::NotFound => (
            StatusCode::NOT_FOUND,
            ApiError {
                code: "not_found",
                message: "The requested catalog item was not found.",
                request_id: next_request_id(),
            },
        ),
        Error::Unavailable => (
            StatusCode::SERVICE_UNAVAILABLE,
            ApiError {
                code: "catalog_unavailable",
                message: "The catalog is temporarily unavailable.",
                request_id: next_request_id(),
            },
        ),
    };
    let mut response = Response::new(Body::from(
        serde_json::to_vec(&body).expect("error response is serializable"),
    ));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

fn next_request_id() -> String {
    format!(
        "req-catalog-{}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod route_tests {
    use super::*;
    use crate::http::catalog_reads::{CatalogFuture, Gallery, Photo};
    use axum::{body::to_bytes, http::Request};
    use serde_json::{Value, json};
    use tower::ServiceExt;

    struct FakeCatalog;

    impl Catalog for FakeCatalog {
        fn galleries(&self) -> CatalogFuture<'_, Result<Vec<Gallery>, Error>> {
            Box::pin(async {
                Ok(vec![Gallery {
                    id: "g1".into(),
                    name: "Gallery 1".into(),
                }])
            })
        }

        fn active_gallery(&self) -> CatalogFuture<'_, Result<Option<String>, Error>> {
            Box::pin(async { Ok(Some("g1".into())) })
        }

        fn photos(
            &self,
            _: &Filter,
            _: Option<&str>,
            _: usize,
        ) -> CatalogFuture<'_, Result<Vec<Photo>, Error>> {
            Box::pin(async { Ok(vec![photo("p2", "REJECT"), photo("p1", "KEEP")]) })
        }

        fn photo<'a>(&'a self, id: &'a str) -> CatalogFuture<'a, Result<Option<Photo>, Error>> {
            Box::pin(async move { Ok((id == "p1").then(|| photo("p1", "KEEP"))) })
        }

        fn sessions(
            &self,
            _: &str,
        ) -> CatalogFuture<'_, Result<Vec<super::super::Session>, Error>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn groups(&self, _: &str) -> CatalogFuture<'_, Result<Vec<super::super::Group>, Error>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn profiles(&self) -> CatalogFuture<'_, Result<Vec<super::super::Profile>, Error>> {
            Box::pin(async { Ok(Vec::new()) })
        }

        fn summary(&self, _: &str) -> CatalogFuture<'_, Result<super::super::Summary, Error>> {
            Box::pin(async {
                Ok(super::super::Summary {
                    total_photos: 2,
                    total_files: 2,
                    selected_count: 0,
                    pending_count: 2,
                    decisions: Default::default(),
                    quality_tiers: Default::default(),
                })
            })
        }

        fn thumbnail<'a>(
            &'a self,
            id: &'a str,
            _: u32,
        ) -> CatalogFuture<'a, Result<Option<super::super::Thumbnail>, Error>> {
            Box::pin(async move {
                Ok((id == "p1").then(|| super::super::Thumbnail {
                    content_type: "image/jpeg".into(),
                    bytes: vec![0xff, 0xd8, 0xff, 0xd9],
                }))
            })
        }
    }

    fn photo(id: &str, decision: &str) -> Photo {
        Photo {
            id: id.into(),
            gallery_id: "g1".into(),
            name: format!("{id}.jpg"),
            decision: decision.into(),
            decision_revision: 0,
            score: None,
            quality_tier: None,
            analysis_summary: Value::Null,
            metadata: json!({"iso": 400}),
        }
    }

    async fn call(uri: &str) -> Response<Body> {
        router::<FakeCatalog>()
            .with_state(Arc::new(Reads(FakeCatalog)))
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap()
    }

    async fn json_body(response: Response<Body>) -> Value {
        serde_json::from_slice(&to_bytes(response.into_body(), 16 * 1024).await.unwrap()).unwrap()
    }

    #[tokio::test]
    async fn exposes_gallery_and_active_gallery_contracts() {
        let galleries = call("/api/v1/galleries").await;
        assert_eq!(galleries.status(), StatusCode::OK);
        assert_eq!(json_body(galleries).await["items"][0]["id"], "g1");

        let active = call("/api/v1/galleries/active").await;
        assert_eq!(active.status(), StatusCode::OK);
        assert_eq!(json_body(active).await["gallery_id"], "g1");
    }

    #[tokio::test]
    async fn page_route_filters_sorts_and_continues_from_json_cursor() {
        let first = call("/api/v1/galleries/g1/photos?decision=KEEP&limit=1").await;
        assert_eq!(first.status(), StatusCode::OK);
        let first_body = json_body(first).await;
        assert_eq!(first_body["items"][0]["id"], "p1");
        assert!(first_body["next_cursor"].is_null());

        let all = call("/api/v1/galleries/g1/photos?limit=1").await;
        let all_body = json_body(all).await;
        let cursor = all_body["next_cursor"].to_string();
        let uri = format!(
            "/api/v1/galleries/g1/photos?limit=1&cursor={}",
            url_encode(&cursor)
        );
        let second = call(&uri).await;
        let second_body = json_body(second).await;
        assert_eq!(second_body["items"][0]["id"], "p2");
    }

    #[tokio::test]
    async fn malformed_cursor_and_filter_mismatch_return_client_errors() {
        let malformed = call("/api/v1/galleries/g1/photos?cursor=not-json").await;
        assert_eq!(malformed.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(malformed).await["code"], "invalid_page");

        let cursor = serde_json::to_string(&Cursor {
            filter: Filter {
                gallery_id: "other".into(),
                decision: None,
            },
            after_id: "p1".into(),
        })
        .unwrap();
        let uri = format!("/api/v1/galleries/g1/photos?cursor={}", url_encode(&cursor));
        let mismatch = call(&uri).await;
        assert_eq!(mismatch.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(mismatch).await["code"], "cursor_filter_mismatch");
    }

    fn url_encode(value: &str) -> String {
        value
            .bytes()
            .map(|byte| match byte {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                    char::from(byte).to_string()
                }
                _ => format!("%{byte:02X}"),
            })
            .collect()
    }
}
