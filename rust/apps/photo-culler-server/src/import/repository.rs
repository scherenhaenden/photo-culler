//! SQLx persistence for the resumable import worker.
//!
//! A durable ordered path manifest is built before hashing. Observations and
//! their cursor are committed together; catalog rows are reconciled only after
//! the manifest has been consumed, preventing partial scans from marking files
//! missing.

use std::{
    io::Cursor,
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

use sqlx::{Row, SqlitePool};

use crate::{
    catalog::sqlite::migrations,
    import::{
        identity::compute_full_hash,
        scanning::{self, Role},
        worker::{
            ControlRequest, ControlSignal, EntryChange, FinishOutcome, ImportCounts, ImportJob,
            ImportRepository, JobState, ManifestEntry, Observation, ObservationCommit,
            WorkerFuture,
        },
    },
};

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Stable, path-free validation failures transported through the repository's
/// existing string error boundary. Only these exact codes may reach the API.
#[derive(Clone, Copy)]
pub(crate) enum ImportValidationError {
    DirectoryCase,
    NameCase,
    AmbiguousMedia,
    UnassociatedSidecar,
    ConflictingPhotoIdentity,
}

impl ImportValidationError {
    pub(crate) fn code(self) -> &'static str {
        match self {
            Self::DirectoryCase => "ambiguous_directory_case",
            Self::NameCase => "ambiguous_name_case",
            Self::AmbiguousMedia => "ambiguous_media",
            Self::UnassociatedSidecar => "unassociated_sidecar",
            Self::ConflictingPhotoIdentity => "conflicting_photo_identity",
        }
    }

    pub(crate) fn from_code(code: &str) -> Option<Self> {
        [
            Self::DirectoryCase,
            Self::NameCase,
            Self::AmbiguousMedia,
            Self::UnassociatedSidecar,
            Self::ConflictingPhotoIdentity,
        ]
        .into_iter()
        .find(|candidate| candidate.code() == code)
    }

    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::DirectoryCase => {
                "Import stopped: folder names differ only by letter case within a photo group. Check the source folders before retrying."
            }
            Self::NameCase => {
                "Import stopped: photo names differ only by letter case within a group. Check the source filenames before retrying."
            }
            Self::AmbiguousMedia => {
                "Import stopped: multiple files of the same type match one photo. Check the source filenames before retrying."
            }
            Self::UnassociatedSidecar => {
                "Import stopped: a metadata sidecar has no matching photo. Check the source files before retrying."
            }
            Self::ConflictingPhotoIdentity => {
                "Import stopped: separate photo groups match the same existing photo. Check moved or previously associated files before retrying."
            }
        }
    }
}

/// Import job persistence backed by the catalog's single-writer pool.
///
/// The caller must pass the same pool used by `Catalog` so import writes obey
/// the catalog's one-writer rule. `initialize` applies legacy schema upgrades
/// and creates the worker's additive checkpoint/staging tables.
#[derive(Clone)]
pub struct SqliteImportRepository {
    writer: SqlitePool,
    tables_ready: Arc<tokio::sync::Mutex<bool>>,
}

struct ValidatedManifestEntry {
    path: String,
    size: i64,
    modified_ns: Option<String>,
    role: String,
}

impl SqliteImportRepository {
    pub fn new(writer: SqlitePool) -> Self {
        Self {
            writer,
            tables_ready: Arc::new(tokio::sync::Mutex::new(false)),
        }
    }

    /// Upgrade the existing catalog schema and install additive worker state.
    pub async fn initialize(&self) -> Result<(), sqlx::Error> {
        migrations::migrate(&self.writer).await?;
        self.ensure_tables().await
    }

    /// Persist an explicit resume request before placing the job in the
    /// in-memory supervisor queue. A restart can then recover `queued` as
    /// `paused` instead of silently losing the request.
    pub async fn queue_resume(&mut self, job_id: &str) -> Result<(), String> {
        self.ensure_tables().await.map_err(err)?;
        let mut tx = self.writer.begin().await.map_err(err)?;
        set_state_tx(&mut tx, job_id, "paused", "queued", None, false).await?;
        sqlx::query(
            "UPDATE import_jobs SET cancel_requested = 0 WHERE id = ? AND state = 'queued'",
        )
        .bind(job_id)
        .execute(&mut *tx)
        .await
        .map_err(err)?;
        tx.commit().await.map_err(err)
    }

    async fn ensure_tables(&self) -> Result<(), sqlx::Error> {
        let mut tables_ready = self.tables_ready.lock().await;
        if *tables_ready {
            return Ok(());
        }
        // Also makes the repository usable when the application has already
        // run the catalog upgrades but has not yet initialized import state.
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_culler_import_checkpoints (
                job_id TEXT PRIMARY KEY REFERENCES import_jobs(id) ON DELETE CASCADE,
                cursor TEXT,
                processed INTEGER NOT NULL DEFAULT 0 CHECK(processed >= 0),
                revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0)
            )",
        )
        .execute(&self.writer)
        .await?;
        let has_revision: i64 = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_info('photo_culler_import_checkpoints') WHERE name = 'revision')",
        )
        .fetch_one(&self.writer)
        .await?;
        if has_revision == 0 {
            sqlx::query("ALTER TABLE photo_culler_import_checkpoints ADD COLUMN revision INTEGER NOT NULL DEFAULT 0 CHECK(revision >= 0)")
                .execute(&self.writer)
                .await?;
        }
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_culler_import_observations (
                job_id TEXT NOT NULL REFERENCES import_jobs(id) ON DELETE CASCADE,
                source_relative_path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
                modified_time REAL NOT NULL,
                quick_hash TEXT NOT NULL,
                full_hash TEXT NOT NULL,
                role TEXT NOT NULL,
                PRIMARY KEY(job_id, source_relative_path)
            )",
        )
        .execute(&self.writer)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_culler_import_results (
                job_id TEXT PRIMARY KEY REFERENCES import_jobs(id) ON DELETE CASCADE,
                new_files INTEGER NOT NULL DEFAULT 0,
                modified_files INTEGER NOT NULL DEFAULT 0,
                moved_files INTEGER NOT NULL DEFAULT 0,
                unchanged_files INTEGER NOT NULL DEFAULT 0,
                missing_files INTEGER NOT NULL DEFAULT 0
            )",
        )
        .execute(&self.writer)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_culler_import_manifests (
                job_id TEXT PRIMARY KEY REFERENCES import_jobs(id) ON DELETE CASCADE,
                state TEXT NOT NULL CHECK(state IN ('building', 'complete'))
            )",
        )
        .execute(&self.writer)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS photo_culler_import_manifest_entries (
                job_id TEXT NOT NULL REFERENCES import_jobs(id) ON DELETE CASCADE,
                path TEXT NOT NULL,
                size_bytes INTEGER NOT NULL CHECK(size_bytes >= 0),
                modified_ns TEXT,
                role TEXT NOT NULL CHECK(role IN ('RAW', 'JPEG', 'IMAGE', 'SIDECAR')),
                PRIMARY KEY(job_id, path)
            )",
        )
        .execute(&self.writer)
        .await?;
        *tables_ready = true;
        Ok(())
    }
}

