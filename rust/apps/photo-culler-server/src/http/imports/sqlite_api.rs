//! SQLite-backed import API and bounded background job supervisor.
//!
//! Only roots supplied through trusted local configuration are registered and
//! scanned. The database path is metadata, never an authorization source, and
//! no filesystem path is included in an HTTP DTO or error body.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use sqlx::{Row, SqlitePool};

use crate::{
    catalog::sqlite::source_bootstrap::{BootstrappedSource, bootstrap_sources},
    http::imports::routes::{
        ControlAction, ImportApi, ImportApiFuture, ImportError, ImportEstimateView, JobView,
        QueuedJob, RescanView, ScanRevisionView, SourceView,
    },
    import::{
        filesystem_scanner::{FilesystemScanner, SourceConfig},
        repository::{ImportValidationError, clear_import_staging_tx},
        scanning::{self, Options, Role},
        worker::{
            ControlRequest, ImportRepository, ImportWorker, JobState, RunOutcome, WorkerError,
        },
    },
};

const MAX_ACTIVE_JOBS: usize = 2;
const WORKER_PAGE_SIZE: usize = 256;
const FAILED_STATE_PERSIST_ATTEMPTS: usize = 3;
const FAILED_STATE_RETRY_DELAY_MS: u64 = 50;

enum FailedStatePersistOutcome {
    Failed,
    Cancelled,
    Superseded,
}

/// SQLx-backed import API. Construct this with the same single-writer pool
/// used by the persistent catalog.
#[derive(Clone)]
pub struct SqliteImportApi {
    inner: Arc<Inner>,
}

struct Inner {
    writer: SqlitePool,
    gallery_id: String,
    sources: BTreeMap<String, BootstrappedSource>,
    authorized_roots: Vec<PathBuf>,
    active_jobs: Mutex<BTreeSet<String>>,
    pending_jobs: Mutex<VecDeque<PendingJob>>,
    // Keep each persisted control transition ordered with its matching queue
    // update, so a late pause cannot remove a newer resume from the queue.
    control_lock: tokio::sync::Mutex<()>,
    start_in_progress: AtomicBool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaunchMode {
    Start,
    Resume,
}

#[derive(Clone, Debug)]
struct PendingJob {
    id: String,
    mode: LaunchMode,
}

struct StartGuard(Arc<Inner>);

impl Drop for StartGuard {
    fn drop(&mut self) {
        self.0.start_in_progress.store(false, Ordering::Release);
    }
}

/// Releases a scheduler reservation even if a worker task panics or is aborted.
struct ActiveJobGuard {
    inner: Arc<Inner>,
    job_id: String,
}

impl Drop for ActiveJobGuard {
    fn drop(&mut self) {
        if let Ok(mut active) = self.inner.active_jobs.lock() {
            active.remove(&self.job_id);
        }
        // A task may be dropped while the runtime is shutting down. Always
        // release the reservation, but only dispatch more work in a live Tokio
        // context.
        if tokio::runtime::Handle::try_current().is_ok() {
            self.inner.dispatch_pending();
        }
    }
}

impl SqliteImportApi {
    /// Register local roots, initialize import persistence, and recover
    /// interrupted workers as paused. This intentionally does not launch any
    /// job during construction.
    pub async fn new(
        writer: SqlitePool,
        authorized_roots: Vec<PathBuf>,
        gallery_id: Option<String>,
    ) -> Result<Self, String> {
        let mut canonical_roots = BTreeSet::new();
        for configured in &authorized_roots {
            let root = std::fs::canonicalize(configured)
                .map_err(|_| format!("could not validate a locally configured authorized root"))?;
            if !root.is_dir() {
                return Err("a locally configured authorized root is not a directory".into());
            }
            canonical_roots.insert(root);
        }
        if canonical_roots.is_empty() {
            return Err("at least one --authorized-root is required".into());
        }
        let canonical_roots: Vec<_> = canonical_roots.into_iter().collect();
        let bootstrap = bootstrap_sources(&writer, &canonical_roots, gallery_id.as_deref())
            .await
            .map_err(|error| error.to_string())?;

        let sources = bootstrap
            .sources
            .into_iter()
            .map(|source| (source.id.clone(), source))
            .collect::<BTreeMap<_, _>>();
        let repository = crate::import::repository::SqliteImportRepository::new(writer.clone());
        repository
            .initialize()
            .await
            .map_err(|_| "could not initialize persistent import checkpoints".to_owned())?;

        let scanner = make_scanner(&sources, &canonical_roots, None)?;
        let mut worker = ImportWorker::new(scanner, repository, WORKER_PAGE_SIZE);
        worker
            .recover_interrupted()
            .await
            .map_err(|_| "could not recover interrupted import jobs".to_owned())?;

        Ok(Self {
            inner: Arc::new(Inner {
                writer,
                gallery_id: bootstrap.gallery_id,
                sources,
                authorized_roots: canonical_roots,
                active_jobs: Mutex::new(BTreeSet::new()),
                pending_jobs: Mutex::new(VecDeque::new()),
                control_lock: tokio::sync::Mutex::new(()),
                start_in_progress: AtomicBool::new(false),
            }),
        })
    }

