//! WASM entry point. Application reads use the shared typed REST client and
//! gallery controller; the HTML bootstrap only initializes the generated WASM.

#[cfg(target_arch = "wasm32")]
#[derive(Clone, Default)]
struct BrowserTransport {
    bearer: std::sync::Arc<std::sync::Mutex<Option<String>>>,
    window: std::sync::Arc<std::sync::Mutex<Option<slint::Weak<crate::DemoWindow>>>>,
}

#[cfg(target_arch = "wasm32")]
#[derive(serde::Deserialize)]
struct PairSessionResponse {
    token: String,
    expires_at: u64,
}

#[cfg(target_arch = "wasm32")]
impl BrowserTransport {
    fn bearer_state(&self) -> std::sync::Arc<std::sync::Mutex<Option<String>>> {
        self.bearer.clone()
    }

    fn set_window(&self, window: slint::Weak<crate::DemoWindow>) {
        if let Ok(mut current) = self.window.lock() {
            *current = Some(window);
        }
    }

    fn handle_unauthorized(
        bearer: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
        window_state: &std::sync::Arc<std::sync::Mutex<Option<slint::Weak<crate::DemoWindow>>>>,
        status: u16,
    ) {
        if status != 401 {
            return;
        }
        if let Ok(mut bearer) = bearer.lock() {
            *bearer = None;
        }
        if let Ok(window_state) = window_state.lock()
            && let Some(window) = window_state.as_ref().and_then(slint::Weak::upgrade)
        {
            window.set_auth_required(true);
            window.set_auth_busy(false);
            window.set_auth_pairing_code("".into());
            window.set_auth_status(
                "La sesión caducó o fue revocada. Introduce un código nuevo.".into(),
            );
        }
    }

    async fn pair(&self, code: &str) -> Result<(), String> {
        let code = code.trim();
        if code.len() != 64 || !code.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Revisa el código temporal e inténtalo de nuevo.".into());
        }
        let response = super::demo_fetch::pair_session(code, 64 * 1024)
            .await
            .map_err(|_| "No se pudo conectar con el servicio local.".to_owned())?;
        if !(200..300).contains(&response.status) {
            return Err("El código expiró o ya fue utilizado. Solicita uno nuevo.".into());
        }
        let session: PairSessionResponse = serde_json::from_slice(&response.bytes)
            .map_err(|_| "El servicio devolvió una respuesta de sesión inválida.".to_owned())?;
        if session.token.len() != 64
            || !session.token.bytes().all(|byte| byte.is_ascii_hexdigit())
            || session.expires_at == 0
        {
            return Err("El servicio devolvió una respuesta de sesión inválida.".into());
        }
        let mut bearer = self
            .bearer
            .lock()
            .map_err(|_| "No se pudo guardar la sesión en memoria.".to_owned())?;
        *bearer = Some(session.token);
        Ok(())
    }

    fn current_bearer(
        state: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
    ) -> Result<String, photo_culler_client::demo::TransportError> {
        state
            .lock()
            .map_err(|_| photo_culler_client::demo::TransportError("Sesión no disponible".into()))?
            .clone()
            .ok_or_else(|| photo_culler_client::demo::TransportError("Empareja esta sesión".into()))
    }
}

