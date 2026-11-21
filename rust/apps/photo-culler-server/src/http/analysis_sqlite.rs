//! Durable coordinator owner used by the persistent catalog runtime.
//!
//! All synchronous coordinator and image-engine work runs on Tokio's blocking
//! pool. Progress DTOs are projected from the stored snapshot and never expose
//! the resolved local source paths.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

use sqlx::SqlitePool;
use tokio::runtime::Handle;

use crate::{
    http::analysis::{
        AnalysisAction, AnalysisApi, AnalysisApiError, AnalysisApiFuture, AnalysisJobView,
    },
    jobs::analysis::{
        Coordinator, Error as CoordinatorError, ImageEngineAnalyzer, Job, JobKey, Scope,
        SqliteRepository, State, SystemClock, prepare_gallery_plan,
    },
};

type RuntimeCoordinator = Coordinator<SqliteRepository, ImageEngineAnalyzer, SystemClock>;

#[derive(Clone)]
pub struct SqliteAnalysisApi {
    reader: SqlitePool,
    roots: Arc<Vec<PathBuf>>,
    coordinator: Arc<Mutex<RuntimeCoordinator>>,
}

impl SqliteAnalysisApi {
    /// Install the durable job table and recover interrupted jobs as paused.
    pub async fn initialize(
        writer: SqlitePool,
        runtime: Handle,
        authorized_roots: Vec<PathBuf>,
        max_running: usize,
        max_queued: usize,
    ) -> Result<Self, String> {
        if authorized_roots.is_empty() {
            return Err("analysis requires at least one authorized root".into());
        }
        let repository = SqliteRepository::initialize(writer.clone(), runtime)
            .await
            .map_err(|error| format!("could not initialize analysis storage: {error}"))?;
        let coordinator = tokio::task::spawn_blocking(move || {
            Coordinator::new(
                repository,
                ImageEngineAnalyzer::default(),
                SystemClock,
                max_running,
                max_queued,
            )
        })
        .await
        .map_err(|error| format!("analysis coordinator recovery stopped: {error}"))?
        .map_err(|error| format!("analysis coordinator recovery failed: {error:?}"))?;
        Ok(Self {
            reader: writer,
            roots: Arc::new(authorized_roots),
            coordinator: Arc::new(Mutex::new(coordinator)),
        })
    }

    async fn view(&self, key: JobKey) -> Result<AnalysisJobView, AnalysisApiError> {
        let coordinator = self.coordinator.clone();
        tokio::task::spawn_blocking(move || {
            let coordinator = coordinator
                .lock()
                .map_err(|_| AnalysisApiError::Unavailable)?;
            let job = coordinator.get(&key).ok_or(AnalysisApiError::NotFound)?;
            project_job(job, coordinator.progress(&key).map_err(map_error)?)
        })
        .await
        .map_err(|_| AnalysisApiError::Unavailable)?
    }

    fn launch_worker(&self, key: JobKey) {
        let coordinator = self.coordinator.clone();
        tokio::spawn(async move {
            loop {
                let next = coordinator.clone();
                let key_for_step = key.clone();
                let result =
                    tokio::task::spawn_blocking(move || -> Result<bool, AnalysisApiError> {
                        let mut coordinator =
                            next.lock().map_err(|_| AnalysisApiError::Unavailable)?;
                        match coordinator.get(&key_for_step) {
                            Some(job) if job.state == State::Running => {
                                coordinator.step(&key_for_step).map_err(map_error)?;
                                Ok(true)
                            }
                            _ => Ok(false),
                        }
                    })
                    .await;
                match result {
                    Ok(Ok(true)) => tokio::task::yield_now().await,
                    _ => break,
                }
            }
        });
    }
}

