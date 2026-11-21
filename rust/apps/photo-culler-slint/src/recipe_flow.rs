//! Selection-aware state coordinator for the mounted recipe panel.
//!
//! Runtime adapters call `select_photo` when the gallery selection changes,
//! then await `load`. Save/undo/redo use the last server revision and suppress
//! stale completions after the user selects a different photo.

use crate::DemoWindow;
use crate::recipe_rest::{RecipeClientError, RecipeDocument, RecipeRestClient, RecipeValues};
use photo_culler_client::demo::Transport;
use slint::ComponentHandle;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecipeSelection {
    generation: u64,
    photo_id: String,
}

struct State {
    generation: u64,
    photo_id: String,
    document: Option<RecipeDocument>,
}

pub struct RecipePanelController<T> {
    client: RecipeRestClient<T>,
    state: Mutex<State>,
}

impl<T: Transport> RecipePanelController<T> {
    pub fn new(client: RecipeRestClient<T>) -> Self {
        Self {
            client,
            state: Mutex::new(State {
                generation: 0,
                photo_id: String::new(),
                document: None,
            }),
        }
    }

    /// Invalidate outstanding work and return the token for the new selection.
    pub fn select_photo(&self, photo_id: impl Into<String>) -> RecipeSelection {
        let photo_id = photo_id.into();
        let mut state = self.state.lock().expect("recipe controller mutex poisoned");
        state.generation = state.generation.wrapping_add(1);
        state.photo_id = photo_id.clone();
        state.document = None;
        RecipeSelection {
            generation: state.generation,
            photo_id,
        }
    }

    pub async fn load(
        &self,
        selection: &RecipeSelection,
    ) -> Result<Option<RecipeDocument>, RecipeClientError> {
        let document = self.client.load(&selection.photo_id).await?;
        let mut state = self.state.lock().expect("recipe controller mutex poisoned");
        if matches_selection(&state, selection) {
            state.document = Some(document.clone());
            Ok(Some(document))
        } else {
            Ok(None)
        }
    }

    pub fn document(&self, selection: &RecipeSelection) -> Option<RecipeDocument> {
        let state = self.state.lock().ok()?;
        matches_selection(&state, selection)
            .then(|| state.document.clone())
            .flatten()
    }

    pub fn is_current(&self, selection: &RecipeSelection) -> bool {
        self.state
            .lock()
            .ok()
            .is_some_and(|state| matches_selection(&state, selection))
    }

    pub async fn save(
        &self,
        selection: &RecipeSelection,
        recipe: &RecipeValues,
    ) -> Result<Option<RecipeDocument>, RecipeClientError> {
        let expected_revision = self.expected_revision(selection)?;
        let document = self
            .client
            .save(
                &selection.photo_id,
                expected_revision,
                recipe,
                &next_request_id(),
            )
            .await?;
        Ok(self.accept(selection, document))
    }

    pub async fn undo(
        &self,
        selection: &RecipeSelection,
    ) -> Result<Option<RecipeDocument>, RecipeClientError> {
        let expected_revision = self.expected_revision(selection)?;
        let document = self
            .client
            .undo(&selection.photo_id, expected_revision, &next_request_id())
            .await?;
        Ok(self.accept(selection, document))
    }

    pub async fn redo(
        &self,
        selection: &RecipeSelection,
    ) -> Result<Option<RecipeDocument>, RecipeClientError> {
        let expected_revision = self.expected_revision(selection)?;
        let document = self
            .client
            .redo(&selection.photo_id, expected_revision, &next_request_id())
            .await?;
        Ok(self.accept(selection, document))
    }

    /// Restore controls to the last confirmed server snapshot.
    pub fn revert(&self, selection: &RecipeSelection) -> Option<RecipeDocument> {
        self.document(selection)
    }

    fn expected_revision(&self, selection: &RecipeSelection) -> Result<u64, RecipeClientError> {
        let state = self.state.lock().map_err(|_| {
            RecipeClientError::Transport("recipe controller state is unavailable".into())
        })?;
        if !matches_selection(&state, selection) {
            return Err(RecipeClientError::Transport(
                "photo selection changed".into(),
            ));
        }
        state
            .document
            .as_ref()
            .map(|document| document.revision)
            .ok_or_else(|| RecipeClientError::Transport("recipe has not been loaded".into()))
    }

