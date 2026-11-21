//! Typed, platform-neutral REST client contracts and presentation state.
//!
//! Platform transports implement [`Transport`]; this crate owns URL construction,
//! JSON decoding, and stale-response handling shared by native and browser clients.

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::fmt;

pub mod demo;

/// Minimal request port. Native HTTP and browser Fetch adapters can implement it.
pub trait Transport: Send + Sync {
    fn get(&self, path_and_query: &str) -> Result<String, TransportError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransportError {
    pub message: String,
}

impl TransportError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for TransportError {}

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
    InvalidResponse(String),
    Api(ApiErrorBody),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Transport(message) => write!(f, "transport: {message}"),
            Self::InvalidResponse(message) => write!(f, "invalid response: {message}"),
            Self::Api(error) => write!(f, "{}: {}", error.code, error.message),
        }
    }
}

impl std::error::Error for ClientError {}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionState {
    Keep,
    Reject,
    Undecided,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Representation {
    Jpeg,
    Raw,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct PhotoSummary {
    pub photo_id: String,
    pub filename: String,
    pub decision: DecisionState,
    pub representation: Representation,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct GalleryPage {
    pub gallery_id: String,
    pub items: Vec<PhotoSummary>,
    pub next_cursor: Option<String>,
    pub total: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GalleryRequest {
    pub gallery_id: String,
    pub cursor: Option<String>,
    pub limit: Option<u32>,
}

pub struct RestClient<T> {
    transport: T,
}

impl<T: Transport> RestClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn gallery_page(&self, request: &GalleryRequest) -> Result<GalleryPage, ClientError> {
        let mut path = format!(
            "/api/v1/galleries/{}/photos",
            percent_encode(&request.gallery_id)
        );
        let mut query = Vec::new();
        if let Some(cursor) = &request.cursor {
            query.push(format!("cursor={}", percent_encode(cursor)));
        }
        if let Some(limit) = request.limit {
            query.push(format!("limit={limit}"));
        }
        if !query.is_empty() {
            path.push('?');
            path.push_str(&query.join("&"));
        }
        self.get_json(&path)
    }

    fn get_json<R: DeserializeOwned>(&self, path: &str) -> Result<R, ClientError> {
        let body = self
            .transport
            .get(path)
            .map_err(|e| ClientError::Transport(e.message))?;
        serde_json::from_str(&body).map_err(|e| ClientError::InvalidResponse(e.to_string()))
    }
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use fmt::Write as _;
            let _ = write!(encoded, "%{byte:02X}");
        }
    }
    encoded
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadTicket {
    generation: u64,
    request: GalleryRequest,
}

impl LoadTicket {
    pub fn request(&self) -> &GalleryRequest {
        &self.request
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LibraryViewState {
    Empty,
    Loading {
        gallery_id: String,
    },
    Ready(GalleryPage),
    Error {
        gallery_id: String,
        message: String,
        retryable: bool,
    },
}

/// Shared presentation state. Call `begin_load` when the selected gallery changes;
/// only the newest ticket may publish its response.
#[derive(Clone, Debug)]
pub struct LibraryState {
    generation: u64,
    last_request: Option<GalleryRequest>,
    state: LibraryViewState,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            generation: 0,
            last_request: None,
            state: LibraryViewState::Empty,
        }
    }
}

impl LibraryState {
    pub fn state(&self) -> &LibraryViewState {
        &self.state
    }

    pub fn begin_load(&mut self, request: GalleryRequest) -> LoadTicket {
        self.generation = self.generation.wrapping_add(1);
        self.state = LibraryViewState::Loading {
            gallery_id: request.gallery_id.clone(),
        };
        self.last_request = Some(request.clone());
        LoadTicket {
            generation: self.generation,
            request,
        }
    }

    /// Returns false when an out-of-date response is discarded.
    pub fn complete(
        &mut self,
        ticket: LoadTicket,
        result: Result<GalleryPage, ClientError>,
    ) -> bool {
        if ticket.generation != self.generation
            || self.last_request.as_ref() != Some(&ticket.request)
        {
            return false;
        }
        self.state = match result {
            Ok(page) => LibraryViewState::Ready(page),
            Err(error) => LibraryViewState::Error {
                gallery_id: ticket.request.gallery_id,
                message: error.to_string(),
                retryable: true,
            },
        };
        true
    }

    pub fn retry(&mut self) -> Option<LoadTicket> {
        match &self.state {
            LibraryViewState::Error {
                retryable: true, ..
            } => self
                .last_request
                .clone()
                .map(|request| self.begin_load(request)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct FakeTransport {
        replies: Arc<Mutex<Vec<Result<String, TransportError>>>>,
        paths: Arc<Mutex<Vec<String>>>,
    }
    impl FakeTransport {
        fn with_replies(replies: Vec<Result<String, TransportError>>) -> Self {
            Self {
                replies: Arc::new(Mutex::new(replies)),
                paths: Arc::default(),
            }
        }
    }
    impl Transport for FakeTransport {
        fn get(&self, path: &str) -> Result<String, TransportError> {
            self.paths.lock().unwrap().push(path.to_owned());
            self.replies.lock().unwrap().remove(0)
        }
    }

    fn page(gallery: &str) -> GalleryPage {
        GalleryPage {
            gallery_id: gallery.into(),
            items: vec![],
            next_cursor: None,
            total: 0,
        }
    }
    fn json(gallery: &str) -> String {
        serde_json::to_string(&page(gallery)).unwrap()
    }
    fn request(id: &str) -> GalleryRequest {
        GalleryRequest {
            gallery_id: id.into(),
            cursor: None,
            limit: Some(50),
        }
    }

    #[test]
    fn typed_client_encodes_path_and_decodes_gallery_page() {
        let transport = FakeTransport::with_replies(vec![Ok(json("trip / 1"))]);
        let client = RestClient::new(transport.clone());
        let result = client
            .gallery_page(&GalleryRequest {
                gallery_id: "trip / 1".into(),
                cursor: Some("a+b".into()),
                limit: Some(25),
            })
            .unwrap();
        assert_eq!(result, page("trip / 1"));
        assert_eq!(
            transport.paths.lock().unwrap()[0],
            "/api/v1/galleries/trip%20%2F%201/photos?cursor=a%2Bb&limit=25"
        );
    }

    #[test]
    fn inverted_responses_discard_old_gallery_and_keep_newest() {
        let mut state = LibraryState::default();
        let old = state.begin_load(request("old"));
        let current = state.begin_load(request("current"));
        assert!(state.complete(current, Ok(page("current"))));
        assert!(!state.complete(old, Ok(page("old"))));
        assert_eq!(state.state(), &LibraryViewState::Ready(page("current")));
    }

    #[test]
    fn transport_error_is_recoverable_and_retry_succeeds() {
        let transport =
            FakeTransport::with_replies(vec![Err(TransportError::new("offline")), Ok(json("g1"))]);
        let client = RestClient::new(transport);
        let mut state = LibraryState::default();
        let first = state.begin_load(request("g1"));
        assert!(state.complete(first.clone(), client.gallery_page(first.request())));
        assert!(matches!(
            state.state(),
            LibraryViewState::Error {
                retryable: true,
                ..
            }
        ));
        let retry = state.retry().expect("error can be retried");
        assert!(state.complete(retry.clone(), client.gallery_page(retry.request())));
        assert_eq!(state.state(), &LibraryViewState::Ready(page("g1")));
    }

    #[test]
    fn invalid_json_is_reported_as_recoverable_view_error() {
        let transport = FakeTransport::with_replies(vec![Ok("{".into())]);
        let client = RestClient::new(transport);
        let mut state = LibraryState::default();
        let ticket = state.begin_load(request("g"));
        assert!(state.complete(ticket.clone(), client.gallery_page(ticket.request())));
        assert!(matches!(
            state.state(),
            LibraryViewState::Error {
                retryable: true,
                ..
            }
        ));
    }
}
