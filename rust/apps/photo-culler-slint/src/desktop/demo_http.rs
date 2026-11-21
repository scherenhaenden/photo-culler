//! Bounded native HTTP transport for the local Photo Culler demo API.
//!
//! The synchronous operation is intended to run on a worker thread; use
//! [`get_async`] from an event-loop callback so a slow server cannot freeze UI.
//! The owned desktop launcher uses this transport to check the local server's
//! readiness. UI REST requests use the shared client with the desktop transport
//! adapter in `demo::native_transport`.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Clone)]
pub struct NativeHttpTransport {
    address: SocketAddr,
    timeout: Duration,
    max_response_bytes: usize,
    authorization: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum HttpError {
    InvalidEndpoint,
    InvalidPath,
    Connect(String),
    Io(String),
    Timeout,
    InvalidResponse(&'static str),
    RedirectRejected,
    HttpStatus(u16),
    ResponseTooLarge { limit: usize },
}

impl NativeHttpTransport {
    pub fn new(
        address: SocketAddr,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Result<Self, HttpError> {
        Self::with_authorization(address, timeout, max_response_bytes, None)
    }

    /// Construct a transport for authenticated API calls. Tokens are validated
    /// before interpolation into an HTTP header and are never included in errors.
    pub fn new_authenticated(
        address: SocketAddr,
        timeout: Duration,
        max_response_bytes: usize,
        token: String,
    ) -> Result<Self, HttpError> {
        if !valid_session_token(&token) {
            return Err(HttpError::InvalidEndpoint);
        }
        Self::with_authorization(address, timeout, max_response_bytes, Some(token))
    }

    fn with_authorization(
        address: SocketAddr,
        timeout: Duration,
        max_response_bytes: usize,
        authorization: Option<String>,
    ) -> Result<Self, HttpError> {
        if !address.ip().is_loopback()
            || address.port() == 0
            || timeout.is_zero()
            || max_response_bytes == 0
        {
            return Err(HttpError::InvalidEndpoint);
        }
        Ok(Self {
            address,
            timeout,
            max_response_bytes,
            authorization,
        })
    }

    pub fn get(&self, path: &str) -> Result<HttpResponse, HttpError> {
        self.request("GET", path, None)
    }

    pub fn put_json(&self, path: &str, body: &[u8]) -> Result<HttpResponse, HttpError> {
        self.request("PUT", path, Some(body))
    }

    pub fn post_json(&self, path: &str, body: &[u8]) -> Result<HttpResponse, HttpError> {
        self.request("POST", path, Some(body))
    }

    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<&[u8]>,
    ) -> Result<HttpResponse, HttpError> {
        if !valid_api_request(method, path) {
            return Err(HttpError::InvalidPath);
        }
        let mut stream = TcpStream::connect_timeout(&self.address, self.timeout).map_err(map_io)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(map_io)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(map_io)?;
        write!(stream, "{method} {path} HTTP/1.1\r\nHost: {}\r\nOrigin: http://{}\r\nAccept: application/json, image/jpeg\r\nConnection: close\r\n", self.address, self.address)
            .map_err(map_io)?;
        if let Some(token) = self.authorization.as_deref() {
            write!(stream, "Authorization: Bearer {token}\r\n").map_err(map_io)?;
        }
        if let Some(body) = body {
            write!(
                stream,
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            )
            .map_err(map_io)?;
        }
        stream.write_all(b"\r\n").map_err(map_io)?;
        if let Some(body) = body {
            stream.write_all(body).map_err(map_io)?;
        }

        // Bound the entire response, including headers, so a peer cannot make
        // the client allocate without limit before the body-size check.
        let wire_limit = self.max_response_bytes.saturating_add(MAX_HEADER_BYTES);
        let mut wire = Vec::new();
        stream
            .take(wire_limit.saturating_add(1) as u64)
            .read_to_end(&mut wire)
            .map_err(map_io)?;
        if wire.len() > wire_limit {
            return Err(HttpError::ResponseTooLarge {
                limit: self.max_response_bytes,
            });
        }
        let boundary = wire
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .ok_or(HttpError::InvalidResponse("missing HTTP header boundary"))?;
        if boundary > MAX_HEADER_BYTES {
            return Err(HttpError::InvalidResponse("HTTP headers too large"));
        }
        let header = std::str::from_utf8(&wire[..boundary])
            .map_err(|_| HttpError::InvalidResponse("headers are not UTF-8"))?;
        let mut lines = header.split("\r\n");
        let status_line = lines
            .next()
            .ok_or(HttpError::InvalidResponse("missing status line"))?;
        let mut status_parts = status_line.split_ascii_whitespace();
        if !matches!(status_parts.next(), Some("HTTP/1.0" | "HTTP/1.1")) {
            return Err(HttpError::InvalidResponse("invalid HTTP version"));
        }
        let status = status_parts
            .next()
            .and_then(|value| value.parse::<u16>().ok())
            .ok_or(HttpError::InvalidResponse("invalid status code"))?;
        let mut content_type = None;
        let mut chunked = false;
        for line in lines {
            if let Some((name, value)) = line.split_once(':') {
                if name.eq_ignore_ascii_case("content-type") {
                    content_type = Some(value.trim().to_owned());
                }
                if name.eq_ignore_ascii_case("transfer-encoding")
                    && value.trim().eq_ignore_ascii_case("chunked")
                {
                    chunked = true;
                }
            }
        }
        if chunked {
            return Err(HttpError::InvalidResponse(
                "chunked response is not supported",
            ));
        }
        let body = wire[boundary + 4..].to_vec();
        if body.len() > self.max_response_bytes {
            return Err(HttpError::ResponseTooLarge {
                limit: self.max_response_bytes,
            });
        }
        if (300..400).contains(&status) {
            return Err(HttpError::RedirectRejected);
        }
        let content_type =
            content_type.ok_or(HttpError::InvalidResponse("missing content type"))?;
        Ok(HttpResponse {
            status,
            content_type,
            body,
        })
    }

    /// Move socket I/O off the UI thread. Dropping a handle does not block the
    /// event loop; callers may poll or join it from their worker executor.
    pub fn get_async(&self, path: String) -> JoinHandle<Result<HttpResponse, HttpError>> {
        let transport = self.clone();
        thread::spawn(move || transport.get(&path))
    }
}

fn valid_session_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn valid_api_request(method: &str, path: &str) -> bool {
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let has_query = path.contains('?');
    let catalog_page = route.starts_with("/api/v1/galleries/") && route.ends_with("/photos");
    let preview_route = route.starts_with("/api/v1/photos/") && route.ends_with("/preview");
    let v2_route = valid_v2_route(route);
    let query_is_safe = !has_query
        || (!query.is_empty()
            && (catalog_page
                || preview_route
                || route == "/api/v2/import-jobs"
                || route == "/api/v2/scan-revisions")
            && {
                let mut seen = std::collections::BTreeSet::new();
                query.split('&').all(|parameter| {
                    let Some((key, value)) = parameter.split_once('=') else {
                        return false;
                    };
                    let allowed = if catalog_page {
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
                    allowed
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
    let safe_syntax = !route.contains("..")
        && !route.contains(['\\', '?', '#', '\r', '\n', ' '])
        && !route.split('/').any(|segment| segment == ".")
        && !query.contains(['\\', '#', '\r', '\n', ' '])
        && query_is_safe;
    let v1_route = route.starts_with("/api/v1/");
    let method_allowed = match method {
        "GET" => v1_route || is_v2_get_route(route),
        "PUT" => v1_route && route.starts_with("/api/v1/photos/") && route.ends_with("/decision"),
        "POST" => is_v2_post_route(route),
        _ => false,
    };
    safe_syntax && method_allowed && (v1_route || v2_route)
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

fn is_v2_get_route(route: &str) -> bool {
    matches!(route, "/api/v2/import-jobs" | "/api/v2/scan-revisions")
        || route.ends_with("/sources") && route.starts_with("/api/v2/galleries/")
        || route.starts_with("/api/v2/import-jobs/")
            && !route.ends_with("/pause")
            && !route.ends_with("/resume")
            && !route.ends_with("/cancel")
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

fn valid_import_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
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

fn map_io(error: std::io::Error) -> HttpError {
    if matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    ) {
        HttpError::Timeout
    } else {
        HttpError::Io(error.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_bounded_preview_size_queries() {
        assert!(valid_api_request(
            "GET",
            "/api/v1/photos/demo-001/preview?size=medium"
        ));
        assert!(!valid_api_request(
            "GET",
            "/api/v1/photos/demo-001/preview?size=original"
        ));
        assert!(!valid_api_request(
            "GET",
            "/api/v1/photos/demo-001/preview?size=medium&token=secret"
        ));
    }
    use std::net::TcpListener;

    fn serve(responses: Vec<(String, String, Vec<u8>)>) -> (SocketAddr, JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let task = thread::spawn(move || {
            let mut paths = Vec::new();
            for (status, content_type, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = Vec::new();
                let mut chunk = [0u8; 512];
                while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                    let n = stream.read(&mut chunk).unwrap();
                    assert_ne!(n, 0, "client closed before completing request headers");
                    request.extend_from_slice(&chunk[..n]);
                }
                let line = String::from_utf8_lossy(&request)
                    .lines()
                    .next()
                    .unwrap()
                    .to_owned();
                paths.push(line.split_ascii_whitespace().nth(1).unwrap().to_owned());
                write!(stream, "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).unwrap();
                stream.write_all(&body).unwrap();
            }
            paths
        });
        (address, task)
    }

    #[test]
    fn health_gallery_and_thumbnail_reach_local_server_with_expected_types() {
        let (address, server) = serve(vec![
            (
                "200 OK".into(),
                "application/json".into(),
                br#"{"status":"ready","resources":null,"request_id":"req-example"}"#.to_vec(),
            ),
            (
                "200 OK".into(),
                "application/json".into(),
                br#"{"gallery_id":"demo","items":[],"next_cursor":null,"total":0}"#.to_vec(),
            ),
            (
                "200 OK".into(),
                "image/jpeg".into(),
                vec![0xff, 0xd8, 0xff, 0xd9],
            ),
        ]);
        let client = NativeHttpTransport::new(address, Duration::from_secs(1), 1024).unwrap();
        assert!(
            client
                .get("/api/v1/health")
                .unwrap()
                .body
                .windows(5)
                .any(|w| w == b"ready")
        );
        assert_eq!(
            client
                .get("/api/v1/galleries/demo/photos")
                .unwrap()
                .content_type,
            "application/json"
        );
        assert_eq!(
            client
                .get("/api/v1/photos/demo-001/thumbnail/800")
                .unwrap()
                .content_type,
            "image/jpeg"
        );
        assert_eq!(
            server.join().unwrap(),
            vec![
                "/api/v1/health",
                "/api/v1/galleries/demo/photos",
                "/api/v1/photos/demo-001/thumbnail/800"
            ]
        );
    }

    #[test]
    fn timeout_and_redirect_are_controlled_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let stalled = thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            thread::sleep(Duration::from_millis(150));
        });
        let client = NativeHttpTransport::new(address, Duration::from_millis(25), 1024).unwrap();
        assert_eq!(client.get("/api/v1/health"), Err(HttpError::Timeout));
        stalled.join().unwrap();

        let (address, server) = serve(vec![(
            "302 Found".into(),
            "text/plain".into(),
            b"redirect".to_vec(),
        )]);
        let client = NativeHttpTransport::new(address, Duration::from_secs(1), 1024).unwrap();
        assert_eq!(
            client.get("/api/v1/health"),
            Err(HttpError::RedirectRejected)
        );
        server.join().unwrap();
    }

    #[test]
    fn rejects_non_loopback_invalid_paths_and_oversized_responses() {
        assert_eq!(
            NativeHttpTransport::new("192.0.2.1:80".parse().unwrap(), Duration::from_secs(1), 10)
                .err()
                .expect("a non-loopback endpoint must be rejected"),
            HttpError::InvalidEndpoint
        );
        let (address, server) = serve(vec![(
            "200 OK".into(),
            "application/json".into(),
            vec![b'x'; 64],
        )]);
        let client = NativeHttpTransport::new(address, Duration::from_secs(1), 8).unwrap();
        assert_eq!(client.get("/api/v1/../secret"), Err(HttpError::InvalidPath));
        assert!(matches!(
            client.get("/api/v1/health"),
            Err(HttpError::ResponseTooLarge { limit: 8 })
        ));
        server.join().unwrap();
    }

    #[test]
    fn async_request_runs_on_a_worker_thread() {
        let (address, server) = serve(vec![("200 OK".into(), "image/jpeg".into(), vec![1, 2, 3])]);
        let client = NativeHttpTransport::new(address, Duration::from_secs(1), 100).unwrap();
        assert_eq!(
            client
                .get_async("/api/v1/photos/demo-001/thumbnail/800".into())
                .join()
                .unwrap()
                .unwrap()
                .body,
            vec![1, 2, 3]
        );
        server.join().unwrap();
    }
}