    fn accept(
        &self,
        selection: &RecipeSelection,
        document: RecipeDocument,
    ) -> Option<RecipeDocument> {
        let mut state = self.state.lock().ok()?;
        if !matches_selection(&state, selection) {
            return None;
        }
        state.document = Some(document.clone());
        Some(document)
    }
}

/// Bind the mounted `RecipeEditor` callbacks to the persistent recipe API.
/// Runtime entrypoints only need to construct the shared controller with their
/// existing platform transport and call this function once after creating the
/// window.
pub fn bind<T: Transport + 'static>(
    window: &DemoWindow,
    controller: SharedRecipePanelController<T>,
) {
    let selection = Arc::new(Mutex::new(None::<RecipeSelection>));

    let weak = window.as_weak();
    let load_controller = controller.clone();
    let load_selection = selection.clone();
    window.on_recipe_photo_selected(move |photo_id| {
        if let Some(window) = weak.upgrade() {
            start_load_for_photo(
                &window,
                &load_controller,
                &load_selection,
                photo_id.to_string(),
            );
        }
    });
    // The initial selected ID often equals the Slint default, so no property
    // change is emitted when the first catalog page arrives.
    start_load_for_photo(
        window,
        &controller,
        &selection,
        window.get_selected_photo_id().to_string(),
    );

    let weak = window.as_weak();
    let edit_selection = selection.clone();
    window.on_recipe_changed(move |_exposure, _temperature, _tint| {
        if let Some(window) = weak.upgrade()
            && edit_selection
                .lock()
                .ok()
                .and_then(|current| current.as_ref().cloned())
                .is_some()
        {
            window.set_recipe_dirty(true);
            window.set_recipe_status("Borrador local; guarda para persistir los cambios.".into());
        }
    });

    let weak = window.as_weak();
    let save_controller = controller.clone();
    let save_selection = selection.clone();
    window.on_recipe_save_requested(move || {
        let Some(window) = weak.upgrade() else { return; };
        let Some(ticket) = save_selection.lock().ok().and_then(|current| current.as_ref().cloned()) else {
            window.set_recipe_status("Selecciona una foto antes de guardar.".into());
            return;
        };
        let recipe = RecipeValues {
            schema_version: 1,
            exposure: f64::from(window.get_recipe_exposure()),
            temperature: f64::from(window.get_recipe_temperature()),
            tint: f64::from(window.get_recipe_tint()),
        };
        window.set_recipe_busy(true);
        window.set_recipe_status("Guardando receta…".into());
        let controller = save_controller.clone();
        let weak = window.as_weak();
        let _ = slint::spawn_local(async move {
            let result = controller.save(&ticket, &recipe).await;
            let Some(window) = weak.upgrade() else { return; };
            if !controller.is_current(&ticket) { return; }
            window.set_recipe_busy(false);
            match result {
                Ok(Some(document)) => publish_document(&window, &document, true, "Receta guardada"),
                Ok(None) => {},
                Err(RecipeClientError::Http { status: 409, .. }) => {
                    window.set_recipe_status("Conflicto de revisión; se conserva el borrador y se recarga la versión del servidor…".into());
                    match controller.load(&ticket).await {
                        Ok(Some(document)) => publish_document(&window, &document, false, "Conflicto: borrador conservado; revisa la nueva revisión antes de guardar"),
                        Ok(None) => {},
                        Err(error) => window.set_recipe_status(recipe_error_text(&error).into()),
                    }
                }
                Err(error) => window.set_recipe_status(recipe_error_text(&error).into()),
            }
        });
    });

    bind_history_action(window, &controller, &selection, true);
    bind_history_action(window, &controller, &selection, false);

    let weak = window.as_weak();
    let revert_controller = controller;
    let revert_selection = selection;
    window.on_recipe_revert_requested(move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(ticket) = revert_selection
            .lock()
            .ok()
            .and_then(|current| current.as_ref().cloned())
        else {
            return;
        };
        if let Some(document) = revert_controller.revert(&ticket) {
            publish_document(
                &window,
                &document,
                true,
                "Borrador descartado; se restaura la receta persistida",
            );
        }
    });
}