#[cfg(target_arch = "wasm32")]
impl photo_culler_client::demo::Transport for BrowserTransport {
    fn get<'a>(
        &'a self,
        path: &'a str,
        max_response_bytes: usize,
    ) -> photo_culler_client::demo::RequestFuture<'a> {
        let bearer_state = self.bearer_state();
        let window_state = self.window.clone();
        Box::pin(async move {
            let bearer = Self::current_bearer(&bearer_state)?;
            let response = super::demo_fetch::fetch(path, &bearer, max_response_bytes)
                .await
                .map_err(|error| photo_culler_client::demo::TransportError(error.to_string()))?;
            Self::handle_unauthorized(&bearer_state, &window_state, response.status);
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

    fn put_json<'a>(
        &'a self,
        path: &'a str,
        body: Vec<u8>,
        max_response_bytes: usize,
    ) -> photo_culler_client::demo::RequestFuture<'a> {
        let bearer_state = self.bearer_state();
        let window_state = self.window.clone();
        Box::pin(async move {
            let bearer = Self::current_bearer(&bearer_state)?;
            let response = super::demo_fetch::put_json(path, body, &bearer, max_response_bytes)
                .await
                .map_err(|error| photo_culler_client::demo::TransportError(error.to_string()))?;
            Self::handle_unauthorized(&bearer_state, &window_state, response.status);
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

    fn post_json<'a>(
        &'a self,
        path: &'a str,
        body: Vec<u8>,
        max_response_bytes: usize,
    ) -> photo_culler_client::demo::RequestFuture<'a> {
        let bearer_state = self.bearer_state();
        let window_state = self.window.clone();
        Box::pin(async move {
            let bearer = Self::current_bearer(&bearer_state)?;
            let response = super::demo_fetch::post_json(path, body, &bearer, max_response_bytes)
                .await
                .map_err(|error| photo_culler_client::demo::TransportError(error.to_string()))?;
            Self::handle_unauthorized(&bearer_state, &window_state, response.status);
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
        analysis_flow,
        controller::DemoController,
        import_flow,
        rest::DemoRestClient,
        slint_view::{SlintDemoView, SlintDispatcher},
    };
    use crate::{
        recipe_flow::{self, RecipePanelController},
        recipe_rest::RecipeRestClient,
    };
    use photo_culler_client::demo::Decision;
    use slint::ComponentHandle;
    use std::sync::Arc;

    const INITIAL_GALLERY: &str = "active";

    let window = crate::DemoWindow::new()
        .map_err(|error| wasm_bindgen::JsValue::from_str(&error.to_string()))?;
    let view = Arc::new(SlintDemoView::new(window.as_weak(), true));
    let controller = Arc::new(DemoController::new(view.clone(), Arc::new(SlintDispatcher)));
    let browser_transport = BrowserTransport::default();
    browser_transport.set_window(window.as_weak());
    let client = Arc::new(DemoRestClient::new(browser_transport.clone()));
    let analysis_gallery_changed = analysis_flow::bind(&window, controller.clone(), client.clone());
    import_flow::bind(&window, controller.clone(), client.clone());
    recipe_flow::bind(
        &window,
        Arc::new(RecipePanelController::new(RecipeRestClient::new(
            browser_transport.clone(),
        ))),
    );
    window.set_auth_required(true);
    let auth_window = window.as_weak();
    let auth_transport = browser_transport.clone();
    let auth_controller = controller.clone();
    let auth_client = client.clone();
    window.on_auth_pair_requested(move |code| {
        let code = code.to_string();
        if let Some(window) = auth_window.upgrade() {
            window.set_auth_busy(true);
            window.set_auth_status("Validando el código temporal…".into());
        }
        let weak_window = auth_window.clone();
        let transport = auth_transport.clone();
        let controller = auth_controller.clone();
        let client = auth_client.clone();
        let _ = slint::spawn_local(async move {
            match transport.pair(&code).await {
                Ok(()) => {
                    if let Some(window) = weak_window.upgrade() {
                        window.set_auth_busy(false);
                        window.set_auth_required(false);
                        window.set_auth_pairing_code("".into());
                        window.set_auth_status("Sesión conectada.".into());
                    }
                    let ticket = controller.begin_load(INITIAL_GALLERY);
                    let _ = controller.execute(client.as_ref(), ticket).await;
                }
                Err(message) => {
                    if let Some(window) = weak_window.upgrade() {
                        window.set_auth_busy(false);
                        window.set_auth_status(message.into());
                    }
                }
            }
        });
    });

    let retry_controller = controller.clone();
    let retry_client = client.clone();
    window.on_retry(move || {
        let ticket = retry_controller.retry(INITIAL_GALLERY);
        let controller = retry_controller.clone();
        let client = retry_client.clone();
        let _ = slint::spawn_local(async move {
            let _ = controller.execute(client.as_ref(), ticket).await;
        });
    });
    let refresh_controller = controller.clone();
    let refresh_client = client.clone();
    window.on_refresh_gallery(move || {
        let Some(gallery_id) = refresh_controller.active_gallery_id() else {
            return;
        };
        let ticket = refresh_controller.begin_load(gallery_id);
        let controller = refresh_controller.clone();
        let client = refresh_client.clone();
        let _ = slint::spawn_local(async move {
            let _ = controller.execute(client.as_ref(), ticket).await;
        });
    });
    let previous_window = window.as_weak();
    let previous_controller = controller.clone();
    let previous_client = client.clone();
    window.on_previous_photo(move || {
        let controller = previous_controller.clone();
        let client = previous_client.clone();
        let weak_window = previous_window.clone();
        let _ = slint::spawn_local(async move {
            if let Err(error) = controller.previous_photo(client.as_ref()).await
                && let Some(window) = weak_window.upgrade()
            {
                window.set_navigation_error(error.message.into());
                window.set_service_status("No se pudo cargar la página; puedes reintentar.".into());
            }
        });
    });

    let next_window = window.as_weak();
    let next_controller = controller.clone();
    let next_client = client.clone();
    window.on_next_photo(move || {
        let controller = next_controller.clone();
        let client = next_client.clone();
        let weak_window = next_window.clone();
        let _ = slint::spawn_local(async move {
            if let Err(error) = controller.next_photo(client.as_ref()).await
                && let Some(window) = weak_window.upgrade()
            {
                window.set_navigation_error(error.message.into());
                window.set_service_status("No se pudo cargar la página; puedes reintentar.".into());
            }
        });
    });
    let selected_window = window.as_weak();
    let select_controller = controller.clone();
    window.on_select_photo(move |photo_id| {
        let _ = select_controller.select_photo(photo_id.as_str());
        if let Some(window) = selected_window.upgrade() {
            window.set_selected_photo_id(photo_id.clone());
            window.invoke_recipe_photo_selected(photo_id);
        }
    });
    let decision_controller = controller.clone();
    let decision_client = client.clone();
    let decision_view = view.clone();
    let decision_window = window.as_weak();
    let decision_busy = Arc::new(std::sync::Mutex::new(false));
    window.on_decision_requested(move |value| {
        let decision = match value.as_str() {
            "keep" => Decision::Keep,
            "alt" => Decision::Alt,
            "reject" => Decision::Reject,
            "undecided" => Decision::Undecided,
            _ => return,
        };
        let photo_ids = decision_view.selected_photo_ids();
        if photo_ids.is_empty() {
            return;
        }
        let Ok(mut pending) = decision_busy.lock() else {
            return;
        };
        if *pending {
            return;
        }
        *pending = true;
        drop(pending);
        if let Some(window) = decision_window.upgrade() {
            window.set_service_status("Guardando decisiones…".into());
        }
        let controller = decision_controller.clone();
        let client = decision_client.clone();
        let busy = decision_busy.clone();
        let weak = decision_window.clone();
        if slint::spawn_local(async move {
            let result = controller
                .update_decisions(client.as_ref(), &photo_ids, decision)
                .await;
            if let Ok(mut pending) = busy.lock() {
                *pending = false;
            }
            if let Some(window) = weak.upgrade() {
                match result {
                    Ok(()) => window.set_service_status("Decisiones guardadas".into()),
                    Err(error) => {
                        window.set_service_status(format!("No se guardó: {}", error.message).into())
                    }
                }
            }
        })
        .is_err()
        {
            if let Ok(mut pending) = decision_busy.lock() {
                *pending = false;
            }
            if let Some(window) = decision_window.upgrade() {
                window.set_service_status("No se pudo iniciar el guardado de decisiones.".into());
            }
        }
    });
    let archive_controller = controller.clone();
    let archive_client = client.clone();
    let archive_analysis_gallery_changed = analysis_gallery_changed.clone();
    window.on_select_archive(move |gallery_id| {
        archive_analysis_gallery_changed(gallery_id.to_string());
        let ticket = archive_controller.retry(gallery_id.to_string());
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
