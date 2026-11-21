//! Persistent, explicitly resumable import/revision worker.
//!
//! This module owns orchestration only. Filesystem traversal and catalog writes are
//! injected through [`Scanner`] and [`ImportRepository`], so the worker never opens,
//! renames, or writes an original. `commit_observation` atomically records the
//! hashed observation, job cursor, and progress. The repository reconciles catalog
//! rows only after the complete manifest has been observed, so a partial scan cannot
//! mark unseen files missing.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

/// Sendable asynchronous operation used by scanners and persistent repositories.
pub type WorkerFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Completed,
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ImportJob {
    pub id: String,
    pub source_id: String,
    pub revision: u64,
    /// Last cursor atomically committed with an observation. `None` means start.
    pub cursor: Option<String>,
    pub state: JobState,
    pub processed: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Observation {
    /// Stable, source-relative path. Scanner output must be in deterministic order.
    pub path: PathBuf,
    pub size: u64,
    pub modified_ns: Option<u128>,
    pub quick_hash: Vec<u8>,
    pub content_hash: Vec<u8>,
}

/// One supported source path captured during the durable, bounded-memory walk.
/// The portable `/` path and metadata are persisted before any content is hashed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestEntry {
    pub path: String,
    pub size: u64,
    pub modified_ns: Option<u128>,
    pub role: String,
}