    pub fn gallery_id(&self) -> &str {
        &self.inner.gallery_id
    }
}

impl Inner {
    fn source(&self, source_id: &str) -> Result<&BootstrappedSource, ImportError> {
        self.sources
            .get(source_id)
            .ok_or(ImportError::SourceUnavailable)
    }

    fn source_is_online(source: &BootstrappedSource) -> bool {
        std::fs::canonicalize(&source.root)
            .map(|canonical| canonical == source.root && canonical.is_dir())
            .unwrap_or(false)
    }

    async fn source_options(&self, source_id: &str) -> Result<Options, ImportError> {
        let row = sqlx::query(
            "SELECT recursive, exclude_patterns FROM import_sources WHERE id = ? AND gallery_id = ? AND status = 'online'",
        )
        .bind(source_id)
        .bind(&self.gallery_id)
        .fetch_optional(&self.writer)
        .await
        .map_err(|_| ImportError::Unavailable)?
        .ok_or(ImportError::SourceUnavailable)?;
        let recursive: bool = row
            .try_get("recursive")
            .map_err(|_| ImportError::Unavailable)?;
        let patterns: String = row
            .try_get("exclude_patterns")
            .map_err(|_| ImportError::Unavailable)?;
        let exclude_patterns =
            serde_json::from_str(&patterns).map_err(|_| ImportError::Unavailable)?;
        Ok(Options {
            recursive,
            exclude_patterns,
        })
    }

    fn scanner_for(
        &self,
        source_id: &str,
        options: Options,
    ) -> Result<FilesystemScanner, ImportError> {
        let source = self.source(source_id)?;
        if !Self::source_is_online(source) {
            return Err(ImportError::SourceUnavailable);
        }
        make_scanner(
            &BTreeMap::from([(
                source_id.to_owned(),
                BootstrappedSource {
                    id: source.id.clone(),
                    gallery_id: source.gallery_id.clone(),
                    root: source.root.clone(),
                    name: source.name.clone(),
                },
            )]),
            &self.authorized_roots,
            Some(options),
        )
        .map_err(|_| ImportError::SourceUnavailable)
    }

    async fn persist_options(
        &self,
        gallery_id: &str,
        source_id: &str,
        recursive: bool,
        exclude_patterns: &[String],
    ) -> Result<(), ImportError> {
        if gallery_id != self.gallery_id {
            return Err(ImportError::NotFound);
        }
        let source = self.source(source_id)?;
        if !Self::source_is_online(source) {
            return Err(ImportError::SourceUnavailable);
        }
        if !valid_patterns(exclude_patterns) {
            return Err(ImportError::InvalidRequest);
        }
        let encoded =
            serde_json::to_string(exclude_patterns).map_err(|_| ImportError::Unavailable)?;
        let mut tx = self
            .writer
            .begin()
            .await
            .map_err(|_| ImportError::Unavailable)?;
        let active: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM import_jobs WHERE source_id = ? AND state IN ('queued', 'running', 'paused')",
        )
        .bind(source_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ImportError::Unavailable)?;
        if active != 0 {
            return Err(ImportError::InvalidTransition);
        }
        let result = sqlx::query(
            "UPDATE import_sources SET recursive = ?, exclude_patterns = ?, status = 'online', last_seen_at = ? WHERE id = ? AND gallery_id = ?",
        )
        .bind(recursive)
        .bind(encoded)
        .bind(now_text())
        .bind(source_id)
        .bind(gallery_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| ImportError::Unavailable)?;
        if result.rows_affected() != 1 {
            return Err(ImportError::SourceUnavailable);
        }
        tx.commit().await.map_err(|_| ImportError::Unavailable)
    }

    async fn start(
        self: &Arc<Self>,
        gallery_id: &str,
        source_id: &str,
        recursive: bool,
        exclude_patterns: &[String],
    ) -> Result<QueuedJob, ImportError> {
        if gallery_id != self.gallery_id {
            return Err(ImportError::NotFound);
        }
        if self.source(source_id).is_err() {
            return Err(ImportError::SourceUnavailable);
        }
        if self
            .start_in_progress
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            // This is global startup contention, not proof that this source
            // already has a queued/running/paused job. Keep it distinct from
            // `persist_options`' per-source active-job rejection so rescan
            // reports the source as deferred instead of silently omitting it.
            return Err(ImportError::Unavailable);
        }
        let _guard = StartGuard(Arc::clone(self));
        self.persist_options(gallery_id, source_id, recursive, exclude_patterns)
            .await?;

