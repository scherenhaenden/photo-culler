//! WASM entry point. Application reads use the shared typed REST client and
//! gallery controller; the HTML bootstrap only initializes the generated WASM.

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy, Default)]
struct BrowserTransport;

#[cfg(target_arch = "wasm32")]
impl photo_culler_client::demo::Transport for BrowserTransport {
    fn get<'a>(
        &'a self,
        path: &'a str,
        max_response_bytes: usize,
    ) -> photo_culler_client::demo::RequestFuture<'a> {
        Box::pin(async move {
            let response = super::demo_fetch::fetch(path, max_response_bytes)
                .await
                .map_err(|error| photo_culler_client::demo::TransportError(error.to_string()))?;
            if response.bytes.len() > max_response_bytes {
                return Err(photo_culler_client::demo::TransportError(format!(
                    "response exceeds the {max_response_bytes} byte limit"
                )));
            }
            Ok(photo_culler_client::demo::Response {
                status: response.status,
                content_type: response.content_type,
                body: response.bytes,
                redirected: false,
            })
        })
    }
}

/// Slint's Rust WASM entry. Slint binds to the page's `canvas` element during
/// `run()`. `spawn_local` is used because browser futures are intentionally not
/// required to implement `Send`.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() -> Result<(), wasm_bindgen::JsValue> {
    use crate::demo::{
        controller::DemoController,
        rest::DemoRestClient,
        slint_view::{SlintDemoView, SlintDispatcher},
    };
    use slint::ComponentHandle;
    use std::sync::Arc;

    let window = crate::DemoWindow::new()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    let controller = Arc::new(DemoController::new(
        Arc::new(SlintDemoView::new(window.as_weak(), true)),
        Arc::new(SlintDispatcher),
    ));
    let client = Arc::new(DemoRestClient::new(BrowserTransport));
    let ticket = controller.begin_load("demo");
    let load_controller = controller.clone();
    let load_client = client.clone();
    slint::spawn_local(async move {
        let _ = load_controller.execute(load_client.as_ref(), ticket).await;
    })
    .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;

    let retry_controller = controller.clone();
    let retry_client = client.clone();
    window.on_retry(move || {
        let ticket = retry_controller.retry("demo");
        let controller = retry_controller.clone();
        let client = retry_client.clone();
        let _ = slint::spawn_local(async move {
            let _ = controller.execute(client.as_ref(), ticket).await;
        });
    });
    window.on_previous_photo(|| {});
    window.on_next_photo(|| {});
    let selected_window = window.as_weak();
    window.on_select_photo(move |photo_id| {
        if let Some(window) = selected_window.upgrade() {
            window.set_selected_photo_id(photo_id);
        }
    });
    let archive_controller = controller.clone();
    let archive_client = client.clone();
    window.on_select_archive(move |_| {
        let ticket = archive_controller.retry("demo");
        let controller = archive_controller.clone();
        let client = archive_client.clone();
        let _ = slint::spawn_local(async move {
            let _ = controller.execute(client.as_ref(), ticket).await;
        });
    });
    window
        .run()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))
}