impl ImportRepository for SqliteImportRepository {
    fn create_job<'a>(
        &'a mut self,
        source_id: &'a str,
        revision: u64,
    ) -> WorkerFuture<'a, Result<ImportJob, String>> {
        Box::pin(async move {
            self.initialize().await.map_err(err)?;
            let mut tx = self.writer.begin().await.map_err(err)?;
            let source = sqlx::query(
                "SELECT gallery_id FROM import_sources WHERE id = ? AND status = 'online'",
            )
            .bind(source_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?
            .ok_or_else(|| format!("import source `{source_id}` does not exist or is offline"))?;
            let gallery_id: String = source.try_get("gallery_id").map_err(err)?;
            let id = new_id();
            let now = now_text();
            sqlx::query(
                "INSERT INTO scan_revisions(id, gallery_id, source_id, state, discovered, new_files, modified_files, missing_files, moved_files, started_at) VALUES (?, ?, ?, 'queued', 0, 0, 0, 0, 0, ?)",
            )
            .bind(&id)
            .bind(&gallery_id)
            .bind(source_id)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query(
                "INSERT INTO import_jobs(id, gallery_id, source_id, scan_revision_id, state, discovered, imported, issues, cancel_requested, pause_requested, resume_state, error, created_at, updated_at) VALUES (?, ?, ?, ?, 'queued', 0, 0, 0, 0, 0, NULL, NULL, ?, ?)",
            )
            .bind(&id)
            .bind(&gallery_id)
            .bind(source_id)
            .bind(&id)
            .bind(&now)
            .bind(&now)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query("INSERT INTO photo_culler_import_checkpoints(job_id, cursor, processed, revision) VALUES (?, NULL, 0, ?)")
                .bind(&id)
                .bind(i64::try_from(revision).unwrap_or(i64::MAX))
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            tx.commit().await.map_err(err)?;
            Ok(ImportJob {
                id,
                source_id: source_id.to_owned(),
                revision,
                cursor: None,
                state: JobState::Queued,
                processed: 0,
            })
        })
    }

    fn load_job<'a>(&'a self, job_id: &'a str) -> WorkerFuture<'a, Result<ImportJob, String>> {
        Box::pin(async move { load_job(&self.writer, job_id).await })
    }

    fn list_jobs(&self) -> WorkerFuture<'_, Result<Vec<ImportJob>, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let rows = sqlx::query(
                "SELECT j.id, j.source_id, j.state, j.error, c.cursor, COALESCE(c.processed, 0) AS processed, COALESCE(c.revision, 0) AS revision FROM import_jobs j LEFT JOIN photo_culler_import_checkpoints c ON c.job_id = j.id ORDER BY j.created_at, j.id",
            )
            .fetch_all(&self.writer)
            .await
            .map_err(err)?;
            rows.into_iter().map(job_from_row).collect()
        })
    }

    fn claim_job<'a>(
        &'a mut self,
        job_id: &'a str,
        expected: JobState,
    ) -> WorkerFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            if !matches!(&expected, JobState::Queued | JobState::Paused) {
                return Ok(false);
            }
            let expected_name = state_name(&expected);
            let now = now_text();
            let mut tx = self.writer.begin().await.map_err(err)?;
            let result = sqlx::query(
                "UPDATE import_jobs SET state = 'running', error = NULL, pause_requested = 0, cancel_requested = 0, updated_at = ? WHERE id = ? AND state = ? AND cancel_requested = 0 AND pause_requested = 0",
            )
            .bind(&now)
            .bind(job_id)
            .bind(expected_name)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            let claimed = result.rows_affected() == 1;
            if claimed {
                sqlx::query("UPDATE scan_revisions SET state = 'running' WHERE id = (SELECT scan_revision_id FROM import_jobs WHERE id = ?)")
                    .bind(job_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(err)?;
            }
            tx.commit().await.map_err(err)?;
            Ok(claimed)
        })
    }

    fn request_control<'a>(
        &'a mut self,
        job_id: &'a str,
        request: ControlRequest,
    ) -> WorkerFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let mut tx = self.writer.begin().await.map_err(err)?;
            let row = sqlx::query("SELECT state FROM import_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_optional(&mut *tx)
                .await
                .map_err(err)?
                .ok_or_else(|| format!("import job `{job_id}` does not exist"))?;
            let state: String = row.try_get("state").map_err(err)?;
            let now = now_text();
            match (request, state.as_str()) {
                (ControlRequest::Pause, "running") => {
                    let result = sqlx::query("UPDATE import_jobs SET pause_requested = 1, updated_at = ? WHERE id = ? AND state = 'running'")
                        .bind(&now)
                        .bind(job_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(err)?;
                    if result.rows_affected() != 1 {
                        return Err(format!("job `{job_id}` changed state during pause request"));
                    }
                }
                (ControlRequest::Pause, "queued") => {
                    set_state_tx(&mut tx, job_id, "queued", "paused", None, false).await?;
                    persist_pause_flag_tx(&mut tx, job_id).await?;
                }
                (ControlRequest::Pause, "paused") => {
                    // A self-yield also leaves the job paused briefly while its
                    // supervisor prepares an automatic resume. Persist an
                    // operator pause so that resume's CAS cannot claim it.
                    let result = sqlx::query("UPDATE import_jobs SET pause_requested = 1, updated_at = ? WHERE id = ? AND state = 'paused'")
                        .bind(&now)
                        .bind(job_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(err)?;
                    if result.rows_affected() != 1 {
                        return Err(format!("job `{job_id}` changed state during pause request"));
                    }
                }
                (ControlRequest::Cancel, "running") => {
                    let result = sqlx::query("UPDATE import_jobs SET cancel_requested = 1, updated_at = ? WHERE id = ? AND state = 'running'")
                        .bind(&now)
                        .bind(job_id)
                        .execute(&mut *tx)
                        .await
                        .map_err(err)?;
                    if result.rows_affected() != 1 {
                        return Err(format!(
                            "job `{job_id}` changed state during cancel request"
                        ));
                    }
                }
                (ControlRequest::Cancel, "queued" | "paused") => {
                    set_state_tx(&mut tx, job_id, &state, "cancelled", None, true).await?;
                    persist_cancel_flag_tx(&mut tx, job_id).await?;
                }
                (ControlRequest::Cancel, "cancelled") => {}
                _ => return Err(format!("cannot {request:?} job in state `{state}`")),
            }
            tx.commit().await.map_err(err)
        })
    }

    fn checkpoint_control<'a>(
        &'a mut self,
        job_id: &'a str,
        yield_if_running: bool,
    ) -> WorkerFuture<'a, Result<ControlSignal, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let mut tx = self.writer.begin().await.map_err(err)?;
            let row = sqlx::query(
                "SELECT state, cancel_requested, pause_requested FROM import_jobs WHERE id = ?",
            )
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?
            .ok_or_else(|| format!("import job `{job_id}` does not exist"))?;
            let state: String = row.try_get("state").map_err(err)?;
            let cancel: bool = row.try_get("cancel_requested").map_err(err)?;
            let pause: bool = row.try_get("pause_requested").map_err(err)?;
            let signal = match state.as_str() {
                "cancelled" => ControlSignal::Cancelled,
                "paused" => ControlSignal::Paused,
                "running" if cancel => {
                    set_state_tx(&mut tx, job_id, "running", "cancelled", None, true).await?;
                    ControlSignal::Cancelled
                }
                "running" if pause => {
                    set_state_tx(&mut tx, job_id, "running", "paused", None, false).await?;
                    persist_pause_flag_tx(&mut tx, job_id).await?;
                    ControlSignal::Paused
                }
                "running" if yield_if_running => {
                    set_state_tx(&mut tx, job_id, "running", "paused", None, false).await?;
                    ControlSignal::Yielded
                }
                "running" => ControlSignal::Continue,
                _ => return Err(format!("cannot checkpoint job in state `{state}`")),
            };
            tx.commit().await.map_err(err)?;
            Ok(signal)
        })
    }

    fn reset_manifest<'a>(&'a mut self, job_id: &'a str) -> WorkerFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let mut tx = self.writer.begin().await.map_err(err)?;
            let state: Option<String> =
                sqlx::query_scalar("SELECT state FROM import_jobs WHERE id = ?")
                    .bind(job_id)
                    .fetch_optional(&mut *tx)
                    .await
                    .map_err(err)?;
            if state.as_deref() != Some("running") {
                return Err(format!(
                    "cannot reset import manifest for job `{job_id}` outside running state"
                ));
            }
            sqlx::query("DELETE FROM photo_culler_import_manifest_entries WHERE job_id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            // A missing or partial manifest cannot be paired with observations
            // from an older scanner version. Hashing starts only after this
            // transaction completes the new manifest, so discard that stage
            // and restart from the first portable path.
            sqlx::query("DELETE FROM photo_culler_import_observations WHERE job_id = ?")
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            sqlx::query(
                "UPDATE photo_culler_import_checkpoints SET cursor = NULL, processed = 0 WHERE job_id = ?",
            )
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query(
                "UPDATE import_jobs SET discovered = 0, imported = 0, issues = 0, updated_at = ? WHERE id = ? AND state = 'running'",
            )
            .bind(now_text())
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query(
                "UPDATE scan_revisions SET discovered = 0, new_files = 0, modified_files = 0, moved_files = 0, missing_files = 0, completed_at = NULL WHERE id = (SELECT scan_revision_id FROM import_jobs WHERE id = ?)",
            )
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query(
                "INSERT INTO photo_culler_import_manifests(job_id, state) VALUES (?, 'building') ON CONFLICT(job_id) DO UPDATE SET state = 'building'",
            )
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            tx.commit().await.map_err(err)
        })
    }

    fn append_manifest<'a>(
        &'a mut self,
        job_id: &'a str,
        entries: &'a [ManifestEntry],
    ) -> WorkerFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            if entries.is_empty() {
                return Ok(());
            }
            let mut validated = Vec::with_capacity(entries.len());
            for entry in entries {
                let path = normalized_relative_path(Path::new(&entry.path))?;
                if path != entry.path {
                    return Err("manifest path is not in canonical relative form".into());
                }
                let size = i64::try_from(entry.size)
                    .map_err(|_| "manifest file size exceeds SQLite integer range".to_string())?;
                if !matches!(entry.role.as_str(), "RAW" | "JPEG" | "IMAGE" | "SIDECAR") {
                    return Err("manifest entry has an unsupported role".into());
                }
                validated.push(ValidatedManifestEntry {
                    path,
                    size,
                    modified_ns: entry.modified_ns.map(|value| value.to_string()),
                    role: entry.role.clone(),
                });
            }

            let mut tx = self.writer.begin().await.map_err(err)?;
            let state: Option<String> = sqlx::query_scalar(
                "SELECT m.state FROM photo_culler_import_manifests m JOIN import_jobs j ON j.id = m.job_id WHERE m.job_id = ? AND j.state = 'running'",
            )
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?;
            if state.as_deref() != Some("building") {
                return Err(format!(
                    "cannot append to a manifest that is not building for job `{job_id}`"
                ));
            }

            // Stay below SQLite's conservative 999 bind-parameter limit.
            const INSERT_BATCH_SIZE: usize = 128;
            for chunk in validated.chunks(INSERT_BATCH_SIZE) {
                let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                    "INSERT INTO photo_culler_import_manifest_entries(job_id, path, size_bytes, modified_ns, role) ",
                );
                query.push_values(chunk, |mut row, entry| {
                    row.push_bind(job_id)
                        .push_bind(&entry.path)
                        .push_bind(entry.size)
                        .push_bind(entry.modified_ns.as_deref())
                        .push_bind(&entry.role);
                });
                query.build().execute(&mut *tx).await.map_err(err)?;
            }
            tx.commit().await.map_err(err)
        })
    }

    fn complete_manifest<'a>(
        &'a mut self,
        job_id: &'a str,
    ) -> WorkerFuture<'a, Result<(), String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let result = sqlx::query(
                "UPDATE photo_culler_import_manifests SET state = 'complete' WHERE job_id = ? AND state = 'building' AND EXISTS (SELECT 1 FROM import_jobs WHERE id = ? AND state = 'running')",
            )
            .bind(job_id)
            .bind(job_id)
            .execute(&self.writer)
            .await
            .map_err(err)?;
            if result.rows_affected() != 1 {
                return Err(format!(
                    "could not complete import manifest for job `{job_id}`"
                ));
            }
            Ok(())
        })
    }

    fn manifest_complete<'a>(&'a self, job_id: &'a str) -> WorkerFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM photo_culler_import_manifests m JOIN import_jobs j ON j.id = m.job_id WHERE m.job_id = ? AND m.state = 'complete' AND j.state = 'running')",
            )
            .bind(job_id)
            .fetch_one(&self.writer)
            .await
            .map_err(err)
        })
    }

    fn manifest_page<'a>(
        &'a self,
        job_id: &'a str,
        after: Option<&'a str>,
        limit: usize,
    ) -> WorkerFuture<'a, Result<Vec<ManifestEntry>, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            if limit == 0 {
                return Ok(Vec::new());
            }
            const MAX_MANIFEST_PAGE_SIZE: usize = 1024;
            let limit = limit.min(MAX_MANIFEST_PAGE_SIZE);
            let complete: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM photo_culler_import_manifests m JOIN import_jobs j ON j.id = m.job_id WHERE m.job_id = ? AND m.state = 'complete' AND j.state = 'running')",
            )
            .bind(job_id)
            .fetch_one(&self.writer)
            .await
            .map_err(err)?;
            if !complete {
                return Err(format!(
                    "manifest is not complete for running job `{job_id}`"
                ));
            }
            let rows = if let Some(after) = after {
                sqlx::query(
                    "SELECT path, size_bytes, modified_ns, role FROM photo_culler_import_manifest_entries WHERE job_id = ? AND path > ? ORDER BY path LIMIT ?",
                )
                .bind(job_id)
                .bind(after)
                .bind(limit as i64)
                .fetch_all(&self.writer)
                .await
                .map_err(err)?
            } else {
                sqlx::query(
                    "SELECT path, size_bytes, modified_ns, role FROM photo_culler_import_manifest_entries WHERE job_id = ? ORDER BY path LIMIT ?",
                )
                .bind(job_id)
                .bind(limit as i64)
                .fetch_all(&self.writer)
                .await
                .map_err(err)?
            };
            rows.into_iter()
                .map(|row| {
                    let path: String = row.try_get("path").map_err(err)?;
                    let size: i64 = row.try_get("size_bytes").map_err(err)?;
                    let modified_ns: Option<String> = row.try_get("modified_ns").map_err(err)?;
                    let role: String = row.try_get("role").map_err(err)?;
                    Ok(ManifestEntry {
                        path,
                        size: u64::try_from(size)
                            .map_err(|_| "manifest file size is negative".to_string())?,
                        modified_ns: modified_ns
                            .map(|value| {
                                value.parse::<u128>().map_err(|_| {
                                    "manifest modification time is invalid".to_string()
                                })
                            })
                            .transpose()?,
                        role,
                    })
                })
                .collect()
        })
    }

    fn commit_observation<'a>(
        &'a mut self,
        job_id: &'a str,
        cursor: &'a str,
        observation: &'a Observation,
    ) -> WorkerFuture<'a, Result<ObservationCommit, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let relative = normalized_relative_path(&observation.path)?;
            if relative != cursor.replace('\\', "/") {
                return Err("observation cursor does not match its relative path".into());
            }
            let size = i64::try_from(observation.size)
                .map_err(|_| "file size exceeds SQLite integer range".to_string())?;
            let modified = observation
                .modified_ns
                .map(|value| value as f64 / 1_000_000_000.0)
                .unwrap_or(0.0);
            let quick_hash = digest_text(&observation.quick_hash);
            let full_hash = digest_text(&observation.content_hash);
            let role = file_role(&observation.path);

            let mut tx = self.writer.begin().await.map_err(err)?;
            let job = sqlx::query(
                "SELECT j.state, j.source_id, j.cancel_requested, j.pause_requested, c.cursor FROM import_jobs j JOIN photo_culler_import_checkpoints c ON c.job_id = j.id WHERE j.id = ?",
            )
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?
            .ok_or_else(|| format!("import job `{job_id}` does not exist"))?;
            let state: String = job.try_get("state").map_err(err)?;
            let cancel: bool = job.try_get("cancel_requested").map_err(err)?;
            let pause: bool = job.try_get("pause_requested").map_err(err)?;
            let signal = match state.as_str() {
                "cancelled" => Some(ControlSignal::Cancelled),
                "paused" => Some(ControlSignal::Paused),
                "running" if cancel => {
                    set_state_tx(&mut tx, job_id, "running", "cancelled", None, true).await?;
                    Some(ControlSignal::Cancelled)
                }
                "running" if pause => {
                    set_state_tx(&mut tx, job_id, "running", "paused", None, false).await?;
                    persist_pause_flag_tx(&mut tx, job_id).await?;
                    Some(ControlSignal::Paused)
                }
                "running" => None,
                _ => {
                    return Err(format!(
                        "cannot commit an observation for job in state `{state}`"
                    ));
                }
            };
            if let Some(signal) = signal {
                tx.commit().await.map_err(err)?;
                return Ok(match signal {
                    ControlSignal::Paused => ObservationCommit::Paused,
                    ControlSignal::Yielded => unreachable!("yielding is not requested here"),
                    ControlSignal::Cancelled => ObservationCommit::Cancelled,
                    ControlSignal::Continue => unreachable!(),
                });
            }
            let source_id: String = job.try_get("source_id").map_err(err)?;
            let previous_cursor: Option<String> = job.try_get("cursor").map_err(err)?;
            if previous_cursor
                .as_deref()
                .is_some_and(|previous| cursor < previous)
            {
                return Err("observation cursor moved backwards".into());
            }
            let previous = sqlx::query(
                "SELECT size_bytes, quick_hash, full_hash FROM photo_culler_import_observations WHERE job_id = ? AND source_relative_path = ?",
            )
            .bind(job_id)
            .bind(&relative)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?;
            if let Some(previous) = previous {
                let old_size: i64 = previous.try_get("size_bytes").map_err(err)?;
                let old_quick: String = previous.try_get("quick_hash").map_err(err)?;
                let old_full: String = previous.try_get("full_hash").map_err(err)?;
                if old_size != size || old_quick != quick_hash || old_full != full_hash {
                    return Err("a committed observation changed during replay".into());
                }
            } else {
                sqlx::query(
                    "INSERT INTO photo_culler_import_observations(job_id, source_relative_path, size_bytes, modified_time, quick_hash, full_hash, role) VALUES (?, ?, ?, ?, ?, ?, ?)",
                )
                .bind(job_id)
                .bind(&relative)
                .bind(size)
                .bind(modified)
                .bind(&quick_hash)
                .bind(&full_hash)
                .bind(&role)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            }

            let previous = sqlx::query(
                "SELECT f.source_relative_path, f.size_bytes, f.quick_hash, f.full_hash FROM files f WHERE f.import_source_id = ? AND f.source_relative_path = ? ORDER BY f.id LIMIT 1",
            )
            .bind(&source_id)
            .bind(&relative)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?;
            let change = if let Some(previous) = previous {
                let old_size: i64 = previous.try_get("size_bytes").map_err(err)?;
                let old_quick: Option<String> = previous.try_get("quick_hash").map_err(err)?;
                let old_full: Option<String> = previous.try_get("full_hash").map_err(err)?;
                if old_size == size && old_full.as_deref() == Some(full_hash.as_str()) {
                    EntryChange::Unchanged
                } else {
                    let _ = (old_quick, modified);
                    EntryChange::Modified
                }
            } else {
                EntryChange::New
            };
            if previous_cursor.as_deref() != Some(cursor) {
                sqlx::query("UPDATE photo_culler_import_checkpoints SET cursor = ?, processed = processed + 1 WHERE job_id = ?")
                    .bind(cursor)
                    .bind(job_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(err)?;
                sqlx::query("UPDATE import_jobs SET discovered = discovered + 1, updated_at = ? WHERE id = ?")
                    .bind(now_text())
                    .bind(job_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(err)?;
                sqlx::query("UPDATE scan_revisions SET discovered = discovered + 1 WHERE id = (SELECT scan_revision_id FROM import_jobs WHERE id = ?)")
                    .bind(job_id)
                    .execute(&mut *tx)
                    .await
                    .map_err(err)?;
            }
            tx.commit().await.map_err(err)?;
            Ok(ObservationCommit::Committed(change))
        })
    }

    fn finish_job<'a>(
        &'a mut self,
        job_id: &'a str,
    ) -> WorkerFuture<'a, Result<FinishOutcome, String>> {
        Box::pin(async move {
            self.ensure_tables().await.map_err(err)?;
            let mut tx = self.writer.begin().await.map_err(err)?;
            let job = sqlx::query(
                "SELECT j.state, j.cancel_requested, j.pause_requested, j.source_id, j.gallery_id, j.scan_revision_id, r.state AS revision_state, s.path AS source_path, s.recursive AS source_recursive, s.exclude_patterns AS source_exclude_patterns FROM import_jobs j JOIN scan_revisions r ON r.id = j.scan_revision_id JOIN import_sources s ON s.id = j.source_id WHERE j.id = ?",
            )
            .bind(job_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(err)?
            .ok_or_else(|| format!("import job `{job_id}` does not exist"))?;
            let revision_state: String = job.try_get("revision_state").map_err(err)?;
            let job_state: String = job.try_get("state").map_err(err)?;
            if job_state == "cancelled" {
                clear_import_staging_tx(&mut tx, job_id).await?;
                tx.commit().await.map_err(err)?;
                return Ok(FinishOutcome::Cancelled);
            }
            if revision_state == "completed" || job_state == "completed" {
                if job_state != "completed" {
                    set_state_tx(&mut tx, job_id, &job_state, "completed", None, true).await?;
                }
                let revision_id: String = job.try_get("scan_revision_id").map_err(err)?;
                let counts = load_counts(&mut tx, job_id, &revision_id).await?;
                clear_import_staging_tx(&mut tx, job_id).await?;
                tx.commit().await.map_err(err)?;
                return Ok(FinishOutcome::Completed(counts));
            }
            let cancel: bool = job.try_get("cancel_requested").map_err(err)?;
            let pause: bool = job.try_get("pause_requested").map_err(err)?;
            let signal = match job_state.as_str() {
                "cancelled" => Some(ControlSignal::Cancelled),
                "paused" => Some(ControlSignal::Paused),
                "running" if cancel => {
                    set_state_tx(&mut tx, job_id, "running", "cancelled", None, true).await?;
                    Some(ControlSignal::Cancelled)
                }
                "running" if pause => {
                    set_state_tx(&mut tx, job_id, "running", "paused", None, false).await?;
                    persist_pause_flag_tx(&mut tx, job_id).await?;
                    Some(ControlSignal::Paused)
                }
                "running" => None,
                _ => return Err(format!("cannot finish job in state `{job_state}`")),
            };
            if let Some(signal) = signal {
                tx.commit().await.map_err(err)?;
                return Ok(match signal {
                    ControlSignal::Paused => FinishOutcome::Paused,
                    ControlSignal::Yielded => unreachable!("yielding is not requested here"),
                    ControlSignal::Cancelled => FinishOutcome::Cancelled,
                    ControlSignal::Continue => unreachable!(),
                });
            }
            let source_id: String = job.try_get("source_id").map_err(err)?;
            let gallery_id: String = job.try_get("gallery_id").map_err(err)?;
            let revision_id: String = job.try_get("scan_revision_id").map_err(err)?;
            let source_path: String = job.try_get("source_path").map_err(err)?;
            let recursive: bool = job.try_get("source_recursive").map_err(err)?;
            let exclude_patterns: String = job.try_get("source_exclude_patterns").map_err(err)?;
            let exclusion_patterns = serde_json::from_str::<Vec<String>>(&exclude_patterns)
                .ok()
                .map(|patterns| {
                    patterns
                        .iter()
                        .map(|pattern| pattern.trim().replace('\\', "/"))
                        .filter(|pattern| !pattern.is_empty())
                        .collect::<Vec<_>>()
                });
            create_finish_tables(&mut tx).await?;
            let discovered = stage_finish_observations(&mut tx, job_id).await?;
            validate_finish_observations(&mut tx).await?;

            adopt_legacy_files(&mut tx, &source_id, &gallery_id, &source_path).await?;
            stage_finish_existing_files(
                &mut tx,
                &source_id,
                Path::new(&source_path),
                recursive,
                exclusion_patterns.as_deref(),
            )
            .await?;
            validate_finish_existing_keys(&mut tx).await?;
            build_finish_hash_indexes(&mut tx).await?;
            assign_finish_groups(&mut tx, &gallery_id, &source_path).await?;
            validate_finish_group_identities(&mut tx).await?;
            upsert_finish_photos(&mut tx, &gallery_id).await?;
            stage_finish_actions(&mut tx).await?;
            let mut counts = apply_finish_actions(&mut tx, &source_id, &revision_id).await?;

            let missing_update = sqlx::query(
                "UPDATE files SET status = 'missing' WHERE import_source_id = ? AND source_relative_path IS NOT NULL AND COALESCE(last_seen_revision_id, '') <> ? AND status <> 'missing' AND NOT EXISTS (SELECT 1 FROM photo_culler_import_observations o WHERE o.job_id = ? AND o.source_relative_path = files.source_relative_path) AND EXISTS (SELECT 1 FROM temp.pc_finish_old old WHERE old.id = files.id AND old.observed = 0 AND old.in_scope = 1)",
            )
            .bind(&source_id)
            .bind(&revision_id)
            .bind(job_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            counts.missing = missing_update.rows_affected();
            drop_finish_tables(&mut tx).await?;

            sqlx::query(
                "UPDATE scan_revisions SET state = 'completed', discovered = ?, new_files = ?, modified_files = ?, moved_files = ?, missing_files = ?, completed_at = ? WHERE id = ?",
            )
            .bind(discovered)
            .bind(i64::try_from(counts.new).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.modified).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.moved).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.missing).unwrap_or(i64::MAX))
            .bind(now_text())
            .bind(&revision_id)
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query(
                "INSERT INTO photo_culler_import_results(job_id, new_files, modified_files, moved_files, unchanged_files, missing_files) VALUES (?, ?, ?, ?, ?, ?) ON CONFLICT(job_id) DO UPDATE SET new_files = excluded.new_files, modified_files = excluded.modified_files, moved_files = excluded.moved_files, unchanged_files = excluded.unchanged_files, missing_files = excluded.missing_files",
            )
            .bind(job_id)
            .bind(i64::try_from(counts.new).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.modified).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.moved).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.unchanged).unwrap_or(i64::MAX))
            .bind(i64::try_from(counts.missing).unwrap_or(i64::MAX))
            .execute(&mut *tx)
            .await
            .map_err(err)?;
            sqlx::query("UPDATE import_jobs SET imported = ?, updated_at = ? WHERE id = ?")
                .bind(
                    i64::try_from(counts.new + counts.modified + counts.moved + counts.unchanged)
                        .unwrap_or(i64::MAX),
                )
                .bind(now_text())
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            sqlx::query("UPDATE import_jobs SET state = 'completed', error = NULL, pause_requested = 0 WHERE id = ? AND state = 'running'")
                .bind(job_id)
                .execute(&mut *tx)
                .await
                .map_err(err)?;
            clear_import_staging_tx(&mut tx, job_id).await?;
            tx.commit().await.map_err(err)?;
            Ok(FinishOutcome::Completed(counts))
        })
    }
}

