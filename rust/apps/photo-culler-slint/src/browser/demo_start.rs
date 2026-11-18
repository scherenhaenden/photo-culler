//! WASM entry point and Rust-owned demo bootstrap.
//!
//! The generated Slint screen/controller are supplied by D11–D13. This module
//! defines the fetch sequence and its view-model payload so the controller can
//! consume health, gallery JSON, and JPEG bytes without JavaScript API calls.

use std::future::Future;
use std::pin::Pin;

#[cfg(target_arch = "wasm32")]
use super::demo_fetch;

// Native acceptance compiles this adapter without the WASM app package. These
// stubs preserve the D21 contract; production WASM builds use the sibling
// implementation directly.
#[cfg(not(target_arch = "wasm32"))]
mod demo_fetch {
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
        Network,
    }
    pub const HEALTH_PATH: &str = "/api/v1/health";
    pub const GALLERY_PATH: &str = "/api/v1/galleries/demo/photos";
    pub const THUMBNAIL_PATH: &str = "/api/v1/photos/demo-001/thumbnail/800";
    pub const MAX_JSON_BYTES: usize = 64 * 1024;
    pub const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;
}
use demo_fetch::{HttpResponse, TransportError};

pub const JSON_MIME: &str = "application/json";
pub const JPEG_MIME: &str = "image/jpeg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartupError {
    Transport(TransportError),
    HttpStatus {
        path: &'static str,
        status: u16,
    },
    UnexpectedMime {
        path: &'static str,
        actual: Option<String>,
    },
}

impl From<TransportError> for StartupError {
    fn from(value: TransportError) -> Self {
        Self::Transport(value)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoPayload {
    pub health_json: Vec<u8>,
    pub gallery_json: Vec<u8>,
    pub thumbnail_jpeg: Vec<u8>,
}

pub trait DemoFetcher {
    fn get<'a>(
        &'a self,
        path: &'static str,
        max_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, TransportError>> + 'a>>;
}

/// All application reads happen in Rust, in order: health, gallery, thumbnail.
/// The controller owns JSON DTO decoding and Slint view updates.
pub async fn load_demo<F: DemoFetcher>(fetcher: &F) -> Result<DemoPayload, StartupError> {
    let health = checked(
        fetcher,
        demo_fetch::HEALTH_PATH,
        demo_fetch::MAX_JSON_BYTES,
        JSON_MIME,
    )
    .await?;
    let gallery = checked(
        fetcher,
        demo_fetch::GALLERY_PATH,
        demo_fetch::MAX_JSON_BYTES,
        JSON_MIME,
    )
    .await?;
    let thumbnail = checked(
        fetcher,
        demo_fetch::THUMBNAIL_PATH,
        demo_fetch::MAX_THUMBNAIL_BYTES,
        JPEG_MIME,
    )
    .await?;
    Ok(DemoPayload {
        health_json: health.bytes,
        gallery_json: gallery.bytes,
        thumbnail_jpeg: thumbnail.bytes,
    })
}

async fn checked<F: DemoFetcher>(
    fetcher: &F,
    path: &'static str,
    cap: usize,
    mime: &str,
) -> Result<HttpResponse, StartupError> {
    let response = fetcher.get(path, cap).await?;
    if !response.is_success() {
        return Err(StartupError::HttpStatus {
            path,
            status: response.status,
        });
    }
    let actual = response
        .content_type
        .as_deref()
        .map(|value| value.split(';').next().unwrap_or(value).trim());
    if actual != Some(mime) {
        return Err(StartupError::UnexpectedMime {
            path,
            actual: response.content_type,
        });
    }
    Ok(response)
}

#[cfg(target_arch = "wasm32")]
struct BrowserFetcher;

#[cfg(target_arch = "wasm32")]
impl DemoFetcher for BrowserFetcher {
    fn get<'a>(
        &'a self,
        path: &'static str,
        _max_bytes: usize,
    ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, TransportError>> + 'a>> {
        Box::pin(demo_fetch::fetch(path))
    }
}

/// Slint's Rust WASM entry. Slint binds to the page's `canvas` element during
/// `run()`. `spawn_local` is used because browser futures are intentionally not
/// required to implement `Send`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use slint::ComponentHandle;

    let window = crate::DemoScaffold::new()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    slint::spawn_local(async move {
        match load_demo(&BrowserFetcher).await {
            Ok(payload) => {
                // D13 replaces this handoff with its shared controller/view model.
                web_sys::console::debug_1(
                    &format!(
                        "Photo Culler demo API ready (gallery {} bytes, thumbnail {} bytes)",
                        payload.gallery_json.len(),
                        payload.thumbnail_jpeg.len()
                    )
                    .into(),
                );
            }
            Err(error) => web_sys::console::error_1(
                &format!("Photo Culler demo API failed: {error:?}").into(),
            ),
        }
    });
    window
        .run()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    struct FakeFetcher {
        replies: RefCell<Vec<Result<HttpResponse, TransportError>>>,
        paths: RefCell<Vec<&'static str>>,
    }
    impl DemoFetcher for FakeFetcher {
        fn get<'a>(
            &'a self,
            path: &'static str,
            _max_bytes: usize,
        ) -> Pin<Box<dyn Future<Output = Result<HttpResponse, TransportError>> + 'a>> {
            self.paths.borrow_mut().push(path);
            let reply = self.replies.borrow_mut().remove(0);
            Box::pin(async move { reply })
        }
    }
    fn ok(mime: &str, bytes: &[u8]) -> Result<HttpResponse, TransportError> {
        Ok(HttpResponse {
            status: 200,
            content_type: Some(mime.into()),
            bytes: bytes.into(),
        })
    }
    fn run<F: Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};
        fn no_op(_: *const ()) {}
        fn clone(_: *const ()) -> RawWaker {
            RawWaker::new(std::ptr::null(), &VTABLE)
        }
        static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, no_op, no_op, no_op);
        let waker = unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) };
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("fake futures must be ready"),
        }
    }

    #[test]
    fn loads_health_gallery_and_thumbnail_through_the_rust_transport() {
        let fake = FakeFetcher {
            replies: RefCell::new(vec![
                ok(JSON_MIME, b"{\"status\":\"ready\"}"),
                ok(JSON_MIME, b"{\"gallery_id\":\"demo\"}"),
                ok(JPEG_MIME, &[0xff, 0xd8]),
            ]),
            paths: RefCell::new(Vec::new()),
        };
        let payload = run(load_demo(&fake)).unwrap();
        assert_eq!(
            &*fake.paths.borrow(),
            &[
                demo_fetch::HEALTH_PATH,
                demo_fetch::GALLERY_PATH,
                demo_fetch::THUMBNAIL_PATH
            ]
        );
        assert_eq!(payload.thumbnail_jpeg, [0xff, 0xd8]);
    }

    #[test]
    fn surfaces_http_and_mime_failures_to_the_rust_controller() {
        let http = FakeFetcher {
            replies: RefCell::new(vec![Ok(HttpResponse {
                status: 503,
                content_type: Some(JSON_MIME.into()),
                bytes: vec![],
            })]),
            paths: RefCell::new(Vec::new()),
        };
        assert_eq!(
            run(load_demo(&http)),
            Err(StartupError::HttpStatus {
                path: demo_fetch::HEALTH_PATH,
                status: 503
            })
        );
        let mime = FakeFetcher {
            replies: RefCell::new(vec![ok("text/plain", b"ready")]),
            paths: RefCell::new(Vec::new()),
        };
        assert!(matches!(
            run(load_demo(&mime)),
            Err(StartupError::UnexpectedMime { .. })
        ));
    }
}
