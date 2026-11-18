//! Persistent, explicitly resumable import/revision worker.
//!
//! This module owns orchestration only. Filesystem traversal and catalog writes are
//! injected through [`Scanner`] and [`ImportRepository`], so the worker never opens,
//! renames, or writes an original. `commit_observation` must atomically record the
//! idempotent catalog change and its cursor; this is what makes a crash between
//! batches safe.

use std::collections::BTreeSet;
use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Completed,
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

impl Observation {
    pub fn cursor(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EntryChange {
    New,
    Modified,
    Moved { from: PathBuf },
    Unchanged,
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

/// The scanner must return paths in strictly increasing lexical order and honor
/// the exclusive `after` cursor. Its results are read-only descriptions of files.
pub trait Scanner {
    fn scan_after(
        &mut self,
        source_id: &str,
        after: Option<&str>,
        limit: usize,
    ) -> Result<Vec<Observation>, String>;
}

/// Implementations persist jobs/catalog entries. `commit_observation` is a single
/// transaction: upsert/classify the path, associate its cursor with this job,
/// increment processed, and make retries of the same cursor idempotent.
pub trait ImportRepository {
    fn create_job(&mut self, source_id: &str, revision: u64) -> Result<ImportJob, String>;
    fn load_job(&self, job_id: &str) -> Result<ImportJob, String>;
    fn list_jobs(&self) -> Result<Vec<ImportJob>, String>;
    fn set_state(&mut self, job_id: &str, state: JobState) -> Result<(), String>;
    fn commit_observation(
        &mut self,
        job_id: &str,
        cursor: &str,
        observation: &Observation,
    ) -> Result<EntryChange, String>;
    /// Mark catalog entries belonging to this source missing unless they were
    /// observed by this job. Must be idempotent when finalization is retried.
    fn finish_job(
        &mut self,
        job_id: &str,
        seen_paths: &BTreeSet<PathBuf>,
    ) -> Result<ImportCounts, String>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    Yielded { processed_this_run: usize },
    Completed(ImportCounts),
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

    pub fn enqueue(&mut self, source_id: &str, revision: u64) -> Result<ImportJob, WorkerError> {
        self.repository
            .create_job(source_id, revision)
            .map_err(WorkerError::Repository)
    }

    /// Called after opening persistent storage. A process that died while a job
    /// was running leaves it in `Running`; recovery makes it visibly `Paused`.
    /// No job is run implicitly: the operator must call `resume` explicitly.
    pub fn recover_interrupted(&mut self) -> Result<Vec<String>, WorkerError> {
        let jobs = self
            .repository
            .list_jobs()
            .map_err(WorkerError::Repository)?;
        let mut recovered = Vec::new();
        for job in jobs {
            if job.state == JobState::Running {
                self.repository
                    .set_state(&job.id, JobState::Paused)
                    .map_err(WorkerError::Repository)?;
                recovered.push(job.id);
            }
        }
        Ok(recovered)
    }

    pub fn run(
        &mut self,
        job_id: &str,
        max_observations: usize,
    ) -> Result<RunOutcome, WorkerError> {
        let job = self
            .repository
            .load_job(job_id)
            .map_err(WorkerError::Repository)?;
        if job.state != JobState::Queued {
            return Err(WorkerError::InvalidState {
                job_id: job_id.to_owned(),
                state: job.state,
            });
        }
        self.repository
            .set_state(job_id, JobState::Running)
            .map_err(WorkerError::Repository)?;
        self.execute(job_id, max_observations)
    }

    pub fn resume(
        &mut self,
        job_id: &str,
        max_observations: usize,
    ) -> Result<RunOutcome, WorkerError> {
        let job = self
            .repository
            .load_job(job_id)
            .map_err(WorkerError::Repository)?;
        if job.state != JobState::Paused {
            return Err(WorkerError::InvalidState {
                job_id: job_id.to_owned(),
                state: job.state,
            });
        }
        self.repository
            .set_state(job_id, JobState::Running)
            .map_err(WorkerError::Repository)?;
        self.execute(job_id, max_observations)
    }

    pub fn pause(&mut self, job_id: &str) -> Result<(), WorkerError> {
        let job = self
            .repository
            .load_job(job_id)
            .map_err(WorkerError::Repository)?;
        if job.state != JobState::Running {
            return Err(WorkerError::InvalidState {
                job_id: job_id.to_owned(),
                state: job.state,
            });
        }
        self.repository
            .set_state(job_id, JobState::Paused)
            .map_err(WorkerError::Repository)
    }

    fn execute(
        &mut self,
        job_id: &str,
        max_observations: usize,
    ) -> Result<RunOutcome, WorkerError> {
        let job = self
            .repository
            .load_job(job_id)
            .map_err(WorkerError::Repository)?;
        let mut total_this_run = 0usize;
        let mut seen_paths = BTreeSet::new();
        // Rebuild the seen set from the committed prefix; scanner ordering and
        // cursor semantics make this bounded by the work already completed.
        let mut replay_after: Option<String> = None;
        while replay_after.as_deref() != job.cursor.as_deref() {
            let page = self
                .scanner
                .scan_after(&job.source_id, replay_after.as_deref(), self.batch_size)
                .map_err(WorkerError::Scanner)?;
            if page.is_empty() {
                break;
            }
            for observation in page {
                let cursor = observation.cursor();
                seen_paths.insert(observation.path.clone());
                replay_after = Some(cursor);
                if replay_after.as_deref() == job.cursor.as_deref() {
                    break;
                }
            }
        }

        let mut after = job.cursor.clone();
        loop {
            let page = self
                .scanner
                .scan_after(&job.source_id, after.as_deref(), self.batch_size)
                .map_err(WorkerError::Scanner)?;
            if page.is_empty() {
                let counts = self
                    .repository
                    .finish_job(job_id, &seen_paths)
                    .map_err(WorkerError::Repository)?;
                self.repository
                    .set_state(job_id, JobState::Completed)
                    .map_err(WorkerError::Repository)?;
                return Ok(RunOutcome::Completed(counts));
            }
            let mut previous = after.clone();
            for observation in page {
                let cursor = observation.cursor();
                if let Some(previous) = previous.as_ref() {
                    if cursor <= *previous {
                        return Err(WorkerError::InvalidScanOrder {
                            previous: previous.clone(),
                            next: cursor,
                        });
                    }
                }
                // The repository transaction makes observation + cursor atomic.
                self.repository
                    .commit_observation(job_id, &cursor, &observation)
                    .map_err(WorkerError::Repository)?;
                seen_paths.insert(observation.path);
                after = Some(cursor);
                previous = after.clone();
                total_this_run += 1;
                if total_this_run >= max_observations.max(1) {
                    return Ok(RunOutcome::Yielded {
                        processed_this_run: total_this_run,
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Clone, Default)]
    struct FakeScanner {
        files: Vec<Observation>,
    }
    impl Scanner for FakeScanner {
        fn scan_after(
            &mut self,
            _source: &str,
            after: Option<&str>,
            limit: usize,
        ) -> Result<Vec<Observation>, String> {
            Ok(self
                .files
                .iter()
                .filter(|file| after.map_or(true, |cursor| file.cursor().as_str() > cursor))
                .take(limit)
                .cloned()
                .collect())
        }
    }

    #[derive(Clone, Default)]
    struct FakeRepo {
        jobs: BTreeMap<String, ImportJob>,
        entries: BTreeMap<PathBuf, Observation>,
        missing: BTreeSet<PathBuf>,
    }
    impl ImportRepository for FakeRepo {
        fn create_job(&mut self, source: &str, revision: u64) -> Result<ImportJob, String> {
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
        }
        fn load_job(&self, id: &str) -> Result<ImportJob, String> {
            self.jobs
                .get(id)
                .cloned()
                .ok_or_else(|| "unknown job".into())
        }
        fn list_jobs(&self) -> Result<Vec<ImportJob>, String> {
            Ok(self.jobs.values().cloned().collect())
        }
        fn set_state(&mut self, id: &str, state: JobState) -> Result<(), String> {
            self.jobs.get_mut(id).ok_or("unknown job")?.state = state;
            Ok(())
        }
        fn commit_observation(
            &mut self,
            id: &str,
            cursor: &str,
            obs: &Observation,
        ) -> Result<EntryChange, String> {
            let job = self.jobs.get_mut(id).ok_or("unknown job")?;
            if job.cursor.as_deref() == Some(cursor) {
                return Ok(EntryChange::Unchanged);
            }
            let change = match self.entries.get(&obs.path) {
                None => EntryChange::New,
                Some(previous) if previous.content_hash != obs.content_hash => {
                    EntryChange::Modified
                }
                Some(_) => EntryChange::Unchanged,
            };
            self.entries.insert(obs.path.clone(), obs.clone());
            job.cursor = Some(cursor.into());
            job.processed += 1;
            Ok(change)
        }
        fn finish_job(
            &mut self,
            _id: &str,
            seen: &BTreeSet<PathBuf>,
        ) -> Result<ImportCounts, String> {
            let mut counts = ImportCounts::default();
            self.missing = self
                .entries
                .keys()
                .filter(|path| !seen.contains(*path))
                .cloned()
                .collect();
            counts.missing = self.missing.len() as u64;
            Ok(counts)
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

    #[test]
    fn interruption_recovers_paused_and_explicit_resume_is_idempotent_and_read_only() {
        let originals_before = vec![
            file("a.jpg", b"one"),
            file("b.jpg", b"two"),
            file("c.jpg", b"three"),
        ];
        let scanner = FakeScanner {
            files: originals_before.clone(),
        };
        let mut worker = ImportWorker::new(scanner.clone(), FakeRepo::default(), 2);
        let job = worker.enqueue("source", 7).unwrap();
        assert_eq!(
            worker.run(&job.id, 1).unwrap(),
            RunOutcome::Yielded {
                processed_this_run: 1
            }
        );
        // Simulate process death: persist Running state, then construct a new worker
        // from the same repository and scanner, as a database-backed host would.
        let (scanner, mut repository) = worker.into_parts();
        let recovered_worker = ImportWorker::new(scanner.clone(), repository.clone(), 2);
        let mut worker = recovered_worker;
        assert_eq!(worker.recover_interrupted().unwrap(), vec![job.id.clone()]);
        assert_eq!(
            worker.repository().load_job(&job.id).unwrap().state,
            JobState::Paused
        );
        let outcome = worker.resume(&job.id, 20).unwrap();
        assert!(matches!(outcome, RunOutcome::Completed(_)));
        let done = worker.repository().load_job(&job.id).unwrap();
        assert_eq!(done.state, JobState::Completed);
        assert_eq!(done.processed, 3);
        assert_eq!(worker.repository().entries.len(), 3);
        assert_eq!(originals_before, scanner.files);
        // Completed jobs cannot be resumed or replayed accidentally.
        assert!(matches!(
            worker.resume(&job.id, 20),
            Err(WorkerError::InvalidState { .. })
        ));
        repository = worker.repository().clone();
        assert_eq!(repository.entries.len(), 3);
    }

    #[test]
    fn same_path_hash_change_is_modified_and_absent_paths_become_missing_at_completion() {
        let initial = vec![file("a.jpg", b"one"), file("gone.jpg", b"old")];
        let mut repo = FakeRepo::default();
        repo.entries.insert("a.jpg".into(), initial[0].clone());
        repo.entries.insert("gone.jpg".into(), initial[1].clone());
        let changed = file("a.jpg", b"changed");
        let scanner = FakeScanner {
            files: vec![changed],
        };
        let mut worker = ImportWorker::new(scanner, repo, 2);
        let job = worker.enqueue("source", 8).unwrap();
        worker.run(&job.id, 20).unwrap();
        assert!(worker
            .repository()
            .missing
            .contains(&PathBuf::from("gone.jpg")));
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
