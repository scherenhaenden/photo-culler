//! SQLite-backed bridge from recipe HTTP operations to the versioned domain service.
//!
//! RecipeService is synchronous by contract, so HTTP handlers dispatch its
//! operations to Tokio's blocking pool. The SQLite store is shared with the
//! running catalog and uses compare-and-swap revisions for every write.

use sqlx::SqlitePool;
use tokio::runtime::Handle;

use crate::{
    editing::recipes::{Recipe, RecipeError, RecipeService, SqliteRecipeStore},
    http::recipes::{RecipeApi, RecipeApiError, RecipeApiFuture, RecipeValues, RecipeView},
};

#[derive(Clone)]
pub struct SqliteRecipeApi {
    writer: SqlitePool,
    store: SqliteRecipeStore,
}

impl SqliteRecipeApi {
    pub fn new(writer: SqlitePool, runtime: Handle) -> Self {
        let store = SqliteRecipeStore::new(writer.clone(), runtime);
        Self { writer, store }
    }

    async fn photo_exists(&self, photo_id: &str) -> Result<bool, RecipeApiError> {
        sqlx::query_scalar::<_, i64>("SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id = ?)")
            .bind(photo_id)
            .fetch_one(&self.writer)
            .await
            .map(|exists| exists != 0)
            .map_err(|_| RecipeApiError::Unavailable)
    }

    async fn service_call<T: Send + 'static>(
        &self,
        operation: impl FnOnce(RecipeService<SqliteRecipeStore>) -> Result<T, RecipeError>
        + Send
        + 'static,
    ) -> Result<T, RecipeApiError> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || operation(RecipeService::new(store)))
            .await
            .map_err(|_| RecipeApiError::Unavailable)?
            .map_err(map_recipe_error)
    }
}

impl RecipeApi for SqliteRecipeApi {
    fn load<'a>(
        &'a self,
        photo_id: &'a str,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>> {
        Box::pin(async move {
            if !self.photo_exists(photo_id).await? {
                return Err(RecipeApiError::NotFound);
            }
            let photo_id_owned = photo_id.to_owned();
            let document = self
                .service_call(move |service| service.get(&photo_id_owned))
                .await?;
            Ok(to_view(photo_id, document))
        })
    }

    fn update<'a>(
        &'a self,
        photo_id: &'a str,
        expected_revision: u64,
        _request_id: &'a str,
        recipe: &'a RecipeValues,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>> {
        let recipe = Recipe {
            schema_version: recipe.schema_version,
            exposure: recipe.exposure,
            temperature: recipe.temperature,
            tint: recipe.tint,
        };
        Box::pin(async move {
            if !self.photo_exists(photo_id).await? {
                return Err(RecipeApiError::NotFound);
            }
            let photo_id_owned = photo_id.to_owned();
            let document = self
                .service_call(move |service| {
                    service.update(&photo_id_owned, expected_revision, recipe)
                })
                .await?;
            Ok(to_view(photo_id, document))
        })
    }

    fn undo<'a>(
        &'a self,
        photo_id: &'a str,
        expected_revision: u64,
        _request_id: &'a str,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>> {
        Box::pin(async move {
            if !self.photo_exists(photo_id).await? {
                return Err(RecipeApiError::NotFound);
            }
            let photo_id_owned = photo_id.to_owned();
            let document = self
                .service_call(move |service| service.undo(&photo_id_owned, expected_revision))
                .await?;
            Ok(to_view(photo_id, document))
        })
    }

    fn redo<'a>(
        &'a self,
        photo_id: &'a str,
        expected_revision: u64,
        _request_id: &'a str,
    ) -> RecipeApiFuture<'a, Result<RecipeView, RecipeApiError>> {
        Box::pin(async move {
            if !self.photo_exists(photo_id).await? {
                return Err(RecipeApiError::NotFound);
            }
            let photo_id_owned = photo_id.to_owned();
            let document = self
                .service_call(move |service| service.redo(&photo_id_owned, expected_revision))
                .await?;
            Ok(to_view(photo_id, document))
        })
    }
}

fn to_view(photo_id: &str, document: crate::editing::recipes::RecipeDocument) -> RecipeView {
    RecipeView {
        contract_version: 1,
        request_id: String::new(),
        photo_id: photo_id.to_owned(),
        revision: document.revision,
        current: RecipeValues {
            schema_version: document.current.schema_version,
            exposure: document.current.exposure,
            temperature: document.current.temperature,
            tint: document.current.tint,
        },
        can_undo: document.can_undo(),
        can_redo: document.can_redo(),
    }
}

fn map_recipe_error(error: RecipeError) -> RecipeApiError {
    match error {
        RecipeError::EmptyPhotoId => RecipeApiError::InvalidRequest,
        RecipeError::Conflict { actual, .. } => RecipeApiError::VersionConflict {
            actual_revision: actual,
        },
        RecipeError::NothingToUndo => RecipeApiError::NothingToUndo,
        RecipeError::NothingToRedo => RecipeApiError::NothingToRedo,
        RecipeError::InvalidLegacyRecipe
        | RecipeError::UnsupportedVersion(_)
        | RecipeError::InvalidExposure
        | RecipeError::InvalidTemperature
        | RecipeError::InvalidTint
        | RecipeError::RevisionOverflow => RecipeApiError::InvalidRecipe,
        RecipeError::PersistenceFailed => RecipeApiError::Unavailable,
    }
}
