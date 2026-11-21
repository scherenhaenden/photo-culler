//! Versioned, non-destructive edit recipes.
//!
//! The repository owns persistence and must atomically replace a document. This
//! module stores edit parameters and history only; it never opens or writes photos.

use serde::{Deserialize, Serialize};
use std::fmt;

mod sqlite;

pub use sqlite::SqliteRecipeStore;

pub const RECIPE_SCHEMA_VERSION: u32 = 1;
pub const MAX_HISTORY: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub schema_version: u32,
    pub exposure: f64,
    pub temperature: f64,
    pub tint: f64,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            exposure: 0.0,
            temperature: 6500.0,
            tint: 0.0,
        }
    }
}

impl Recipe {
    pub fn validate(&self) -> Result<(), RecipeError> {
        if self.schema_version != RECIPE_SCHEMA_VERSION {
            return Err(RecipeError::UnsupportedVersion(self.schema_version));
        }
        if !self.exposure.is_finite() || !(-5.0..=5.0).contains(&self.exposure) {
            return Err(RecipeError::InvalidExposure);
        }
        if !self.temperature.is_finite() || !(1000.0..=40000.0).contains(&self.temperature) {
            return Err(RecipeError::InvalidTemperature);
        }
        if !self.tint.is_finite() || !(-100.0..=100.0).contains(&self.tint) {
            return Err(RecipeError::InvalidTint);
        }
        Ok(())
    }