const FINISH_STAGE_PAGE_SIZE: i64 = 512;
const FINISH_WRITE_PAGE_SIZE: i64 = 256;

/// Create connection-local relational staging used to finalize a scan without
/// retaining the whole scan and source catalog in Rust collections. All tables
/// are transaction-scoped in practice: successful finalization drops them
/// before commit, and any error rolls their creation back with the catalog tx.
async fn create_finish_tables(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), String> {
    for table in [
        "pc_finish_actions",
        "pc_finish_new_hash_single",
        "pc_finish_old_hash_single",
        "pc_finish_groups",
        "pc_finish_old",
        "pc_finish_new",
    ] {
        sqlx::query(&format!("DROP TABLE IF EXISTS temp.{table}"))
            .execute(&mut **tx)
            .await
            .map_err(err)?;
    }
    for statement in [
        "CREATE TEMP TABLE pc_finish_new (
            path TEXT PRIMARY KEY,
            logical_key TEXT NOT NULL,
            parent_path TEXT NOT NULL,
            exact_key TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            modified_time REAL NOT NULL,
            quick_hash TEXT NOT NULL,
            full_hash TEXT NOT NULL,
            role TEXT NOT NULL
        )",
        "CREATE INDEX pc_finish_new_key_path ON pc_finish_new(logical_key, path)",
        "CREATE INDEX pc_finish_new_hash_size ON pc_finish_new(full_hash, size_bytes)",
        "CREATE TEMP TABLE pc_finish_old (
            id INTEGER PRIMARY KEY,
            photo_id TEXT NOT NULL,
            path TEXT NOT NULL,
            logical_key TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            full_hash TEXT,
            observed INTEGER NOT NULL,
            in_scope INTEGER NOT NULL
        )",
        "CREATE INDEX pc_finish_old_key ON pc_finish_old(logical_key, photo_id)",
        "CREATE INDEX pc_finish_old_path ON pc_finish_old(path, id)",
        "CREATE INDEX pc_finish_old_hash_size ON pc_finish_old(full_hash, size_bytes, observed)",
        "CREATE TEMP TABLE pc_finish_groups (
            logical_key TEXT PRIMARY KEY,
            photo_id TEXT NOT NULL,
            stem_name TEXT NOT NULL,
            photo_row_id INTEGER
        )",
        "CREATE TEMP TABLE pc_finish_new_hash_single (
            full_hash TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            path TEXT NOT NULL,
            PRIMARY KEY(full_hash, size_bytes)
        )",
        "CREATE TEMP TABLE pc_finish_old_hash_single (
            full_hash TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            id INTEGER NOT NULL,
            photo_id TEXT NOT NULL,
            PRIMARY KEY(full_hash, size_bytes)
        )",
        "CREATE TEMP TABLE pc_finish_actions (
            logical_key TEXT NOT NULL,
            path TEXT NOT NULL,
            size_bytes INTEGER NOT NULL,
            modified_time REAL NOT NULL,
            quick_hash TEXT NOT NULL,
            full_hash TEXT NOT NULL,
            role TEXT NOT NULL,
            photo_id TEXT NOT NULL,
            photo_row_id INTEGER NOT NULL,
            exact_id INTEGER,
            exact_size INTEGER,
            exact_full_hash TEXT,
            moved_id INTEGER,
            PRIMARY KEY(logical_key, path)
        )",
    ] {
        sqlx::query(statement)
            .execute(&mut **tx)
            .await
            .map_err(err)?;
    }
    Ok(())
}

