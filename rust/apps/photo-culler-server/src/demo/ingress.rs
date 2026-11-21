//! Axum adapter for the existing OriginPolicy. Only allowlisted decision,
//! recipe, session, and configured import/analysis routes accept mutations.
//! Source policy: `http::origin::OriginPolicy` at sprint base db9a00723da400c479c09f8381657874d620e845.

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::Response,
};
use serde::Serialize;
use std::sync::Arc;

use crate::origin::{OriginPolicy, Rejection};

#[derive(Clone)]
pub struct IngressState(pub Arc<OriginPolicy>);

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: &'static str,
    request_id: &'static str,
}

pub fn protect(router: Router, policy: Arc<OriginPolicy>) -> Router {
    router.layer(middleware::from_fn_with_state(
        IngressState(policy),
        enforce,
    ))
}

async fn enforce(
    State(state): State<IngressState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().as_str();
    let path = request.uri().path();
    let allowed_decision_write = method == "PUT" && is_decision_path(path);
    let allowed_recipe_write = (method == "PUT" && is_recipe_update_path(path))
        || (method == "POST" && is_recipe_history_path(path));
    let allowed_import_write = method == "POST" && is_import_post_path(path);
    let allowed_analysis_write = method == "POST" && is_analysis_post_path(path);
    let allowed_session_write = (method == "POST" && is_session_post_path(path))
        || (method == "DELETE" && path == "/api/v1/session");
    if !matches!(method, "GET" | "HEAD")
        && !allowed_decision_write
        && !allowed_recipe_write
        && !allowed_import_write
        && !allowed_analysis_write
        && !allowed_session_write
    {
        return rejection(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Only reads and allowlisted catalog, recipe, import, or analysis actions are supported",
        );
    }
    let headers = request.headers();
    let hosts = values(headers, "host");
    let origins = values(headers, "origin");
    let fetch_sites = values(headers, "sec-fetch-site");
    let Ok((hosts, origins, fetch_sites)) = hosts
        .and_then(|h| origins.map(|o| (h, o)).map_err(|_| ()))
        .and_then(|(h, o)| fetch_sites.map(|f| (h, o, f)))
    else {
        return rejection(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Request authority or origin was rejected",
        );
    };
    let host_refs = hosts.iter().map(String::as_str).collect::<Vec<_>>();
    let origin_refs = origins.iter().map(String::as_str).collect::<Vec<_>>();
    let fetch_refs = fetch_sites.iter().map(String::as_str).collect::<Vec<_>>();
    // Firefox currently serializes the page origin as `null` for its same-origin
    // WASM pairing fetch. Keep this compatibility exception limited to the
    // one-time public pairing route, an exact Host, and browser metadata that
    // explicitly says same-origin. Other writes still require the exact Origin.
    let firefox_pairing_origin = method == "POST"
        && path == "/api/v1/session/pair"
        && origin_refs.as_slice() == ["null"]
        && fetch_refs.as_slice() == ["same-origin"];
    let policy = if firefox_pairing_origin {
        state.0.check(method, &host_refs, &[], &fetch_refs, true)
    } else {
        state
            .0
            .check(method, &host_refs, &origin_refs, &fetch_refs, false)
    };
    match policy {
        Ok(()) => {
            let mut response = next.run(request).await;
            // The local catalog and fixture API must never be cached by a
            // browser or intermediary. This also covers API error responses.
            response.headers_mut().insert(
                "cache-control",
                axum::http::HeaderValue::from_static("no-store"),
            );
            response
        }
        Err(Rejection::InvalidMethod) => rejection(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Only read requests are supported",
        ),
        Err(_) => rejection(
            StatusCode::FORBIDDEN,
            "forbidden",
            "Request authority or origin was rejected",
        ),
    }
}

fn is_session_post_path(path: &str) -> bool {
    matches!(
        exact_path_segments(path).as_deref(),
        Some(["api", "v1", "session", "pair"] | ["api", "v1", "session", "pairing-codes"])
    )
}

fn is_decision_path(path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v1", "photos", photo_id, "decision"] => valid_path_id(photo_id),
        _ => false,
    }
}

fn is_recipe_update_path(path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v1", "photos", photo_id, "recipe"] => valid_path_id(photo_id),
        _ => false,
    }
}

fn is_recipe_history_path(path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v1", "photos", photo_id, "recipe", action]
            if valid_path_id(photo_id) && matches!(*action, "undo" | "redo") =>
        {
            true
        }
        _ => false,
    }
}

fn is_import_post_path(path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v2", "galleries", gallery_id, "imports"]
        | ["api", "v2", "galleries", gallery_id, "rescan"] => valid_path_id(gallery_id),
        ["api", "v2", "import-estimates"] => true,
        ["api", "v2", "import-jobs", job_id, action]
            if valid_path_id(job_id) && matches!(*action, "pause" | "resume" | "cancel") =>
        {
            true
        }
        _ => false,
    }
}

