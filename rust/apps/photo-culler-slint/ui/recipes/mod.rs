//! Versioned, non-destructive recipe editor state and REST command contract.
//!
//! A Slint controller dispatches the returned commands through the shared REST
//! client and feeds responses back with `apply_response`. Originals are shown
//! read-only; all previews and exports are derivatives.

#[derive(Clone, Debug, PartialEq)]
pub struct Recipe {
    pub exposure_ev: f32,
    pub temperature_kelvin: i32,
    pub tint: f32,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            temperature_kelvin: 6_500,
            tint: 0.0,
        }
    }
}

impl Recipe {
    pub fn validate(&self) -> Result<(), EditorError> {
        if !self.exposure_ev.is_finite() || !(-5.0..=5.0).contains(&self.exposure_ev) {
            return Err(EditorError::InvalidExposure);
        }
        if !(2_000..=12_000).contains(&self.temperature_kelvin) {
            return Err(EditorError::InvalidTemperature);
        }
        if !self.tint.is_finite() || !(-100.0..=100.0).contains(&self.tint) {
            return Err(EditorError::InvalidTint);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EditDocument {
    pub revision: u64,
    pub recipe: Recipe,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ExportPlan {
    pub destination: String,
    pub expected_revision: u64,
    pub recipe: Recipe,
    pub filenames: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecipeCommand {
    RenderPreview {
        photo_id: String,
        expected_revision: u64,
        recipe: Recipe,
    },
    SaveRecipe {
        photo_id: String,
        expected_revision: u64,
        recipe: Recipe,
    },
    PreviewExport {
        photo_id: String,
        expected_revision: u64,
        recipe: Recipe,
        destination: String,
    },
    ExportDerived {
        photo_id: String,
        expected_revision: u64,
        recipe: Recipe,
        destination: String,
        filenames: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum RecipeResponse {
    PreviewReady(String),
    Saved(EditDocument),
    ExportPlanReady {
        destination: String,
        filenames: Vec<String>,
    },
    Exported {
        filenames: Vec<String>,
    },
    Conflict(EditDocument),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum EditorError {
    Busy,
    NoDraftChanges,
    InvalidExposure,
    InvalidTemperature,
    InvalidTint,
    EmptyDestination,
    NoExportPlan,
    DestinationNotConfirmed,
    StaleExportPlan,
    NoPendingRequest,
    UnexpectedResponse,
}

#[derive(Clone, Debug, PartialEq)]
enum PendingAction {
    Preview,
    Save {
        expected_revision: u64,
        recipe: Recipe,
    },
    PreviewExport {
        expected_revision: u64,
        recipe: Recipe,
        destination: String,
    },
    Export {
        plan: ExportPlan,
    },
}

/// View-model consumed by `recipes.slint`.
#[derive(Clone, Debug, PartialEq)]
pub struct RecipeEditor {
    pub photo_id: String,
    /// Read-only original preview URI and identity checksum.
    pub original_preview_uri: String,
    pub original_hash: String,
    /// Server-rendered derivative preview URI, when available.
    pub edited_preview_uri: Option<String>,
    pub confirmed: EditDocument,
    /// Local edit buffer. Discarding it never changes `confirmed`.
    pub draft: Option<Recipe>,
    pub export_plan: Option<ExportPlan>,
    pub confirmed_destination: Option<String>,
    pub last_exported_files: Vec<String>,
    pub status: String,
    pending: Option<PendingAction>,
    conflict_document: Option<EditDocument>,
}

impl RecipeEditor {
    pub fn new(
        photo_id: impl Into<String>,
        original_preview_uri: impl Into<String>,
        original_hash: impl Into<String>,
        confirmed: EditDocument,
    ) -> Self {
        Self {
            photo_id: photo_id.into(),
            original_preview_uri: original_preview_uri.into(),
            original_hash: original_hash.into(),
            edited_preview_uri: None,
            confirmed,
            draft: None,
            export_plan: None,
            confirmed_destination: None,
            last_exported_files: Vec::new(),
            status: "Receta cargada".into(),
            pending: None,
            conflict_document: None,
        }
    }

    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
    }

    pub fn begin_edit(&mut self) {
        self.draft
            .get_or_insert_with(|| self.confirmed.recipe.clone());
        self.status = "Editando borrador".into();
    }

    pub fn update_draft(&mut self, recipe: Recipe) -> Result<(), EditorError> {
        self.ensure_idle()?;
        recipe.validate()?;
        if self.draft.is_none() {
            self.begin_edit();
        }
        self.draft = Some(recipe);
        self.edited_preview_uri = None;
        self.clear_export_confirmation();
        Ok(())
    }

    /// Destination text edits invalidate any earlier plan and confirmation.
    pub fn destination_changed(&mut self, destination: &str) -> Result<(), EditorError> {
        self.ensure_idle()?;
        if self
            .export_plan
            .as_ref()
            .is_some_and(|plan| plan.destination != destination)
        {
            self.clear_export_confirmation();
            self.status = "El destino cambió; vuelve a previsualizar y confirmar".into();
        }
        Ok(())
    }

    /// Revert local controls to the last confirmed server recipe.
    pub fn cancel_edit(&mut self) -> Result<(), EditorError> {
        if self.pending.is_some() {
            return Err(EditorError::Busy);
        }
        self.draft = None;
        self.edited_preview_uri = None;
        self.clear_export_confirmation();
        self.status = "Cambios descartados; receta confirmada conservada".into();
        Ok(())
    }

    pub fn begin_preview(&mut self) -> Result<RecipeCommand, EditorError> {
        self.ensure_idle()?;
        let recipe = self.draft.as_ref().unwrap_or(&self.confirmed.recipe);
        recipe.validate()?;
        self.pending = Some(PendingAction::Preview);
        Ok(RecipeCommand::RenderPreview {
            photo_id: self.photo_id.clone(),
            expected_revision: self.confirmed.revision,
            recipe: recipe.clone(),
        })
    }

    pub fn begin_save(&mut self) -> Result<RecipeCommand, EditorError> {
        self.ensure_idle()?;
        let recipe = self.draft.as_ref().ok_or(EditorError::NoDraftChanges)?;
        recipe.validate()?;
        if recipe == &self.confirmed.recipe {
            return Err(EditorError::NoDraftChanges);
        }
        let expected_revision = self.confirmed.revision;
        self.pending = Some(PendingAction::Save {
            expected_revision,
            recipe: recipe.clone(),
        });
        Ok(RecipeCommand::SaveRecipe {
            photo_id: self.photo_id.clone(),
            expected_revision,
            recipe: recipe.clone(),
        })
    }

    pub fn begin_export_plan(
        &mut self,
        destination: impl Into<String>,
    ) -> Result<RecipeCommand, EditorError> {
        self.ensure_idle()?;
        let destination = destination.into();
        if destination.trim().is_empty() {
            return Err(EditorError::EmptyDestination);
        }
        let recipe = self
            .draft
            .as_ref()
            .unwrap_or(&self.confirmed.recipe)
            .clone();
        recipe.validate()?;
        self.clear_export_confirmation();
        self.pending = Some(PendingAction::PreviewExport {
            expected_revision: self.confirmed.revision,
            recipe: recipe.clone(),
            destination: destination.clone(),
        });
        Ok(RecipeCommand::PreviewExport {
            photo_id: self.photo_id.clone(),
            expected_revision: self.confirmed.revision,
            recipe: recipe.clone(),
            destination,
        })
    }

    /// Confirm the exact destination shown in the current export plan.
    pub fn confirm_destination(&mut self, destination: &str) -> Result<(), EditorError> {
        let plan = self.export_plan.as_ref().ok_or(EditorError::NoExportPlan)?;
        if plan.destination != destination {
            return Err(EditorError::DestinationNotConfirmed);
        }
        if plan.expected_revision != self.confirmed.revision
            || plan.recipe != *self.draft.as_ref().unwrap_or(&self.confirmed.recipe)
        {
            self.clear_export_confirmation();
            return Err(EditorError::StaleExportPlan);
        }
        self.confirmed_destination = Some(destination.to_owned());
        self.status = format!("Destino confirmado: {destination}");
        Ok(())
    }

    pub fn begin_export(&mut self) -> Result<RecipeCommand, EditorError> {
        self.ensure_idle()?;
        let plan = self.export_plan.clone().ok_or(EditorError::NoExportPlan)?;
        if self.confirmed_destination.as_deref() != Some(plan.destination.as_str()) {
            return Err(EditorError::DestinationNotConfirmed);
        }
        if plan.expected_revision != self.confirmed.revision
            || plan.recipe != *self.draft.as_ref().unwrap_or(&self.confirmed.recipe)
        {
            self.clear_export_confirmation();
            return Err(EditorError::StaleExportPlan);
        }
        self.pending = Some(PendingAction::Export { plan: plan.clone() });
        Ok(RecipeCommand::ExportDerived {
            photo_id: self.photo_id.clone(),
            expected_revision: plan.expected_revision,
            recipe: plan.recipe,
            destination: plan.destination,
            filenames: plan.filenames,
        })
    }

    /// Apply a response from the injected REST transport. The response kind must
    /// match the one outstanding request; a failed/conflicting write keeps the
    /// local draft intact for recovery or retry.
    pub fn apply_response(&mut self, response: RecipeResponse) -> Result<(), EditorError> {
        let pending = self.pending.take().ok_or(EditorError::NoPendingRequest)?;
        let outcome = match (pending.clone(), response) {
            (PendingAction::Preview, RecipeResponse::PreviewReady(uri)) => {
                self.edited_preview_uri = Some(uri);
                self.status = "Vista previa derivada actualizada".into();
                Ok(())
            }
            (
                PendingAction::Save {
                    expected_revision,
                    recipe,
                },
                RecipeResponse::Saved(document),
            ) => {
                if document.revision <= expected_revision || document.recipe != recipe {
                    self.pending = Some(pending);
                    return Err(EditorError::UnexpectedResponse);
                }
                self.confirmed = document;
                self.draft = None;
                self.edited_preview_uri = None;
                self.conflict_document = None;
                self.clear_export_confirmation();
                self.status = format!("Receta guardada · revisión {}", self.confirmed.revision);
                Ok(())
            }
            (
                PendingAction::PreviewExport {
                    expected_revision,
                    recipe,
                    destination,
                },
                RecipeResponse::ExportPlanReady {
                    destination: returned_destination,
                    filenames,
                },
            ) => {
                if returned_destination != destination {
                    self.pending = Some(pending);
                    return Err(EditorError::UnexpectedResponse);
                }
                self.export_plan = Some(ExportPlan {
                    destination,
                    expected_revision,
                    recipe,
                    filenames,
                });
                self.confirmed_destination = None;
                self.status = "Revisa los nombres y confirma el destino antes de exportar".into();
                Ok(())
            }
            (PendingAction::Export { plan }, RecipeResponse::Exported { filenames }) => {
                if filenames != plan.filenames {
                    self.pending = Some(pending);
                    return Err(EditorError::UnexpectedResponse);
                }
                self.last_exported_files = filenames;
                self.export_plan = None;
                self.confirmed_destination = None;
                self.status = "Derivados exportados".into();
                Ok(())
            }
            (PendingAction::Save { .. }, RecipeResponse::Conflict(document)) => {
                self.conflict_document = Some(document);
                self.status =
                    "La receta cambió en el servidor; conserva tu borrador o recarga".into();
                Ok(())
            }
            (_, RecipeResponse::Failed(message)) => {
                self.status = format!("Error REST: {message}");
                Ok(())
            }
            (other_pending, _) => {
                self.pending = Some(other_pending);
                Err(EditorError::UnexpectedResponse)
            }
        };
        outcome
    }

    pub fn conflict_document(&self) -> Option<&EditDocument> {
        self.conflict_document.as_ref()
    }

    /// Explicitly accept the server version after a conflict. The user's draft
    /// remains available; callers can then merge it or cancel it deliberately.
    pub fn reload_confirmed_conflict(&mut self) -> Result<(), EditorError> {
        if self.pending.is_some() {
            return Err(EditorError::Busy);
        }
        let document = self
            .conflict_document
            .take()
            .ok_or(EditorError::NoDraftChanges)?;
        self.confirmed = document;
        self.edited_preview_uri = None;
        self.clear_export_confirmation();
        self.status = format!(
            "Estado confirmado recargado · revisión {}",
            self.confirmed.revision
        );
        Ok(())
    }

    fn ensure_idle(&self) -> Result<(), EditorError> {
        if self.pending.is_some() {
            Err(EditorError::Busy)
        } else {
            Ok(())
        }
    }

    fn clear_export_confirmation(&mut self) {
        self.export_plan = None;
        self.confirmed_destination = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct RestSimulator {
        server_document: Option<EditDocument>,
        original_hash: String,
        original_write_count: usize,
        derived_files: Vec<String>,
    }

    impl RestSimulator {
        fn execute(&mut self, command: RecipeCommand) -> RecipeResponse {
            match command {
                RecipeCommand::RenderPreview { recipe, .. } => RecipeResponse::PreviewReady(
                    format!("/api/v1/derived-preview?exposure={:.1}", recipe.exposure_ev),
                ),
                RecipeCommand::SaveRecipe {
                    expected_revision,
                    recipe,
                    ..
                } => {
                    let document = self.server_document.as_mut().expect("test document");
                    if document.revision != expected_revision {
                        return RecipeResponse::Conflict(document.clone());
                    }
                    document.recipe = recipe;
                    document.revision += 1;
                    RecipeResponse::Saved(document.clone())
                }
                RecipeCommand::PreviewExport {
                    destination,
                    recipe,
                    expected_revision,
                    ..
                } => {
                    let suffix = if recipe.exposure_ev != 0.0 {
                        "edited"
                    } else {
                        "original-look"
                    };
                    RecipeResponse::ExportPlanReady {
                        destination,
                        filenames: vec![
                            format!("photo-{suffix}.jpg"),
                            format!("manifest-{expected_revision}.csv"),
                        ],
                    }
                }
                RecipeCommand::ExportDerived { filenames, .. } => {
                    self.derived_files = filenames.clone();
                    RecipeResponse::Exported { filenames }
                }
            }
        }
    }

    fn editor() -> (RecipeEditor, RestSimulator) {
        let document = EditDocument {
            revision: 7,
            recipe: Recipe::default(),
        };
        (
            RecipeEditor::new(
                "photo-1",
                "/api/v1/photos/photo-1/preview/original",
                "sha256:original-before",
                document.clone(),
            ),
            RestSimulator {
                server_document: Some(document),
                original_hash: "sha256:original-before".into(),
                ..RestSimulator::default()
            },
        )
    }

    #[test]
    fn cancelling_edit_discards_draft_and_preserves_confirmed_recipe() {
        let (mut editor, mut rest) = editor();
        let original = editor.confirmed.clone();
        let original_hash = rest.original_hash.clone();
        editor.begin_edit();
        let draft = Recipe {
            exposure_ev: 1.25,
            temperature_kelvin: 7_200,
            tint: -12.0,
        };
        editor.update_draft(draft).unwrap();
        let command = editor.begin_preview().unwrap();
        let response = rest.execute(command);
        editor.apply_response(response).unwrap();
        assert!(editor.edited_preview_uri.is_some());
        editor.cancel_edit().unwrap();
        assert_eq!(editor.confirmed, original);
        assert_eq!(editor.draft, None);
        assert_eq!(rest.server_document.as_ref().unwrap(), &original);
        assert_eq!(rest.original_hash, original_hash);
        assert_eq!(rest.original_write_count, 0);
    }

    #[test]
    fn save_updates_revision_and_suppresses_duplicate_pending_submission() {
        let (mut editor, mut rest) = editor();
        editor.begin_edit();
        editor
            .update_draft(Recipe {
                exposure_ev: 0.75,
                temperature_kelvin: 6_900,
                tint: 4.0,
            })
            .unwrap();
        let command = editor.begin_save().unwrap();
        assert_eq!(
            editor.begin_save(),
            Err(EditorError::Busy),
            "a repeated click while the REST write is pending must not send twice"
        );
        assert_eq!(
            editor.update_draft(Recipe {
                exposure_ev: -1.0,
                ..Recipe::default()
            }),
            Err(EditorError::Busy),
            "the draft cannot drift while a save is in flight"
        );
        let response = rest.execute(command);
        editor.apply_response(response).unwrap();
        assert_eq!(editor.confirmed.revision, 8);
        assert_eq!(editor.confirmed.recipe.exposure_ev, 0.75);
        assert!(editor.draft.is_none());
        assert_eq!(rest.server_document.as_ref().unwrap(), &editor.confirmed);
        assert_eq!(editor.begin_save(), Err(EditorError::NoDraftChanges));
    }

    #[test]
    fn export_requires_previewed_and_explicitly_confirmed_destination() {
        let (mut editor, mut rest) = editor();
        let original_hash = rest.original_hash.clone();
        editor.begin_edit();
        editor
            .update_draft(Recipe {
                exposure_ev: 0.5,
                temperature_kelvin: 6_800,
                tint: 3.0,
            })
            .unwrap();

        let command = editor.begin_export_plan("/derivatives/session-1").unwrap();
        editor.apply_response(rest.execute(command)).unwrap();
        assert_eq!(
            editor.begin_export(),
            Err(EditorError::DestinationNotConfirmed)
        );
        assert_eq!(
            editor.confirm_destination("/derivatives/other"),
            Err(EditorError::DestinationNotConfirmed)
        );
        editor
            .confirm_destination("/derivatives/session-1")
            .unwrap();
        editor.destination_changed("/derivatives/renamed").unwrap();
        assert!(editor.export_plan.is_none());
        assert!(editor.confirmed_destination.is_none());

        assert_eq!(editor.begin_export(), Err(EditorError::NoExportPlan));
        let command = editor.begin_export_plan("/derivatives/renamed").unwrap();
        editor.apply_response(rest.execute(command)).unwrap();
        editor.confirm_destination("/derivatives/renamed").unwrap();
        let command = editor.begin_export().unwrap();
        assert_eq!(editor.begin_export(), Err(EditorError::Busy));
        assert!(matches!(command, RecipeCommand::ExportDerived { .. }));
        editor.apply_response(rest.execute(command)).unwrap();

        assert_eq!(editor.last_exported_files, rest.derived_files);
        assert_eq!(rest.original_hash, original_hash);
        assert_eq!(rest.original_write_count, 0);
        assert!(editor.export_plan.is_none());
    }

    #[test]
    fn destination_plan_is_invalidated_when_draft_changes() {
        let (mut editor, mut rest) = editor();
        editor.begin_edit();
        editor
            .update_draft(Recipe {
                exposure_ev: 0.5,
                ..Recipe::default()
            })
            .unwrap();
        let command = editor.begin_export_plan("/derivatives/session-1").unwrap();
        editor.apply_response(rest.execute(command)).unwrap();
        editor
            .confirm_destination("/derivatives/session-1")
            .unwrap();

        editor
            .update_draft(Recipe {
                exposure_ev: 1.0,
                ..Recipe::default()
            })
            .unwrap();
        assert!(editor.export_plan.is_none());
        assert!(editor.confirmed_destination.is_none());
        assert_eq!(editor.begin_export(), Err(EditorError::NoExportPlan));
    }

    #[test]
    fn conflict_keeps_proposal_until_explicit_server_reload() {
        let (mut editor, mut rest) = editor();
        editor.begin_edit();
        let proposal = Recipe {
            exposure_ev: 1.0,
            ..Recipe::default()
        };
        editor.update_draft(proposal.clone()).unwrap();
        rest.server_document.as_mut().unwrap().revision += 1;
        rest.server_document.as_mut().unwrap().recipe.tint = 20.0;

        let command = editor.begin_save().unwrap();
        editor.apply_response(rest.execute(command)).unwrap();
        assert_eq!(editor.draft.as_ref(), Some(&proposal));
        assert_eq!(editor.confirmed.revision, 7);
        assert_eq!(editor.conflict_document().unwrap().revision, 8);

        editor.reload_confirmed_conflict().unwrap();
        assert_eq!(editor.confirmed.revision, 8);
        assert_eq!(editor.confirmed.recipe.tint, 20.0);
        assert_eq!(editor.draft.as_ref(), Some(&proposal));
        assert_eq!(rest.original_write_count, 0);
    }
}
