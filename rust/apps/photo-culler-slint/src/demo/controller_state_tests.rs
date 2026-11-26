//! Exercise the shared controller's user-visible error and retry flow.

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
struct RecordingView(Mutex<Vec<ViewState>>);

impl DemoView for RecordingView {
    fn set_state(&self, state: ViewState) {
        self.0.lock().unwrap().push(state);
    }
}

struct ImmediateDispatcher;

impl UiDispatcher for ImmediateDispatcher {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError> {
        task();
        Ok(())
    }
}

struct RetryClient {
    galleries: Mutex<VecDeque<Result<DemoGallery, DemoError>>>,
    thumbnails: Mutex<VecDeque<Result<Vec<u8>, DemoError>>>,
}

impl DemoClient for RetryClient {
    fn gallery<'a>(
        &'a self,
        _: &'a str,
        _: Option<&'a CatalogCursor>,
        _: Option<&'a str>,
    ) -> DemoFuture<'a> {
        let result = self.galleries.lock().unwrap().pop_front().unwrap();
        Box::pin(async move { result })
    }

    fn summary<'a>(&'a self, _: &'a str) -> SummaryFuture<'a> {
        Box::pin(async {
            Ok(photo_culler_client::demo::CatalogSummary {
                total_photos: 1,
                total_files: 1,
                selected_count: 0,
                pending_count: 1,
                decisions: Default::default(),
                quality_tiers: Default::default(),
            })
        })
    }

    fn thumbnail<'a>(&'a self, _: &'a DemoPhoto) -> ThumbnailFuture<'a> {
        let result = self.thumbnails.lock().unwrap().pop_front().unwrap();
        Box::pin(async move { result })
    }

    fn update_decision<'a>(
        &'a self,
        _: &'a DemoPhoto,
        _: Decision,
        _: &'a str,
    ) -> DecisionFuture<'a> {
        Box::pin(async { Ok(1) })
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
        Poll::Pending => panic!("fake client must resolve immediately"),
    }
}

fn fixture_photo() -> DemoPhoto {
    DemoPhoto {
        photo_id: "demo-001".into(),
        filename: "demo-001.jpg".into(),
        decision: "undecided".into(),
        thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
        decision_revision: 0,
    }
}

fn valid_jpeg() -> Vec<u8> {
    let mut bytes = Vec::new();
    JpegEncoder::new(&mut bytes)
        .encode(&[255, 0, 0, 0, 255, 0], 2, 1, ColorType::Rgb8.into())
        .unwrap();
    bytes
}

#[test]
fn api_error_can_be_retried_into_a_decoded_gallery() {
    let view = Arc::new(RecordingView::default());
    let controller = DemoController::new(view.clone(), Arc::new(ImmediateDispatcher));
    let jpeg = valid_jpeg();
    let client = RetryClient {
        galleries: Mutex::new(VecDeque::from([
            Err(DemoError {
                message: "API unavailable".into(),
            }),
            Ok(DemoGallery {
                gallery_id: "demo".into(),
                photos: vec![fixture_photo()],
                next_cursor: None,
            }),
        ])),
        thumbnails: Mutex::new(VecDeque::from([Ok(jpeg.clone())])),
    };

    let first = controller.begin_load("demo");
    assert_eq!(controller.snapshot(), ViewState::Loading);
    block_on_ready(controller.execute(&client, first)).unwrap();
    assert_eq!(
        controller.snapshot(),
        ViewState::Error {
            message: "API unavailable".into()
        }
    );

    let retry = controller.retry("demo");
    assert_eq!(controller.snapshot(), ViewState::Loading);
    block_on_ready(controller.execute(&client, retry)).unwrap();
    assert!(matches!(
        controller.snapshot(),
        ViewState::Ready { photos, selected_index: 0, .. }
            if photos[0].photo.filename == "demo-001.jpg"
                && photos[0].jpeg.as_ref() == Some(&jpeg)
                && photos[0].preview_error.is_none()
    ));

    let decoded = decode_jpeg(&jpeg, "image/jpeg").unwrap();
    assert_eq!(decoded.size().width, 2);
    assert_eq!(decoded.size().height, 1);
    assert!(matches!(
        view.0.lock().unwrap().as_slice(),
        [ViewState::Loading, ViewState::Error { message }, ViewState::Loading,
         ViewState::Ready { photos, .. }]
            if message == "API unavailable" && photos[0].photo.filename == "demo-001.jpg"
    ));
}

#[test]
fn slint_binding_contract_keeps_loading_error_retry_and_gallery_in_one_shell() {
    assert!(SLINT_VIEW_SOURCE.contains("ViewState::Loading =>"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_view_state(0);"));
    assert!(SLINT_VIEW_SOURCE.contains("ViewState::Error { message } =>"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_error_message(message.into());"));
    assert!(SLINT_VIEW_SOURCE.contains("window.set_view_state(3);"));
    assert!(SLINT_VIEW_SOURCE.contains("decode_jpeg(bytes, \"image/jpeg\").ok()"));
    assert!(
        SLINT_VIEW_SOURCE
            .contains("window.set_photo_thumbnail(preview.clone().unwrap_or_default());")
    );
    assert!(SLINT_VIEW_SOURCE.contains("window.set_view_state(1);"));

    assert!(DEMO_WINDOW_SOURCE.contains("export component DemoWindow inherits Window"));
    assert!(DEMO_WINDOW_SOURCE.contains("title: \"Photo Culler Pro\";"));
    assert!(DEMO_WINDOW_SOURCE.contains("in property <string> gallery-title:"));
    assert!(DEMO_WINDOW_SOURCE.contains("in property <string> service-status:"));
    assert!(DEMO_WINDOW_SOURCE.contains("callback select-photo(string);"));
    assert!(DEMO_WINDOW_SOURCE.contains("view-state: root.view-state;"));
    assert!(DEMO_WINDOW_SOURCE.contains("photos: root.gallery-photos;"));
    assert!(DEMO_WINDOW_SOURCE.contains("retry => { root.retry(); }"));
    assert!(DEMO_WINDOW_SOURCE.contains("fallback-thumbnail: root.photo-thumbnail;"));
    assert!(GALLERY_SOURCE.contains("if root.view-state == 3: StatusPanel"));
    assert!(GALLERY_SOURCE.contains("action-label: \"Reintentar\";"));
    assert!(GALLERY_SOURCE.contains("source: root.photo.thumbnail;"));
}