    /// Import the legacy Python JSON shape `{exposure, temperature, tint}`.
    /// Missing fields use legacy defaults; extra and malformed fields are rejected.
    pub fn from_legacy_json(input: &str) -> Result<Self, RecipeError> {
        let value: serde_json::Value =
            serde_json::from_str(input).map_err(|_| RecipeError::InvalidLegacyRecipe)?;
        let object = value.as_object().ok_or(RecipeError::InvalidLegacyRecipe)?;
        if object
            .keys()
            .any(|k| !["exposure", "temperature", "tint", "schema_version"].contains(&k.as_str()))
        {
            return Err(RecipeError::InvalidLegacyRecipe);
        }
        if let Some(v) = object.get("schema_version") {
            if v.as_u64() != Some(RECIPE_SCHEMA_VERSION as u64) {
                return Err(RecipeError::InvalidLegacyRecipe);
            }
        }
        let number = |key: &str, default: f64| -> Result<f64, RecipeError> {
            match object.get(key) {
                None => Ok(default),
                Some(v) => v.as_f64().ok_or(RecipeError::InvalidLegacyRecipe),
            }
        };
        let recipe = Self {
            schema_version: RECIPE_SCHEMA_VERSION,
            exposure: number("exposure", 0.0)?,
            temperature: number("temperature", 6500.0)?,
            tint: number("tint", 0.0)?,
        };
        recipe.validate()?;
        Ok(recipe)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecipeDocument {
    pub revision: u64,
    pub current: Recipe,
    undo: Vec<Recipe>,
    redo: Vec<Recipe>,
}

impl Default for RecipeDocument {
    fn default() -> Self {
        Self {
            revision: 0,
            current: Recipe::default(),
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }
}

impl RecipeDocument {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

pub trait RecipeStore {
    type Error;
    fn load(&self, photo_id: &str) -> Result<Option<RecipeDocument>, Self::Error>;
    /// Persist the complete snapshot atomically. Implementations should use a
    /// transaction and enforce the expected current revision where supported.
    fn save(
        &self,
        photo_id: &str,
        document: &RecipeDocument,
    ) -> Result<(), RecipeStoreError<Self::Error>>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecipeStoreError<E> {
    Conflict { expected: u64, actual: u64 },
    Persistence(E),
}

#[derive(Debug, Clone, PartialEq)]
pub enum RecipeError {
    EmptyPhotoId,
    Conflict { expected: u64, actual: u64 },
    NothingToUndo,
    NothingToRedo,
    InvalidLegacyRecipe,
    UnsupportedVersion(u32),
    InvalidExposure,
    InvalidTemperature,
    InvalidTint,
    RevisionOverflow,
    PersistenceFailed,
}

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RecipeError {}

pub struct RecipeService<S> {
    store: S,
}

impl<S: RecipeStore> RecipeService<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }
    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn get(&self, photo_id: &str) -> Result<RecipeDocument, RecipeError> {
        validate_id(photo_id)?;
        self.store
            .load(photo_id)
            .map_err(|_| RecipeError::PersistenceFailed)
            .map(|d| d.unwrap_or_default())
    }

    /// Load the legacy row once and persist it in the versioned document form.
    pub fn import_legacy(
        &self,
        photo_id: &str,
        legacy_json: &str,
    ) -> Result<RecipeDocument, RecipeError> {
        validate_id(photo_id)?;
        let recipe = Recipe::from_legacy_json(legacy_json)?;
        let mut document = self.get(photo_id)?;
        if document.revision != 0
            || document.current != Recipe::default()
            || document.can_undo()
            || document.can_redo()
        {
            return Ok(document);
        }
        document.current = recipe;
        document.revision = 1;
        self.store
            .save(photo_id, &document)
            .map_err(map_store_error)?;
        Ok(document)
    }

    pub fn update(
        &self,
        photo_id: &str,
        expected_revision: u64,
        mut recipe: Recipe,
    ) -> Result<RecipeDocument, RecipeError> {
        validate_id(photo_id)?;
        recipe.schema_version = RECIPE_SCHEMA_VERSION;
        recipe.validate()?;
        let mut document = self.get(photo_id)?;
        check_revision(expected_revision, document.revision)?;
        if document.current == recipe {
            return Ok(document);
        }
        push_bounded(&mut document.undo, document.current.clone());
        document.redo.clear();
        document.current = recipe;
        bump(&mut document)?;
        self.store
            .save(photo_id, &document)
            .map_err(map_store_error)?;
        Ok(document)
    }

    pub fn undo(
        &self,
        photo_id: &str,
        expected_revision: u64,
    ) -> Result<RecipeDocument, RecipeError> {
        self.move_history(photo_id, expected_revision, true)
    }

    pub fn redo(
        &self,
        photo_id: &str,
        expected_revision: u64,
    ) -> Result<RecipeDocument, RecipeError> {
        self.move_history(photo_id, expected_revision, false)
    }

    fn move_history(
        &self,
        photo_id: &str,
        expected_revision: u64,
        undo: bool,
    ) -> Result<RecipeDocument, RecipeError> {
        validate_id(photo_id)?;
        let mut document = self.get(photo_id)?;
        check_revision(expected_revision, document.revision)?;
        let (source, destination) = if undo {
            (&mut document.undo, &mut document.redo)
        } else {
            (&mut document.redo, &mut document.undo)
        };
        let restored = source.pop().ok_or(if undo {
            RecipeError::NothingToUndo
        } else {
            RecipeError::NothingToRedo
        })?;
        push_bounded(destination, document.current.clone());
        document.current = restored;
        bump(&mut document)?;
        self.store
            .save(photo_id, &document)
            .map_err(map_store_error)?;
        Ok(document)
    }
}

fn validate_id(id: &str) -> Result<(), RecipeError> {
    if id.trim().is_empty() {
        Err(RecipeError::EmptyPhotoId)
    } else {
        Ok(())
    }
}

fn map_store_error<E>(error: RecipeStoreError<E>) -> RecipeError {
    match error {
        RecipeStoreError::Conflict { expected, actual } => {
            RecipeError::Conflict { expected, actual }
        }
        RecipeStoreError::Persistence(_) => RecipeError::PersistenceFailed,
    }
}
fn check_revision(expected: u64, actual: u64) -> Result<(), RecipeError> {
    if expected == actual {
        Ok(())
    } else {
        Err(RecipeError::Conflict { expected, actual })
    }
}
fn bump(document: &mut RecipeDocument) -> Result<(), RecipeError> {
    document.revision = document
        .revision
        .checked_add(1)
        .ok_or(RecipeError::RevisionOverflow)?;
    Ok(())
}
fn push_bounded(stack: &mut Vec<Recipe>, recipe: Recipe) {
    stack.push(recipe);
    if stack.len() > MAX_HISTORY {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap, fs, path::PathBuf, rc::Rc};

    #[derive(Clone, Default)]
    struct MemoryStore(Rc<RefCell<HashMap<String, RecipeDocument>>>);
    impl RecipeStore for MemoryStore {
        type Error = ();
        fn load(&self, id: &str) -> Result<Option<RecipeDocument>, ()> {
            Ok(self.0.borrow().get(id).cloned())
        }
        fn save(&self, id: &str, doc: &RecipeDocument) -> Result<(), RecipeStoreError<()>> {
            self.0.borrow_mut().insert(id.into(), doc.clone());
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FileStore(PathBuf);
    impl RecipeStore for FileStore {
        type Error = std::io::Error;
        fn load(&self, id: &str) -> Result<Option<RecipeDocument>, Self::Error> {
            let path = self.0.join(format!("{id}.json"));
            match fs::read(path) {
                Ok(bytes) => serde_json::from_slice(&bytes)
                    .map(Some)
                    .map_err(std::io::Error::other),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            }
        }
        fn save(
            &self,
            id: &str,
            doc: &RecipeDocument,
        ) -> Result<(), RecipeStoreError<Self::Error>> {
            let path = self.0.join(format!("{id}.json"));
            let tmp = self.0.join(format!(".{id}.tmp"));
            fs::write(
                &tmp,
                serde_json::to_vec(doc)
                    .map_err(std::io::Error::other)
                    .map_err(RecipeStoreError::Persistence)?,
            )
            .map_err(RecipeStoreError::Persistence)?;
            fs::rename(tmp, path).map_err(RecipeStoreError::Persistence)
        }
    }

    fn updated(recipe: &Recipe, exposure: f64, temperature: f64) -> Recipe {
        Recipe {
            exposure,
            temperature,
            ..recipe.clone()
        }
    }

    #[test]
    fn legacy_edit_undo_redo_and_revision_conflicts_use_memory_store() {
        let service = RecipeService::new(MemoryStore::default());
        let imported = service
            .import_legacy("p1", r#"{"exposure":0.5,"temperature":7000,"tint":-10}"#)
            .unwrap();
        assert_eq!(imported.revision, 1);
        let changed = service
            .update("p1", 1, updated(&imported.current, 1.25, 7200.0))
            .unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(service.undo("p1", 2).unwrap().current, imported.current);
        assert_eq!(service.redo("p1", 3).unwrap().current, changed.current);
        assert_eq!(
            service.undo("p1", 2),
            Err(RecipeError::Conflict {
                expected: 2,
                actual: 4
            })
        );
    }

    #[test]
    fn reopening_store_restores_undo_redo_sequence_and_keeps_original_hash() {
        let root = std::env::temp_dir().join(format!("pc-recipes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let original = root.join("original.jpg");
        fs::write(&original, b"original fixture bytes").unwrap();
        let original_hash = hash(&original);
        let store = FileStore(root.join("recipes"));
        fs::create_dir_all(&store.0).unwrap();
        {
            let service = RecipeService::new(store.clone());
            let initial = service
                .import_legacy("p1", r#"{"exposure":-0.25,"temperature":6000}"#)
                .unwrap();
            service
                .update(
                    "p1",
                    initial.revision,
                    updated(&initial.current, 0.75, 6800.0),
                )
                .unwrap();
        }
        let reopened = RecipeService::new(store.clone());
        let loaded = reopened.get("p1").unwrap();
        let undone = reopened.undo("p1", loaded.revision).unwrap();
        assert_eq!(undone.current.exposure, -0.25);
        let redone = reopened.redo("p1", undone.revision).unwrap();
        assert_eq!(redone.current.exposure, 0.75);
        assert_eq!(hash(&original), original_hash);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn validates_ranges_legacy_shape_and_empty_history() {
        let service = RecipeService::new(MemoryStore::default());
        assert_eq!(
            Recipe::from_legacy_json(r#"{"exposure":99}"#),
            Err(RecipeError::InvalidExposure)
        );
        assert_eq!(
            Recipe::from_legacy_json(r#"{"unknown":1}"#),
            Err(RecipeError::InvalidLegacyRecipe)
        );
        assert_eq!(service.undo("p1", 0), Err(RecipeError::NothingToUndo));
    }

    fn hash(path: &PathBuf) -> u64 {
        fs::read(path)
            .unwrap()
            .into_iter()
            .fold(14695981039346656037u64, |h, b| {
                (h ^ u64::from(b)).wrapping_mul(1099511628211)
            })
    }
}