async fn stage_finish_observations(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: &str,
) -> Result<i64, String> {
    let mut after: Option<String> = None;
    let mut discovered = 0_i64;
    loop {
        let rows = if let Some(after) = after.as_deref() {
            sqlx::query(
                "SELECT source_relative_path, size_bytes, modified_time, quick_hash, full_hash, role FROM photo_culler_import_observations WHERE job_id = ? AND source_relative_path > ? ORDER BY source_relative_path LIMIT ?",
            )
            .bind(job_id)
            .bind(after)
            .bind(FINISH_STAGE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        } else {
            sqlx::query(
                "SELECT source_relative_path, size_bytes, modified_time, quick_hash, full_hash, role FROM photo_culler_import_observations WHERE job_id = ? ORDER BY source_relative_path LIMIT ?",
            )
            .bind(job_id)
            .bind(FINISH_STAGE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        };
        if rows.is_empty() {
            break;
        }
        let page = rows
            .into_iter()
            .map(ObservationRow::from_row)
            .collect::<Result<Vec<_>, _>>()?;
        after = page.last().map(|row| row.path.clone());
        discovered = discovered.saturating_add(i64::try_from(page.len()).unwrap_or(i64::MAX));
        let keyed = page
            .into_iter()
            .map(|row| {
                let key = logical_key(&row.path);
                let exact_key = case_sensitive_logical_key(&row.path);
                // Paths in the observation table use portable '/' separators.
                // Keep the original spelling to detect case-folding collisions.
                let parent = row
                    .path
                    .rsplit_once('/')
                    .map_or("", |(parent, _)| parent)
                    .to_owned();
                (row, key, parent, exact_key)
            })
            .collect::<Vec<_>>();
        for chunk in keyed.chunks(110) {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO temp.pc_finish_new(path, logical_key, parent_path, exact_key, size_bytes, modified_time, quick_hash, full_hash, role) ",
            );
            query.push_values(chunk, |mut builder, (row, key, parent, exact_key)| {
                builder
                    .push_bind(&row.path)
                    .push_bind(key)
                    .push_bind(parent)
                    .push_bind(exact_key)
                    .push_bind(row.size)
                    .push_bind(row.modified)
                    .push_bind(&row.quick_hash)
                    .push_bind(&row.full_hash)
                    .push_bind(&row.role);
            });
            query.build().execute(&mut **tx).await.map_err(err)?;
        }
    }
    Ok(discovered)
}

async fn validate_finish_observations(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), String> {
    // Case folding the legacy logical key must never join files from distinct
    // directories on a case-sensitive filesystem, including sidecars.
    let conflicting_directories: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pc_finish_new GROUP BY logical_key HAVING COUNT(DISTINCT parent_path COLLATE BINARY) > 1)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(err)?;
    if conflicting_directories != 0 {
        return Err(ImportValidationError::DirectoryCase.code().into());
    }
    // Different stem spelling can refer to distinct files on case-sensitive
    // sources. Do not silently attach their media or sidecars to one photo.
    let conflicting_stems: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pc_finish_new GROUP BY logical_key HAVING COUNT(DISTINCT exact_key COLLATE BINARY) > 1)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(err)?;
    if conflicting_stems != 0 {
        return Err(ImportValidationError::NameCase.code().into());
    }
    let ambiguous: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pc_finish_new WHERE role <> 'SIDECAR' GROUP BY logical_key HAVING SUM(role = 'RAW') > 1 OR SUM(role = 'JPEG') > 1 OR SUM(role = 'IMAGE') > 1)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(err)?;
    if ambiguous != 0 {
        return Err(ImportValidationError::AmbiguousMedia.code().into());
    }
    let unattached: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM pc_finish_new sidecar WHERE sidecar.role = 'SIDECAR' AND NOT EXISTS (SELECT 1 FROM pc_finish_new media WHERE media.logical_key = sidecar.logical_key AND media.role <> 'SIDECAR')",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(err)?;
    if unattached > 0 {
        return Err(ImportValidationError::UnassociatedSidecar.code().into());
    }
    Ok(())
}

