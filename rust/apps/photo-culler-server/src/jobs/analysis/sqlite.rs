//! SQLite persistence and the native image-engine adapter for D25.
//!
//! The coordinator trait is synchronous so analyzers can advance one photo at
//! a time. Database calls therefore run through the supplied multi-threaded
//! Tokio handle. Callers inside async handlers must place coordinator calls in
//! `spawn_blocking`; never block an Axum executor thread with this adapter.

use std::{
    fs,
    future::Future,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use photo_culler_core::{AnalysisEngine, AnalysisRequest};
use sqlx::{Row, SqlitePool};
use tokio::runtime::{Handle, RuntimeFlavor};

use super::{Analyzer, Clock, Job, Outcome, Photo, Plan, Repository, Scope};

/// Build a stable analysis plan from present catalog files. The server caller
/// must provide its configured authorized roots; paths never enter an HTTP
/// response. Unsupported/missing files stay in the plan and produce a typed
/// per-photo analyzer failure instead of silently disappearing from progress.
pub async fn prepare_gallery_plan(
    reader: &SqlitePool,
    gallery_id: &str,
    profile_id: &str,
    authorized_roots: &[PathBuf],
) -> Result<Plan, String> {
    if gallery_id.trim().is_empty() || profile_id.trim().is_empty() {
        return Err("analysis gallery and profile are required".into());
    }
    if authorized_roots.is_empty() {
        return Err("analysis requires at least one configured authorized root".into());
    }
    let gallery_exists: i64 =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM galleries WHERE id = ?)")
            .bind(gallery_id)
            .fetch_one(reader)
            .await
            .map_err(|error| format!("could not resolve analysis gallery: {error}"))?;
    if gallery_exists == 0 {
        return Err("analysis gallery was not found".into());
    }
    let rows = sqlx::query(
        "SELECT p.photo_id, f.role, COALESCE(f.source_relative_path, f.relative_path) AS relative_path, COALESCE(s.path, v.mount_point) AS root_path FROM photos p JOIN files f ON f.photo_id = p.id LEFT JOIN import_sources s ON s.id = f.import_source_id LEFT JOIN volumes v ON v.id = f.volume_id WHERE p.gallery_id = ? AND f.status = 'present' ORDER BY p.photo_id, CASE UPPER(f.role) WHEN 'JPEG' THEN 0 WHEN 'IMAGE' THEN 1 WHEN 'RAW' THEN 2 ELSE 3 END, f.id",
    )
    .bind(gallery_id)
    .fetch_all(reader)
    .await
    .map_err(|error| format!("could not read analysis source files: {error}"))?;
    let candidates = rows
        .into_iter()
        .map(|row| {
            Ok::<_, sqlx::Error>((
                row.try_get::<String, _>("photo_id")?,
                row.try_get::<Option<String>, _>("root_path")?,
                row.try_get::<String, _>("relative_path")?,
            ))
        })
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("could not decode analysis source rows: {error}"))?;
    let roots = authorized_roots.to_vec();
    let photos = tokio::task::spawn_blocking(move || resolve_analysis_photos(candidates, roots))
        .await
        .map_err(|error| format!("analysis source resolver stopped: {error}"))?;
    if photos.is_empty() {
        let present_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT p.photo_id) FROM photos p JOIN files f ON f.photo_id = p.id WHERE p.gallery_id = ? AND f.status = 'present'",
        )
        .bind(gallery_id)
        .fetch_one(reader)
        .await
        .map_err(|error| format!("could not check analysis source availability: {error}"))?;
        if present_count > 0 {
            return Err("no present gallery files resolve inside the authorized roots".into());
        }
    }
    Ok(Plan {
        catalog_id: gallery_id.to_owned(),
        scope: Scope::Gallery {
            gallery_id: gallery_id.to_owned(),
        },
        profile_id: profile_id.to_owned(),
        photos,
    })
}