impl Observation {
    pub fn cursor(&self) -> String {
        let value = self.path.to_string_lossy();
        if std::path::MAIN_SEPARATOR == '\\' {
            value.replace('\\', "/")
        } else {
            value.into_owned()
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryChange {
    New,
    Modified,
    Moved { from: PathBuf },
    Unchanged,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlRequest {
    Pause,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ControlSignal {
    Continue,
    /// The operator requested a pause (or the job was already paused).
    Paused,
    /// The worker reached its fairness/budget boundary and paused itself.
    Yielded,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObservationCommit {
    Committed(EntryChange),
    Paused,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FinishOutcome {
    Completed(ImportCounts),
    Paused,
    Cancelled,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ImportCounts {
    pub new: u64,
    pub modified: u64,
    pub moved: u64,
    pub unchanged: u64,
    pub missing: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkerError {
    Repository(String),
    Scanner(String),
    InvalidState { job_id: String, state: JobState },
    InvalidScanOrder { previous: String, next: String },
}

impl std::fmt::Display for WorkerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for WorkerError {}

/// Scanner paths and manifest entries use portable `/`-separated cursors. Manifest
/// page observation must preserve the input order and return one observation per
/// entry. All scanner results are read-only descriptions of source files.
pub trait Scanner: Send {
    /// Walk a source once and stream bounded batches of supported paths to the
    /// worker. The repository marks the manifest complete only after this future
    /// returns successfully.
    fn build_manifest<'a>(
        &'a mut self,
        source_id: &'a str,
        batches: tokio::sync::mpsc::Sender<Vec<ManifestEntry>>,
    ) -> WorkerFuture<'a, Result<(), String>>;
    /// Reopen and hash one already-persisted manifest page.
    fn observe_page<'a>(
        &'a mut self,
        source_id: &'a str,
        entries: &'a [ManifestEntry],
    ) -> WorkerFuture<'a, Result<Vec<Observation>, String>>;
}

/// Implementations persist jobs and import state. `commit_observation` is a single
/// transaction: stage the observation, associate its cursor with this job, increment
/// processed, and make retries of the same cursor idempotent. Catalog reconciliation
/// belongs to `finish_job` and occurs only after a complete manifest has been read.
pub trait ImportRepository: Send {
    fn create_job<'a>(
        &'a mut self,
        source_id: &'a str,
        revision: u64,
    ) -> WorkerFuture<'a, Result<ImportJob, String>>;
    fn load_job<'a>(&'a self, job_id: &'a str) -> WorkerFuture<'a, Result<ImportJob, String>>;
    fn list_jobs(&self) -> WorkerFuture<'_, Result<Vec<ImportJob>, String>>;
    /// Atomically claim a queued or paused job. A false result means its state
    /// changed before this worker acquired it.
    fn claim_job<'a>(
        &'a mut self,
        job_id: &'a str,
        expected: JobState,
    ) -> WorkerFuture<'a, Result<bool, String>>;
    /// Persist a control request. A running job observes it at its next
    /// checkpoint; queued/paused jobs can transition immediately.
    fn request_control<'a>(
        &'a mut self,
        job_id: &'a str,
        request: ControlRequest,
    ) -> WorkerFuture<'a, Result<(), String>>;
    /// Atomically acknowledge pending control requests. A pending user pause
    /// must return `Paused`, including when `yield_if_running` is true. If no
    /// user pause is pending, `yield_if_running` pauses a running job for
    /// fairness and returns `Yielded`. A budget yield must remain distinguishable
    /// from an operator pause because callers may automatically resume yields.
    fn checkpoint_control<'a>(
        &'a mut self,
        job_id: &'a str,
        yield_if_running: bool,
    ) -> WorkerFuture<'a, Result<ControlSignal, String>>;
    /// Replace any interrupted partial manifest and enter the `building` state.
    fn reset_manifest<'a>(&'a mut self, job_id: &'a str) -> WorkerFuture<'a, Result<(), String>>;
    /// Persist one bounded path batch for this job.
    fn append_manifest<'a>(
        &'a mut self,
        job_id: &'a str,
        entries: &'a [ManifestEntry],
    ) -> WorkerFuture<'a, Result<(), String>>;
    /// Mark the fully enumerated path set complete and ready for hashing.
    fn complete_manifest<'a>(&'a mut self, job_id: &'a str)
    -> WorkerFuture<'a, Result<(), String>>;
    fn manifest_complete<'a>(&'a self, job_id: &'a str) -> WorkerFuture<'a, Result<bool, String>>;
    /// Read one lexically ordered manifest page after the exclusive cursor.
    fn manifest_page<'a>(
        &'a self,
        job_id: &'a str,
        after: Option<&'a str>,
        limit: usize,
    ) -> WorkerFuture<'a, Result<Vec<ManifestEntry>, String>>;
    fn commit_observation<'a>(
        &'a mut self,
        job_id: &'a str,
        cursor: &'a str,
        observation: &'a Observation,
    ) -> WorkerFuture<'a, Result<ObservationCommit, String>>;
    /// Mark catalog entries belonging to this source missing unless they were
    /// observed by this job. Must be idempotent when finalization is retried.
    fn finish_job<'a>(
        &'a mut self,
        job_id: &'a str,
    ) -> WorkerFuture<'a, Result<FinishOutcome, String>>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Yielded { processed_this_run: usize },
    Completed(ImportCounts),
    Paused,
    Cancelled,
}

pub struct ImportWorker<S, R> {
    scanner: S,
    repository: R,
    batch_size: usize,
}

impl<S: Scanner, R: ImportRepository> ImportWorker<S, R> {
    pub fn new(scanner: S, repository: R, batch_size: usize) -> Self {
        Self {
            scanner,
            repository,
            batch_size: batch_size.max(1),
        }
    }

    pub fn repository(&self) -> &R {
        &self.repository
    }
    pub fn repository_mut(&mut self) -> &mut R {
        &mut self.repository
    }
    pub fn into_parts(self) -> (S, R) {
        (self.scanner, self.repository)
    }

    pub async fn enqueue(
        &mut self,
        source_id: &str,
        revision: u64,
    ) -> Result<ImportJob, WorkerError> {
        self.repository
            .create_job(source_id, revision)
            .await
            .map_err(WorkerError::Repository)
    }

