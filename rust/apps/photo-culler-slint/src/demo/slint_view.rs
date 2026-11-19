//! Bind the platform-neutral controller states to the shared Slint window.

use crate::demo::{
    controller::{DemoError, DemoView, UiDispatcher, ViewState},
    thumbnail_image::decode_jpeg,
};
use slint::ComponentHandle;

pub struct SlintDemoView {
    window: slint::Weak<crate::DemoWindow>,
    web_layout: bool,
}

impl SlintDemoView {
    pub fn new(window: slint::Weak<crate::DemoWindow>, web_layout: bool) -> Self {
        if let Some(window) = window.upgrade() {
            window
                .global::<crate::DesignTokens>()
                .set_web_layout(web_layout);
            window.set_web_layout(web_layout);
        }
        Self { window, web_layout }
    }
}

impl DemoView for SlintDemoView {
    fn set_state(&self, state: ViewState) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        match state {
            ViewState::Loading => {
                window.set_error_message("Connecting to the local Photo Culler service.".into());
                window.set_service_status("Conectando con el servicio local…".into());
                window.set_view_state(0);
                set_browser_status("loading", "Loading gallery");
            }
            ViewState::Empty => {
                window.set_photo_count(0);
                window.set_photo_index(0);
                window.set_navigation_label("0 fotos".into());
                window.set_service_status("Galería vacía".into());
                window.set_view_state(2);
                set_browser_status("empty", "The selected gallery contains no photos.");
            }
            ViewState::Error { message } => {
                set_browser_status("error", &message);
                window.set_service_status("Servicio no disponible".into());
                window.set_error_message(message.into());
                window.set_view_state(3);
            }
            ViewState::Ready {
                photo,
                jpeg,
                preview_error,
                ..
            } => {
                let filename = photo.filename;
                let photo_id = photo.photo_id;
                let decision = photo.decision;
                window.set_photo_name(filename.clone().into());
                window.set_gallery_title("Sesión de muestra".into());
                window.set_fallback_photo_id(photo_id.clone().into());
                window.set_selected_photo_id(photo_id.clone().into());
                window.set_photo_index(0);
                window.set_photo_count(1);
                window.set_navigation_label("Foto 1 de 1".into());
                window.set_service_status("Servicio local listo".into());
                if let Some(bytes) = jpeg {
                    match decode_jpeg(&bytes, "image/jpeg") {
                        Ok(image) => {
                            window.set_photo_thumbnail(image.clone());
                            let badge = match decision.as_str() {
                                "keep" if self.web_layout => "KEEP",
                                "keep" => "BEST",
                                "reject" => "REJECTED",
                                _ => "UNRATED",
                            };
                            window.set_gallery_photos(slint::ModelRc::new(slint::VecModel::from(
                                vec![crate::GalleryPhoto {
                                    id: photo_id.into(),
                                    filename: filename.clone().into(),
                                    decision: badge.into(),
                                    quality: "—".into(),
                                    score: "—".into(),
                                    thumbnail: image,
                                    rating: 0,
                                    iso: 0,
                                    is_raw: false,
                                }],
                            )));
                            window.set_kept_count(if decision == "keep" { 1 } else { 0 });
                            window.set_alternate_count(if decision == "alternate" { 1 } else { 0 });
                            window.set_rejected_count(if decision == "reject" { 1 } else { 0 });
                            window.set_unrated_count(if decision == "undecided" { 1 } else { 0 });
                            window.set_error_message("".into());
                            window.set_view_state(1);
                            set_browser_status("ready", &filename);
                        }
                        Err(error) => {
                            let message = format!("Preview unavailable: {error:?}");
                            set_browser_status("error", &message);
                            window.set_error_message(message.into());
                            window.set_view_state(3);
                        }
                    }
                } else {
                    let message =
                        preview_error.unwrap_or_else(|| "The photo preview is unavailable.".into());
                    set_browser_status("error", &message);
                    window.set_service_status("Vista previa no disponible".into());
                    window.set_error_message(message.into());
                    window.set_view_state(3);
                }
            }
        }
    }
}

#[cfg(target_arch = "wasm32")]
fn set_browser_status(state: &str, message: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(root) = document.document_element() {
        let _ = root.set_attribute("data-photo-culler-state", state);
    }
    if let Some(status) = document.get_element_by_id("application-status") {
        status.set_text_content(Some(message));
        let _ = status.set_attribute("data-state", state);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn set_browser_status(_: &str, _: &str) {}

pub struct SlintDispatcher;

impl UiDispatcher for SlintDispatcher {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError> {
        slint::invoke_from_event_loop(move || task()).map_err(|error| DemoError {
            message: format!("Could not update the Photo Culler window: {error}"),
        })
    }
}