fn resolve_analysis_photos(
    candidates: Vec<(String, Option<String>, String)>,
    authorized_roots: Vec<PathBuf>,
) -> Vec<Photo> {
    let roots = authorized_roots
        .iter()
        .filter_map(|root| fs::canonicalize(root).ok())
        .collect::<Vec<_>>();
    let mut photos = Vec::new();
    for (photo_id, root, relative) in candidates {
        if photos
            .last()
            .is_some_and(|photo: &Photo| photo.photo_id == photo_id)
        {
            continue;
        }
        let Some(root) = root else { continue };
        let root = PathBuf::from(root);
        if !root.is_absolute() {
            continue;
        }
        let relative = Path::new(&relative);
        if relative.is_absolute()
            || relative
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            continue;
        }
        let candidate = root.join(relative);
        if has_symlink_component(&root, relative) {
            continue;
        }
        let Ok(resolved) = fs::canonicalize(&candidate) else {
            continue;
        };
        if !resolved.is_file() || !roots.iter().any(|allowed| resolved.starts_with(allowed)) {
            continue;
        }
        photos.push(Photo {
            photo_id,
            source: resolved.to_string_lossy().into_owned(),
        });
    }
    photos
}

fn has_symlink_component(root: &Path, relative: &Path) -> bool {
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return true;
        };
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => return true,
            Ok(_) => {}
            Err(_) => return false,
        }
    }
    false
}

#[derive(Clone)]
pub struct SqliteRepository {
    writer: SqlitePool,
    runtime: Handle,
}

impl SqliteRepository {
    /// Install the additive analysis-job table on the catalog's writer pool.
    pub async fn initialize(writer: SqlitePool, runtime: Handle) -> Result<Self, sqlx::Error> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_culler_analysis_jobs (
                catalog_id TEXT NOT NULL,
                job_id TEXT NOT NULL,
                state TEXT NOT NULL,
                next_index INTEGER NOT NULL CHECK(next_index >= 0),
                snapshot_json TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                PRIMARY KEY(catalog_id, job_id)
            )",
        )
        .execute(&writer)
        .await?;
        Ok(Self { writer, runtime })
    }

    fn block_on<T>(
        &self,
        future: impl Future<Output = Result<T, sqlx::Error>> + Send,
    ) -> Result<T, String>
    where
        T: Send,
    {
        let result = if Handle::try_current().is_ok() {
            if self.runtime.runtime_flavor() != RuntimeFlavor::MultiThread {
                return Err(
                    "SQLite analysis persistence requires a multi-threaded Tokio runtime".into(),
                );
            }
            tokio::task::block_in_place(|| self.runtime.block_on(future))
        } else {
            self.runtime.block_on(future)
        };
        result.map_err(|error| format!("analysis catalog persistence failed: {error}"))
    }
}

impl Repository for SqliteRepository {
    fn load_all(&self) -> Result<Vec<Job>, String> {
        let writer = self.writer.clone();
        self.block_on(async move {
            let rows = sqlx::query(
                "SELECT catalog_id, job_id, state, next_index, snapshot_json FROM photo_culler_analysis_jobs ORDER BY catalog_id, job_id",
            )
            .fetch_all(&writer)
            .await?;
            rows.into_iter()
                .map(|row| {
                    let catalog_id: String = row.try_get("catalog_id")?;
                    let job_id: String = row.try_get("job_id")?;
                    let stored_state: String = row.try_get("state")?;
                    let stored_next_index: i64 = row.try_get("next_index")?;
                    let snapshot: String = row.try_get("snapshot_json")?;
                    let job = serde_json::from_str::<Job>(&snapshot)
                        .map_err(|error| sqlx::Error::Decode(Box::new(error)))?;
                    let next_index = usize::try_from(stored_next_index).map_err(|error| {
                        sqlx::Error::Decode(Box::new(std::io::Error::other(format!(
                            "analysis job cursor is invalid: {error}"
                        ))))
                    })?;
                    if job.key.catalog_id != catalog_id
                        || job.key.job_id != job_id
                        || state_name(&job.state) != stored_state
                        || job.next_index != next_index
                    {
                        return Err(sqlx::Error::Decode(Box::new(std::io::Error::other(
                            "analysis job snapshot disagrees with its indexed columns",
                        ))));
                    }
                    Ok(job)
                })
                .collect()
        })
    }

