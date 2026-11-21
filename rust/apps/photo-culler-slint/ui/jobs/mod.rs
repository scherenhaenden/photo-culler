//! Jobs-panel presentation state with an injected REST client.
//!
//! The Slint component is kept beside this module. The integration-owned UI
//! root binds it to `JobsPanel` and supplies the real REST transport.

use std::fmt;

pub const ACTIVE_POLL_INTERVAL_MS: u64 = 500;
pub const PAUSED_POLL_INTERVAL_MS: u64 = 1_500;
pub const IDLE_POLL_INTERVAL_MS: u64 = 5_000;
pub const MAX_OFFLINE_POLL_INTERVAL_MS: u64 = 15_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobKind {
    Import,
    Rescan,
    Analysis,
}

impl JobKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Import => "Importación",
            Self::Rescan => "Reescaneo",
            Self::Analysis => "Análisis",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobState {
    Queued,
    Running,
    Paused,
    Cancelling,
    Completed,
    Failed,
    Cancelled,
}

impl JobState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Queued => "En cola",
            Self::Running => "En curso",
            Self::Paused => "Pausado",
            Self::Cancelling => "Cancelando",
            Self::Completed => "Completado",
            Self::Failed => "Fallido",
            Self::Cancelled => "Cancelado",
        }
    }

    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JobSnapshot {
    pub id: String,
    pub kind: JobKind,
    pub state: JobState,
    pub phase: String,
    pub completed: u64,
    pub total: Option<u64>,
    /// Monotonic server revision used to acknowledge actions and reject stale UI state.
    pub version: u64,
}

impl JobSnapshot {
    /// Normalized progress for a Slint progress bar. Unknown or zero totals
    /// remain indeterminate and report 0 rather than NaN.
    pub fn progress(&self) -> f32 {
        match self.total {
            Some(total) if total > 0 => {
                (self.completed as f64 / total as f64).clamp(0.0, 1.0) as f32
            }
            _ => 0.0,
        }
    }

    pub fn progress_label(&self) -> String {
        match self.total {
            Some(total) => format!("{} / {}", self.completed.min(total), total),
            None => format!("{} elementos", self.completed),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum JobAction {
    Pause,
    Resume,
    Cancel,
}

impl JobAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pausar",
            Self::Resume => "Reanudar",
            Self::Cancel => "Cancelar",
        }
    }

