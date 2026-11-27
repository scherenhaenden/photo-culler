//! Same-origin Fetch transport for the WASM demo.
//!
//! Application requests are made here in Rust. The hand-written bootstrap only
//! loads the generated WASM module and reports startup failures. Responses are
//! streamed and bounded because Fetch's `array_buffer()` would allocate the
//! complete body before a limit could be enforced.

use std::fmt;

pub const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_IMPORT_JSON_BYTES: usize = 1_048_576;
pub const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PREVIEW_BYTES: usize = 16 * 1024 * 1024;
const API_PREFIX: &str = "/api/v1";
pub const HEALTH_PATH: &str = "/api/v1/health";
pub const GALLERY_PATH: &str = "/api/v1/galleries/demo/photos";
pub const SESSION_PAIR_PATH: &str = "/api/v1/session/pair";
#[cfg(test)]
const THUMBNAIL_PATH: &str = "/api/v1/photos/demo-001/thumbnail/800";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl HttpResponse {
    #[allow(dead_code)] // Used by contract tests; production callers inspect typed statuses.
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransportError {
    InvalidPath,
    Network,
    RedirectRejected,
    BodyUnavailable,
    ResponseTooLarge { limit: usize },
    ReadFailed,
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidPath => {
                f.write_str("request path must be an allowed same-origin Photo Culler API path")
            }
            Self::Network => f.write_str("network request failed"),
            Self::RedirectRejected => f.write_str("redirects are not allowed for API requests"),
            Self::BodyUnavailable => f.write_str("response body is unavailable"),
            Self::ResponseTooLarge { limit } => write!(f, "response exceeds {limit} byte limit"),
            Self::ReadFailed => f.write_str("response body could not be read"),
        }
    }
}

/// Validate before passing a route to Fetch. Reject absolute and scheme-relative
/// URLs so the browser cannot send application data to another origin.
pub fn validate_api_path(path: &str) -> Result<(), TransportError> {
    let api_prefix = format!("{API_PREFIX}/");
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let has_query = path.contains('?');
    let gallery_page = route.starts_with("/api/v1/galleries/") && route.ends_with("/photos");
    let preview_route = route.starts_with("/api/v1/photos/") && route.ends_with("/preview");
    let v2_route = valid_v2_route(route);
    #[cfg(target_arch = "wasm32")]
    let same_origin_path = {
        let suffix = route.strip_prefix(&api_prefix).unwrap_or_default();
        route == SESSION_PAIR_PATH || super::api_path(suffix).is_ok()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let same_origin_path = true;
    let safe_query = !has_query
        || (!query.is_empty()
            && (gallery_page
                || preview_route
                || route == "/api/v2/import-jobs"
                || route == "/api/v2/scan-revisions")
            && {
                let mut seen = std::collections::BTreeSet::new();
                query.split('&').all(|part| {
                    let Some((key, value)) = part.split_once('=') else {
                        return false;
                    };
                    let allowed_key = if gallery_page {
                        matches!(key, "limit" | "decision" | "cursor")
                    } else if preview_route {
                        key == "size"
                    } else if route == "/api/v2/scan-revisions" {
                        matches!(key, "limit" | "gallery_id")
                    } else if route == "/api/v2/import-jobs" {
                        matches!(key, "limit" | "active_only")
                    } else {
                        key == "limit"
                    };
                    allowed_key
                        && seen.insert(key)
                        && (key != "active_only" || value == "true")
                        && (key != "size" || matches!(value, "small" | "medium" | "large" | "full"))
                        && !value.is_empty()
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(byte, b'-' | b'_' | b'.' | b'~' | b'%')
                        })
                        && valid_percent_escapes(value)
                })
            });
    let v1_route = route.starts_with(&api_prefix) && same_origin_path;
    if !(v1_route || v2_route)
        || route.starts_with("//")
        || route.contains(['?', '#', '\\'])
        || route.split('/').any(|part| part == "." || part == "..")
        || query.contains(['#', '\\'])
        || !safe_query
    {
        return Err(TransportError::InvalidPath);
    }
    Ok(())
}