impl AnalysisApi for SqliteAnalysisApi {
    fn submit<'a>(
        &'a self,
        gallery_id: &'a str,
        job_id: &'a str,
        profile_id: &'a str,
    ) -> AnalysisApiFuture<'a, Result<AnalysisJobView, AnalysisApiError>> {
        Box::pin(async move {
            let plan = prepare_gallery_plan(&self.reader, gallery_id, profile_id, &self.roots)
                .await
                .map_err(|message| {
                    if message.contains("not found") {
                        AnalysisApiError::NotFound
                    } else if message.contains("authorized roots")
                        || message.contains("no present gallery files")
                    {
                        AnalysisApiError::InvalidPlan
                    } else {
                        AnalysisApiError::Unavailable
                    }
                })?;
            let coordinator = self.coordinator.clone();
            let job_id = job_id.to_owned();
            let key = tokio::task::spawn_blocking(move || {
                let mut coordinator = coordinator
                    .lock()
                    .map_err(|_| AnalysisApiError::Unavailable)?;
                coordinator.submit(job_id, plan).map_err(map_error)
            })
            .await
            .map_err(|_| AnalysisApiError::Unavailable)??;
            self.view(key).await
        })
    }

    fn get<'a>(
        &'a self,
        catalog_id: &'a str,
        job_id: &'a str,
    ) -> AnalysisApiFuture<'a, Result<AnalysisJobView, AnalysisApiError>> {
        Box::pin(async move {
            self.view(JobKey {
                catalog_id: catalog_id.to_owned(),
                job_id: job_id.to_owned(),
            })
            .await
        })
    }

    fn active_for_gallery<'a>(
        &'a self,
        gallery_id: &'a str,
    ) -> AnalysisApiFuture<'a, Result<Option<AnalysisJobView>, AnalysisApiError>> {
        Box::pin(async move {
            let coordinator = self.coordinator.clone();
            let gallery_id = gallery_id.to_owned();
            tokio::task::spawn_blocking(move || {
                let coordinator = coordinator
                    .lock()
                    .map_err(|_| AnalysisApiError::Unavailable)?;
                let Some(job) = coordinator.active_gallery_job(&gallery_id) else {
                    return Ok(None);
                };
                let progress = coordinator.progress(&job.key).map_err(map_error)?;
                project_job(job, progress).map(Some)
            })
            .await
            .map_err(|_| AnalysisApiError::Unavailable)?
        })
    }

    fn control<'a>(
        &'a self,
        catalog_id: &'a str,
        job_id: &'a str,
        action: AnalysisAction,
    ) -> AnalysisApiFuture<'a, Result<AnalysisJobView, AnalysisApiError>> {
        Box::pin(async move {
            let key = JobKey {
                catalog_id: catalog_id.to_owned(),
                job_id: job_id.to_owned(),
            };
            let refreshed_plan = if matches!(action, AnalysisAction::Start | AnalysisAction::Resume)
            {
                let coordinator = self.coordinator.clone();
                let key_for_plan = key.clone();
                let stored_plan = tokio::task::spawn_blocking(move || {
                    let coordinator = coordinator
                        .lock()
                        .map_err(|_| AnalysisApiError::Unavailable)?;
                    coordinator
                        .get(&key_for_plan)
                        .map(|job| job.plan.clone())
                        .ok_or(AnalysisApiError::NotFound)
                })
                .await
                .map_err(|_| AnalysisApiError::Unavailable)??;
                let gallery_id = match &stored_plan.scope {
                    Scope::Gallery { gallery_id } => gallery_id,
                    Scope::Session { .. } => return Err(AnalysisApiError::InvalidPlan),
                };
                Some(
                    prepare_gallery_plan(
                        &self.reader,
                        gallery_id,
                        &stored_plan.profile_id,
                        &self.roots,
                    )
                    .await
                    .map_err(|message| {
                        if message.contains("not found") {
                            AnalysisApiError::NotFound
                        } else if message.contains("authorized roots")
                            || message.contains("no present gallery files")
                        {
                            AnalysisApiError::InvalidPlan
                        } else {
                            AnalysisApiError::Unavailable
                        }
                    })?,
                )
            } else {
                None
            };
            let coordinator = self.coordinator.clone();
            let key_for_action = key.clone();
            let should_launch = tokio::task::spawn_blocking(move || {
                let mut coordinator = coordinator
                    .lock()
                    .map_err(|_| AnalysisApiError::Unavailable)?;
                match action {
                    AnalysisAction::Start => {
                        coordinator
                            .refresh_plan(
                                &key_for_action,
                                refreshed_plan.ok_or(AnalysisApiError::InvalidPlan)?,
                            )
                            .map_err(map_error)?;
                        coordinator.start(&key_for_action).map_err(map_error)?
                    }
                    AnalysisAction::Pause => {
                        coordinator.pause(&key_for_action).map_err(map_error)?
                    }
                    AnalysisAction::Resume => {
                        coordinator
                            .refresh_plan(
                                &key_for_action,
                                refreshed_plan.ok_or(AnalysisApiError::InvalidPlan)?,
                            )
                            .map_err(map_error)?;
                        coordinator.resume(&key_for_action).map_err(map_error)?
                    }
                    AnalysisAction::Cancel => {
                        coordinator.cancel(&key_for_action).map_err(map_error)?
                    }
                }
                Ok(matches!(
                    action,
                    AnalysisAction::Start | AnalysisAction::Resume
                ))
            })
            .await
            .map_err(|_| AnalysisApiError::Unavailable)??;
            if should_launch {
                self.launch_worker(key.clone());
            }
            self.view(key).await
        })
    }
}

fn project_job(
    job: &Job,
    progress: crate::jobs::analysis::Progress,
) -> Result<AnalysisJobView, AnalysisApiError> {
    let gallery_id = match &job.plan.scope {
        Scope::Gallery { gallery_id } => gallery_id.clone(),
        Scope::Session { .. } => return Err(AnalysisApiError::InvalidPlan),
    };
    Ok(AnalysisJobView {
        contract_version: 1,
        catalog_id: job.key.catalog_id.clone(),
        job_id: job.key.job_id.clone(),
        gallery_id,
        profile_id: job.plan.profile_id.clone(),
        state: state_name(&job.state).to_owned(),
        completed: progress.completed,
        total: progress.total,
        failed: progress.failed,
        percent: progress.percent,
        created_at: job.created_at,
        updated_at: job.updated_at,
    })
}

fn state_name(state: &State) -> &'static str {
    match state {
        State::Queued => "queued",
        State::Running => "running",
        State::Paused => "paused",
        State::Succeeded => "succeeded",
        State::Failed { .. } => "failed",
        State::Cancelled => "cancelled",
    }
}

fn map_error(error: CoordinatorError) -> AnalysisApiError {
    match error {
        CoordinatorError::JobNotFound => AnalysisApiError::NotFound,
        CoordinatorError::QueueFull => AnalysisApiError::QueueFull,
        CoordinatorError::InvalidTransition => AnalysisApiError::InvalidTransition,
        CoordinatorError::InvalidPlan | CoordinatorError::DuplicatePhoto => {
            AnalysisApiError::InvalidPlan
        }
        CoordinatorError::Repository(_) => AnalysisApiError::Unavailable,
    }
}