async fn stage_finish_existing_files(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_id: &str,
    source_root: &Path,
    recursive: bool,
    exclusion_patterns: Option<&[String]>,
) -> Result<(), String> {
    let mut after_id: Option<i64> = None;
    loop {
        let rows = if let Some(after_id) = after_id {
            sqlx::query(
                "SELECT f.id, p.photo_id, f.source_relative_path, f.size_bytes, f.full_hash, EXISTS(SELECT 1 FROM pc_finish_new n WHERE n.path = f.source_relative_path) AS observed_in_scan FROM files f JOIN photos p ON p.id = f.photo_id WHERE f.import_source_id = ? AND f.source_relative_path IS NOT NULL AND f.id > ? ORDER BY f.id LIMIT ?",
            )
            .bind(source_id)
            .bind(after_id)
            .bind(FINISH_STAGE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        } else {
            sqlx::query(
                "SELECT f.id, p.photo_id, f.source_relative_path, f.size_bytes, f.full_hash, EXISTS(SELECT 1 FROM pc_finish_new n WHERE n.path = f.source_relative_path) AS observed_in_scan FROM files f JOIN photos p ON p.id = f.photo_id WHERE f.import_source_id = ? AND f.source_relative_path IS NOT NULL ORDER BY f.id LIMIT ?",
            )
            .bind(source_id)
            .bind(FINISH_STAGE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        };
        if rows.is_empty() {
            break;
        }
        let page = rows
            .into_iter()
            .map(ExistingFile::from_row)
            .collect::<Result<Vec<_>, _>>()?;
        after_id = page.last().map(|file| file.id);
        let keyed = page
            .into_iter()
            .map(|file| {
                let path = file
                    .path
                    .as_deref()
                    .ok_or_else(|| "source file has no relative path".to_string())?;
                let outside_scope = !file.observed_in_scan
                    && old_path_is_outside_reconciliation_scope(
                        source_root,
                        path,
                        recursive,
                        exclusion_patterns,
                    );
                let key = logical_key(path);
                Ok((file, key, outside_scope))
            })
            .collect::<Result<Vec<_>, String>>()?;
        for chunk in keyed.chunks(128) {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO temp.pc_finish_old(id, photo_id, path, logical_key, size_bytes, full_hash, observed, in_scope) ",
            );
            query.push_values(chunk, |mut builder, (file, key, outside_scope)| {
                builder
                    .push_bind(file.id)
                    .push_bind(&file.photo_id)
                    .push_bind(file.path.as_deref().unwrap_or_default())
                    .push_bind(key)
                    .push_bind(file.size)
                    .push_bind(file.full_hash.as_deref())
                    .push_bind(file.observed_in_scan || *outside_scope)
                    .push_bind(!*outside_scope);
            });
            query.build().execute(&mut **tx).await.map_err(err)?;
        }
    }
    Ok(())
}

async fn validate_finish_existing_keys(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), String> {
    let conflicting_key: Option<String> = sqlx::query_scalar(
        "SELECT old.logical_key FROM pc_finish_old old JOIN pc_finish_new new ON new.logical_key = old.logical_key WHERE old.in_scope = 1 AND new.role <> 'SIDECAR' GROUP BY old.logical_key HAVING COUNT(DISTINCT old.photo_id) > 1 LIMIT 1",
    )
    .fetch_optional(&mut **tx)
    .await
    .map_err(err)?;
    if let Some(key) = conflicting_key {
        return Err(format!(
            "cannot complete import: existing files for logical key `{key}` belong to different photo identities"
        ));
    }
    Ok(())
}

async fn build_finish_hash_indexes(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pc_finish_new_hash_single(full_hash, size_bytes, path) SELECT full_hash, size_bytes, MIN(path) FROM pc_finish_new GROUP BY full_hash, size_bytes HAVING COUNT(*) = 1",
    )
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    sqlx::query(
        "INSERT INTO pc_finish_old_hash_single(full_hash, size_bytes, id, photo_id) SELECT full_hash, size_bytes, MIN(id), MIN(photo_id) FROM pc_finish_old WHERE observed = 0 AND in_scope = 1 AND full_hash IS NOT NULL GROUP BY full_hash, size_bytes HAVING COUNT(*) = 1",
    )
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    Ok(())
}

async fn assign_finish_groups(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    gallery_id: &str,
    source_path: &str,
) -> Result<(), String> {
    let mut after: Option<String> = None;
    loop {
        let keys = if let Some(after) = after.as_deref() {
            sqlx::query_scalar::<_, String>(
                "SELECT logical_key FROM pc_finish_new WHERE role <> 'SIDECAR' AND logical_key > ? GROUP BY logical_key ORDER BY logical_key LIMIT ?",
            )
            .bind(after)
            .bind(FINISH_WRITE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        } else {
            sqlx::query_scalar::<_, String>(
                "SELECT logical_key FROM pc_finish_new WHERE role <> 'SIDECAR' GROUP BY logical_key ORDER BY logical_key LIMIT ?",
            )
            .bind(FINISH_WRITE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        };
        if keys.is_empty() {
            break;
        }
        let mut page = sqlx::QueryBuilder::<sqlx::Sqlite>::new("WITH page(logical_key) AS (");
        page.push_values(&keys, |mut builder, key| {
            builder.push_bind(key);
        });
        page.push(
            "), resolved AS (
                SELECT page.logical_key,
                    (SELECT old.photo_id FROM pc_finish_new new JOIN pc_finish_old old ON old.path = new.path AND old.in_scope = 1 WHERE new.logical_key = page.logical_key ORDER BY (new.role = 'SIDECAR'), new.path, old.id DESC LIMIT 1) AS exact_photo_id,
                    (SELECT path FROM pc_finish_new WHERE logical_key = page.logical_key AND role <> 'SIDECAR' ORDER BY path LIMIT 1) AS first_media_path,
                    (SELECT full_hash FROM pc_finish_new WHERE logical_key = page.logical_key AND role <> 'SIDECAR' ORDER BY path LIMIT 1) AS first_media_hash
                FROM page
            ), candidates AS (
                SELECT new.logical_key, old.photo_id
                FROM pc_finish_new new
                JOIN resolved ON resolved.logical_key = new.logical_key
                JOIN pc_finish_new_hash_single nh ON nh.full_hash = new.full_hash AND nh.size_bytes = new.size_bytes AND nh.path = new.path
                JOIN pc_finish_old_hash_single old ON old.full_hash = nh.full_hash AND old.size_bytes = nh.size_bytes
            )
            SELECT resolved.logical_key, resolved.exact_photo_id, resolved.first_media_path, resolved.first_media_hash,
                COUNT(DISTINCT candidates.photo_id) AS moved_id_count,
                MIN(candidates.photo_id) AS moved_photo_id,
                COUNT(DISTINCT CASE WHEN resolved.exact_photo_id IS NOT NULL AND candidates.photo_id <> resolved.exact_photo_id THEN candidates.photo_id END) AS conflicting_move_count
            FROM resolved LEFT JOIN candidates ON candidates.logical_key = resolved.logical_key
            GROUP BY resolved.logical_key, resolved.exact_photo_id, resolved.first_media_path
            ORDER BY resolved.logical_key",
        );
        let rows = page.build().fetch_all(&mut **tx).await.map_err(err)?;
        let mut inserts = Vec::with_capacity(rows.len());
        for row in rows {
            let key: String = row.try_get("logical_key").map_err(err)?;
            let exact: Option<String> = row.try_get("exact_photo_id").map_err(err)?;
            let moved_id_count: i64 = row.try_get("moved_id_count").map_err(err)?;
            let moved: Option<String> = row.try_get("moved_photo_id").map_err(err)?;
            let conflicting_move_count: i64 = row.try_get("conflicting_move_count").map_err(err)?;
            let first_media_hash: String = row.try_get("first_media_hash").map_err(err)?;
            if conflicting_move_count > 0 || (exact.is_none() && moved_id_count > 1) {
                return Err(
                    "import contains conflicting move identities; no catalog changes were applied"
                        .into(),
                );
            }
            let photo_id = if let Some(exact) = exact {
                exact
            } else if moved_id_count == 1 {
                moved.ok_or_else(|| "move candidate has no photo identity".to_string())?
            } else {
                let path_identity = stable_photo_id(gallery_id, source_path, &key)?;
                let path_identity_exists: bool =
                    sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id = ?)")
                        .bind(&path_identity)
                        .fetch_one(&mut **tx)
                        .await
                        .map_err(err)?;
                if path_identity_exists {
                    // A deterministic path ID may belong to a photo that was
                    // previously moved away. Reusing it would transfer that
                    // photo's decision and analysis to unrelated contents.
                    // Derive a second deterministic ID from the complete
                    // content hash. Subsequent imports of the new path match
                    // this ID exactly; the historical ID remains reserved.
                    let content_identity = stable_photo_id_for_reused_path(
                        gallery_id,
                        source_path,
                        &key,
                        &first_media_hash,
                    )?;
                    let content_identity_exists: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM photos WHERE photo_id = ?)",
                    )
                    .bind(&content_identity)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(err)?;
                    if content_identity_exists {
                        return Err(
                            "import contains a reused-path identity that conflicts with an existing photo; no catalog changes were applied"
                                .into(),
                        );
                    }
                    content_identity
                } else {
                    path_identity
                }
            };
            let first_media_path: String = row.try_get("first_media_path").map_err(err)?;
            let stem = Path::new(&first_media_path)
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("Photo");
            inserts.push((key, photo_id, stem.to_owned()));
        }
        for chunk in inserts.chunks(128) {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO pc_finish_groups(logical_key, photo_id, stem_name) ",
            );
            query.push_values(chunk, |mut builder, (key, photo_id, stem)| {
                builder.push_bind(key).push_bind(photo_id).push_bind(stem);
            });
            query.build().execute(&mut **tx).await.map_err(err)?;
        }
        after = keys.last().cloned();
    }
    Ok(())
}