        let revision: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(c.revision), 0) + 1 FROM import_jobs j JOIN photo_culler_import_checkpoints c ON c.job_id = j.id WHERE j.source_id = ?",
        )
        .bind(source_id)
        .fetch_one(&self.writer)
        .await
        .map_err(|_| ImportError::Unavailable)?;
        let mut repository =
            crate::import::repository::SqliteImportRepository::new(self.writer.clone());
        let job = repository
            .create_job(source_id, u64::try_from(revision).unwrap_or(1))
            .await
            .map_err(|_| ImportError::Unavailable)?;
        self.enqueue_for_execution(job.id.clone(), LaunchMode::Start);
        self.dispatch_pending();
        Ok(QueuedJob {
            job_id: job.id,
            state: "queued".into(),
        })
    }

    fn enqueue_for_execution(self: &Arc<Self>, job_id: String, mode: LaunchMode) {
        if let Ok(mut pending) = self.pending_jobs.lock() {
            if let Some(existing) = pending.iter_mut().find(|job| job.id == job_id) {
                if mode == LaunchMode::Resume {
                    existing.mode = LaunchMode::Resume;
                }
            } else {
                pending.push_back(PendingJob { id: job_id, mode });
            }
        }
    }

    fn remove_pending(&self, job_id: &str) {
        if let Ok(mut pending) = self.pending_jobs.lock() {
            pending.retain(|pending| pending.id != job_id);
        }
    }

    fn dispatch_pending(self: &Arc<Self>) {
        loop {
            let next = {
                let Ok(mut active) = self.active_jobs.lock() else {
                    return;
                };
                if active.len() >= MAX_ACTIVE_JOBS {
                    return;
                }
                let Ok(mut pending) = self.pending_jobs.lock() else {
                    return;
                };
                let mut next = None;
                // Visit each currently queued job at most once: a resume may
                // still be waiting for its previous worker to release the
                // reservation, but must not block unrelated jobs behind it.
                for _ in 0..pending.len() {
                    let Some(job) = pending.pop_front() else {
                        break;
                    };
                    if active.contains(&job.id) {
                        pending.push_back(job);
                        continue;
                    }
                    active.insert(job.id.clone());
                    next = Some(job);
                    break;
                }
                next
            };
            let Some(job) = next else { return };
            self.spawn_reserved(job);
        }
    }

    fn spawn_reserved(self: &Arc<Self>, job: PendingJob) {
        let job_id = job.id.clone();
        let inner = Arc::clone(self);
        tokio::spawn(async move {
            let _active_job = ActiveJobGuard {
                inner: Arc::clone(&inner),
                job_id: job_id.clone(),
            };
            let worker_inner = Arc::clone(&inner);
            let worker_job = job.clone();
            let result = tokio::spawn(async move { worker_inner.run_job(&worker_job).await }).await;
            match result {
                Ok(Ok(())) => {}
                Ok(Err(error)) => inner.mark_failed(&job_id, &error).await,
                Err(error) => {
                    inner
                        .mark_failed(
                            &job_id,
                            &WorkerError::Repository(format!(
                                "import worker task terminated unexpectedly: {error}"
                            )),
                        )
                        .await;
                }
            }
        });
    }

    async fn run_job(self: &Arc<Self>, pending: &PendingJob) -> Result<(), WorkerError> {
        let job_id = pending.id.as_str();
        let repository =
            crate::import::repository::SqliteImportRepository::new(self.writer.clone());
        let job = repository
            .load_job(job_id)
            .await
            .map_err(WorkerError::Repository)?;
        let options = self
            .source_options(&job.source_id)
            .await
            .map_err(|error| WorkerError::Scanner(format!("source unavailable: {error:?}")))?;
        let scanner = self
            .scanner_for(&job.source_id, options)
            .map_err(|error| WorkerError::Scanner(format!("source unavailable: {error:?}")))?;
        let mut worker = ImportWorker::new(scanner, repository, WORKER_PAGE_SIZE);
        let mut outcome = match (&job.state, pending.mode) {
            (JobState::Queued, LaunchMode::Start | LaunchMode::Resume) => {
                match worker.run(job_id, WORKER_PAGE_SIZE).await {
                    Ok(outcome) => outcome,
                    Err(WorkerError::InvalidState {
                        state: JobState::Paused | JobState::Cancelled,
                        ..
                    }) => return Ok(()),
                    Err(error) => return Err(error),
                }
            }
            (JobState::Paused, LaunchMode::Resume) => {
                // Resume is persisted as queued before dispatch. If a pause
                // races with a reserved task, leave the now-paused job alone.
                return Ok(());
            }
            (JobState::Paused, LaunchMode::Start) => return Ok(()),
            (JobState::Running, _) => return Ok(()),
            (JobState::Completed | JobState::Cancelled | JobState::Failed(_), _) => return Ok(()),
        };
        loop {
            match outcome {
                RunOutcome::Yielded { .. } => {
                    outcome = match worker.resume(job_id, WORKER_PAGE_SIZE).await {
                        Ok(outcome) => outcome,
                        Err(WorkerError::InvalidState {
                            state: JobState::Paused | JobState::Cancelled,
                            ..
                        }) => return Ok(()),
                        Err(error) => return Err(error),
                    };
                }
                RunOutcome::Completed(_) | RunOutcome::Paused | RunOutcome::Cancelled => {
                    return Ok(());
                }
            }
        }
    }

    async fn mark_failed(&self, job_id: &str, error: &WorkerError) {
        let code = match error {
            WorkerError::Scanner(_) => "scan_failed",
            WorkerError::Repository(detail) => ImportValidationError::from_code(detail)
                .map(ImportValidationError::code)
                .unwrap_or("import_failed"),
            _ => "import_failed",
        };
        let now = now_text();
        for attempt in 1..=FAILED_STATE_PERSIST_ATTEMPTS {
            match Self::persist_failed_state_once(&self.writer, job_id, code, &now).await {
                Ok(FailedStatePersistOutcome::Failed) => {
                    eprintln!("Photo Culler import job failed ({code}): {error}");
                    return;
                }
                Ok(FailedStatePersistOutcome::Cancelled) => {
                    eprintln!(
                        "Photo Culler import job was cancelled while failing; cancellation took precedence ({code}): {error}"
                    );
                    return;
                }
                Ok(FailedStatePersistOutcome::Superseded) => {
                    // A control action may have changed the job while its
                    // worker was failing. Preserve that newer state.
                    eprintln!(
                        "Photo Culler did not persist failed import job {job_id} ({code}): job state already changed"
                    );
                    eprintln!("Photo Culler import job failed ({code}): {error}");
                    return;
                }
                Err(persist_error) if attempt < FAILED_STATE_PERSIST_ATTEMPTS => {
                    let delay = FAILED_STATE_RETRY_DELAY_MS * (1 << (attempt - 1));
                    eprintln!(
                        "Photo Culler could not persist failed import job {job_id} ({code}) on attempt {attempt}/{FAILED_STATE_PERSIST_ATTEMPTS}: {persist_error}; retrying in {delay} ms"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
                Err(persist_error) => {
                    eprintln!(
                        "Photo Culler could not confirm failed import job {job_id} ({code}) after {FAILED_STATE_PERSIST_ATTEMPTS} attempts; state and staging may already be committed or may remain unchanged: {persist_error}"
                    );
                    eprintln!("Photo Culler import job failed ({code}): {error}");
                    return;
                }
            }
        }
    }

    async fn persist_failed_state_once(
        writer: &SqlitePool,
        job_id: &str,
        code: &str,
        now: &str,
    ) -> Result<FailedStatePersistOutcome, String> {
        let mut tx = writer
            .begin()
            .await
            .map_err(|error| format!("could not begin transaction: {error}"))?;
        // A cancellation accepted before this transaction takes precedence
        // over a scanner error that arrived before the next checkpoint. The
        // writer transaction serializes this decision with control requests.
        let job_update = sqlx::query(
            "UPDATE import_jobs SET state = CASE WHEN cancel_requested != 0 THEN 'cancelled' ELSE 'failed' END, error = CASE WHEN cancel_requested != 0 THEN NULL ELSE ? END, pause_requested = 0, updated_at = ? WHERE id = ? AND state IN ('queued', 'running')",
        )
        .bind(code)
        .bind(now)
        .bind(job_id)
        .execute(&mut *tx)
        .await;
        let job_update = match job_update {
            Ok(result) => result,
            Err(error) => {
                let _ = tx.rollback().await;
                return Err(format!("could not update job state: {error}"));
            }
        };
        if job_update.rows_affected() != 1 {
            let _ = tx.rollback().await;
            let persisted_state: Option<String> =
                sqlx::query_scalar("SELECT state FROM import_jobs WHERE id = ?")
                    .bind(job_id)
                    .fetch_optional(writer)
                    .await
                    .map_err(|error| {
                        format!("could not inspect job after state conflict: {error}")
                    })?;
            return Ok(match persisted_state.as_deref() {
                // A previous commit can report an error even though SQLite
                // committed atomically. Reconcile that ambiguous result on
                // the next attempt before reporting a persistence failure.
                Some("failed") => FailedStatePersistOutcome::Failed,
                Some("cancelled") => FailedStatePersistOutcome::Cancelled,
                _ => FailedStatePersistOutcome::Superseded,
            });
        }

        let revision_update = sqlx::query(
            "UPDATE scan_revisions SET state = (SELECT state FROM import_jobs WHERE id = ?), completed_at = ? WHERE id = (SELECT scan_revision_id FROM import_jobs WHERE id = ?) AND state IN ('queued', 'running')",
        )
        .bind(job_id)
        .bind(now)
        .bind(job_id)
        .execute(&mut *tx)
        .await;
        let revision_update = match revision_update {
            Ok(result) => result,
            Err(error) => {
                let _ = tx.rollback().await;
                return Err(format!("could not update scan revision state: {error}"));
            }
        };
        if revision_update.rows_affected() != 1 {
            let _ = tx.rollback().await;
            return Err("scan revision state already changed".into());
        }

        if let Err(error) = clear_import_staging_tx(&mut tx, job_id).await {
            let _ = tx.rollback().await;
            return Err(format!("could not clear failed-job staging: {error}"));
        }
        let cancelled: bool =
            match sqlx::query_scalar("SELECT state = 'cancelled' FROM import_jobs WHERE id = ?")
                .bind(job_id)
                .fetch_one(&mut *tx)
                .await
            {
                Ok(cancelled) => cancelled,
                Err(error) => {
                    let _ = tx.rollback().await;
                    return Err(format!("could not read terminal job state: {error}"));
                }
            };
        tx.commit()
            .await
            .map_err(|error| format!("could not commit failure state and cleanup: {error}"))?;
        Ok(if cancelled {
            FailedStatePersistOutcome::Cancelled
        } else {
            FailedStatePersistOutcome::Failed
        })
    }

    async fn load_job_row(&self, job_id: &str) -> Result<sqlx::sqlite::SqliteRow, ImportError> {
        sqlx::query(
            "SELECT j.id, j.gallery_id, j.scan_revision_id, j.source_id, j.state, j.discovered, j.imported, j.issues, j.cancel_requested, j.pause_requested, j.updated_at, c.revision, c.processed, j.error FROM import_jobs j LEFT JOIN photo_culler_import_checkpoints c ON c.job_id = j.id WHERE j.id = ? AND j.gallery_id = ?",
        )
        .bind(job_id)
        .bind(&self.gallery_id)
        .fetch_optional(&self.writer)
        .await
        .map_err(|_| ImportError::Unavailable)?
        .ok_or(ImportError::NotFound)
    }

    async fn job_view(&self, job_id: &str) -> Result<JobView, ImportError> {
        let row = self.load_job_row(job_id).await?;
        Ok(job_view_from_row(row)?)
    }

    async fn control_write_failure(&self, job_id: &str, action: ControlAction) -> ImportError {
        let row = match self.load_job_row(job_id).await {
            Ok(row) => row,
            Err(error) => return error,
        };
        let state: String = match row.try_get("state") {
            Ok(state) => state,
            Err(_) => return ImportError::Unavailable,
        };
        let transition_still_possible = match action {
            ControlAction::Pause => matches!(state.as_str(), "running" | "queued" | "paused"),
            ControlAction::Cancel => {
                matches!(
                    state.as_str(),
                    "running" | "queued" | "paused" | "cancelled"
                )
            }
            ControlAction::Resume => state == "paused",
        };
        if transition_still_possible {
            ImportError::Unavailable
        } else {
            ImportError::InvalidTransition
        }
    }
}

impl ImportApi for SqliteImportApi {
    fn sources<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> ImportApiFuture<'a, Result<Vec<SourceView>, ImportError>> {
        Box::pin(async move {
            if gallery_id != self.inner.gallery_id {
                return Err(ImportError::NotFound);
            }
            Ok(self
                .inner
                .sources
                .values()
                .map(|source| SourceView {
                    contract_version: 2,
                    id: source.id.clone(),
                    name: source.name.clone(),
                    status: if Inner::source_is_online(source) {
                        "online".into()
                    } else {
                        "offline".into()
                    },
                })
                .collect())
        })
    }

    fn start_import<'a>(
        &'a self,
        gallery_id: &'a str,
        source_id: &'a str,
        recursive: bool,
        exclude_patterns: &'a [String],
    ) -> ImportApiFuture<'a, Result<QueuedJob, ImportError>> {
        Box::pin(async move {
            Arc::clone(&self.inner)
                .start(gallery_id, source_id, recursive, exclude_patterns)
                .await
        })
    }

    fn estimate<'a>(
        &'a self,
        source_id: &'a str,
        recursive: bool,
        exclude_patterns: &'a [String],
    ) -> ImportApiFuture<'a, Result<ImportEstimateView, ImportError>> {
        Box::pin(async move {
            if !valid_patterns(exclude_patterns) {
                return Err(ImportError::InvalidRequest);
            }
            let source = self.inner.source(source_id)?;
            if !Inner::source_is_online(source) {
                return Err(ImportError::SourceUnavailable);
            }
            estimate_source(
                source,
                &self.inner.authorized_roots,
                Options {
                    recursive,
                    exclude_patterns: exclude_patterns.to_vec(),
                },
            )
            .await
        })
    }

    fn rescan<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> ImportApiFuture<'a, Result<RescanView, ImportError>> {
        Box::pin(async move {
            if gallery_id != self.inner.gallery_id {
                return Err(ImportError::NotFound);
            }
            let mut queued = Vec::new();
            let mut unavailable = Vec::new();
            let mut deferred = Vec::new();
            for source in self.inner.sources.values() {
                if !Inner::source_is_online(source) {
                    unavailable.push(source.id.clone());
                    continue;
                }
                let options = match self.inner.source_options(&source.id).await {
                    Ok(options) => options,
                    Err(ImportError::SourceUnavailable) => {
                        if Inner::source_is_online(source) {
                            deferred.push(source.id.clone());
                        } else {
                            unavailable.push(source.id.clone());
                        }
                        continue;
                    }
                    Err(ImportError::Unavailable) => {
                        deferred.push(source.id.clone());
                        continue;
                    }
                    Err(error) => return Err(error),
                };
                match Arc::clone(&self.inner)
                    .start(
                        gallery_id,
                        &source.id,
                        options.recursive,
                        &options.exclude_patterns,
                    )
                    .await
                {
                    Ok(job) => queued.push(job),
                    // A source with an existing queued/running/paused job is
                    // not offline; leave that job under explicit control.
                    Err(ImportError::InvalidTransition) => {}
                    Err(ImportError::Unavailable) => deferred.push(source.id.clone()),
                    Err(ImportError::SourceUnavailable) => {
                        if Inner::source_is_online(source) {
                            deferred.push(source.id.clone());
                        } else {
                            unavailable.push(source.id.clone());
                        }
                    }
                    Err(error) => return Err(error),
                }
            }
            Ok(RescanView {
                contract_version: 2,
                gallery_id: self.inner.gallery_id.clone(),
                job_ids: queued.into_iter().map(|job| job.job_id).collect(),
                offline_source_ids: unavailable,
                deferred_source_ids: deferred,
            })
        })
    }

    fn job<'a>(&'a self, job_id: &'a str) -> ImportApiFuture<'a, Result<JobView, ImportError>> {
        Box::pin(async move { self.inner.job_view(job_id).await })
    }

    fn jobs<'a>(
        &'a self,
        limit: usize,
        active_only: bool,
    ) -> ImportApiFuture<'a, Result<Vec<JobView>, ImportError>> {
        Box::pin(async move {
            if !(1..=100).contains(&limit) {
                return Err(ImportError::InvalidRequest);
            }
            let rows = sqlx::query(
                "SELECT j.id, j.gallery_id, j.scan_revision_id, j.source_id, j.state, j.discovered, j.imported, j.issues, j.cancel_requested, j.pause_requested, j.updated_at, c.revision, c.processed, j.error FROM import_jobs j LEFT JOIN photo_culler_import_checkpoints c ON c.job_id = j.id WHERE j.gallery_id = ? AND (? = 0 OR j.state IN ('queued', 'running', 'paused')) ORDER BY j.created_at DESC, j.id DESC LIMIT ?",
            )
            .bind(&self.inner.gallery_id)
            .bind(active_only)
            .bind(limit as i64)
            .fetch_all(&self.inner.writer)
            .await
            .map_err(|_| ImportError::Unavailable)?;
            rows.into_iter().map(job_view_from_row).collect()
        })
    }

    fn revisions<'a>(
        &'a self,
        gallery_id: Option<&'a str>,
        limit: usize,
    ) -> ImportApiFuture<'a, Result<Vec<ScanRevisionView>, ImportError>> {
        Box::pin(async move {
            if !(1..=100).contains(&limit)
                || gallery_id.is_some_and(|value| value != self.inner.gallery_id)
            {
                return Err(ImportError::InvalidRequest);
            }
            let rows = sqlx::query(
                "SELECT r.id, r.gallery_id, r.source_id, r.state, r.discovered, r.new_files, r.modified_files, r.moved_files, r.missing_files, r.started_at, r.completed_at FROM scan_revisions r WHERE r.gallery_id = ? ORDER BY r.started_at DESC, r.id DESC LIMIT ?",
            )
            .bind(&self.inner.gallery_id)
            .bind(limit as i64)
            .fetch_all(&self.inner.writer)
            .await
            .map_err(|_| ImportError::Unavailable)?;
            rows.into_iter()
                .map(|row| {
                    Ok(ScanRevisionView {
                        contract_version: 2,
                        id: row.try_get("id").map_err(|_| ImportError::Unavailable)?,
                        gallery_id: row
                            .try_get("gallery_id")
                            .map_err(|_| ImportError::Unavailable)?,
                        source_id: row
                            .try_get("source_id")
                            .map_err(|_| ImportError::Unavailable)?,
                        state: row.try_get("state").map_err(|_| ImportError::Unavailable)?,
                        discovered: as_u64(
                            row.try_get("discovered")
                                .map_err(|_| ImportError::Unavailable)?,
                        ),
                        new_files: as_u64(
                            row.try_get("new_files")
                                .map_err(|_| ImportError::Unavailable)?,
                        ),
                        modified_files: as_u64(
                            row.try_get("modified_files")
                                .map_err(|_| ImportError::Unavailable)?,
                        ),
                        moved_files: as_u64(
                            row.try_get("moved_files")
                                .map_err(|_| ImportError::Unavailable)?,
                        ),
                        missing_files: as_u64(
                            row.try_get("missing_files")
                                .map_err(|_| ImportError::Unavailable)?,
                        ),
                        started_at: row
                            .try_get("started_at")
                            .map_err(|_| ImportError::Unavailable)?,
                        completed_at: row
                            .try_get("completed_at")
                            .map_err(|_| ImportError::Unavailable)?,
                    })
                })
                .collect()
        })
    }

    fn control<'a>(
        &'a self,
        job_id: &'a str,
        action: ControlAction,
    ) -> ImportApiFuture<'a, Result<(), ImportError>> {
        Box::pin(async move {
            let _control_guard = self.inner.control_lock.lock().await;
            let row = self.inner.load_job_row(job_id).await?;
            let source_id: String = row
                .try_get("source_id")
                .map_err(|_| ImportError::Unavailable)?;
            let state: String = row.try_get("state").map_err(|_| ImportError::Unavailable)?;
            let mut repository =
                crate::import::repository::SqliteImportRepository::new(self.inner.writer.clone());
            match action {
                ControlAction::Pause => {
                    if repository
                        .request_control(job_id, ControlRequest::Pause)
                        .await
                        .is_err()
                    {
                        return Err(self.inner.control_write_failure(job_id, action).await);
                    }
                    self.inner.remove_pending(job_id);
                    Ok(())
                }
                ControlAction::Cancel => {
                    if repository
                        .request_control(job_id, ControlRequest::Cancel)
                        .await
                        .is_err()
                    {
                        return Err(self.inner.control_write_failure(job_id, action).await);
                    }
                    self.inner.remove_pending(job_id);
                    Ok(())
                }
                ControlAction::Resume => {
                    if state != "paused" {
                        return Err(ImportError::InvalidTransition);
                    }
                    let source = self.inner.source(&source_id)?;
                    if !Inner::source_is_online(source) {
                        return Err(ImportError::SourceUnavailable);
                    }
                    if repository.queue_resume(job_id).await.is_err() {
                        return Err(self.inner.control_write_failure(job_id, action).await);
                    }
                    self.inner
                        .enqueue_for_execution(job_id.to_owned(), LaunchMode::Resume);
                    self.inner.dispatch_pending();
                    Ok(())
                }
            }
        })
    }
}