    fn save(&mut self, job: &Job) -> Result<(), String> {
        let snapshot = serde_json::to_string(job)
            .map_err(|error| format!("could not encode analysis job snapshot: {error}"))?;
        let metrics = job
            .completed
            .last()
            .into_iter()
            .filter_map(|entry| match &entry.outcome {
                Outcome::Analyzed { metrics } => Some((entry.photo_id.clone(), metrics.clone())),
                Outcome::Failed { .. } => None,
            })
            .collect::<Vec<_>>();
        let writer = self.writer.clone();
        let catalog_id = job.key.catalog_id.clone();
        let job_id = job.key.job_id.clone();
        let profile_id = job.plan.profile_id.clone();
        let state = state_name(&job.state).to_owned();
        let next_index = i64::try_from(job.next_index).unwrap_or(i64::MAX);
        let created_at = i64::try_from(job.created_at).unwrap_or(i64::MAX);
        let updated_at = i64::try_from(job.updated_at).unwrap_or(i64::MAX);
        self.block_on(async move {
            let mut tx = writer.begin().await?;
            sqlx::query(
                "INSERT INTO photo_culler_analysis_jobs(catalog_id, job_id, state, next_index, snapshot_json, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?) ON CONFLICT(catalog_id, job_id) DO UPDATE SET state = excluded.state, next_index = excluded.next_index, snapshot_json = excluded.snapshot_json, updated_at = excluded.updated_at",
            )
            .bind(&catalog_id)
            .bind(&job_id)
            .bind(&state)
            .bind(next_index)
            .bind(&snapshot)
            .bind(created_at)
            .bind(updated_at)
            .execute(&mut *tx)
            .await?;

            // Keep completed Rust metrics discoverable through the legacy
            // catalog read DTO while preserving prior summaries under their
            // existing keys. Scores remain unset until a calibrated scoring
            // profile is wired to the Rust engine.
            for (photo_id, metrics) in metrics {
                let current: Option<String> = sqlx::query_scalar(
                    "SELECT analysis_summary_json FROM photos WHERE photo_id = ?",
                )
                .bind(&photo_id)
                .fetch_optional(&mut *tx)
                .await?;
                let Some(current) = current else {
                    continue;
                };
                let mut summary = serde_json::from_str::<serde_json::Value>(&current)
                    .unwrap_or_else(|_| serde_json::json!({}));
                let normalized_metrics = metrics
                    .into_iter()
                    .map(|metric| {
                        serde_json::from_str::<serde_json::Value>(&metric)
                            .unwrap_or(serde_json::Value::String(metric))
                    })
                    .collect::<Vec<_>>();
                if !summary.is_object() {
                    summary = serde_json::json!({ "legacy_summary": summary });
                }
                if let Some(object) = summary.as_object_mut() {
                    object.insert(
                        "rust_shadow".into(),
                        serde_json::json!({
                            "job_id": job_id.clone(),
                            "profile_id": profile_id.clone(),
                            "metrics": normalized_metrics,
                        }),
                    );
                }
                let encoded = serde_json::to_string(&summary)
                    .map_err(|error| sqlx::Error::Encode(Box::new(error)))?;
                sqlx::query("UPDATE photos SET analysis_summary_json = ? WHERE photo_id = ?")
                    .bind(encoded)
                    .bind(photo_id)
                    .execute(&mut *tx)
                    .await?;
            }
            tx.commit().await
        })
    }
}

fn state_name(state: &super::State) -> &'static str {
    match state {
        super::State::Queued => "queued",
        super::State::Running => "running",
        super::State::Paused => "paused",
        super::State::Succeeded => "succeeded",
        super::State::Failed { .. } => "failed",
        super::State::Cancelled => "cancelled",
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_secs())
            .unwrap_or(0)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ImageEngineAnalyzer {
    pub max_dimension: u32,
}

impl Default for ImageEngineAnalyzer {
    fn default() -> Self {
        Self {
            max_dimension: 1920,
        }
    }
}

impl Analyzer for ImageEngineAnalyzer {
    fn analyze(
        &mut self,
        _catalog_id: &str,
        photo: &Photo,
        profile_id: &str,
    ) -> Result<Vec<String>, String> {
        let engine = photo_culler_image_engine::NativePixelEngine {
            max_dimension: self.max_dimension.max(1),
        };
        let metrics = engine.analyze(&AnalysisRequest {
            photo_id: photo.photo_id.clone(),
            source: PathBuf::from(&photo.source),
            profile: profile_id.to_owned(),
        })?;
        metrics
            .into_iter()
            .map(|metric| {
                serde_json::to_string(&serde_json::json!({
                    "analyzer": metric.analyzer,
                    "implementation_version": metric.implementation_version,
                    "value": metric.value,
                    "confidence": metric.confidence,
                    "elapsed_micros": metric.elapsed_micros,
                }))
                .map_err(|error| format!("could not encode analysis metric: {error}"))
            })
            .collect()
    }
}
