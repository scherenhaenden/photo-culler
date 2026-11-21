//! SQLite persistence for versioned recipe documents.
//!
//! The synchronous `RecipeStore` port is bridged through a supplied Tokio
//! multi-thread runtime. Call it from `spawn_blocking` when invoked by an
//! asynchronous HTTP handler.

use std::future::Future;

use sqlx::{Row, SqlitePool};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::{
    MAX_HISTORY, RECIPE_SCHEMA_VERSION, Recipe, RecipeDocument, RecipeStore, RecipeStoreError,
};

#[derive(Clone)]
pub struct SqliteRecipeStore {
    writer: SqlitePool,
    runtime: Handle,
}

impl SqliteRecipeStore {
    pub fn new(writer: SqlitePool, runtime: Handle) -> Self {
        Self { writer, runtime }
    }

    fn block_on<T: Send>(
        &self,
        future: impl Future<Output = Result<T, String>> + Send,
    ) -> Result<T, String> {
        if Handle::try_current().is_ok() {
            if self.runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
                return Err("recipe persistence requires a multi-threaded Tokio runtime".into());
            }
            tokio::task::block_in_place(|| self.runtime.block_on(future))
        } else {
            self.runtime.block_on(future)
        }
    }

    fn block_on_save<T: Send>(
        &self,
        future: impl Future<Output = Result<T, RecipeStoreError<String>>> + Send,
    ) -> Result<T, RecipeStoreError<String>> {
        if Handle::try_current().is_ok() {
            if self.runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
                return Err(RecipeStoreError::Persistence(
                    "recipe persistence requires a multi-threaded Tokio runtime".into(),
                ));
            }
            tokio::task::block_in_place(|| self.runtime.block_on(future))
        } else {
            self.runtime.block_on(future)
        }
    }
}

impl RecipeStore for SqliteRecipeStore {
    type Error = String;

    fn load(&self, photo_id: &str) -> Result<Option<RecipeDocument>, Self::Error> {
        let writer = self.writer.clone();
        let photo_id = photo_id.to_owned();
        self.block_on(async move {
            let row = sqlx::query(
                "SELECT e.contract_version, e.revision, e.recipe_json, e.undo_stack_json, e.redo_stack_json FROM edit_documents e JOIN photos p ON p.id = e.photo_id WHERE p.photo_id = ?",
            )
            .bind(&photo_id)
            .fetch_optional(&writer)
            .await
            .map_err(|error| format!("could not load edit recipe: {error}"))?;
            let Some(row) = row else {
                return Ok(None);
            };

            let contract_version = row
                .try_get::<i64, _>("contract_version")
                .map_err(|error| format!("could not decode recipe contract version: {error}"))?;
            if contract_version != i64::from(RECIPE_SCHEMA_VERSION) {
                return Err(format!(
                    "unsupported persisted recipe contract version {contract_version}"
                ));
            }
            let revision = u64::try_from(
                row.try_get::<i64, _>("revision")
                    .map_err(|error| format!("could not decode recipe revision: {error}"))?,
            )
            .map_err(|_| "persisted recipe revision is negative".to_owned())?;
            let current_json = row
                .try_get::<String, _>("recipe_json")
                .map_err(|error| format!("could not read recipe JSON: {error}"))?;
            let undo_json = row
                .try_get::<String, _>("undo_stack_json")
                .map_err(|error| format!("could not read recipe undo history: {error}"))?;
            let redo_json = row
                .try_get::<String, _>("redo_stack_json")
                .map_err(|error| format!("could not read recipe redo history: {error}"))?;
            let current = serde_json::from_str::<Recipe>(&current_json)
                .map_err(|error| format!("persisted recipe is invalid: {error}"))?;
            let undo = serde_json::from_str::<Vec<Recipe>>(&undo_json)
                .map_err(|error| format!("persisted recipe undo history is invalid: {error}"))?;
            let redo = serde_json::from_str::<Vec<Recipe>>(&redo_json)
                .map_err(|error| format!("persisted recipe redo history is invalid: {error}"))?;
            current
                .validate()
                .map_err(|error| format!("persisted current recipe is invalid: {error}"))?;
            validate_history(&undo)?;
            validate_history(&redo)?;
            if undo.len() > MAX_HISTORY || redo.len() > MAX_HISTORY {
                return Err("persisted recipe history exceeds its limit".into());
            }
            Ok(Some(RecipeDocument {
                revision,
                current,
                undo,
                redo,
            }))
        })
    }