async fn validate_finish_group_identities(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
) -> Result<(), String> {
    // Different observed groups may resolve to the same historical identity
    // through exact paths or a partially moved pair. Do not silently merge
    // them or choose which group keeps existing decisions.
    let conflicting: i64 = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pc_finish_groups GROUP BY photo_id HAVING COUNT(*) > 1)",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(err)?;
    if conflicting != 0 {
        return Err(ImportValidationError::ConflictingPhotoIdentity
            .code()
            .into());
    }
    Ok(())
}

async fn upsert_finish_photos(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    gallery_id: &str,
) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO photos(photo_id, gallery_id, stem_name, analysis_summary_json) SELECT groups.photo_id, ?, groups.stem_name, '{}' FROM pc_finish_groups groups WHERE groups.logical_key = (SELECT MAX(candidate.logical_key) FROM pc_finish_groups candidate WHERE candidate.photo_id = groups.photo_id) ON CONFLICT(photo_id) DO UPDATE SET gallery_id = excluded.gallery_id, stem_name = excluded.stem_name",
    )
    .bind(gallery_id)
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    sqlx::query(
        "UPDATE pc_finish_groups SET photo_row_id = (SELECT id FROM photos WHERE photo_id = pc_finish_groups.photo_id)",
    )
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    Ok(())
}

async fn stage_finish_actions(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), String> {
    sqlx::query(
        "INSERT INTO pc_finish_actions(logical_key, path, size_bytes, modified_time, quick_hash, full_hash, role, photo_id, photo_row_id, exact_id, exact_size, exact_full_hash, moved_id) SELECT new.logical_key, new.path, new.size_bytes, new.modified_time, new.quick_hash, new.full_hash, new.role, groups.photo_id, groups.photo_row_id, exact.id, exact.size_bytes, exact.full_hash, CASE WHEN exact.id IS NULL AND moved.photo_id = groups.photo_id THEN moved.id END FROM pc_finish_new new JOIN pc_finish_groups groups ON groups.logical_key = new.logical_key LEFT JOIN pc_finish_old exact ON exact.id = (SELECT MAX(candidate.id) FROM pc_finish_old candidate WHERE candidate.path = new.path AND candidate.in_scope = 1) LEFT JOIN pc_finish_old_hash_single moved ON moved.full_hash = new.full_hash AND moved.size_bytes = new.size_bytes AND moved.photo_id = groups.photo_id AND EXISTS (SELECT 1 FROM pc_finish_new_hash_single unique_new WHERE unique_new.full_hash = new.full_hash AND unique_new.size_bytes = new.size_bytes AND unique_new.path = new.path)",
    )
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    Ok(())
}

async fn apply_finish_actions(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_id: &str,
    revision_id: &str,
) -> Result<ImportCounts, String> {
    let mut after_key: Option<String> = None;
    let mut after_path: Option<String> = None;
    let mut counts = ImportCounts::default();
    loop {
        let rows = if let (Some(after_key), Some(after_path)) =
            (after_key.as_deref(), after_path.as_deref())
        {
            sqlx::query(
                "SELECT logical_key, path, size_bytes, modified_time, quick_hash, full_hash, role, photo_row_id, exact_id, exact_size, exact_full_hash, moved_id FROM pc_finish_actions WHERE logical_key > ? OR (logical_key = ? AND path > ?) ORDER BY logical_key, path LIMIT ?",
            )
            .bind(after_key)
            .bind(after_key)
            .bind(after_path)
            .bind(FINISH_WRITE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        } else {
            sqlx::query(
                "SELECT logical_key, path, size_bytes, modified_time, quick_hash, full_hash, role, photo_row_id, exact_id, exact_size, exact_full_hash, moved_id FROM pc_finish_actions ORDER BY logical_key, path LIMIT ?",
            )
            .bind(FINISH_WRITE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        };
        if rows.is_empty() {
            break;
        }
        let last = rows
            .last()
            .ok_or_else(|| "file action page was unexpectedly empty".to_string())?;
        let last_key: String = last.try_get("logical_key").map_err(err)?;
        let last_path: String = last.try_get("path").map_err(err)?;
        for row in rows {
            let path: String = row.try_get("path").map_err(err)?;
            let size: i64 = row.try_get("size_bytes").map_err(err)?;
            let modified: f64 = row.try_get("modified_time").map_err(err)?;
            let quick_hash: String = row.try_get("quick_hash").map_err(err)?;
            let full_hash: String = row.try_get("full_hash").map_err(err)?;
            let role: String = row.try_get("role").map_err(err)?;
            let photo_row_id: i64 = row.try_get("photo_row_id").map_err(err)?;
            let exact_id: Option<i64> = row.try_get("exact_id").map_err(err)?;
            let exact_size: Option<i64> = row.try_get("exact_size").map_err(err)?;
            let exact_full_hash: Option<String> = row.try_get("exact_full_hash").map_err(err)?;
            let moved_id: Option<i64> = row.try_get("moved_id").map_err(err)?;
            if let Some(file_id) = exact_id.or(moved_id) {
                if exact_id.is_some() {
                    if exact_size == Some(size)
                        && exact_full_hash.as_deref() == Some(full_hash.as_str())
                    {
                        counts.unchanged += 1;
                    } else {
                        counts.modified += 1;
                    }
                } else {
                    counts.moved += 1;
                }
                sqlx::query(
                    "UPDATE files SET photo_id = ?, import_source_id = ?, last_seen_revision_id = ?, relative_path = ?, source_relative_path = ?, status = 'present', role = ?, size_bytes = ?, modified_time = ?, quick_hash = ?, full_hash = ? WHERE id = ?",
                )
                .bind(photo_row_id)
                .bind(source_id)
                .bind(revision_id)
                .bind(&path)
                .bind(&path)
                .bind(role)
                .bind(size)
                .bind(modified)
                .bind(quick_hash)
                .bind(full_hash)
                .bind(file_id)
                .execute(&mut **tx)
                .await
                .map_err(err)?;
            } else {
                counts.new += 1;
                sqlx::query(
                    "INSERT INTO files(photo_id, import_source_id, last_seen_revision_id, relative_path, source_relative_path, status, role, size_bytes, modified_time, quick_hash, full_hash) VALUES (?, ?, ?, ?, ?, 'present', ?, ?, ?, ?, ?) ON CONFLICT(photo_id, relative_path) DO UPDATE SET import_source_id = excluded.import_source_id, last_seen_revision_id = excluded.last_seen_revision_id, source_relative_path = excluded.source_relative_path, status = 'present', role = excluded.role, size_bytes = excluded.size_bytes, modified_time = excluded.modified_time, quick_hash = excluded.quick_hash, full_hash = excluded.full_hash",
                )
                .bind(photo_row_id)
                .bind(source_id)
                .bind(revision_id)
                .bind(&path)
                .bind(&path)
                .bind(role)
                .bind(size)
                .bind(modified)
                .bind(quick_hash)
                .bind(full_hash)
                .execute(&mut **tx)
                .await
                .map_err(err)?;
            }
        }
        after_key = Some(last_key);
        after_path = Some(last_path);
    }
    Ok(counts)
}

async fn drop_finish_tables(tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>) -> Result<(), String> {
    for table in [
        "pc_finish_actions",
        "pc_finish_new_hash_single",
        "pc_finish_old_hash_single",
        "pc_finish_groups",
        "pc_finish_old",
        "pc_finish_new",
    ] {
        sqlx::query(&format!("DROP TABLE temp.{table}"))
            .execute(&mut **tx)
            .await
            .map_err(err)?;
    }
    Ok(())
}

#[derive(Clone)]
struct ObservationRow {
    path: String,
    size: i64,
    modified: f64,
    quick_hash: String,
    full_hash: String,
    role: String,
}

impl ObservationRow {
    fn from_row(row: sqlx::sqlite::SqliteRow) -> Result<Self, String> {
        Ok(Self {
            path: row.try_get("source_relative_path").map_err(err)?,
            size: row.try_get("size_bytes").map_err(err)?,
            modified: row.try_get("modified_time").map_err(err)?,
            quick_hash: row.try_get("quick_hash").map_err(err)?,
            full_hash: row.try_get("full_hash").map_err(err)?,
            role: row.try_get("role").map_err(err)?,
        })
    }
}

struct ExistingFile {
    id: i64,
    photo_id: String,
    path: Option<String>,
    size: i64,
    full_hash: Option<String>,
    observed_in_scan: bool,
}

/// Return true when a prior catalog path cannot safely participate in this
/// scan's absence or move reconciliation. The scanner deliberately skips
/// exclusions, hidden entries, symlinks, and selected platform directories.
/// Exclusions are checked against the same normalized glob semantics as the
/// scanner, including every ancestor directory because an excluded directory
/// prevents traversal of all descendants.
fn old_path_is_outside_reconciliation_scope(
    source_root: &Path,
    relative_path: &str,
    recursive: bool,
    exclusion_patterns: Option<&[String]>,
) -> bool {
    let Some(exclusion_patterns) = exclusion_patterns else {
        return true;
    };

    let path = Path::new(relative_path);
    if path.is_absolute() {
        return true;
    }
    let components = path.components().collect::<Vec<_>>();
    if components.is_empty()
        || components
            .iter()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return true;
    }
    if !recursive && components.len() > 1 {
        return true;
    }

    let mut absolute = source_root.to_path_buf();
    let mut relative_prefix = PathBuf::new();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            return true;
        };
        let Some(name) = name.to_str() else {
            return true;
        };
        if name.starts_with('.')
            || matches!(name.to_ascii_lowercase().as_str(), "__pycache__" | "@eadir")
        {
            return true;
        }
        relative_prefix.push(name);
        if scanning::excluded(&relative_prefix, exclusion_patterns) {
            return true;
        }
        absolute.push(name);
        match std::fs::symlink_metadata(&absolute) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return true;
                }
                // If a path still exists but was not observed, it was skipped
                // or unreadable. Absence is not evidence that it was deleted.
                if index + 1 == components.len() {
                    return true;
                }
                if !metadata.is_dir() {
                    return true;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return false,
            Err(_) => return true,
        }
    }
    false
}

