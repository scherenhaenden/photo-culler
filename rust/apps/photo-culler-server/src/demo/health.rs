//! Readiness endpoint for the local, read-only demo.
//! Contract source: GitHub issue #83 at sprint base db9a00723da400c479c09f8381657874d620e845.

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::get};
use serde::Serialize;

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

pub trait ReadinessProbe: Send + Sync + 'static {
    fn ready(&self) -> bool;
}

#[derive(Clone)]
pub struct HealthState(pub Arc<dyn ReadinessProbe>);

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HealthBody {
    pub status: &'static str,
    pub resources: Option<()>,
    pub request_id: String,
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HealthError {
    pub code: &'static str,
    pub message: &'static str,
    pub request_id: String,
}

pub fn router<P: ReadinessProbe>(probe: Arc<P>) -> Router {
    let probe: Arc<dyn ReadinessProbe> = probe;
    Router::new()
        .route("/api/v1/health", get(health))
        .with_state(HealthState(probe))
}

async fn health(State(state): State<HealthState>) -> (StatusCode, Json<serde_json::Value>) {
    let request_id = format!("req-{:x}", NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed));
    if state.0.ready() {
        (
            StatusCode::OK,
            Json(serde_json::json!({"status":"ready","resources":null,"request_id":request_id})),
        )
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(
                serde_json::json!({"code":"service_unavailable","message":"Service is unavailable","request_id":request_id}),
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    struct Probe(bool);
    impl ReadinessProbe for Probe {
        fn ready(&self) -> bool {
            self.0
        }
    }

    async fn call(ready: bool) -> (StatusCode, serde_json::Value) {
        let response = router(Arc::new(Probe(ready)))
            .oneshot(
                Request::builder()
                    .uri("/api/v1/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        (status, serde_json::from_slice(&bytes).unwrap())
    }

    #[tokio::test]
    async fn ready_uses_d01_shape_without_system_telemetry() {
        let (status, body) = call(true).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["status"], "ready");
        assert!(body["resources"].is_null());
        assert!(body["request_id"].as_str().unwrap().starts_with("req-"));
    }
    #[tokio::test]
    async fn unavailable_uses_typed_error_and_correlated_id() {
        let (status, body) = call(false).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(body["code"], "service_unavailable");
        assert_eq!(
            body["request_id"].as_str().unwrap().starts_with("req-"),
            true
        );
        assert!(!body.to_string().contains('/'));
    }
}