    fn save(
        &self,
        photo_id: &str,
        document: &RecipeDocument,
    ) -> Result<(), RecipeStoreError<Self::Error>> {
        let expected_revision_u64 = document.revision.checked_sub(1).ok_or_else(|| {
            RecipeStoreError::Persistence(
                "a recipe snapshot must have a positive revision before saving".into(),
            )
        })?;
        document
            .current
            .validate()
            .map_err(|error| RecipeStoreError::Persistence(error.to_string()))?;
        validate_history(&document.undo).map_err(RecipeStoreError::Persistence)?;
        validate_history(&document.redo).map_err(RecipeStoreError::Persistence)?;
        if document.undo.len() > MAX_HISTORY || document.redo.len() > MAX_HISTORY {
            return Err(RecipeStoreError::Persistence(
                "recipe history exceeds its limit".into(),
            ));
        }

        let current_json = serde_json::to_string(&document.current)
            .map_err(|error| RecipeStoreError::Persistence(error.to_string()))?;
        let undo_json = serde_json::to_string(&document.undo)
            .map_err(|error| RecipeStoreError::Persistence(error.to_string()))?;
        let redo_json = serde_json::to_string(&document.redo)
            .map_err(|error| RecipeStoreError::Persistence(error.to_string()))?;
        let photo_id = photo_id.to_owned();
        let new_revision = i64::try_from(document.revision).map_err(|_| {
            RecipeStoreError::Persistence("recipe revision exceeds SQLite integer range".into())
        })?;
        let expected_revision = i64::try_from(expected_revision_u64).map_err(|_| {
            RecipeStoreError::Persistence("expected recipe revision exceeds SQLite range".into())
        })?;
        let contract_version = i64::from(RECIPE_SCHEMA_VERSION);
        let writer = self.writer.clone();
        self.block_on_save(async move {
            let mut tx = writer
                .begin()
                .await
                .map_err(|error| {
                    RecipeStoreError::Persistence(format!(
                        "could not begin recipe transaction: {error}"
                    ))
                })?;
            let photo_row_id = sqlx::query_scalar::<_, i64>(
                "SELECT id FROM photos WHERE photo_id = ?",
            )
            .bind(&photo_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| {
                RecipeStoreError::Persistence(format!(
                    "could not resolve recipe photo: {error}"
                ))
            })?;
            let Some(photo_row_id) = photo_row_id else {
                return Err(RecipeStoreError::Persistence(
                    "recipe photo was not found in the catalog".into(),
                ));
            };

            let actual_revision = sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM edit_documents WHERE photo_id = ?",
            )
            .bind(photo_row_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|error| {
                RecipeStoreError::Persistence(format!(
                    "could not read stored recipe revision: {error}"
                ))
            })?;

            match actual_revision {
                Some(actual) => {
                    let actual_u64 = u64::try_from(actual)
                        .map_err(|_| {
                            RecipeStoreError::Persistence(
                                "stored recipe revision is negative".into(),
                            )
                        })?;
                    if actual != expected_revision {
                        tx.rollback().await.map_err(|error| {
                            RecipeStoreError::Persistence(format!(
                                "could not close conflicted recipe transaction: {error}"
                            ))
                        })?;
                        return Err(RecipeStoreError::Conflict {
                            expected: expected_revision_u64,
                            actual: actual_u64,
                        });
                    }
                    let updated = sqlx::query(
                        "UPDATE edit_documents SET contract_version = ?, revision = ?, recipe_json = ?, undo_stack_json = ?, redo_stack_json = ?, updated_at = CURRENT_TIMESTAMP WHERE photo_id = ? AND revision = ?",
                    )
                    .bind(contract_version)
                    .bind(new_revision)
                    .bind(&current_json)
                    .bind(&undo_json)
                    .bind(&redo_json)
                    .bind(photo_row_id)
                    .bind(expected_revision)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| {
                        RecipeStoreError::Persistence(format!(
                            "could not update edit recipe: {error}"
                        ))
                    })?;
                    if updated.rows_affected() != 1 {
                        let actual = sqlx::query_scalar::<_, i64>(
                            "SELECT revision FROM edit_documents WHERE photo_id = ?",
                        )
                        .bind(photo_row_id)
                        .fetch_optional(&mut *tx)
                        .await
                        .map_err(|error| {
                            RecipeStoreError::Persistence(format!(
                                "could not re-read conflicted recipe revision: {error}"
                            ))
                        })?
                        .ok_or_else(|| {
                            RecipeStoreError::Persistence(
                                "recipe row disappeared during revision update".into(),
                            )
                        })?;
                        let actual = u64::try_from(actual).map_err(|_| {
                            RecipeStoreError::Persistence(
                                "stored recipe revision is negative".into(),
                            )
                        })?;
                        tx.rollback().await.map_err(|error| {
                            RecipeStoreError::Persistence(format!(
                                "could not close conflicted recipe transaction: {error}"
                            ))
                        })?;
                        return Err(RecipeStoreError::Conflict {
                            expected: expected_revision_u64,
                            actual,
                        });
                    }
                }
                None => {
                    if expected_revision != 0 {
                        tx.rollback().await.map_err(|error| {
                            RecipeStoreError::Persistence(format!(
                                "could not close missing recipe transaction: {error}"
                            ))
                        })?;
                        return Err(RecipeStoreError::Conflict {
                            expected: expected_revision_u64,
                            actual: 0,
                        });
                    }
                    let id = format!("edit-{photo_row_id}");
                    sqlx::query(
                        "INSERT INTO edit_documents(id, photo_id, contract_version, revision, recipe_json, undo_stack_json, redo_stack_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)",
                    )
                    .bind(id)
                    .bind(photo_row_id)
                    .bind(contract_version)
                    .bind(new_revision)
                    .bind(&current_json)
                    .bind(&undo_json)
                    .bind(&redo_json)
                    .execute(&mut *tx)
                    .await
                    .map_err(|error| {
                        RecipeStoreError::Persistence(format!(
                            "could not create edit recipe: {error}"
                        ))
                    })?;
                }
            }

            tx.commit().await.map_err(|error| {
                RecipeStoreError::Persistence(format!("could not commit edit recipe: {error}"))
            })
        })
    }
}

fn validate_history(history: &[Recipe]) -> Result<(), String> {
    for recipe in history {
        recipe
            .validate()
            .map_err(|error| format!("persisted recipe history is invalid: {error}"))?;
    }
    Ok(())
}
