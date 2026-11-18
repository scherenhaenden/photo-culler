//! Bounded native HTTP transport for the local Photo Culler demo API.
//!
//! The synchronous operation is intended to run on a worker thread; use
//! [`get_async`] from an event-loop callback so a slow server cannot freeze UI.
//! Integration: declare this module from the desktop crate's module tree when
//! D33 registers the Slint package; that crate/module integration is absent at
//! the sprint base, so this file is verified as a standalone Rust module.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::thread::{self, JoinHandle};
use std::time::Duration;

const MAX_HEADER_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct NativeHttpTransport {
    address: SocketAddr,
    timeout: Duration,
    max_response_bytes: usize,
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
        })
    }

    pub fn get(&self, path: &str) -> Result<HttpResponse, HttpError> {
        if !valid_api_path(path) {
            return Err(HttpError::InvalidPath);
        }
        let mut stream = TcpStream::connect_timeout(&self.address, self.timeout).map_err(map_io)?;
        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(map_io)?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(map_io)?;
        write!(stream, "GET {path} HTTP/1.1\r\nHost: {}\r\nAccept: application/json, image/jpeg\r\nConnection: close\r\n\r\n", self.address)
            .map_err(map_io)?;

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
        if status != 200 {
            return Err(HttpError::HttpStatus(status));
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

fn valid_api_path(path: &str) -> bool {
    path.starts_with("/api/v1/")
        && !path.contains("..")
        && !path.contains(['\\', '?', '#', '\r', '\n', ' '])
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
                .unwrap_err(),
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
