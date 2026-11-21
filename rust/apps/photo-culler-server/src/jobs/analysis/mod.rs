//! Bounded, resumable analysis coordinator.
//!
//! This module is transport- and storage-independent. A server adapter supplies
//! a durable repository, analyzer, and clock. Repository writes must replace a
//! job atomically so a cursor and its completed photo result cannot diverge.

mod sqlite;

pub use sqlite::{ImageEngineAnalyzer, SqliteRepository, SystemClock, prepare_gallery_plan};

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    Gallery { gallery_id: String },
    Session { session_id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Photo {
    pub photo_id: String,
    pub source: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub catalog_id: String,
    pub scope: Scope,
    pub profile_id: String,
    pub photos: Vec<Photo>,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct JobKey {
    pub catalog_id: String,
    pub job_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Queued,
    Running,
    Paused,
    Succeeded,
    Failed { message: String },
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    Analyzed { metrics: Vec<String> },
    Failed { message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhotoProgress {
    pub photo_id: String,
    pub outcome: Outcome,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    pub key: JobKey,
    pub plan: Plan,
    pub state: State,
    /// Index of the next photo to process. Completed results are committed with it.
    pub next_index: usize,
    pub completed: Vec<PhotoProgress>,
    /// Analyzer output not yet committed to `completed`; always discarded on recovery.
    pub partial: Option<PhotoProgress>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl Job {
    pub fn total(&self) -> usize {
        self.plan.photos.len()
    }

    pub fn completed_count(&self) -> usize {
        self.completed.len()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    QueueFull,
    JobNotFound,
    InvalidTransition,
    DuplicatePhoto,
    InvalidPlan,
    Repository(String),
}

pub trait Repository {
    fn load_all(&self) -> Result<Vec<Job>, String>;
    /// Atomically persist the complete job snapshot.
    fn save(&mut self, job: &Job) -> Result<(), String>;
}

pub trait Analyzer {
    fn analyze(
        &mut self,
        catalog_id: &str,
        photo: &Photo,
        profile_id: &str,
    ) -> Result<Vec<String>, String>;
}

pub trait Clock {
    fn now(&self) -> u64;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Progress {
    pub completed: usize,
    pub total: usize,
    pub failed: usize,
    pub percent: u8,
}

pub struct Coordinator<R, A, C> {
    repository: R,
    analyzer: A,
    clock: C,
    max_running: usize,
    max_queued: usize,
    jobs: BTreeMap<JobKey, Job>,
}

impl<R: Repository, A: Analyzer, C: Clock> Coordinator<R, A, C> {
    pub fn new(
        mut repository: R,
        analyzer: A,
        clock: C,
        max_running: usize,
        max_queued: usize,
    ) -> Result<Self, Error> {
        if max_running == 0 {
            return Err(Error::InvalidPlan);
        }
        let mut jobs = BTreeMap::new();
        for mut job in repository.load_all().map_err(Error::Repository)? {
            validate_job_snapshot(&job)?;
            if job.state == State::Running {
                // A crash may have left analyzer output without a durable cursor commit.
                job.partial = None;
                job.state = State::Paused;
                job.updated_at = clock.now();
                repository.save(&job).map_err(Error::Repository)?;
            } else if job.partial.take().is_some() {
                // Partial output is never committed with the cursor, regardless
                // of the persisted state. Do not expose it after recovery.
                repository.save(&job).map_err(Error::Repository)?;
            }
            jobs.insert(job.key.clone(), job);
        }
        Ok(Self {
            repository,
            analyzer,
            clock,
            max_running,
            max_queued,
            jobs,
        })
    }

    pub fn submit(&mut self, job_id: impl Into<String>, plan: Plan) -> Result<JobKey, Error> {
        if plan.catalog_id.trim().is_empty() || plan.profile_id.trim().is_empty() {
            return Err(Error::InvalidPlan);
        }
        let mut ids = std::collections::BTreeSet::new();
        if plan
            .photos
            .iter()
            .any(|photo| photo.photo_id.trim().is_empty() || !ids.insert(photo.photo_id.clone()))
        {
            return Err(Error::DuplicatePhoto);
        }
        let active = self
            .jobs
            .values()
            .filter(|job| matches!(job.state, State::Queued | State::Running | State::Paused))
            .count();
        if active >= self.max_running + self.max_queued {
            return Err(Error::QueueFull);
        }
        let key = JobKey {
            catalog_id: plan.catalog_id.clone(),
            job_id: job_id.into(),
        };
        if self.jobs.contains_key(&key) {
            return Err(Error::InvalidPlan);
        }
        let now = self.clock.now();
        let job = Job {
            key: key.clone(),
            plan,
            state: State::Queued,
            next_index: 0,
            completed: Vec::new(),
            partial: None,
            created_at: now,
            updated_at: now,
        };
        self.repository.save(&job).map_err(Error::Repository)?;
        self.jobs.insert(key.clone(), job);
        Ok(key)
    }

    pub fn start(&mut self, key: &JobKey) -> Result<(), Error> {
        let job = self.jobs.get(key).ok_or(Error::JobNotFound)?;
        if !matches!(job.state, State::Queued | State::Paused) {
            return Err(Error::InvalidTransition);
        }
        let running = self
            .jobs
            .values()
            .filter(|job| job.state == State::Running)
            .count();
        if running >= self.max_running {
            return Err(Error::QueueFull);
        }
        self.update(key, |job| {
            job.state = State::Running;
            job.partial = None;
        })
    }

    pub fn pause(&mut self, key: &JobKey) -> Result<(), Error> {
        self.transition_running(key, State::Paused)
    }

    pub fn resume(&mut self, key: &JobKey) -> Result<(), Error> {
        self.start(key)
    }

    /// Replace a queued or recovered plan with its freshly authorized catalog
    /// snapshot before starting analysis. The photo identity/order, scope and
    /// profile must remain stable so persisted progress cannot shift to a
    /// different set of photos.
    pub fn refresh_plan(&mut self, key: &JobKey, plan: Plan) -> Result<(), Error> {
        let job = self.jobs.get(key).ok_or(Error::JobNotFound)?;
        if !matches!(job.state, State::Queued | State::Paused) {
            return Err(Error::InvalidTransition);
        }
        let old_photo_ids = job
            .plan
            .photos
            .iter()
            .map(|photo| photo.photo_id.as_str())
            .collect::<Vec<_>>();
        let new_photo_ids = plan
            .photos
            .iter()
            .map(|photo| photo.photo_id.as_str())
            .collect::<Vec<_>>();
        if plan.catalog_id != job.plan.catalog_id
            || plan.scope != job.plan.scope
            || plan.profile_id != job.plan.profile_id
            || old_photo_ids != new_photo_ids
        {
            return Err(Error::InvalidPlan);
        }
        self.update(key, |job| {
            job.plan = plan;
            job.partial = None;
        })
    }

    pub fn cancel(&mut self, key: &JobKey) -> Result<(), Error> {
        let state = &self.jobs.get(key).ok_or(Error::JobNotFound)?.state;
        if !matches!(state, State::Queued | State::Running | State::Paused) {
            return Err(Error::InvalidTransition);
        }
        self.update(key, |job| {
            job.state = State::Cancelled;
            job.partial = None;
        })
    }

    /// Analyze one photo at a time; pause/cancel take effect at the next boundary.
    pub fn step(&mut self, key: &JobKey) -> Result<Progress, Error> {
        let job = self.jobs.get(key).ok_or(Error::JobNotFound)?.clone();
        if job.state != State::Running {
            return Err(Error::InvalidTransition);
        }
        if job.next_index >= job.plan.photos.len() {
            self.update(key, |job| job.state = State::Succeeded)?;
            return self.progress(key);
        }
        let photo = job.plan.photos[job.next_index].clone();
        let outcome =
            match self
                .analyzer
                .analyze(&job.plan.catalog_id, &photo, &job.plan.profile_id)
            {
                Ok(metrics) => Outcome::Analyzed { metrics },
                Err(message) => Outcome::Failed { message },
            };
        self.update(key, |job| {
            let result = PhotoProgress {
                photo_id: photo.photo_id,
                outcome,
            };
            job.partial = Some(result.clone());
            job.completed.push(result);
            job.next_index += 1;
            job.partial = None;
            if job.next_index == job.plan.photos.len() {
                job.state = State::Succeeded;
            }
        })?;
        self.progress(key)
    }

    pub fn progress(&self, key: &JobKey) -> Result<Progress, Error> {
        let job = self.jobs.get(key).ok_or(Error::JobNotFound)?;
        let completed = job.completed.len();
        let total = job.total();
        let failed = job
            .completed
            .iter()
            .filter(|entry| matches!(entry.outcome, Outcome::Failed { .. }))
            .count();
        Ok(Progress {
            completed,
            total,
            failed,
            percent: if total == 0 {
                100
            } else {
                ((completed * 100) / total) as u8
            },
        })
    }

    pub fn get(&self, key: &JobKey) -> Option<&Job> {
        self.jobs.get(key)
    }

    /// Return one recoverable job for a gallery, preferring the most recently
    /// updated active job. Terminal jobs are not returned.
    pub fn active_gallery_job(&self, gallery_id: &str) -> Option<&Job> {
        self.jobs
            .values()
            .filter(|job| {
                matches!(&job.state, State::Queued | State::Running | State::Paused)
                    && matches!(
                        &job.plan.scope,
                        Scope::Gallery { gallery_id: stored } if stored == gallery_id
                    )
            })
            .max_by(|left, right| {
                (left.updated_at, left.created_at, left.key.job_id.as_str()).cmp(&(
                    right.updated_at,
                    right.created_at,
                    right.key.job_id.as_str(),
                ))
            })
    }

    pub fn repository(&self) -> &R {
        &self.repository
    }
    pub fn analyzer(&self) -> &A {
        &self.analyzer
    }

    fn transition_running(&mut self, key: &JobKey, state: State) -> Result<(), Error> {
        if self.jobs.get(key).ok_or(Error::JobNotFound)?.state != State::Running {
            return Err(Error::InvalidTransition);
        }
        self.update(key, |job| job.state = state)
    }

    fn update(&mut self, key: &JobKey, change: impl FnOnce(&mut Job)) -> Result<(), Error> {
        let mut candidate = self.jobs.get(key).ok_or(Error::JobNotFound)?.clone();
        change(&mut candidate);
        candidate.updated_at = self.clock.now();
        self.repository
            .save(&candidate)
            .map_err(Error::Repository)?;
        self.jobs.insert(key.clone(), candidate);
        Ok(())
    }
}

fn validate_job_snapshot(job: &Job) -> Result<(), Error> {
    if job.key.catalog_id.trim().is_empty()
        || job.key.job_id.trim().is_empty()
        || job.key.catalog_id != job.plan.catalog_id
        || job.plan.profile_id.trim().is_empty()
        || job.next_index > job.plan.photos.len()
        || job.completed.len() != job.next_index
        || (job.state == State::Queued && job.next_index != 0)
        || (job.state == State::Succeeded && job.next_index != job.plan.photos.len())
    {
        return Err(Error::InvalidPlan);
    }
    let mut photo_ids = std::collections::BTreeSet::new();
    for (index, photo) in job.plan.photos.iter().enumerate() {
        if photo.photo_id.trim().is_empty() || !photo_ids.insert(photo.photo_id.as_str()) {
            return Err(Error::DuplicatePhoto);
        }
        if job
            .completed
            .get(index)
            .is_some_and(|result| result.photo_id != photo.photo_id)
        {
            return Err(Error::InvalidPlan);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeMap,
    };

    #[derive(Default)]
    struct FakeRepo(RefCell<BTreeMap<JobKey, Job>>);
    impl Repository for FakeRepo {
        fn load_all(&self) -> Result<Vec<Job>, String> {
            Ok(self.0.borrow().values().cloned().collect())
        }
        fn save(&mut self, job: &Job) -> Result<(), String> {
            self.0.get_mut().insert(job.key.clone(), job.clone());
            Ok(())
        }
    }
    #[derive(Default)]
    struct FakeClock(Cell<u64>);
    impl Clock for FakeClock {
        fn now(&self) -> u64 {
            self.0.get()
        }
    }
    #[derive(Default)]
    struct FakeAnalyzer {
        calls: RefCell<Vec<(String, String)>>,
    }
    impl Analyzer for FakeAnalyzer {
        fn analyze(
            &mut self,
            catalog_id: &str,
            photo: &Photo,
            _profile_id: &str,
        ) -> Result<Vec<String>, String> {
            self.calls
                .borrow_mut()
                .push((catalog_id.into(), photo.photo_id.clone()));
            Ok(vec![format!("metric:{}", photo.photo_id)])
        }
    }
    fn plan(catalog_id: &str, scope: Scope, photos: &[&str]) -> Plan {
        Plan {
            catalog_id: catalog_id.into(),
            scope,
            profile_id: "technical".into(),
            photos: photos
                .iter()
                .map(|id| Photo {
                    photo_id: (*id).into(),
                    source: format!("{id}.jpg"),
                })
                .collect(),
        }
    }

    #[test]
    fn recovers_interrupted_job_invalidates_partial_and_resumes_from_persisted_cursor() {
        let mut repo = FakeRepo::default();
        let key = JobKey {
            catalog_id: "catalog-a".into(),
            job_id: "job-a".into(),
        };
        repo.save(&Job {
            key: key.clone(),
            plan: plan(
                "catalog-a",
                Scope::Gallery {
                    gallery_id: "g".into(),
                },
                &["p1", "p2"],
            ),
            state: State::Running,
            next_index: 1,
            completed: vec![PhotoProgress {
                photo_id: "p1".into(),
                outcome: Outcome::Analyzed {
                    metrics: vec!["m".into()],
                },
            }],
            partial: Some(PhotoProgress {
                photo_id: "p2".into(),
                outcome: Outcome::Analyzed {
                    metrics: vec!["uncommitted".into()],
                },
            }),
            created_at: 1,
            updated_at: 1,
        })
        .unwrap();
        let mut coordinator =
            Coordinator::new(repo, FakeAnalyzer::default(), FakeClock(Cell::new(2)), 1, 3).unwrap();
        assert_eq!(coordinator.get(&key).unwrap().state, State::Paused);
        assert_eq!(coordinator.get(&key).unwrap().partial, None);
        assert_eq!(coordinator.progress(&key).unwrap().completed, 1);
        coordinator.resume(&key).unwrap();
        let progress = coordinator.step(&key).unwrap();
        assert_eq!(progress.completed, 2);
        assert_eq!(progress.percent, 100);
        assert_eq!(coordinator.get(&key).unwrap().completed[1].photo_id, "p2");
    }

    #[test]
    fn cancellation_is_cooperative_at_photo_boundaries_and_catalogs_stay_isolated() {
        let repo = FakeRepo::default();
        let mut coordinator =
            Coordinator::new(repo, FakeAnalyzer::default(), FakeClock::default(), 1, 2).unwrap();
        let a = coordinator
            .submit(
                "same-id",
                plan(
                    "catalog-a",
                    Scope::Session {
                        session_id: "s1".into(),
                    },
                    &["a1", "a2"],
                ),
            )
            .unwrap();
        let b = coordinator
            .submit(
                "same-id",
                plan(
                    "catalog-b",
                    Scope::Gallery {
                        gallery_id: "g2".into(),
                    },
                    &["b1"],
                ),
            )
            .unwrap();
        coordinator.start(&a).unwrap();
        coordinator.start(&b).unwrap_err();
        assert_eq!(coordinator.step(&a).unwrap().completed, 1);
        coordinator.cancel(&a).unwrap();
        assert_eq!(coordinator.get(&a).unwrap().state, State::Cancelled);
        coordinator.start(&b).unwrap();
        coordinator.step(&b).unwrap();
        assert_eq!(coordinator.get(&b).unwrap().completed[0].photo_id, "b1");
        assert_eq!(coordinator.get(&a).unwrap().completed_count(), 1);
    }

    #[test]
    fn admission_is_bounded_and_pause_resume_are_checked() {
        let mut coordinator = Coordinator::new(
            FakeRepo::default(),
            FakeAnalyzer::default(),
            FakeClock::default(),
            1,
            1,
        )
        .unwrap();
        let a = coordinator
            .submit(
                "a",
                plan(
                    "c",
                    Scope::Gallery {
                        gallery_id: "g".into(),
                    },
                    &["1"],
                ),
            )
            .unwrap();
        let b = coordinator
            .submit(
                "b",
                plan(
                    "c",
                    Scope::Gallery {
                        gallery_id: "g".into(),
                    },
                    &["2"],
                ),
            )
            .unwrap();
        assert_eq!(
            coordinator.submit(
                "c",
                plan(
                    "c",
                    Scope::Gallery {
                        gallery_id: "g".into()
                    },
                    &["3"]
                )
            ),
            Err(Error::QueueFull)
        );
        coordinator.start(&a).unwrap();
        assert_eq!(coordinator.pause(&b), Err(Error::InvalidTransition));
        coordinator.pause(&a).unwrap();
        coordinator.resume(&a).unwrap();
        assert_eq!(coordinator.step(&a).unwrap().completed, 1);
    }
}
