//! Local bootstrap for source roots explicitly authorized by the operator.
//!
//! The caller must pass only values obtained from trusted local configuration
//! (for example, repeated `--authorized-root` CLI arguments). This function
//! never accepts HTTP data and does not infer authorization from
//! `import_sources.path`. HTTP handlers should expose only the returned opaque
//! source IDs and safe display labels, never the returned canonical paths.
//!
//! Source IDs are generated once and then retained by the
//! `(gallery_id, normalized_path)` unique key. Reopening with the same
//! canonical path preserves the ID. Moving a root to a different canonical
//! path creates a new source ID; callers should not assume that file identity
//! or move reconciliation carries across that change.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{Row, SqlitePool};

use super::migrations;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootstrappedSource {
    pub id: String,
    pub gallery_id: String,
    /// Canonical filesystem root for local scanner wiring. Never serialize this
    /// field into an HTTP response.
    pub root: PathBuf,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceBootstrap {
    pub gallery_id: String,
    pub created_gallery: bool,
    pub sources: Vec<BootstrappedSource>,
}

#[derive(Debug)]
pub enum SourceBootstrapError {
    Database(sqlx::Error),
    NoAuthorizedRoots,
    InvalidAuthorizedRoot { path: PathBuf, reason: String },
    OverlappingAuthorizedRoots { first: PathBuf, second: PathBuf },
    NonUtf8AuthorizedRoot(PathBuf),
    GalleryNotFound(String),
    AmbiguousGallerySelection,
}

impl std::fmt::Display for SourceBootstrapError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(error) => write!(formatter, "catalog bootstrap failed: {error}"),
            Self::NoAuthorizedRoots => {
                formatter.write_str("at least one --authorized-root is required")
            }
            Self::InvalidAuthorizedRoot { path, reason } => {
                write!(
                    formatter,
                    "authorized root `{}` is invalid: {reason}",
                    path.display()
                )
            }
            Self::OverlappingAuthorizedRoots { first, second } => write!(
                formatter,
                "authorized roots overlap: `{}` and `{}`; configure non-overlapping roots",
                first.display(),
                second.display()
            ),
            Self::NonUtf8AuthorizedRoot(path) => write!(
                formatter,
                "authorized root `{}` cannot be represented in the catalog",
                path.display()
            ),
            Self::GalleryNotFound(id) => {
                write!(formatter, "selected gallery `{id}` does not exist")
            }
            Self::AmbiguousGallerySelection => formatter.write_str(
                "multiple galleries exist; pass an explicit gallery_id to bootstrap sources",
            ),
        }
    }
}

impl std::error::Error for SourceBootstrapError {}

impl From<sqlx::Error> for SourceBootstrapError {
    fn from(error: sqlx::Error) -> Self {
        Self::Database(error)
    }
}