    /// Called after opening persistent storage. Interrupted `Running` jobs and
    /// persisted `Queued` jobs are made visibly `Paused`. No job is run
    /// implicitly: the operator must call `resume` explicitly.
    pub async fn recover_interrupted(&mut self) -> Result<Vec<String>, WorkerError> {
        let jobs = self
            .repository
            .list_jobs()
            .await
            .map_err(WorkerError::Repository)?;
        let mut recovered = Vec::new();
        for job in jobs {
            match job.state {
                JobState::Running => {
                    let signal = self
                        .repository
                        .checkpoint_control(&job.id, true)
                        .await
                        .map_err(WorkerError::Repository)?;
                    if matches!(signal, ControlSignal::Paused | ControlSignal::Yielded) {
                        recovered.push(job.id);
                    }
                }
                JobState::Queued => {
                    self.repository
                        .request_control(&job.id, ControlRequest::Pause)
                        .await
                        .map_err(WorkerError::Repository)?;
                    recovered.push(job.id);
                }
                _ => {}
            }
        }
        Ok(recovered)
    }

    pub async fn run<'a>(
        &'a mut self,
        job_id: &'a str,
        max_observations: usize,
    ) -> Result<RunOutcome, WorkerError> {
        let claimed = self
            .repository
            .claim_job(job_id, JobState::Queued)
            .await
            .map_err(WorkerError::Repository)?;
        if !claimed {
            let job = self
                .repository
                .load_job(job_id)
                .await
                .map_err(WorkerError::Repository)?;
            return Err(WorkerError::InvalidState {
                job_id: job_id.to_owned(),
                state: job.state,
            });
        }
        self.execute(job_id, max_observations).await
    }

    pub async fn resume<'a>(
        &'a mut self,
        job_id: &'a str,
        max_observations: usize,
    ) -> Result<RunOutcome, WorkerError> {
        let claimed = self
            .repository
            .claim_job(job_id, JobState::Paused)
            .await
            .map_err(WorkerError::Repository)?;
        if !claimed {
            let job = self
                .repository
                .load_job(job_id)
                .await
                .map_err(WorkerError::Repository)?;
            return Err(WorkerError::InvalidState {
                job_id: job_id.to_owned(),
                state: job.state,
            });
        }
        self.execute(job_id, max_observations).await
    }

    pub async fn pause<'a>(&'a mut self, job_id: &'a str) -> Result<(), WorkerError> {
        self.repository
            .request_control(job_id, ControlRequest::Pause)
            .await
            .map_err(WorkerError::Repository)
    }

    pub async fn cancel<'a>(&'a mut self, job_id: &'a str) -> Result<(), WorkerError> {
        self.repository
            .request_control(job_id, ControlRequest::Cancel)
            .await
            .map_err(WorkerError::Repository)
    }

    async fn execute<'a>(
        &'a mut self,
        job_id: &'a str,
        max_observations: usize,
    ) -> Result<RunOutcome, WorkerError> {
        let job = self
            .repository
            .load_job(job_id)
            .await
            .map_err(WorkerError::Repository)?;

        let manifest_was_complete = self
            .repository
            .manifest_complete(job_id)
            .await
            .map_err(WorkerError::Repository)?;
        if !manifest_was_complete {
            // A partial manifest is never used for hashing. Restarting its
            // construction from a clean table makes process recovery safe.
            self.repository
                .reset_manifest(job_id)
                .await
                .map_err(WorkerError::Repository)?;

            let (sender, mut batches) = tokio::sync::mpsc::channel(2);
            let producer = self.scanner.build_manifest(&job.source_id, sender);
            tokio::pin!(producer);
            let mut producer_result = None;

            loop {
                let batch = if producer_result.is_some() {
                    batches.recv().await
                } else {
                    tokio::select! {
                        result = &mut producer => {
                            producer_result = Some(result);
                            continue;
                        }
                        batch = batches.recv() => batch,
                    }
                };

                let Some(batch) = batch else {
                    break;
                };

                match self
                    .repository
                    .checkpoint_control(job_id, false)
                    .await
                    .map_err(WorkerError::Repository)?
                {
                    ControlSignal::Continue => {}
                    ControlSignal::Paused => {
                        drop(batches);
                        if producer_result.is_none() {
                            let _ = (&mut producer).await;
                        }
                        return Ok(RunOutcome::Paused);
                    }
                    ControlSignal::Cancelled => {
                        drop(batches);
                        if producer_result.is_none() {
                            let _ = (&mut producer).await;
                        }
                        return Ok(RunOutcome::Cancelled);
                    }
                    ControlSignal::Yielded => {
                        return Err(WorkerError::Repository(
                            "manifest checkpoint yielded without a batch boundary".into(),
                        ));
                    }
                }

                if !batch.is_empty() {
                    self.repository
                        .append_manifest(job_id, &batch)
                        .await
                        .map_err(WorkerError::Repository)?;
                }
            }

            let producer_result = match producer_result {
                Some(result) => result,
                None => (&mut producer).await,
            };
            producer_result.map_err(WorkerError::Scanner)?;
            self.repository
                .complete_manifest(job_id)
                .await
                .map_err(WorkerError::Repository)?;
        }

        let mut total_this_run = 0usize;
        // `reset_manifest` also discards observations from any older scanner
        // version, so never resume at a cursor loaded before that reset.
        let mut after = if manifest_was_complete {
            job.cursor.clone()
        } else {
            None
        };
        loop {
            match self
                .repository
                .checkpoint_control(job_id, false)
                .await
                .map_err(WorkerError::Repository)?
            {
                ControlSignal::Continue => {}
                ControlSignal::Paused => return Ok(RunOutcome::Paused),
                ControlSignal::Yielded => {
                    return Err(WorkerError::Repository(
                        "checkpoint returned Yielded without a batch boundary".into(),
                    ));
                }
                ControlSignal::Cancelled => return Ok(RunOutcome::Cancelled),
            }
            let entries = self
                .repository
                .manifest_page(job_id, after.as_deref(), self.batch_size)
                .await
                .map_err(WorkerError::Repository)?;
            if entries.is_empty() {
                return match self
                    .repository
                    .finish_job(job_id)
                    .await
                    .map_err(WorkerError::Repository)?
                {
                    FinishOutcome::Completed(counts) => Ok(RunOutcome::Completed(counts)),
                    FinishOutcome::Paused => Ok(RunOutcome::Paused),
                    FinishOutcome::Cancelled => Ok(RunOutcome::Cancelled),
                };
            }
            let page = self
                .scanner
                .observe_page(&job.source_id, &entries)
                .await
                .map_err(WorkerError::Scanner)?;
            if page.len() != entries.len() {
                return Err(WorkerError::Scanner(
                    "scanner returned a different number of observations than manifest entries"
                        .into(),
                ));
            }
            let mut previous = after.clone();
            for (entry, observation) in entries.iter().zip(page) {
                let cursor = observation.cursor();
                if cursor != entry.path {
                    return Err(WorkerError::Scanner(
                        "scanner observation path does not match its manifest entry".into(),
                    ));
                }
                if let Some(previous) = previous.as_ref() {
                    if cursor <= *previous {
                        return Err(WorkerError::InvalidScanOrder {
                            previous: previous.clone(),
                            next: cursor,
                        });
                    }
                }
                // The repository transaction makes observation + cursor atomic.
                match self
                    .repository
                    .commit_observation(job_id, &cursor, &observation)
                    .await
                    .map_err(WorkerError::Repository)?
                {
                    ObservationCommit::Committed(_) => {}
                    ObservationCommit::Paused => return Ok(RunOutcome::Paused),
                    ObservationCommit::Cancelled => return Ok(RunOutcome::Cancelled),
                }
                after = Some(cursor);
                previous = after.clone();
                total_this_run += 1;
                if total_this_run >= max_observations.max(1) {
                    return match self
                        .repository
                        .checkpoint_control(job_id, true)
                        .await
                        .map_err(WorkerError::Repository)?
                    {
                        ControlSignal::Cancelled => Ok(RunOutcome::Cancelled),
                        ControlSignal::Paused => Ok(RunOutcome::Paused),
                        ControlSignal::Yielded => Ok(RunOutcome::Yielded {
                            processed_this_run: total_this_run,
                        }),
                        ControlSignal::Continue => Err(WorkerError::Repository(
                            "batch checkpoint continued instead of yielding".into(),
                        )),
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};

    #[derive(Clone, Default)]
    struct FakeScanner {
        files: Vec<Observation>,
    }
    impl Scanner for FakeScanner {
        fn build_manifest<'a>(
            &'a mut self,
            _source: &'a str,
            batches: tokio::sync::mpsc::Sender<Vec<ManifestEntry>>,
        ) -> WorkerFuture<'a, Result<(), String>> {
            Box::pin(async move {
                let files = self.files.clone();
                let mut entries = BTreeMap::new();
                for file in files {
                    let path = file.cursor();
                    entries.entry(path.clone()).or_insert(ManifestEntry {
                        path,
                        size: file.size,
                        modified_ns: file.modified_ns,
                        role: "IMAGE".into(),
                    });
                }
                for batch in entries.into_values().collect::<Vec<_>>().chunks(64) {
                    batches
                        .send(batch.to_vec())
                        .await
                        .map_err(|_| "manifest receiver closed".to_owned())?;
                }
                Ok(())
            })
        }

        fn observe_page<'a>(
            &'a mut self,
            _source: &'a str,
            entries: &'a [ManifestEntry],
        ) -> WorkerFuture<'a, Result<Vec<Observation>, String>> {
            Box::pin(async move {
                entries
                    .iter()
                    .map(|entry| {
                        self.files
                            .iter()
                            .find(|file| file.cursor().as_str() == entry.path.as_str())
                            .cloned()
                            .ok_or_else(|| "manifest entry not found in fake scanner".to_owned())
                    })
                    .collect()
            })
        }
    }

    #[derive(Clone, Default)]
    struct FakeRepo {
        jobs: BTreeMap<String, ImportJob>,
        entries: BTreeMap<PathBuf, Observation>,
        missing: BTreeSet<PathBuf>,
        observed: BTreeSet<PathBuf>,
        manifest: BTreeMap<String, ManifestEntry>,
        manifest_ready: bool,
    }
    impl ImportRepository for FakeRepo {
        fn create_job<'a>(
            &'a mut self,
            source: &'a str,
            revision: u64,
        ) -> WorkerFuture<'a, Result<ImportJob, String>> {
            Box::pin(async move {
                let id = format!("job-{}", self.jobs.len() + 1);
                let job = ImportJob {
                    id: id.clone(),
                    source_id: source.into(),
                    revision,
                    cursor: None,
                    state: JobState::Queued,
                    processed: 0,
                };
                self.jobs.insert(id, job.clone());
                Ok(job)
            })
        }
        fn load_job<'a>(&'a self, id: &'a str) -> WorkerFuture<'a, Result<ImportJob, String>> {
            Box::pin(async move {
                self.jobs
                    .get(id)
                    .cloned()
                    .ok_or_else(|| "unknown job".into())
            })
        }
        fn list_jobs(&self) -> WorkerFuture<'_, Result<Vec<ImportJob>, String>> {
            Box::pin(async move { Ok(self.jobs.values().cloned().collect()) })
        }
        fn claim_job<'a>(
            &'a mut self,
            id: &'a str,
            expected: JobState,
        ) -> WorkerFuture<'a, Result<bool, String>> {
            Box::pin(async move {
                let job = self.jobs.get_mut(id).ok_or("unknown job")?;
                if job.state != expected {
                    return Ok(false);
                }
                job.state = JobState::Running;
                Ok(true)
            })
        }
        fn request_control<'a>(
            &'a mut self,
            id: &'a str,
            request: ControlRequest,
        ) -> WorkerFuture<'a, Result<(), String>> {
            Box::pin(async move {
                let job = self.jobs.get_mut(id).ok_or("unknown job")?;
                match request {
                    ControlRequest::Pause if job.state == JobState::Running => {
                        job.state = JobState::Paused;
                    }
                    ControlRequest::Pause if job.state == JobState::Queued => {
                        job.state = JobState::Paused;
                    }
                    ControlRequest::Pause if job.state == JobState::Paused => {}
                    ControlRequest::Cancel if job.state == JobState::Completed => {
                        return Err("cannot cancel completed job".into());
                    }
                    ControlRequest::Cancel => job.state = JobState::Cancelled,
                    ControlRequest::Pause => return Err("cannot pause terminal job".into()),
                }
                Ok(())
            })
        }
        fn checkpoint_control<'a>(
            &'a mut self,
            id: &'a str,
            yield_if_running: bool,
        ) -> WorkerFuture<'a, Result<ControlSignal, String>> {
            Box::pin(async move {
                let job = self.jobs.get_mut(id).ok_or("unknown job")?;
                match job.state.clone() {
                    JobState::Running if yield_if_running => {
                        job.state = JobState::Paused;
                        Ok(ControlSignal::Yielded)
                    }
                    JobState::Running => Ok(ControlSignal::Continue),
                    JobState::Paused => Ok(ControlSignal::Paused),
                    JobState::Cancelled => Ok(ControlSignal::Cancelled),
                    state => Err(format!("cannot checkpoint job in state {state:?}")),
                }
            })
        }
        fn reset_manifest<'a>(&'a mut self, _id: &'a str) -> WorkerFuture<'a, Result<(), String>> {
            Box::pin(async move {
                self.manifest.clear();
                self.manifest_ready = false;
                Ok(())
            })
        }
        fn append_manifest<'a>(
            &'a mut self,
            _id: &'a str,
            entries: &'a [ManifestEntry],
        ) -> WorkerFuture<'a, Result<(), String>> {
            Box::pin(async move {
                for entry in entries {
                    self.manifest.insert(entry.path.clone(), entry.clone());
                }
                Ok(())
            })
        }
        fn complete_manifest<'a>(
            &'a mut self,
            _id: &'a str,
        ) -> WorkerFuture<'a, Result<(), String>> {
            Box::pin(async move {
                self.manifest_ready = true;
                Ok(())
            })
        }
        fn manifest_complete<'a>(&'a self, _id: &'a str) -> WorkerFuture<'a, Result<bool, String>> {
            Box::pin(async move { Ok(self.manifest_ready) })
        }
        fn manifest_page<'a>(
            &'a self,
            _id: &'a str,
            after: Option<&'a str>,
            limit: usize,
        ) -> WorkerFuture<'a, Result<Vec<ManifestEntry>, String>> {
            Box::pin(async move {
                Ok(self
                    .manifest
                    .iter()
                    .filter(|(path, _)| after.map_or(true, |cursor| path.as_str() > cursor))
                    .take(limit)
                    .map(|(_, entry)| entry.clone())
                    .collect())
            })
        }
        fn commit_observation<'a>(
            &'a mut self,
            id: &'a str,
            cursor: &'a str,
            obs: &'a Observation,
        ) -> WorkerFuture<'a, Result<ObservationCommit, String>> {
            Box::pin(async move {
                let job = self.jobs.get_mut(id).ok_or("unknown job")?;
                if job.state == JobState::Paused {
                    return Ok(ObservationCommit::Paused);
                }
                if job.state == JobState::Cancelled {
                    return Ok(ObservationCommit::Cancelled);
                }
                if job.cursor.as_deref() == Some(cursor) {
                    return Ok(ObservationCommit::Committed(EntryChange::Unchanged));
                }
                let change = match self.entries.get(&obs.path) {
                    None => EntryChange::New,
                    Some(previous) if previous.content_hash != obs.content_hash => {
                        EntryChange::Modified
                    }
                    Some(_) => EntryChange::Unchanged,
                };
                self.entries.insert(obs.path.clone(), obs.clone());
                self.observed.insert(obs.path.clone());
                job.cursor = Some(cursor.into());
                job.processed += 1;
                Ok(ObservationCommit::Committed(change))
            })
        }
        fn finish_job<'a>(
            &'a mut self,
            id: &'a str,
        ) -> WorkerFuture<'a, Result<FinishOutcome, String>> {
            Box::pin(async move {
                let job = self.jobs.get_mut(id).ok_or("unknown job")?;
                if job.state == JobState::Cancelled {
                    return Ok(FinishOutcome::Cancelled);
                }
                if job.state == JobState::Paused {
                    return Ok(FinishOutcome::Paused);
                }
                let mut counts = ImportCounts::default();
                self.missing = self
                    .entries
                    .keys()
                    .filter(|path| !self.observed.contains(*path))
                    .cloned()
                    .collect();
                counts.missing = self.missing.len() as u64;
                job.state = JobState::Completed;
                Ok(FinishOutcome::Completed(counts))
            })
        }
    }

    fn file(path: &str, bytes: &[u8]) -> Observation {
        Observation {
            path: path.into(),
            size: bytes.len() as u64,
            modified_ns: Some(10),
            quick_hash: vec![1],
            content_hash: bytes.to_vec(),
        }
    }

    #[tokio::test]
    async fn interruption_recovers_paused_and_explicit_resume_is_idempotent_and_read_only() {
        let originals_before = vec![
            file("a.jpg", b"one"),
            file("b.jpg", b"two"),
            file("c.jpg", b"three"),
        ];
        let scanner = FakeScanner {
            files: originals_before.clone(),
        };
        let mut worker = ImportWorker::new(scanner.clone(), FakeRepo::default(), 2);
        let job = worker.enqueue("source", 7).await.unwrap();
        assert_eq!(
            worker.run(&job.id, 1).await.unwrap(),
            RunOutcome::Yielded {
                processed_this_run: 1
            }
        );
        // Simulate process death: persist Running state, then construct a new worker
        // from the same repository and scanner, as a database-backed host would.
        let (scanner, mut repository) = worker.into_parts();
        // Simulate a crash after a committed checkpoint: persisted state still
        // says Running, so startup recovery must pause it atomically.
        repository.jobs.get_mut(&job.id).unwrap().state = JobState::Running;
        let recovered_worker = ImportWorker::new(scanner.clone(), repository.clone(), 2);
        let mut worker = recovered_worker;
        assert_eq!(
            worker.recover_interrupted().await.unwrap(),
            vec![job.id.clone()]
        );
        assert_eq!(
            worker.repository().load_job(&job.id).await.unwrap().state,
            JobState::Paused
        );
        let outcome = worker.resume(&job.id, 20).await.unwrap();
        assert!(matches!(outcome, RunOutcome::Completed(_)));
        let done = worker.repository().load_job(&job.id).await.unwrap();
        assert_eq!(done.state, JobState::Completed);
        assert_eq!(done.processed, 3);
        assert_eq!(worker.repository().entries.len(), 3);
        assert_eq!(originals_before, scanner.files);
        // Completed jobs cannot be resumed or replayed accidentally.
        assert!(matches!(
            worker.resume(&job.id, 20).await,
            Err(WorkerError::InvalidState { .. })
        ));
        repository = worker.repository().clone();
        assert_eq!(repository.entries.len(), 3);
    }

    #[tokio::test]
    async fn same_path_hash_change_is_modified_and_absent_paths_become_missing_at_completion() {
        let initial = vec![file("a.jpg", b"one"), file("gone.jpg", b"old")];
        let mut repo = FakeRepo::default();
        repo.entries.insert("a.jpg".into(), initial[0].clone());
        repo.entries.insert("gone.jpg".into(), initial[1].clone());
        let changed = file("a.jpg", b"changed");
        let scanner = FakeScanner {
            files: vec![changed],
        };
        let mut worker = ImportWorker::new(scanner, repo, 2);
        let job = worker.enqueue("source", 8).await.unwrap();
        worker.run(&job.id, 20).await.unwrap();
        assert!(
            worker
                .repository()
                .missing
                .contains(&PathBuf::from("gone.jpg"))
        );
        assert_eq!(
            worker
                .repository()
                .entries
                .get(&PathBuf::from("a.jpg"))
                .unwrap()
                .content_hash,
            b"changed"
        );
    }
}