impl ExistingFile {
    fn from_row(row: sqlx::sqlite::SqliteRow) -> Result<Self, String> {
        Ok(Self {
            id: row.try_get("id").map_err(err)?,
            photo_id: row.try_get("photo_id").map_err(err)?,
            path: row.try_get("source_relative_path").map_err(err)?,
            size: row.try_get("size_bytes").map_err(err)?,
            full_hash: row.try_get("full_hash").map_err(err)?,
            observed_in_scan: row.try_get("observed_in_scan").map_err(err)?,
        })
    }
}

async fn load_job(pool: &SqlitePool, job_id: &str) -> Result<ImportJob, String> {
    let row = sqlx::query(
        "SELECT j.id, j.source_id, j.state, j.error, c.cursor, COALESCE(c.processed, 0) AS processed, COALESCE(c.revision, 0) AS revision FROM import_jobs j LEFT JOIN photo_culler_import_checkpoints c ON c.job_id = j.id WHERE j.id = ?",
    )
    .bind(job_id)
    .fetch_optional(pool)
    .await
    .map_err(err)?
    .ok_or_else(|| format!("import job `{job_id}` does not exist"))?;
    job_from_row(row)
}

fn job_from_row(row: sqlx::sqlite::SqliteRow) -> Result<ImportJob, String> {
    let state_name: String = row.try_get("state").map_err(err)?;
    let error: Option<String> = row.try_get("error").map_err(err)?;
    let state = match state_name.as_str() {
        "queued" => JobState::Queued,
        "running" => JobState::Running,
        "paused" => JobState::Paused,
        "completed" => JobState::Completed,
        "cancelled" => JobState::Cancelled,
        "failed" => JobState::Failed(error.unwrap_or(state_name)),
        _ => JobState::Failed(format!("unknown persisted job state `{state_name}`")),
    };
    Ok(ImportJob {
        id: row.try_get("id").map_err(err)?,
        source_id: row.try_get("source_id").map_err(err)?,
        revision: as_u64(row.try_get("revision").map_err(err)?),
        cursor: row.try_get("cursor").map_err(err)?,
        state,
        processed: u64::try_from(row.try_get::<i64, _>("processed").map_err(err)?).unwrap_or(0),
    })
}

async fn load_counts(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: &str,
    revision_id: &str,
) -> Result<ImportCounts, String> {
    let row = sqlx::query("SELECT new_files, modified_files, moved_files, missing_files FROM photo_culler_import_results WHERE job_id = ?")
        .bind(job_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(err)?;
    let Some(row) = row else {
        let row = sqlx::query("SELECT new_files, modified_files, moved_files, missing_files FROM scan_revisions WHERE id = ?")
            .bind(revision_id)
            .fetch_one(&mut **tx)
            .await
            .map_err(err)?;
        return Ok(ImportCounts {
            new: as_u64(row.try_get("new_files").map_err(err)?),
            modified: as_u64(row.try_get("modified_files").map_err(err)?),
            moved: as_u64(row.try_get("moved_files").map_err(err)?),
            unchanged: 0,
            missing: as_u64(row.try_get("missing_files").map_err(err)?),
        });
    };
    Ok(ImportCounts {
        new: as_u64(row.try_get("new_files").map_err(err)?),
        modified: as_u64(row.try_get("modified_files").map_err(err)?),
        moved: as_u64(row.try_get("moved_files").map_err(err)?),
        unchanged: as_u64(row.try_get("unchanged_files").map_err(err)?),
        missing: as_u64(row.try_get("missing_files").map_err(err)?),
    })
}

fn state_name(state: &JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Running => "running",
        JobState::Paused => "paused",
        JobState::Completed => "completed",
        JobState::Cancelled => "cancelled",
        JobState::Failed(_) => "failed",
    }
}

async fn persist_pause_flag_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: &str,
) -> Result<(), String> {
    let result =
        sqlx::query("UPDATE import_jobs SET pause_requested = 1 WHERE id = ? AND state = 'paused'")
            .bind(job_id)
            .execute(&mut **tx)
            .await
            .map_err(err)?;
    if result.rows_affected() != 1 {
        return Err(format!("job `{job_id}` changed state during pause request"));
    }
    Ok(())
}

async fn persist_cancel_flag_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: &str,
) -> Result<(), String> {
    let result = sqlx::query(
        "UPDATE import_jobs SET cancel_requested = 1, pause_requested = 0 WHERE id = ? AND state = 'cancelled'",
    )
    .bind(job_id)
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    if result.rows_affected() != 1 {
        return Err(format!(
            "job `{job_id}` changed state during cancel request"
        ));
    }
    Ok(())
}

async fn set_state_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: &str,
    expected: &str,
    next: &str,
    error: Option<&str>,
    terminal: bool,
) -> Result<(), String> {
    let now = now_text();
    let result = sqlx::query(
        "UPDATE import_jobs SET state = ?, error = ?, updated_at = ?, pause_requested = CASE WHEN ? IN ('paused', 'queued', 'cancelled', 'completed', 'failed') THEN 0 ELSE pause_requested END WHERE id = ? AND state = ?",
    )
    .bind(next)
    .bind(error)
    .bind(&now)
    .bind(next)
    .bind(job_id)
    .bind(expected)
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    if result.rows_affected() != 1 {
        return Err(format!("job `{job_id}` changed state during transition"));
    }
    if terminal {
        sqlx::query("UPDATE scan_revisions SET state = ?, completed_at = ? WHERE id = (SELECT scan_revision_id FROM import_jobs WHERE id = ?)")
            .bind(next)
            .bind(&now)
            .bind(job_id)
            .execute(&mut **tx)
            .await
            .map_err(err)?;
    } else {
        sqlx::query("UPDATE scan_revisions SET state = ? WHERE id = (SELECT scan_revision_id FROM import_jobs WHERE id = ?)")
            .bind(next)
            .bind(job_id)
            .execute(&mut **tx)
            .await
            .map_err(err)?;
    }
    if terminal && matches!(next, "cancelled" | "completed") {
        clear_import_staging_tx(tx, job_id).await?;
    }
    Ok(())
}

pub(crate) async fn clear_import_staging_tx(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    job_id: &str,
) -> Result<(), String> {
    // These rows are rebuildable scan staging, not job history. Catalog rows,
    // terminal job/revision state, and completed results are persisted
    // separately; retaining observations after a terminal state would grow
    // the database once per scan.
    sqlx::query("DELETE FROM photo_culler_import_observations WHERE job_id = ?")
        .bind(job_id)
        .execute(&mut **tx)
        .await
        .map_err(err)?;
    sqlx::query("DELETE FROM photo_culler_import_manifest_entries WHERE job_id = ?")
        .bind(job_id)
        .execute(&mut **tx)
        .await
        .map_err(err)?;
    sqlx::query("DELETE FROM photo_culler_import_manifests WHERE job_id = ?")
        .bind(job_id)
        .execute(&mut **tx)
        .await
        .map_err(err)?;
    Ok(())
}