/// Return the contract's response cap for gallery, import JSON, or thumbnails.
pub fn request_limit(path: &str) -> Result<usize, TransportError> {
    validate_api_path(path)?;
    let route = path.split_once('?').map_or(path, |(route, _)| route);
    if valid_v2_route(route) {
        return Ok(MAX_IMPORT_JSON_BYTES);
    }
    if route == HEALTH_PATH
        || route == SESSION_PAIR_PATH
        || route == GALLERY_PATH
        || route == "/api/v1/galleries/active"
        || (route.starts_with("/api/v1/galleries/")
            && (route.ends_with("/photos") || route.ends_with("/summary")))
    {
        return Ok(MAX_JSON_BYTES);
    }
    if route.starts_with("/api/v1/photos/")
        && route.ends_with("/preview")
        && path
            .split_once('?')
            .is_some_and(|(_, query)| query.starts_with("size=") && !query.contains('&'))
    {
        return Ok(MAX_PREVIEW_BYTES);
    }
    if route.starts_with("/api/v1/photos/")
        && let Some((_, size)) = route.rsplit_once("/thumbnail/")
        && size
            .parse::<u32>()
            .is_ok_and(|size| (1..=4096).contains(&size))
    {
        return Ok(MAX_THUMBNAIL_BYTES);
    }
    if route.starts_with("/api/v1/photos/") && route.ends_with("/decision") {
        return Ok(MAX_JSON_BYTES);
    }
    Err(TransportError::InvalidPath)
}

fn valid_percent_escapes(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len()
                || !bytes[index + 1].is_ascii_hexdigit()
                || !bytes[index + 2].is_ascii_hexdigit()
            {
                return false;
            }
            index += 3;
        } else {
            index += 1;
        }
    }
    true
}

/// Decode metadata delivered by the WASM fetch implementation. All HTTP
/// statuses are returned as responses so callers can inspect typed error JSON;
/// a network failure remains a `TransportError`.
pub fn response(
    status: u16,
    content_type: Option<String>,
    bytes: Vec<u8>,
    limit: usize,
) -> Result<HttpResponse, TransportError> {
    if bytes.len() > limit {
        return Err(TransportError::ResponseTooLarge { limit });
    }
    Ok(HttpResponse {
        status,
        content_type,
        bytes,
    })
}

#[cfg(target_arch = "wasm32")]
pub async fn fetch(
    path: &str,
    bearer: &str,
    max_response_bytes: usize,
) -> Result<HttpResponse, TransportError> {
    request(path, "GET", None, max_response_bytes, Some(bearer)).await
}

#[cfg(target_arch = "wasm32")]
pub async fn put_json(
    path: &str,
    body: Vec<u8>,
    bearer: &str,
    max_response_bytes: usize,
) -> Result<HttpResponse, TransportError> {
    if body.len() > 4 * 1024 {
        return Err(TransportError::ResponseTooLarge { limit: 4 * 1024 });
    }
    request(path, "PUT", Some(body), max_response_bytes, Some(bearer)).await
}

#[cfg(target_arch = "wasm32")]
pub async fn post_json(
    path: &str,
    body: Vec<u8>,
    bearer: &str,
    max_response_bytes: usize,
) -> Result<HttpResponse, TransportError> {
    if body.len() > photo_culler_client::demo::MAX_IMPORT_REQUEST_BYTES {
        return Err(TransportError::ResponseTooLarge {
            limit: photo_culler_client::demo::MAX_IMPORT_REQUEST_BYTES,
        });
    }
    request(path, "POST", Some(body), max_response_bytes, Some(bearer)).await
}

#[cfg(target_arch = "wasm32")]
pub async fn pair_session(
    code: &str,
    max_response_bytes: usize,
) -> Result<HttpResponse, TransportError> {
    if code.len() != 64 || !code.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(TransportError::InvalidPath);
    }
    let body = serde_json::to_vec(&PairRequest { code }).map_err(|_| TransportError::Network)?;
    request(
        SESSION_PAIR_PATH,
        "POST",
        Some(body),
        max_response_bytes,
        None,
    )
    .await
}

#[derive(serde::Serialize)]
struct PairRequest<'a> {
    code: &'a str,
}