    const fn acknowledged_by(self, state: JobState) -> bool {
        match self {
            Self::Pause => matches!(state, JobState::Paused) || state.is_terminal(),
            Self::Resume => matches!(state, JobState::Running) || state.is_terminal(),
            Self::Cancel => matches!(state, JobState::Cancelling) || state.is_terminal(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum JobsApiError {
    Disconnected(String),
    Rejected(String),
    UnexpectedJob { expected: String, actual: String },
}

impl fmt::Display for JobsApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Disconnected(message) => write!(f, "REST no disponible: {message}"),
            Self::Rejected(message) => write!(f, "acción rechazada: {message}"),
            Self::UnexpectedJob { expected, actual } => {
                write!(f, "se esperaba el trabajo {expected}, se recibió {actual}")
            }
        }
    }
}

pub trait JobsRest {
    fn snapshot(&mut self, job_id: &str) -> Result<JobSnapshot, JobsApiError>;
    fn action(&mut self, job_id: &str, action: JobAction) -> Result<JobSnapshot, JobsApiError>;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Connectivity {
    Connected,
    Disconnected,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActionOutcome {
    Sent,
    DuplicateSuppressed,
    NotAllowed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PendingAction {
    action: JobAction,
}

/// Controller state for a single tracked job. The server remains authoritative:
/// actions update from their REST response, and reconnect always reloads by the
/// retained job ID rather than starting a second job.
pub struct JobsPanel<R> {
    rest: R,
    snapshot: Option<JobSnapshot>,
    connectivity: Connectivity,
    pending_action: Option<PendingAction>,
    last_error: Option<String>,
    next_poll_at_ms: u64,
    last_poll_interval_ms: u64,
    offline_attempts: u8,
}

impl<R: JobsRest> JobsPanel<R> {
    pub fn new(rest: R) -> Self {
        Self {
            rest,
            snapshot: None,
            connectivity: Connectivity::Connected,
            pending_action: None,
            last_error: None,
            next_poll_at_ms: 0,
            last_poll_interval_ms: ACTIVE_POLL_INTERVAL_MS,
            offline_attempts: 0,
        }
    }

    pub fn track_job(&mut self, snapshot: JobSnapshot, now_ms: u64) {
        self.snapshot = Some(snapshot);
        self.connectivity = Connectivity::Connected;
        self.pending_action = None;
        self.last_error = None;
        self.offline_attempts = 0;
        self.last_poll_interval_ms = self.interval_for_current_state();
        self.next_poll_at_ms = now_ms.saturating_add(self.last_poll_interval_ms);
    }

    pub fn snapshot(&self) -> Option<&JobSnapshot> {
        self.snapshot.as_ref()
    }

    pub fn job_id(&self) -> Option<&str> {
        self.snapshot.as_ref().map(|job| job.id.as_str())
    }

    pub const fn connectivity(&self) -> Connectivity {
        self.connectivity
    }

    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }

    pub const fn pending_action(&self) -> Option<JobAction> {
        match self.pending_action {
            Some(pending) => Some(pending.action),
            None => None,
        }
    }

    pub const fn next_poll_at_ms(&self) -> u64 {
        self.next_poll_at_ms
    }

    pub const fn last_poll_interval_ms(&self) -> u64 {
        self.last_poll_interval_ms
    }

    pub fn can_pause(&self) -> bool {
        self.pending_action.is_none()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|job| matches!(job.state, JobState::Queued | JobState::Running))
    }

    pub fn can_resume(&self) -> bool {
        self.pending_action.is_none()
            && self
                .snapshot
                .as_ref()
                .is_some_and(|job| job.state == JobState::Paused)
    }

    pub fn can_cancel(&self) -> bool {
        self.pending_action.is_none()
            && self.snapshot.as_ref().is_some_and(|job| {
                matches!(
                    job.state,
                    JobState::Queued | JobState::Running | JobState::Paused
                )
            })
    }

    /// Controlled polling: active jobs are queried quickly, paused jobs more
    /// slowly, and terminal jobs at an idle cadence. A REST failure preserves
    /// the last snapshot and backs off; callers may make reconnect immediate.
    pub fn poll(&mut self, now_ms: u64) -> bool {
        if self.snapshot.is_none() || now_ms < self.next_poll_at_ms {
            return false;
        }
        let job_id = self.snapshot.as_ref().expect("snapshot checked").id.clone();
        match self.rest.snapshot(&job_id) {
            Ok(snapshot) => {
                if let Err(error) = self.accept_snapshot(&job_id, snapshot) {
                    self.last_error = Some(error.to_string());
                    self.schedule_offline_retry(now_ms);
                    return false;
                }
                self.connectivity = Connectivity::Connected;
                self.offline_attempts = 0;
                self.last_error = None;
                self.last_poll_interval_ms = self.interval_for_current_state();
                self.next_poll_at_ms = now_ms.saturating_add(self.last_poll_interval_ms);
                true
            }
            Err(error) => {
                self.last_error = Some(error.to_string());
                self.schedule_offline_retry(now_ms);
                false
            }
        }
    }

    /// Make the next snapshot request immediate after the transport signals
    /// that connectivity has returned. The tracked job ID is retained.
    pub fn connection_restored(&mut self, now_ms: u64) {
        self.connectivity = Connectivity::Connected;
        self.next_poll_at_ms = now_ms;
    }

    pub fn send_action(
        &mut self,
        action: JobAction,
        now_ms: u64,
    ) -> Result<ActionOutcome, JobsApiError> {
        if self.pending_action.is_some() {
            return Ok(ActionOutcome::DuplicateSuppressed);
        }
        let Some(job) = self.snapshot.as_ref() else {
            return Ok(ActionOutcome::NotAllowed);
        };
        let allowed = match action {
            JobAction::Pause => matches!(job.state, JobState::Queued | JobState::Running),
            JobAction::Resume => job.state == JobState::Paused,
            JobAction::Cancel => {
                matches!(
                    job.state,
                    JobState::Queued | JobState::Running | JobState::Paused
                )
            }
        };
        if !allowed {
            return Ok(ActionOutcome::NotAllowed);
        }

        let job_id = job.id.clone();
        self.pending_action = Some(PendingAction { action });
        match self.rest.action(&job_id, action) {
            Ok(snapshot) => {
                if let Err(error) = self.accept_snapshot(&job_id, snapshot) {
                    self.pending_action = None;
                    self.last_error = Some(error.to_string());
                    self.next_poll_at_ms = now_ms;
                    return Err(error);
                }
                self.connectivity = Connectivity::Connected;
                self.offline_attempts = 0;
                self.last_error = None;
                self.clear_acknowledged_action();
                self.last_poll_interval_ms = self.interval_for_current_state();
                self.next_poll_at_ms = now_ms.saturating_add(self.last_poll_interval_ms);
                Ok(ActionOutcome::Sent)
            }
            Err(error) => {
                self.pending_action = None;
                self.last_error = Some(error.to_string());
                if matches!(error, JobsApiError::Disconnected(_)) {
                    self.connectivity = Connectivity::Disconnected;
                    self.schedule_offline_retry(now_ms);
                } else {
                    // A conflict response is followed by a snapshot refresh.
                    self.next_poll_at_ms = now_ms;
                }
                Err(error)
            }
        }
    }

    fn accept_snapshot(
        &mut self,
        expected_id: &str,
        snapshot: JobSnapshot,
    ) -> Result<(), JobsApiError> {
        if snapshot.id != expected_id {
            return Err(JobsApiError::UnexpectedJob {
                expected: expected_id.to_owned(),
                actual: snapshot.id,
            });
        }
        if self
            .snapshot
            .as_ref()
            .is_some_and(|current| snapshot.version < current.version)
        {
            return Ok(());
        }
        self.snapshot = Some(snapshot);
        self.clear_acknowledged_action();
        Ok(())
    }

    fn clear_acknowledged_action(&mut self) {
        let acknowledged = match (&self.pending_action, &self.snapshot) {
            (Some(pending), Some(snapshot)) => pending.action.acknowledged_by(snapshot.state),
            _ => false,
        };
        if acknowledged {
            self.pending_action = None;
        }
    }

    fn interval_for_current_state(&self) -> u64 {
        match self.snapshot.as_ref().map(|job| job.state) {
            Some(JobState::Queued | JobState::Running | JobState::Cancelling) => {
                ACTIVE_POLL_INTERVAL_MS
            }
            Some(JobState::Paused) => PAUSED_POLL_INTERVAL_MS,
            Some(JobState::Completed | JobState::Failed | JobState::Cancelled) | None => {
                IDLE_POLL_INTERVAL_MS
            }
        }
    }

    fn schedule_offline_retry(&mut self, now_ms: u64) {
        self.connectivity = Connectivity::Disconnected;
        self.offline_attempts = self.offline_attempts.saturating_add(1).min(5);
        self.last_poll_interval_ms =
            (1_000_u64 << (self.offline_attempts - 1)).min(MAX_OFFLINE_POLL_INTERVAL_MS);
        self.next_poll_at_ms = now_ms.saturating_add(self.last_poll_interval_ms);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct FakeRest {
        jobs: BTreeMap<String, JobSnapshot>,
        actions: Vec<(String, JobAction)>,
        get_calls: usize,
        fail_get: bool,
        fail_actions: bool,
        acknowledge_actions: bool,
    }

    impl JobsRest for FakeRest {
        fn snapshot(&mut self, job_id: &str) -> Result<JobSnapshot, JobsApiError> {
            self.get_calls += 1;
            if self.fail_get {
                return Err(JobsApiError::Disconnected("simulated link loss".into()));
            }
            self.jobs
                .get(job_id)
                .cloned()
                .ok_or_else(|| JobsApiError::Rejected("job not found".into()))
        }

        fn action(&mut self, job_id: &str, action: JobAction) -> Result<JobSnapshot, JobsApiError> {
            self.actions.push((job_id.into(), action));
            if self.fail_actions {
                return Err(JobsApiError::Rejected("simulated stale action".into()));
            }
            let job = self
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| JobsApiError::Rejected("job not found".into()))?;
            if self.acknowledge_actions {
                job.version += 1;
                job.state = match action {
                    JobAction::Pause => JobState::Paused,
                    JobAction::Resume => JobState::Running,
                    JobAction::Cancel => JobState::Cancelled,
                };
            }
            Ok(job.clone())
        }
    }

    fn snapshot(state: JobState, version: u64) -> JobSnapshot {
        JobSnapshot {
            id: "job-49".into(),
            kind: JobKind::Import,
            state,
            phase: "descubriendo".into(),
            completed: 4,
            total: Some(10),
            version,
        }
    }

    fn panel(api: FakeRest, state: JobState) -> JobsPanel<FakeRest> {
        let initial = snapshot(state, 1);
        let mut api = api;
        api.jobs.insert(initial.id.clone(), initial.clone());
        let mut panel = JobsPanel::new(api);
        panel.track_job(initial, 0);
        panel
    }

    #[test]
    fn queued_running_and_terminal_snapshots_drive_progress_and_controlled_polling() {
        let mut panel = panel(FakeRest::default(), JobState::Queued);
        assert_eq!(panel.snapshot().unwrap().progress(), 0.4);
        assert_eq!(panel.last_poll_interval_ms(), ACTIVE_POLL_INTERVAL_MS);
        assert!(!panel.poll(ACTIVE_POLL_INTERVAL_MS - 1));
        assert!(panel.poll(ACTIVE_POLL_INTERVAL_MS));

        panel.rest.jobs.get_mut("job-49").unwrap().state = JobState::Completed;
        panel.rest.jobs.get_mut("job-49").unwrap().version = 2;
        assert!(panel.poll(panel.next_poll_at_ms()));
        assert_eq!(panel.snapshot().unwrap().state, JobState::Completed);
        assert_eq!(panel.last_poll_interval_ms(), IDLE_POLL_INTERVAL_MS);
    }

    #[test]
    fn disconnect_preserves_job_and_reconnect_resyncs_server_snapshot() {
        let mut api = FakeRest::default();
        api.fail_get = true;
        let mut panel = panel(api, JobState::Running);
        let last = panel.snapshot().unwrap().clone();

        assert!(!panel.poll(ACTIVE_POLL_INTERVAL_MS));
        assert_eq!(panel.connectivity(), Connectivity::Disconnected);
        assert_eq!(panel.job_id(), Some("job-49"));
        assert_eq!(panel.snapshot(), Some(&last));
        assert_eq!(panel.last_poll_interval_ms(), 1_000);

        panel.rest.fail_get = false;
        panel.rest.jobs.get_mut("job-49").unwrap().completed = 8;
        panel.rest.jobs.get_mut("job-49").unwrap().version = 2;
        panel.connection_restored(200);
        assert!(panel.poll(200));
        assert_eq!(panel.connectivity(), Connectivity::Connected);
        assert_eq!(panel.job_id(), Some("job-49"));
        assert_eq!(panel.snapshot().unwrap().completed, 8);
    }

    #[test]
    fn repeated_disconnects_use_bounded_poll_backoff() {
        let mut api = FakeRest {
            fail_get: true,
            ..FakeRest::default()
        };
        let initial = snapshot(JobState::Running, 1);
        api.jobs.insert(initial.id.clone(), initial.clone());
        let mut panel = JobsPanel::new(api);
        panel.track_job(initial, 0);

        for expected_interval in [1_000, 2_000, 4_000, 8_000, 15_000, 15_000] {
            let now = panel.next_poll_at_ms();
            assert!(!panel.poll(now));
            assert_eq!(panel.last_poll_interval_ms(), expected_interval);
        }
    }

    #[test]
    fn pause_resume_and_cancel_actions_are_sent_once_and_reflect_server_state() {
        let mut api = FakeRest {
            acknowledge_actions: true,
            ..FakeRest::default()
        };
        let initial = snapshot(JobState::Running, 1);
        api.jobs.insert(initial.id.clone(), initial.clone());
        let mut panel = JobsPanel::new(api);
        panel.track_job(initial, 0);

        assert_eq!(
            panel.send_action(JobAction::Pause, 10),
            Ok(ActionOutcome::Sent)
        );
        assert_eq!(panel.snapshot().unwrap().state, JobState::Paused);
        assert!(!panel.can_pause());
        assert_eq!(
            panel.send_action(JobAction::Pause, 11),
            Ok(ActionOutcome::NotAllowed)
        );
        assert_eq!(panel.rest.actions.len(), 1);

        assert_eq!(
            panel.send_action(JobAction::Resume, 20),
            Ok(ActionOutcome::Sent)
        );
        assert_eq!(panel.snapshot().unwrap().state, JobState::Running);
        assert_eq!(
            panel.send_action(JobAction::Cancel, 30),
            Ok(ActionOutcome::Sent)
        );
        assert_eq!(panel.snapshot().unwrap().state, JobState::Cancelled);
        assert_eq!(panel.rest.actions.len(), 3);
    }

    #[test]
    fn pending_action_suppresses_double_send_until_snapshot_acknowledges_it() {
        let mut api = FakeRest {
            acknowledge_actions: false,
            ..FakeRest::default()
        };
        let initial = snapshot(JobState::Running, 1);
        api.jobs.insert(initial.id.clone(), initial.clone());
        let mut panel = JobsPanel::new(api);
        panel.track_job(initial, 0);

        assert_eq!(
            panel.send_action(JobAction::Pause, 10),
            Ok(ActionOutcome::Sent)
        );
        assert_eq!(panel.pending_action(), Some(JobAction::Pause));
        assert_eq!(
            panel.send_action(JobAction::Pause, 11),
            Ok(ActionOutcome::DuplicateSuppressed)
        );
        assert_eq!(panel.rest.actions.len(), 1);

        let server = panel.rest.jobs.get_mut("job-49").unwrap();
        server.state = JobState::Paused;
        server.version = 2;
        assert!(panel.poll(panel.next_poll_at_ms()));
        assert_eq!(panel.pending_action(), None);
        assert_eq!(panel.snapshot().unwrap().state, JobState::Paused);
    }

    #[test]
    fn action_error_keeps_last_state_and_schedules_authoritative_resync() {
        let mut api = FakeRest {
            fail_actions: true,
            ..FakeRest::default()
        };
        let initial = snapshot(JobState::Running, 1);
        api.jobs.insert(initial.id.clone(), initial.clone());
        let mut panel = JobsPanel::new(api);
        panel.track_job(initial.clone(), 0);

        assert!(panel.send_action(JobAction::Pause, 50).is_err());
        assert_eq!(panel.snapshot(), Some(&initial));
        assert!(panel.last_error().is_some());
        assert_eq!(panel.next_poll_at_ms(), 50);
        assert_eq!(panel.pending_action(), None);
    }
}
