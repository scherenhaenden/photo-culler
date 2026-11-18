//! Same-origin Fetch transport for the WASM demo.
//!
//! Application requests are made here in Rust. The hand-written bootstrap only
//! loads the generated WASM module and reports startup failures. Responses are
//! streamed and bounded because Fetch's `array_buffer()` would allocate the
//! complete body before a limit could be enforced.

use std::fmt;

pub const MAX_JSON_BYTES: usize = 64 * 1024;
pub const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;
const API_PREFIX: &str = "/api/v1";
pub const HEALTH_PATH: &str = "/api/v1/health";
pub const GALLERY_PATH: &str = "/api/v1/galleries/demo/photos";
pub const THUMBNAIL_PATH: &str = "/api/v1/photos/demo-001/thumbnail/800";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: Option<String>,
    pub bytes: Vec<u8>,
}

impl HttpResponse {
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
            Self::InvalidPath => f.write_str("request path must be a same-origin /api/v1 path"),
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
    #[cfg(target_arch = "wasm32")]
    let same_origin_path = {
        let suffix = path.strip_prefix(&api_prefix).unwrap_or_default();
        super::api_path(suffix).is_ok()
    };
    #[cfg(not(target_arch = "wasm32"))]
    let same_origin_path = true;
    if !path.starts_with(&api_prefix)
        || !same_origin_path
        || path.starts_with("//")
        || path.contains('\\')
        || path.contains('#')
        || path.split('/').any(|part| part == "." || part == "..")
    {
        return Err(TransportError::InvalidPath);
    }
    Ok(())
}

/// Return the contract's body cap for the three read-only demo requests.
pub fn request_limit(path: &str) -> Result<usize, TransportError> {
    validate_api_path(path)?;
    match path {
        HEALTH_PATH | GALLERY_PATH => Ok(MAX_JSON_BYTES),
        THUMBNAIL_PATH => Ok(MAX_THUMBNAIL_BYTES),
        _ => Err(TransportError::InvalidPath),
    }
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
pub async fn fetch(path: &str) -> Result<HttpResponse, TransportError> {
    use wasm_bindgen::{JsCast, JsValue};
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{
        ReadableStreamDefaultReader, Request, RequestInit, RequestMode, RequestRedirect, Response,
        window,
    };

    validate_api_path(path)?;
    let limit = request_limit(path)?;
    let window = window().ok_or(TransportError::Network)?;
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::SameOrigin);
    init.set_redirect(RequestRedirect::Error);
    let request =
        Request::new_with_str_and_init(path, &init).map_err(|_| TransportError::Network)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_routes_are_relative_and_have_contract_limits() {
        assert_eq!(request_limit(HEALTH_PATH), Ok(MAX_JSON_BYTES));
        assert_eq!(request_limit(GALLERY_PATH), Ok(MAX_JSON_BYTES));
        assert_eq!(request_limit(THUMBNAIL_PATH), Ok(MAX_THUMBNAIL_BYTES));
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