fn new_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    format!("{nanos:024x}{counter:08x}")
}

fn now_text() -> String {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos())
}

fn normalized_relative_path(path: &Path) -> Result<String, String> {
    if path.is_absolute() {
        return Err("import observation path must be source-relative".into());
    }
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err("import observation path escapes its source root".into());
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        return Err("import observation path is empty".into());
    }
    Ok(normalized.to_string_lossy().replace('\\', "/"))
}

/// Attach unambiguous Python-era absolute file paths to the source that owns
/// them. Legacy rows have no provenance, so only rows whose absolute path is
/// lexically contained by exactly one registered source root are eligible.
/// Existing source-relative paths must agree with the derived path, and any
/// collision with an already-associated row is left untouched.
async fn adopt_legacy_files(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    source_id: &str,
    gallery_id: &str,
    source_path: &str,
) -> Result<(), String> {
    let configured_root = Path::new(source_path);
    let Ok(root) = std::fs::canonicalize(configured_root) else {
        return Ok(());
    };

    let sources = sqlx::query("SELECT id, path FROM import_sources WHERE gallery_id = ?")
        .bind(gallery_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(err)?;
    let source_paths = sources
        .into_iter()
        .map(|row| {
            Ok((
                row.try_get::<String, _>("id").map_err(err)?,
                row.try_get::<String, _>("path").map_err(err)?,
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let mut roots = Vec::with_capacity(source_paths.len());
    for (id, path) in source_paths {
        let Ok(root) = std::fs::canonicalize(path) else {
            // An unresolved registered root could overlap this source. Leave
            // legacy provenance untouched until ownership can be established.
            return Ok(());
        };
        roots.push((id, root));
    }
    sqlx::query("DROP TABLE IF EXISTS temp.pc_finish_legacy")
        .execute(&mut **tx)
        .await
        .map_err(err)?;
    sqlx::query(
        "CREATE TEMP TABLE pc_finish_legacy (id INTEGER PRIMARY KEY, relative TEXT NOT NULL)",
    )
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    sqlx::query("CREATE INDEX pc_finish_legacy_relative ON pc_finish_legacy(relative)")
        .execute(&mut **tx)
        .await
        .map_err(err)?;

    let mut after_id: Option<i64> = None;
    loop {
        let rows = if let Some(after_id) = after_id {
            sqlx::query(
                "SELECT f.id, f.import_source_id, f.source_relative_path, f.relative_path FROM files f JOIN photos p ON p.id = f.photo_id WHERE p.gallery_id = ? AND (f.import_source_id IS NULL OR f.source_relative_path IS NULL) AND f.id > ? ORDER BY f.id LIMIT ?",
            )
            .bind(gallery_id)
            .bind(after_id)
            .bind(FINISH_STAGE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        } else {
            sqlx::query(
                "SELECT f.id, f.import_source_id, f.source_relative_path, f.relative_path FROM files f JOIN photos p ON p.id = f.photo_id WHERE p.gallery_id = ? AND (f.import_source_id IS NULL OR f.source_relative_path IS NULL) ORDER BY f.id LIMIT ?",
            )
            .bind(gallery_id)
            .bind(FINISH_STAGE_PAGE_SIZE)
            .fetch_all(&mut **tx)
            .await
            .map_err(err)?
        };
        if rows.is_empty() {
            break;
        }
        after_id = rows
            .last()
            .map(|row| row.try_get::<i64, _>("id"))
            .transpose()
            .map_err(err)?;
        let mut candidates = Vec::new();
        for row in rows {
            let row_source: Option<String> = row.try_get("import_source_id").map_err(err)?;
            if row_source.as_deref().is_some_and(|id| id != source_id) {
                continue;
            }
            let legacy_path: String = row.try_get("relative_path").map_err(err)?;
            let absolute = Path::new(&legacy_path);
            let resolved = std::fs::canonicalize(absolute).ok();
            let Some(relative) = legacy_relative_path(&root, absolute, resolved.as_deref()) else {
                continue;
            };
            // If another configured source also contains the legacy absolute
            // path, its owner cannot be inferred from this row alone.
            let mut owning_source_count = 0;
            let mut source_owns_path = false;
            for (id, candidate_root) in &roots {
                if legacy_relative_path(candidate_root, absolute, resolved.as_deref()).is_some() {
                    owning_source_count += 1;
                    source_owns_path |= id == source_id;
                }
            }
            if owning_source_count != 1 || !source_owns_path {
                continue;
            }
            let stored_relative: Option<String> =
                row.try_get("source_relative_path").map_err(err)?;
            if stored_relative
                .as_deref()
                .is_some_and(|stored| stored.replace('\\', "/") != relative)
            {
                continue;
            }
            let id: i64 = row.try_get("id").map_err(err)?;
            candidates.push((id, relative));
        }
        for chunk in candidates.chunks(128) {
            let mut query = sqlx::QueryBuilder::<sqlx::Sqlite>::new(
                "INSERT INTO temp.pc_finish_legacy(id, relative) ",
            );
            query.push_values(chunk, |mut builder, (id, relative)| {
                builder.push_bind(id).push_bind(relative);
            });
            query.build().execute(&mut **tx).await.map_err(err)?;
        }
    }

    sqlx::query(
        "UPDATE files SET import_source_id = ?, source_relative_path = (SELECT candidate.relative FROM temp.pc_finish_legacy candidate WHERE candidate.id = files.id) WHERE id IN (SELECT candidate.id FROM temp.pc_finish_legacy candidate WHERE (SELECT COUNT(*) FROM temp.pc_finish_legacy duplicate WHERE duplicate.relative = candidate.relative) = 1 AND NOT EXISTS (SELECT 1 FROM files collision WHERE collision.import_source_id = ? AND collision.source_relative_path = candidate.relative AND collision.id <> candidate.id)) AND (import_source_id IS NULL OR import_source_id = ?) AND (source_relative_path IS NULL OR source_relative_path = (SELECT candidate.relative FROM temp.pc_finish_legacy candidate WHERE candidate.id = files.id))",
    )
    .bind(source_id)
    .bind(source_id)
    .bind(source_id)
    .execute(&mut **tx)
    .await
    .map_err(err)?;
    sqlx::query("DROP TABLE temp.pc_finish_legacy")
        .execute(&mut **tx)
        .await
        .map_err(err)?;
    Ok(())
}

fn legacy_relative_path(root: &Path, absolute: &Path, resolved: Option<&Path>) -> Option<String> {
    if !root.is_absolute() || !absolute.is_absolute() {
        return None;
    }
    // Existing symlinks that resolve outside the root are not adopted. For a
    // missing legacy file, retain the lexical containment check so it can be
    // correctly marked missing on this first scan.
    if let Some(canonical_file) = resolved {
        if canonical_file.strip_prefix(root).is_err() {
            return None;
        }
    }
    let relative = absolute.strip_prefix(root).ok()?;
    normalized_relative_path(relative).ok()
}

fn digest_text(digest: &[u8]) -> String {
    if digest.len() == 64 && digest.iter().all(|byte| byte.is_ascii_hexdigit()) {
        return String::from_utf8_lossy(digest).to_ascii_lowercase();
    }
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

fn file_role(path: &Path) -> String {
    match scanning::role_for_path(path) {
        Some(Role::Raw) => "RAW".into(),
        Some(Role::Jpeg) => "JPEG".into(),
        Some(Role::Image) => "IMAGE".into(),
        Some(Role::Sidecar) => "SIDECAR".into(),
        None => "IMAGE".into(),
    }
}

fn logical_key(path: &str) -> String {
    case_sensitive_logical_key(path).to_lowercase()
}

fn case_sensitive_logical_key(path: &str) -> String {
    let path = Path::new(path);
    let mut stem = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    stem.set_extension("");
    if matches!(extension.as_str(), "xmp" | "aae" | "pp3" | "dop") {
        if scanning::role_for_path(&stem)
            .is_some_and(|role| matches!(role, Role::Raw | Role::Jpeg | Role::Image))
        {
            stem.set_extension("");
        }
    }
    stem.to_string_lossy().replace('\\', "/")
}

fn stable_photo_id(
    gallery_id: &str,
    source_path: &str,
    logical_key: &str,
) -> Result<String, String> {
    // `logical_key` already has only the final media/sidecar suffix removed;
    // removing another suffix here would break names such as `DSC.001.JPG`.
    // Python hashes str(Path), which uses the platform path separator. The
    // repository keeps source-relative keys slash-normalized for matching,
    // then restores the native separator only for the identity digest.
    let separator = std::path::MAIN_SEPARATOR.to_string();
    let python_key = logical_key.replace('/', &separator);
    let identity = format!("{gallery_id}\0{source_path}\0{python_key}");
    compute_full_hash(&mut Cursor::new(identity.as_bytes()))
        .map(|hash| hash[..32].to_owned())
        .map_err(|error| format!("could not derive stable photo identity: {error}"))
}

fn stable_photo_id_for_reused_path(
    gallery_id: &str,
    source_path: &str,
    logical_key: &str,
    full_hash: &str,
) -> Result<String, String> {
    // Keep the ordinary path-derived identity for compatibility. Only when
    // that ID is already reserved do we add a versioned content namespace.
    // The full hash keeps the replacement stable across reimports while
    // ensuring different contents at a reused path never inherit old state.
    let separator = std::path::MAIN_SEPARATOR.to_string();
    let python_key = logical_key.replace('/', &separator);
    let identity = format!(
        "photo-culler:reused-path:v1\0{gallery_id}\0{source_path}\0{python_key}\0{full_hash}"
    );
    compute_full_hash(&mut Cursor::new(identity.as_bytes()))
        .map(|hash| hash[..32].to_owned())
        .map_err(|error| format!("could not derive stable reused-path photo identity: {error}"))
}

fn as_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn err(error: impl std::fmt::Display) -> String {
    error.to_string()
}