#[cfg(target_arch = "wasm32")]
async fn request(
    path: &str,
    method: &str,
    body: Option<Vec<u8>>,
    max_response_bytes: usize,
    bearer: Option<&str>,
) -> Result<HttpResponse, TransportError> {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        ReadableStreamDefaultReader, Request, RequestCredentials, RequestInit, RequestMode,
        RequestRedirect, Response, window,
    };

    validate_api_path(path)?;
    if !valid_request_method(method, path) {
        return Err(TransportError::InvalidPath);
    }
    let limit = request_limit(path)?.min(max_response_bytes);
    let window = window().ok_or(TransportError::Network)?;
    let init = RequestInit::new();
    init.set_method(method);
    init.set_mode(RequestMode::SameOrigin);
    init.set_redirect(RequestRedirect::Error);
    init.set_credentials(RequestCredentials::Omit);
    let has_body = body.is_some();
    if let Some(body) = body {
        let bytes = js_sys::Uint8Array::from(body.as_slice());
        init.set_body(&bytes.into());
    }
    let request =
        Request::new_with_str_and_init(path, &init).map_err(|_| TransportError::Network)?;
    if has_body {
        request
            .headers()
            .set("content-type", "application/json")
            .map_err(|_| TransportError::Network)?;
    }
    if let Some(bearer) = bearer {
        if bearer.len() != 64 || !bearer.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(TransportError::InvalidPath);
        }
        request
            .headers()
            .set("authorization", &format!("Bearer {bearer}"))
            .map_err(|_| TransportError::Network)?;
    }
    let raw = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|_| TransportError::Network)?;
    let response_obj = raw
        .dyn_into::<Response>()
        .map_err(|_| TransportError::Network)?;
    if response_obj.redirected() {
        return Err(TransportError::RedirectRejected);
    }
    let content_type = response_obj.headers().get("content-type").ok().flatten();
    let status = response_obj.status();
    let stream = response_obj.body().ok_or(TransportError::BodyUnavailable)?;
    let reader = stream
        .get_reader()
        .dyn_into::<ReadableStreamDefaultReader>()
        .map_err(|_| TransportError::BodyUnavailable)?;
    let mut bytes = Vec::new();
    loop {
        let result = JsFuture::from(reader.read())
            .await
            .map_err(|_| TransportError::ReadFailed)?;
        let done = js_sys::Reflect::get(&result, &JsValue::from_str("done"))
            .map_err(|_| TransportError::ReadFailed)?
            .as_bool()
            .unwrap_or(false);
        if done {
            break;
        }
        let value = js_sys::Reflect::get(&result, &JsValue::from_str("value"))
            .map_err(|_| TransportError::ReadFailed)?;
        let chunk = js_sys::Uint8Array::new(&value);
        let length = chunk.length() as usize;
        if bytes.len().saturating_add(length) > limit {
            let _ = JsFuture::from(reader.cancel()).await;
            return Err(TransportError::ResponseTooLarge { limit });
        }
        let old_len = bytes.len();
        bytes.resize(old_len + length, 0);
        chunk.copy_to(&mut bytes[old_len..]);
    }
    response(status, content_type, bytes, limit)
}

fn valid_v2_route(route: &str) -> bool {
    let Some(path) = route.strip_prefix('/') else {
        return false;
    };
    let segments = path.split('/').collect::<Vec<_>>();
    if segments.iter().any(|segment| segment.is_empty()) {
        return false;
    }
    match segments.as_slice() {
        ["api", "v2", "galleries", gallery_id, action]
            if valid_import_id(gallery_id)
                && matches!(*action, "sources" | "imports" | "rescan") =>
        {
            true
        }
        ["api", "v2", "import-estimates"]
        | ["api", "v2", "import-jobs"]
        | ["api", "v2", "scan-revisions"] => true,
        ["api", "v2", "import-jobs", job_id] => valid_import_id(job_id),
        ["api", "v2", "import-jobs", job_id, action]
            if valid_import_id(job_id) && matches!(*action, "pause" | "resume" | "cancel") =>
        {
            true
        }
        _ => false,
    }
}

fn valid_import_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

