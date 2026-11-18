//! Axum adapter for the existing OriginPolicy. Demo requests are read-only.
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
    if !matches!(method, "GET" | "HEAD") {
        return rejection(
            StatusCode::METHOD_NOT_ALLOWED,
            "method_not_allowed",
            "Only read requests are supported",
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
    match state
        .0
        .check(method, &host_refs, &origin_refs, &fetch_refs, false)
    {
        Ok(()) => next.run(request).await,
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
        routing::get,
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
    fn policy() -> Arc<OriginPolicy> {
        Arc::new(OriginPolicy::new("http", "127.0.0.1", 49152).unwrap())
    }
    fn protected() -> Router {
        protect(Router::new().route("/", get(|| async { "read" })), policy())
    }
}