fn make_scanner(
    sources: &BTreeMap<String, BootstrappedSource>,
    authorized_roots: &[PathBuf],
    override_options: Option<Options>,
) -> Result<FilesystemScanner, String> {
    let entries = sources.iter().map(|(id, source)| {
        (
            id.clone(),
            SourceConfig {
                root: source.root.clone(),
                options: override_options.clone().unwrap_or_default(),
            },
        )
    });
    FilesystemScanner::new(entries, authorized_roots.iter().cloned())
}

async fn estimate_source(
    source: &BootstrappedSource,
    authorized_roots: &[PathBuf],
    options: Options,
) -> Result<ImportEstimateView, ImportError> {
    let root = source.root.clone();
    let allowed = authorized_roots.to_vec();
    let source_id = source.id.clone();
    let (total_files, total_bytes, extensions, roles, logical_photos) =
        tokio::task::spawn_blocking(move || {
            let mut total_files = 0u64;
            let mut total_bytes = 0u64;
            let mut extensions = BTreeMap::<String, u64>::new();
            let mut roles = BTreeMap::<String, u64>::new();
            let mut logical_keys = BTreeSet::new();
            scanning::walk(
                &root,
                &allowed,
                &options,
                |entry| {
                    crate::import::filesystem_scanner::normalize_relative_path(&entry.path)
                        .map_err(|error| {
                            std::io::Error::new(std::io::ErrorKind::InvalidData, error)
                        })?;
                    total_files = total_files.saturating_add(1);
                    total_bytes = total_bytes.saturating_add(entry.bytes);
                    let extension = entry
                        .path
                        .extension()
                        .and_then(|value| value.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    let extension_count = extensions.entry(extension).or_default();
                    *extension_count = extension_count.saturating_add(1);
                    let role = role_name(entry.role);
                    let role_count = roles.entry(role.to_owned()).or_default();
                    *role_count = role_count.saturating_add(1);
                    if entry.role != Role::Sidecar {
                        logical_keys.insert(logical_key(&entry.path));
                    }
                    Ok(())
                },
                || Ok(()),
            )
            .map_err(|_| ImportError::SourceUnavailable)?;
            Ok::<_, ImportError>((
                total_files,
                total_bytes,
                extensions,
                roles,
                logical_keys.len() as u64,
            ))
        })
        .await
        .map_err(|_| ImportError::Unavailable)??;
    Ok(ImportEstimateView {
        contract_version: 2,
        source_id,
        total_files,
        total_bytes,
        logical_photos,
        extensions,
        roles,
    })
}

fn job_view_from_row(row: sqlx::sqlite::SqliteRow) -> Result<JobView, ImportError> {
    let state: String = row.try_get("state").map_err(|_| ImportError::Unavailable)?;
    let error_code: Option<String> = row.try_get("error").map_err(|_| ImportError::Unavailable)?;
    let discovered: i64 = row
        .try_get("discovered")
        .map_err(|_| ImportError::Unavailable)?;
    let imported: i64 = row
        .try_get("imported")
        .map_err(|_| ImportError::Unavailable)?;
    let issues: i64 = row
        .try_get("issues")
        .map_err(|_| ImportError::Unavailable)?;
    Ok(JobView {
        contract_version: 2,
        id: row.try_get("id").map_err(|_| ImportError::Unavailable)?,
        gallery_id: row
            .try_get("gallery_id")
            .map_err(|_| ImportError::Unavailable)?,
        scan_revision_id: row
            .try_get("scan_revision_id")
            .map_err(|_| ImportError::Unavailable)?,
        source_id: row
            .try_get("source_id")
            .map_err(|_| ImportError::Unavailable)?,
        state: state.clone(),
        discovered: as_u64(discovered),
        imported: as_u64(imported),
        issues: as_u64(issues),
        cancel_requested: row
            .try_get("cancel_requested")
            .map_err(|_| ImportError::Unavailable)?,
        pause_requested: row
            .try_get("pause_requested")
            .map_err(|_| ImportError::Unavailable)?,
        updated_at: row
            .try_get("updated_at")
            .map_err(|_| ImportError::Unavailable)?,
        revision: as_u64(
            row.try_get("revision")
                .map_err(|_| ImportError::Unavailable)?,
        ),
        processed: as_u64(
            row.try_get("processed")
                .map_err(|_| ImportError::Unavailable)?,
        ),
        total: (state == "completed").then_some(as_u64(discovered)),
        error: (state == "failed").then(|| {
            error_code
                .as_deref()
                .and_then(ImportValidationError::from_code)
                .map(ImportValidationError::message)
                .unwrap_or("Import failed; inspect the local server log.")
                .to_owned()
        }),
    })
}

fn validate_patterns(patterns: &[String]) -> bool {
    patterns.len() <= 50
        && patterns
            .iter()
            .all(|pattern| pattern.chars().count() <= 256 && !pattern.contains('\0'))
}

fn valid_patterns(patterns: &[String]) -> bool {
    validate_patterns(patterns)
}

fn role_name(role: Role) -> &'static str {
    match role {
        Role::Raw => "RAW",
        Role::Jpeg => "JPEG",
        Role::Image => "IMAGE",
        Role::Sidecar => "SIDECAR",
    }
}

fn logical_key(path: &Path) -> String {
    let mut stem = path.to_path_buf();
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    stem.set_extension("");
    if matches!(extension.as_str(), "xmp" | "aae" | "pp3" | "dop") {
        let previous = stem
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            previous.as_str(),
            "jpg"
                | "jpeg"
                | "jpe"
                | "arw"
                | "cr2"
                | "cr3"
                | "dng"
                | "nef"
                | "nrw"
                | "orf"
                | "pef"
                | "raf"
                | "raw"
                | "rwl"
                | "rw2"
                | "srw"
                | "png"
                | "tif"
                | "tiff"
                | "bmp"
                | "gif"
                | "webp"
                | "heic"
                | "heif"
                | "3fr"
                | "fff"
                | "iiq"
                | "kdc"
                | "mos"
                | "mef"
                | "mrw"
        ) {
            stem.set_extension("");
        }
    }
    stem.to_string_lossy().replace('\\', "/").to_lowercase()
}

fn as_u64(value: i64) -> u64 {
    u64::try_from(value).unwrap_or(0)
}

fn now_text() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:09}Z", duration.as_secs(), duration.subsec_nanos())
}
