//! Provisional, transport-independent command adapter. The shared API and server
//! crate do not exist at the base commit; see README.md before integration.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Import { source_id: String },
    Rescan { source_id: String },
    Analyze { scope_id: String },
    Group { scope_id: String },
    Export { approved_plan_id: String },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action { Cancel, Pause, Resume }

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExecutorError { NotFound, InvalidTransition, IdempotencyConflict, Unavailable }

/// Implementations must atomically persist the key, full command and job ID.
/// Replaying the same key and command returns the same ID, even after restart;
/// reusing it for a different command returns IdempotencyConflict. The executor
/// owns transition validation and idempotency of control actions as well.
pub trait Executor {
    fn schedule(&mut self, key: &str, command: Command) -> Result<String, ExecutorError>;
    fn act(&mut self, job_id: &str, action: Action) -> Result<(), ExecutorError>;
}

#[derive(Debug, PartialEq, Eq)]
pub struct ErrorBody { pub code: &'static str, pub message: &'static str, pub request_id: String }
#[derive(Debug, PartialEq, Eq)]
pub enum Body { Job { job_id: String }, Empty, Error(ErrorBody) }
#[derive(Debug, PartialEq, Eq)]
pub struct Response { pub status: u16, pub body: Body }

fn error(status: u16, code: &'static str, message: &'static str, request_id: &str) -> Response {
    Response { status, body: Body::Error(ErrorBody { code, message, request_id: request_id.into() }) }
}
fn map_error(e: ExecutorError, request_id: &str) -> Response {
    match e {
        ExecutorError::NotFound => error(404, "job_not_found", "Job not found", request_id),
        ExecutorError::InvalidTransition => error(409, "invalid_transition", "Invalid job transition", request_id),
        ExecutorError::IdempotencyConflict => error(409, "idempotency_conflict", "Idempotency key already used", request_id),
        ExecutorError::Unavailable => error(503, "executor_unavailable", "Job executor unavailable", request_id),
    }
}

pub fn schedule(executor: &mut impl Executor, key: &str, command: Command, request_id: &str) -> Response {
    if key.trim().is_empty() { return error(400, "invalid_idempotency_key", "Idempotency key is required", request_id); }
    match executor.schedule(key, command) {
        Ok(job_id) => Response { status: 202, body: Body::Job { job_id } },
        Err(e) => map_error(e, request_id),
    }
}
pub fn control(executor: &mut impl Executor, job_id: &str, action: Action, request_id: &str) -> Response {
    match executor.act(job_id, action) {
        Ok(()) => Response { status: 202, body: Body::Empty },
        Err(e) => map_error(e, request_id),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    #[derive(Default)]
    struct Fake { keys: HashMap<String, (Command, String)>, actions: Vec<(String, Action)>, paused: bool, cancelled: bool }
    impl Executor for Fake {
        fn schedule(&mut self, key: &str, command: Command) -> Result<String, ExecutorError> {
            if let Some((prior, id)) = self.keys.get(key) {
                return if *prior == command { Ok(id.clone()) } else { Err(ExecutorError::IdempotencyConflict) };
            }
            let id = format!("job-{}", self.keys.len() + 1);
            self.keys.insert(key.into(), (command, id.clone()));
            Ok(id)
        }
        fn act(&mut self, id: &str, action: Action) -> Result<(), ExecutorError> {
            if !self.keys.values().any(|(_, job)| job == id) { return Err(ExecutorError::NotFound); }
            match action {
                Action::Pause if !self.cancelled => self.paused = true,
                Action::Resume if !self.cancelled && self.paused => self.paused = false,
                Action::Cancel => self.cancelled = true,
                _ => return Err(ExecutorError::InvalidTransition),
            }
            self.actions.push((id.into(), action)); Ok(())
        }
    }
    #[test]
    fn schedules_every_command_and_replays_without_duplicate() {
        let mut f = Fake::default();
        let commands = [Command::Import { source_id: "s".into() }, Command::Rescan { source_id: "s".into() }, Command::Analyze { scope_id: "g".into() }, Command::Group { scope_id: "g".into() }, Command::Export { approved_plan_id: "p".into() }];
        for (i, cmd) in commands.into_iter().enumerate() {
            let key = i.to_string(); let expected = Response { status: 202, body: Body::Job { job_id: format!("job-{}", i + 1) } };
            assert_eq!(schedule(&mut f, &key, cmd.clone(), "r"), expected);
            assert_eq!(schedule(&mut f, &key, cmd, "r"), expected);
        }
        assert_eq!(f.keys.len(), 5);
        let conflict = schedule(&mut f, "0", Command::Group { scope_id: "other".into() }, "r");
        assert_eq!(conflict, error(409, "idempotency_conflict", "Idempotency key already used", "r"));
        assert_eq!(schedule(&mut f, " ", Command::Import { source_id: "s".into() }, "r").status, 400);
    }
    #[test]
    fn controls_and_invalid_transitions_are_stable() {
        let mut f = Fake::default();
        schedule(&mut f, "k", Command::Import { source_id: "s".into() }, "r");
        for action in [Action::Pause, Action::Pause, Action::Resume, Action::Cancel, Action::Cancel] {
            assert_eq!(control(&mut f, "job-1", action, "r").status, 202);
        }
        assert_eq!(f.actions.len(), 5);
        for _ in 0..2 { assert_eq!(control(&mut f, "job-1", Action::Resume, "r"), error(409, "invalid_transition", "Invalid job transition", "r")); }
        assert_eq!(control(&mut f, "absent", Action::Pause, "r").status, 404);
    }
}
