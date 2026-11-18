//! Async, same-origin REST client for the read-only Rust/Slint demo.
use serde::{Deserialize, Serialize};
use std::{future::Future, pin::Pin};

pub const API_PREFIX: &str = "/api/v1";
pub const DEFAULT_MAX_JSON_BYTES: usize = 1_048_576;
pub const DEFAULT_MAX_THUMBNAIL_BYTES: usize = 20 * 1_048_576;

pub type RequestFuture<'a> = Pin<Box<dyn Future<Output = Result<Response, TransportError>> + 'a>>;

/// Adapters must make exactly one same-origin GET and disable redirect following.
/// The core validates returned status, MIME, path and body bounds independently.
pub trait Transport {
    fn get<'a>(&'a self, relative_path: &'a str, max_response_bytes: usize) -> RequestFuture<'a>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
    /// Adapters disable automatic redirects and report any redirect here.
    pub redirected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError(pub String);

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct ApiErrorBody {
    pub code: String,
    pub message: String,
    #[serde(default)]
    pub request_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClientError {
    Transport(String),
    Http {
        status: u16,
        body: Option<ApiErrorBody>,
    },
    Redirect {
        status: u16,
    },
    Redirected,
    InvalidMime {
        expected: &'static str,
        actual: Option<String>,
    },
    BodyTooLarge {
        limit: usize,
        actual: usize,
    },
    InvalidBody(String),
    UnsafePath,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    Keep,
    Reject,
    Undecided,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct DemoPhoto {
    pub photo_id: String,
    pub filename: String,
    pub decision: Decision,
    pub representation: Representation,
    pub thumbnail_url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Representation {
    Jpeg,
    Raw,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GalleryPage {
    pub gallery_id: String,
    pub items: Vec<DemoPhoto>,
    pub next_cursor: Option<String>,
    pub total: u64,
}

#[derive(Clone, Debug)]
pub struct DemoClient<T> {
    transport: T,
    max_json_bytes: usize,
    max_thumbnail_bytes: usize,
}

impl<T: Transport> DemoClient<T> {
    pub fn new(transport: T) -> Self {
        Self {
            transport,
            max_json_bytes: DEFAULT_MAX_JSON_BYTES,
            max_thumbnail_bytes: DEFAULT_MAX_THUMBNAIL_BYTES,
        }
    }

    pub fn with_limits(mut self, max_json_bytes: usize, max_thumbnail_bytes: usize) -> Self {
        self.max_json_bytes = max_json_bytes;
        self.max_thumbnail_bytes = max_thumbnail_bytes;
        self
    }

    pub async fn health(&self) -> Result<(), ClientError> {
        #[derive(Deserialize)]
        struct Health {
            status: String,
        }
        let health: Health = self.json("/api/v1/health").await?;
        if health.status == "ready" {
            Ok(())
        } else {
            Err(ClientError::InvalidBody("service is not ready".into()))
        }
    }

    pub async fn gallery(&self, gallery_id: &str) -> Result<GalleryPage, ClientError> {
        let path = format!(
            "{API_PREFIX}/galleries/{}/photos",
            encode_segment(gallery_id)
        );
        self.json(&path).await
    }

    pub async fn thumbnail(&self, photo: &DemoPhoto) -> Result<Vec<u8>, ClientError> {
        validate_api_path(&photo.thumbnail_url)?;
        let response = self
            .request(&photo.thumbnail_url, self.max_thumbnail_bytes)
            .await?;
        if response.body.len() > self.max_thumbnail_bytes {
            return Err(ClientError::BodyTooLarge {
                limit: self.max_thumbnail_bytes,
                actual: response.body.len(),
            });
        }
        if response.redirected {
            return Err(ClientError::Redirected);
        }
        if (300..400).contains(&response.status) {
            return Err(ClientError::Redirect {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(http_error(response));
        }
        if !mime_is(&response.content_type, "image/jpeg") {
            return Err(ClientError::InvalidMime {
                expected: "image/jpeg",
                actual: response.content_type,
            });
        }
        Ok(response.body)
    }

    async fn json<R: for<'de> Deserialize<'de>>(&self, path: &str) -> Result<R, ClientError> {
        validate_api_path(path)?;
        let response = self.request(path, self.max_json_bytes).await?;
        if response.body.len() > self.max_json_bytes {
            return Err(ClientError::BodyTooLarge {
                limit: self.max_json_bytes,
                actual: response.body.len(),
            });
        }
        if response.redirected {
            return Err(ClientError::Redirected);
        }
        if (300..400).contains(&response.status) {
            return Err(ClientError::Redirect {
                status: response.status,
            });
        }
        if !(200..300).contains(&response.status) {
            return Err(http_error(response));
        }
        if !mime_is(&response.content_type, "application/json") {
            return Err(ClientError::InvalidMime {
                expected: "application/json",
                actual: response.content_type,
            });
        }
        serde_json::from_slice(&response.body).map_err(|e| ClientError::InvalidBody(e.to_string()))
    }

    async fn request(
        &self,
        path: &str,
        max_response_bytes: usize,
    ) -> Result<Response, ClientError> {
        self.transport
            .get(path, max_response_bytes)
            .await
            .map_err(|e| ClientError::Transport(e.0))
    }
}

fn validate_api_path(path: &str) -> Result<(), ClientError> {
    if !path.starts_with("/api/v1/")
        || path.starts_with("//")
        || path.contains(['?', '#', '\\'])
        || path.contains("://")
        || path.split('/').any(|s| s == "..")
    {
        Err(ClientError::UnsafePath)
    } else {
        Ok(())
    }
}

fn mime_is(actual: &Option<String>, expected: &str) -> bool {
    actual
        .as_deref()
        .and_then(|v| v.split(';').next())
        .is_some_and(|v| v.trim().eq_ignore_ascii_case(expected))
}

fn http_error(response: Response) -> ClientError {
    let body = serde_json::from_slice::<ApiErrorBody>(&response.body).ok();
    ClientError::Http {
        status: response.status,
        body,
    }
}

fn encode_segment(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::RefCell,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };
    struct Fake {
        response: RefCell<Option<Result<Response, TransportError>>>,
        paths: RefCell<Vec<String>>,
    }
    impl Fake {
        fn new(response: Result<Response, TransportError>) -> Self {
            Self {
                response: RefCell::new(Some(response)),
                paths: RefCell::new(vec![]),
            }
        }
    }
    impl Transport for Fake {
        fn get<'a>(&'a self, path: &'a str, _: usize) -> RequestFuture<'a> {
            self.paths.borrow_mut().push(path.into());
            let r = self.response.borrow_mut().take().unwrap();
            Box::pin(async move { r })
        }
    }
    fn block_on<F: Future>(future: F) -> F::Output {
        unsafe fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        unsafe fn no_op(_: *const ()) {}
        fn raw() -> RawWaker {
            RawWaker::new(
                std::ptr::null(),
                &RawWakerVTable::new(clone, no_op, no_op, no_op),
            )
        }
        let waker = unsafe { Waker::from_raw(raw()) };
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("unexpected pending"),
        }
    }
    fn json(body: &str) -> Response {
        Response {
            status: 200,
            content_type: Some("application/json; charset=utf-8".into()),
            body: body.as_bytes().to_vec(),
            redirected: false,
        }
    }
    fn demo_photo() -> DemoPhoto {
        DemoPhoto {
            photo_id: "demo-001".into(),
            filename: "demo-001.jpg".into(),
            decision: Decision::Undecided,
            representation: Representation::Jpeg,
            thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
        }
    }

    #[test]
    fn health_and_gallery_match_demo_contract() {
        let fake = Fake::new(Ok(json(
            r#"{"gallery_id":"demo","items":[{"photo_id":"demo-001","filename":"demo-001.jpg","decision":"undecided","representation":"jpeg","thumbnail_url":"/api/v1/photos/demo-001/thumbnail/800"}],"next_cursor":null,"total":1}"#,
        )));
        let client = DemoClient::new(fake);
        let page = block_on(client.gallery("demo")).unwrap();
        assert_eq!(page.gallery_id, "demo");
        assert_eq!(page.items[0], demo_photo());
    }
    #[test]
    fn image_response_requires_mime_and_enforces_limit() {
        let fake = Fake::new(Ok(Response {
            status: 200,
            content_type: Some("image/jpeg".into()),
            body: vec![1; 5],
            redirected: false,
        }));
        let client = DemoClient::new(fake).with_limits(100, 4);
        assert!(matches!(
            block_on(client.thumbnail(&demo_photo())),
            Err(ClientError::BodyTooLarge { .. })
        ));
    }
    #[test]
    fn distinguishes_transport_status_mime_and_bad_json() {
        let t = DemoClient::new(Fake::new(Err(TransportError("offline".into()))));
        assert!(matches!(
            block_on(t.health()),
            Err(ClientError::Transport(_))
        ));
        let h = DemoClient::new(Fake::new(Ok(Response {
            status: 503,
            content_type: None,
            body: b"{}".to_vec(),
            redirected: false,
        })));
        assert!(matches!(
            block_on(h.health()),
            Err(ClientError::Http { status: 503, .. })
        ));
        let m = DemoClient::new(Fake::new(Ok(Response {
            status: 200,
            content_type: Some("text/plain".into()),
            body: b"{}".to_vec(),
            redirected: false,
        })));
        assert!(matches!(
            block_on(m.health()),
            Err(ClientError::InvalidMime { .. })
        ));
        let j = DemoClient::new(Fake::new(Ok(json("{"))));
        assert!(matches!(
            block_on(j.health()),
            Err(ClientError::InvalidBody(_))
        ));
    }
    #[test]
    fn redirects_and_unsafe_paths_are_rejected() {
        let c = DemoClient::new(Fake::new(Ok(Response {
            status: 302,
            content_type: None,
            body: vec![],
            redirected: false,
        })));
        assert!(matches!(
            block_on(c.thumbnail(&demo_photo())),
            Err(ClientError::Redirect { .. })
        ));
        assert_eq!(
            validate_api_path("https://evil.test/api/v1"),
            Err(ClientError::UnsafePath)
        );
        assert_eq!(
            validate_api_path("/api/v1/../../secret"),
            Err(ClientError::UnsafePath)
        );
        let followed = DemoClient::new(Fake::new(Ok(Response {
            status: 200,
            content_type: Some("image/jpeg".into()),
            body: vec![1],
            redirected: true,
        })));
        assert_eq!(
            block_on(followed.thumbnail(&demo_photo())),
            Err(ClientError::Redirected)
        );
    }
    #[test]
    fn checks_health_ready_state() {
        let ready = DemoClient::new(Fake::new(Ok(json(
            r#"{"status":"ready","resources":null,"request_id":"req-example"}"#,
        ))));
        assert!(block_on(ready.health()).is_ok());
    }
}