fn start_load_for_photo<T: Transport + 'static>(
    window: &DemoWindow,
    controller: &SharedRecipePanelController<T>,
    selection: &Arc<Mutex<Option<RecipeSelection>>>,
    photo_id: String,
) {
    let ticket = controller.select_photo(photo_id.clone());
    if let Ok(mut current) = selection.lock() {
        *current = Some(ticket.clone());
    }
    window.set_recipe_busy(true);
    window.set_recipe_dirty(false);
    window.set_recipe_status("Cargando receta persistida…".into());
    window.set_recipe_revision_label(format!("{photo_id} · cargando").into());
    let controller = controller.clone();
    let weak = window.as_weak();
    let _ = slint::spawn_local(async move {
        let result = controller.load(&ticket).await;
        let Some(window) = weak.upgrade() else {
            return;
        };
        if !controller.is_current(&ticket) {
            return;
        }
        window.set_recipe_busy(false);
        match result {
            Ok(Some(document)) => publish_document(&window, &document, true, "Receta cargada"),
            Ok(None) => {} // A newer photo selection owns the panel.
            Err(error) => window.set_recipe_status(recipe_error_text(&error).into()),
        }
    });
}

fn bind_history_action<T: Transport + 'static>(
    window: &DemoWindow,
    controller: &SharedRecipePanelController<T>,
    selection: &Arc<Mutex<Option<RecipeSelection>>>,
    undo: bool,
) {
    let weak = window.as_weak();
    let controller = controller.clone();
    let selection = selection.clone();
    let handler = move || {
        let Some(window) = weak.upgrade() else {
            return;
        };
        let Some(ticket) = selection
            .lock()
            .ok()
            .and_then(|current| current.as_ref().cloned())
        else {
            return;
        };
        window.set_recipe_busy(true);
        window.set_recipe_status(
            if undo {
                "Deshaciendo…"
            } else {
                "Rehaciendo…"
            }
            .into(),
        );
        let controller = controller.clone();
        let weak = window.as_weak();
        let _ = slint::spawn_local(async move {
            let result = if undo {
                controller.undo(&ticket).await
            } else {
                controller.redo(&ticket).await
            };
            if let Some(window) = weak.upgrade() {
                if !controller.is_current(&ticket) {
                    return;
                }
                window.set_recipe_busy(false);
                match result {
                    Ok(Some(document)) => publish_document(
                        &window,
                        &document,
                        true,
                        if undo {
                            "Cambio deshecho"
                        } else {
                            "Cambio rehecho"
                        },
                    ),
                    Ok(None) => {}
                    Err(error) => window.set_recipe_status(recipe_error_text(&error).into()),
                }
            }
        });
    };
    if undo {
        window.on_recipe_undo_requested(handler);
    } else {
        window.on_recipe_redo_requested(handler);
    }
}

fn publish_document(
    window: &DemoWindow,
    document: &RecipeDocument,
    replace_values: bool,
    status: &str,
) {
    if replace_values {
        window.set_recipe_exposure(document.current.exposure as f32);
        window.set_recipe_temperature(document.current.temperature as f32);
        window.set_recipe_tint(document.current.tint as f32);
        window.set_recipe_dirty(false);
    }
    window.set_recipe_revision_label(
        format!("{} · revisión {}", document.photo_id, document.revision).into(),
    );
    window.set_recipe_can_undo(document.can_undo);
    window.set_recipe_can_redo(document.can_redo);
    window.set_recipe_status(status.into());
}

fn recipe_error_text(error: &RecipeClientError) -> String {
    match error {
        RecipeClientError::Http { status: 404, .. } => {
            "No se encontró esta foto en el catálogo.".into()
        }
        RecipeClientError::Http { status: 409, .. } => {
            "La receta cambió en el servidor; vuelve a cargar la revisión.".into()
        }
        RecipeClientError::Http {
            status, message, ..
        } => message
            .clone()
            .unwrap_or_else(|| format!("El servicio rechazó la receta (HTTP {status}).")),
        RecipeClientError::Transport(message) => {
            format!("No se pudo conectar con el servicio: {message}")
        }
        _ => "La respuesta de recetas no es válida.".into(),
    }
}

fn matches_selection(state: &State, selection: &RecipeSelection) -> bool {
    state.generation == selection.generation && state.photo_id == selection.photo_id
}

fn next_request_id() -> String {
    format!(
        "recipe-panel-{}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

/// Share a controller between the platform event callback and async tasks.
pub type SharedRecipePanelController<T> = Arc<RecipePanelController<T>>;
