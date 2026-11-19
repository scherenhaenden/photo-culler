//! Integration contract for the shared controller and its generated Slint view.
//!
//! The controller flow runs against a recording view and the production JPEG
//! decoder; source-contract assertions pin its Slint bindings and shell. The
//! test does not instantiate Winit's platform window, which must be created on
//! the macOS main thread. Hosted GUI/browser smoke tests own pixel rendering.

use super::*;
use crate::demo::thumbnail_image::decode_jpeg;
use image::{ColorType, codecs::jpeg::JpegEncoder};
use std::{
    collections::VecDeque,
    future::Future,
    sync::{Arc, Mutex},
    task::{Context, Poll, Wake, Waker},
};

const SLINT_VIEW_SOURCE: &str = include_str!("slint_view.rs");
const DEMO_WINDOW_SOURCE: &str = include_str!("../../ui/demo_window.slint");
const GALLERY_SOURCE: &str = include_str!("../../ui/library/gallery.slint");

#[derive(Default)]
struct RecordingView {
    states: Mutex<Vec<ViewState>>,
}

impl DemoView for RecordingView {
    fn set_state(&self, state: ViewState) {
        self.states
            .lock()
            .expect("view state lock poisoned")
            .push(state);
    }
}

struct ImmediateDispatcher;

impl UiDispatcher for ImmediateDispatcher {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError> {
        // Controller updates are dispatched on this test thread, never a
        // worker; the production adapter still uses Slint's event loop.
        task();
        Ok(())
    }
}

struct RetryClient {
    galleries: Mutex<VecDeque<Result<DemoGallery, DemoError>>>,
    thumbnails: Mutex<VecDeque<Result<Vec<u8>, DemoError>>>,
}

impl DemoClient for RetryClient {
    fn gallery<'a>(&'a self, _: &'a str) -> DemoFuture<'a> {
        let result = self
            .galleries
            .lock()
            .expect("gallery queue poisoned")
            .pop_front()
            .expect("unexpected gallery request");
        Box::pin(async move { result })
    }

    fn thumbnail<'a>(&'a self, _: &'a DemoPhoto) -> ThumbnailFuture<'a> {
        let result = self
            .thumbnails
            .lock()
            .expect("thumbnail queue poisoned")
            .pop_front()
            .expect("unexpected thumbnail request");
        Box::pin(async move { result })
    }
}

struct NoopWake;

impl Wake for NoopWake {
    fn wake(self: Arc<Self>) {}
}

fn block_on_ready<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(NoopWake));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    match future.as_mut().poll(&mut context) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("fake REST client must resolve immediately"),
    }
}

fn fixture_photo() -> DemoPhoto {
    DemoPhoto {
        photo_id: "demo-001".into(),
        filename: "demo-001.jpg".into(),
        decision: "keep".into(),
        thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
    }
}

fn valid_jpeg() -> Vec<u8> {
    let mut bytes = Vec::new();
    JpegEncoder::new(&mut bytes)
        .encode(&[255, 0, 0, 0, 255, 0], 2, 1, ColorType::Rgb8.into())
        .expect("fixture JPEG should encode");
    bytes
}

#[test]
fn api_error_retry_ready_decodes_thumbnail_and_keeps_demo_shell() {
    let view = Arc::new(RecordingView::default());
    let controller = DemoController::new(view.clone(), Arc::new(ImmediateDispatcher));
    let thumbnail = valid_jpeg();
    let client = RetryClient {
        galleries: Mutex::new(VecDeque::from([
            Err(DemoError {
                message: "API unavailable".into(),
            }),
            Ok(DemoGallery {
                gallery_id: "demo".into(),
                photos: vec![fixture_photo()],
            }),
        ])),
        thumbnails: Mutex::new(VecDeque::from([Ok(thumbnail.clone())])),
    };

    let first = controller.begin_load("demo");
    assert_eq!(
        controller.snapshot(),
        ViewState::Loading,
        "the controller must expose Loading before the request completes"
    );

    block_on_ready(controller.execute(&client, first)).unwrap();
    assert_eq!(
        controller.snapshot(),
        ViewState::Error {
            message: "API unavailable".into()
        }
    );
    assert!(matches!(
        view.states.lock().unwrap().as_slice(),
        [ViewState::Loading, ViewState::Error { message }]
            if message == "API unavailable"
    ));

    let retry = controller.retry("demo");
    assert_eq!(controller.snapshot(), ViewState::Loading);

    block_on_ready(controller.execute(&client, retry)).unwrap();
    assert_eq!(
        controller.snapshot(),
        ViewState::Ready {
            gallery_id: "demo".into(),
            photo: fixture_photo(),
            jpeg: Some(thumbnail.clone()),
            preview_error: None,
        }
    );
    assert!(matches!(
        view.states.lock().unwrap().as_slice(),
        [ViewState::Loading, ViewState::Error { message }, ViewState::Loading,
         ViewState::Ready { photo, jpeg: Some(jpeg), preview_error: None, .. }]
            if message == "API unavailable"
                && photo.filename == "demo-001.jpg"
                && jpeg == &thumbnail
    ));

    let decoded = decode_jpeg(&thumbnail, "image/jpeg").expect("ready response should decode");
    assert_eq!(decoded.size().width, 2);
    assert_eq!(decoded.size().height, 1);
}

#[test]
fn slint_binding_contract_keeps_loading_error_retry_and_gallery_in_one_shell() {
    assert!(SLINT_VIEW_SOURCE.contains("ViewState::Loading =>"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_view_state(0);"));
    assert!(SLINT_VIEW_SOURCE.contains("ViewState::Error { message } =>"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_error_message(message.into());"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_view_state(3);"));
    assert!(SLINT_VIEW_SOURCE.contains("match decode_jpeg(&bytes, \"image/jpeg\")"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_photo_thumbnail(image.clone());"));
    assert!(SLINT_VIEW_SOURCE.contains("thumbnail: image,"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_view_state(1);"));

    assert!(DEMO_WINDOW_SOURCE.contains("export component DemoWindow inherits Window"));
    assert!(DEMO_WINDOW_SOURCE.contains("title: \"Photo Culler\";"));
    assert!(DEMO_WINDOW_SOURCE.contains("in property <string> gallery-title:"));
    assert!(DEMO_WINDOW_SOURCE.contains("in property <string> service-status:"));
    assert!(DEMO_WINDOW_SOURCE.contains("root.gallery-title"));
    assert!(DEMO_WINDOW_SOURCE.contains("root.service-status"));
    assert!(DEMO_WINDOW_SOURCE.contains("callback select-photo(string);"));
    assert!(DEMO_WINDOW_SOURCE.contains("if root.view-state == 0: StatusPanel"));
    assert!(DEMO_WINDOW_SOURCE.contains("if root.view-state == 2: StatusPanel"));
    assert!(DEMO_WINDOW_SOURCE.contains("if root.view-state == 3: StatusPanel"));
    assert!(DEMO_WINDOW_SOURCE.contains("action-label: \"Reintentar\";"));
    assert!(DEMO_WINDOW_SOURCE.contains("retry => { root.retry(); }"));
    assert!(DEMO_WINDOW_SOURCE.contains("if root.view-state == 1: LibraryGallery"));
    assert!(DEMO_WINDOW_SOURCE.contains("fallback-thumbnail: root.photo-thumbnail;"));
    assert!(GALLERY_SOURCE.contains("source: root.photo.thumbnail;"));
}