fn valid_request_method(method: &str, path: &str) -> bool {
    let route = path.split_once('?').map_or(path, |(route, _)| route);
    let is_v1 = route.starts_with("/api/v1/");
    match method {
        "GET" => is_v1 || is_v2_get_route(route),
        "PUT" => is_v1 && route.starts_with("/api/v1/photos/") && route.ends_with("/decision"),
        "POST" => {
            route == SESSION_PAIR_PATH || is_v2_post_route(route) || is_analysis_post_route(route)
        }
        _ => false,
    }
}

fn is_v2_get_route(route: &str) -> bool {
    matches!(route, "/api/v2/import-jobs" | "/api/v2/scan-revisions")
        || route.starts_with("/api/v2/galleries/") && route.ends_with("/sources")
        || route.starts_with("/api/v2/import-jobs/")
            && !["/pause", "/resume", "/cancel"]
                .iter()
                .any(|suffix| route.ends_with(suffix))
}

fn is_v2_post_route(route: &str) -> bool {
    route == "/api/v2/import-estimates"
        || route.starts_with("/api/v2/galleries/")
            && (route.ends_with("/imports") || route.ends_with("/rescan"))
        || route.starts_with("/api/v2/import-jobs/")
            && ["/pause", "/resume", "/cancel"]
                .iter()
                .any(|suffix| route.ends_with(suffix))
}

fn is_analysis_post_route(route: &str) -> bool {
    let Some(path) = route.strip_prefix('/') else {
        return false;
    };
    let segments = path.split('/').collect::<Vec<_>>();
    match segments.as_slice() {
        ["api", "v1", "galleries", gallery_id, "analysis-jobs"] => valid_import_id(gallery_id),
        ["api", "v1", "analysis-jobs", catalog_id, job_id, action]
            if valid_import_id(catalog_id)
                && valid_import_id(job_id)
                && matches!(*action, "start" | "pause" | "resume" | "cancel") =>
        {
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_routes_are_relative_and_have_contract_limits() {
        assert_eq!(request_limit(HEALTH_PATH), Ok(MAX_JSON_BYTES));
        assert_eq!(request_limit(GALLERY_PATH), Ok(MAX_JSON_BYTES));
        assert_eq!(
            request_limit("/api/v1/galleries/demo/summary"),
            Ok(MAX_JSON_BYTES)
        );
        assert_eq!(request_limit(THUMBNAIL_PATH), Ok(MAX_THUMBNAIL_BYTES));
        assert_eq!(
            request_limit("/api/v1/photos/demo-001/preview?size=medium"),
            Ok(MAX_PREVIEW_BYTES)
        );
        assert_eq!(
            validate_api_path("/api/v1/photos/demo-001/preview?size=medium"),
            Ok(())
        );
        assert_eq!(
            validate_api_path("/api/v1/photos/demo-001/preview?size=raw"),
            Err(TransportError::InvalidPath)
        );
        assert_eq!(
            request_limit("https://evil.invalid/api/v1/health"),
            Err(TransportError::InvalidPath)
        );
        assert_eq!(
            request_limit("//evil.invalid/api/v1/health"),
            Err(TransportError::InvalidPath)
        );
        assert_eq!(
            request_limit("/api/v1/../private"),
            Err(TransportError::InvalidPath)
        );
        assert_eq!(
            request_limit("/api/v1/unlisted"),
            Err(TransportError::InvalidPath)
        );
    }

    #[test]
    fn http_errors_remain_inspectable_and_network_errors_are_distinct() {
        let http = response(
            503,
            Some("application/json".into()),
            br#"{"code":"storage_unavailable"}"#.to_vec(),
            MAX_JSON_BYTES,
        )
        .unwrap();
        assert!(!http.is_success());
        assert_eq!(http.status, 503);
        assert!(http.bytes.starts_with(b"{"));
        assert_eq!(
            Err::<HttpResponse, _>(TransportError::Network),
            Err(TransportError::Network)
        );
    }

    #[test]
    fn body_caps_apply_to_json_and_jpeg_bytes() {
        assert_eq!(
            response(200, None, vec![0; 5], 4),
            Err(TransportError::ResponseTooLarge { limit: 4 })
        );
        let jpeg = response(
            200,
            Some("image/jpeg".into()),
            vec![0xff, 0xd8],
            MAX_THUMBNAIL_BYTES,
        )
        .unwrap();
        assert_eq!(jpeg.content_type.as_deref(), Some("image/jpeg"));
    }
}