/// Ensure a gallery exists and register trusted source roots atomically.
///
/// `authorized_roots` must come from local operator configuration only. When
/// `gallery_id` is provided it must already exist. With no explicit ID, the
/// existing sole gallery is selected; no gallery causes a `Photo Culler`
/// gallery to be created; multiple galleries are rejected as ambiguous.
///
/// The caller must first open and validate the catalog with
/// `Catalog::open_initialized`. Migrations run here too so existing supported
/// catalogs have the source status and exclusion columns this bootstrap uses.
pub async fn bootstrap_sources(
    writer: &SqlitePool,
    authorized_roots: &[PathBuf],
    gallery_id: Option<&str>,
) -> Result<SourceBootstrap, SourceBootstrapError> {
    if authorized_roots.is_empty() {
        return Err(SourceBootstrapError::NoAuthorizedRoots);
    }

    // Canonicalize and deduplicate before acquiring a write transaction. Paths
    // supplied by callers are local configuration, not database-derived roots.
    let mut roots = BTreeMap::<String, (PathBuf, String)>::new();
    for configured in authorized_roots {
        let canonical = std::fs::canonicalize(configured).map_err(|error| {
            SourceBootstrapError::InvalidAuthorizedRoot {
                path: configured.clone(),
                reason: error.to_string(),
            }
        })?;
        if !canonical.is_dir() {
            return Err(SourceBootstrapError::InvalidAuthorizedRoot {
                path: configured.clone(),
                reason: "not a directory".into(),
            });
        }
        let path = canonical
            .to_str()
            .ok_or_else(|| SourceBootstrapError::NonUtf8AuthorizedRoot(canonical.clone()))?
            .to_owned();
        let normalized = normalized_path(&canonical)?;
        roots.entry(normalized).or_insert((canonical, path));
    }
    let canonical_roots = roots.values().map(|(root, _)| root).collect::<Vec<_>>();
    for (index, first) in canonical_roots.iter().enumerate() {
        for second in canonical_roots.iter().skip(index + 1) {
            if first.starts_with(second) || second.starts_with(first) {
                return Err(SourceBootstrapError::OverlappingAuthorizedRoots {
                    first: (*first).clone(),
                    second: (*second).clone(),
                });
            }
        }
    }

    migrations::migrate(writer).await?;
    let mut tx = writer.begin().await?;

    let (selected_gallery, created_gallery) = if let Some(id) = gallery_id {
        let exists: Option<String> = sqlx::query_scalar("SELECT id FROM galleries WHERE id = ?")
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?;
        if exists.is_none() {
            return Err(SourceBootstrapError::GalleryNotFound(id.to_owned()));
        }
        (id.to_owned(), false)
    } else {
        let rows = sqlx::query("SELECT id FROM galleries ORDER BY created_at, id LIMIT 2")
            .fetch_all(&mut *tx)
            .await?;
        match rows.as_slice() {
            [row] => (row.try_get("id")?, false),
            [] => {
                let id: String = sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
                    .fetch_one(&mut *tx)
                    .await?;
                let now = now_text();
                sqlx::query(
                    "INSERT INTO galleries(id, name, created_at, updated_at) VALUES (?, 'Photo Culler', ?, ?)",
                )
                .bind(&id)
                .bind(&now)
                .bind(&now)
                .execute(&mut *tx)
                .await?;
                (id, true)
            }
            _ => return Err(SourceBootstrapError::AmbiguousGallerySelection),
        }
    };

    let mut sources = Vec::with_capacity(roots.len());
    for (normalized, (root, path)) in roots {
        let now = now_text();
        sqlx::query(
            "INSERT INTO import_sources(id, gallery_id, path, normalized_path, recursive, created_at, status, last_seen_at, exclude_patterns) VALUES (lower(hex(randomblob(16))), ?, ?, ?, 1, ?, 'online', ?, '[]') ON CONFLICT(gallery_id, normalized_path) DO UPDATE SET path = excluded.path, status = 'online', last_seen_at = excluded.last_seen_at",
        )
        .bind(&selected_gallery)
        .bind(&path)
        .bind(&normalized)
        .bind(&now)
        .bind(&now)
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT id FROM import_sources WHERE gallery_id = ? AND normalized_path = ?",
        )
        .bind(&selected_gallery)
        .bind(&normalized)
        .fetch_one(&mut *tx)
        .await?;
        let id: String = row.try_get("id")?;
        sources.push(BootstrappedSource {
            id,
            gallery_id: selected_gallery.clone(),
            name: root
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .unwrap_or("Photos")
                .to_owned(),
            root,
        });
    }

    tx.commit().await?;
    Ok(SourceBootstrap {
        gallery_id: selected_gallery,
        created_gallery,
        sources,
    })
}

fn normalized_path(path: &Path) -> Result<String, SourceBootstrapError> {
    let text = path
        .to_str()
        .ok_or_else(|| SourceBootstrapError::NonUtf8AuthorizedRoot(path.to_path_buf()))?;
    let mut normalized = text.replace('\\', "/");
    if cfg!(windows) {
        normalized = normalized.to_lowercase();
    }
    let is_volume_root = cfg!(windows)
        && normalized.len() == 3
        && normalized.as_bytes().get(1) == Some(&b':')
        && normalized.ends_with('/');
    if normalized == "/" || is_volume_root {
        Ok(normalized)
    } else {
        Ok(normalized.trim_end_matches('/').to_owned())
    }
}

fn now_text() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos())
}