fn is_analysis_post_path(path: &str) -> bool {
    let Some(segments) = exact_path_segments(path) else {
        return false;
    };
    match segments.as_slice() {
        ["api", "v1", "galleries", gallery_id, "analysis-jobs"] => valid_path_id(gallery_id),
        ["api", "v1", "analysis-jobs", catalog_id, job_id, action]
            if valid_path_id(catalog_id)
                && valid_path_id(job_id)
                && matches!(*action, "start" | "pause" | "resume" | "cancel") =>
        {
            true
        }
        _ => false,
    }
}

/// Parse a path only when it has one leading slash, no empty segments, and no
/// trailing slash. Exact slice matching at each route then rejects extra paths.
fn exact_path_segments(path: &str) -> Option<Vec<&str>> {
    let path = path.strip_prefix('/')?;
    if path.is_empty() || path.ends_with('/') {
        return None;
    }
    let segments: Vec<_> = path.split('/').collect();
    segments
        .iter()
        .all(|segment| !segment.is_empty())
        .then_some(segments)
}

fn valid_path_id(id: &str) -> bool {
    !id.is_empty()
        && !id.contains("..")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn values(headers: &HeaderMap, name: &'static str) -> Result<Vec<String>, ()> {
    headers
        .get_all(name)
        .iter()
        .map(|value| value.to_str().map(str::to_owned).map_err(|_| ()))
        .collect()
}

fn rejection(status: StatusCode, code: &'static str, message: &'static str) -> Response {
    let mut response = (
        status,
        Json(ErrorBody {
            code,
            message,
            request_id: "req-example",
        }),
    )
        .into_response();
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

use axum::response::IntoResponse;

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::to_bytes,
        http::{Method, Request},
        routing::{get, post},
    };
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;

    #[tokio::test]
    async fn same_host_get_without_origin_is_allowed_and_no_cors_is_added() {
        let app = protected();
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/")
                    .header("host", "127.0.0.1:49152")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
    }
    #[tokio::test]
    async fn foreign_origin_or_unexpected_host_is_rejected() {
        for req in [
            Request::builder()
                .uri("/")
                .header("host", "127.0.0.1:49152")
                .header("origin", "http://evil.test")
                .body(Body::empty())
                .unwrap(),
            Request::builder()
                .uri("/")
                .header("host", "evil.test")
                .body(Body::empty())
                .unwrap(),
        ] {
            let response = protected().oneshot(req).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
            let body = to_bytes(response.into_body(), 1024).await.unwrap();
            assert!(std::str::from_utf8(&body).unwrap().contains("forbidden"));
        }
    }
    #[tokio::test]
    async fn post_is_rejected_before_handler_and_never_adds_cors() {
        let called = Arc::new(AtomicUsize::new(0));
        let mark = called.clone();
        let api = Router::new().route(
            "/",
            get(|| async { "read" }).post(move || {
                let mark = mark.clone();
                async move {
                    mark.fetch_add(1, Ordering::SeqCst);
                    "write"
                }
            }),
        );
        let response = protect(api, policy())
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/")
                    .header("host", "127.0.0.1:49152")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
        assert_eq!(called.load(Ordering::SeqCst), 0);
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
    }
    #[tokio::test]
    async fn opaque_origin_pairing_requires_same_origin_fetch_metadata_and_exact_host() {
        let api = Router::new()
            .route("/api/v1/session/pair", post(|| async { "paired" }))
            .route("/api/v1/session/pairing-codes", post(|| async { "issued" }));
        let request = |path: &str, host: &str, fetch_site: &str| {
            Request::builder()
                .method(Method::POST)
                .uri(path)
                .header("host", host)
                .header("origin", "null")
                .header("sec-fetch-site", fetch_site)
                .body(Body::empty())
                .unwrap()
        };

        let allowed = protect(api.clone(), policy())
            .oneshot(request(
                "/api/v1/session/pair",
                "127.0.0.1:49152",
                "same-origin",
            ))
            .await
            .unwrap();
        assert_eq!(allowed.status(), StatusCode::OK);

        for rejected_request in [
            request("/api/v1/session/pair", "127.0.0.1:49152", "cross-site"),
            request("/api/v1/session/pair", "evil.test:49152", "same-origin"),
            request(
                "/api/v1/session/pairing-codes",
                "127.0.0.1:49152",
                "same-origin",
            ),
        ] {
            let rejected = protect(api.clone(), policy())
                .oneshot(rejected_request)
                .await
                .unwrap();
            assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
        }
    }
    fn policy() -> Arc<OriginPolicy> {
        Arc::new(OriginPolicy::new("http", "127.0.0.1", 49152).unwrap())
    }
    fn protected() -> Router {
        protect(Router::new().route("/", get(|| async { "read" })), policy())
    }
}
