//! Load the larger read-only preview when focus inspection opens or retries.

use std::{cell::Cell, rc::Rc, sync::Arc};

use slint::ComponentHandle;

use super::{rest::DemoRestClient, thumbnail_image::decode_jpeg};
use crate::DemoWindow;
use photo_culler_client::demo::Transport;

pub fn bind<T: Transport + 'static>(window: &DemoWindow, client: Arc<DemoRestClient<T>>) {
    let generation = Rc::new(Cell::new(0_u64));
    let weak = window.as_weak();
    window.on_inspector_preview_requested(move |photo_id| {
        let request_id = photo_id.to_string();
        let next_generation = generation.get().wrapping_add(1);
        generation.set(next_generation);
        if let Some(window) = weak.upgrade() {
            window.set_inspector_preview_busy(true);
            window.set_inspector_preview_status("Cargando preview de foco…".into());
        }
        let client = client.clone();
        let weak = weak.clone();
        let error_window = weak.clone();
        let generation = generation.clone();
        if slint::spawn_local(async move {
            let result = client.large_preview(&request_id).await;
            if generation.get() != next_generation {
                return;
            }
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !window.get_inspector_open()
                || window.get_inspector_photo_id().as_str() != request_id
            {
                return;
            }
            window.set_inspector_preview_busy(false);
            match result {
                Ok(bytes) => match decode_jpeg(&bytes, "image/jpeg") {
                    Ok(preview) => {
                        window.set_inspector_thumbnail(preview);
                        window.set_inspector_preview_status(
                            "Preview Large listo · JPEG de origen".into(),
                        );
                    }
                    Err(_) => window.set_inspector_preview_status(
                        "El preview Large está dañado; se conserva el preview de galería. Reintenta.".into(),
                    ),
                },
                Err(_) => window.set_inspector_preview_status(
                    "No se pudo cargar el preview Large; se conserva el preview de galería. Reintenta.".into(),
                ),
            }
        })
        .is_err()
            && let Some(window) = error_window.upgrade()
        {
            window.set_inspector_preview_busy(false);
            window.set_inspector_preview_status(
                "No se pudo iniciar la carga del preview. Reintenta.".into(),
            );
        }
    });
}
