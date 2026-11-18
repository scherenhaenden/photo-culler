//! Transport-independent GET job snapshot adapter. The integrator must connect
//! this module to the shared API DTOs, queue, and `/api/v1` router.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobKind {
    Import,
    Analysis,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    Queued,
    Running,
    Paused { manual: bool, thermal: bool },
    Succeeded,
    Failed { code: String },
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Counters {
    pub completed: u64,
    pub total: Option<u64>,
    pub failed: u64,
}

/// One atomic, versioned view obtained from the queue, including terminal result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Snapshot {
    pub id: String,
    pub kind: JobKind,
    pub version: u64,
    pub phase: String,
    pub counters: Counters,
    pub state: State,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueueError {
    Unavailable,
}

pub trait SnapshotQueue {
    /// Must return an atomic snapshot and preserve monotonic versions per job.
    fn snapshot(&self, id: &str) -> Result<Option<Snapshot>, QueueError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorBody {
    pub code: &'static str,
    pub message: &'static str,
    pub request_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Response {
    Ok(Snapshot),
    Error { status: u16, body: ErrorBody },
}

/// GET by opaque job identifier. No transient response mutates queue state.
pub fn get<Q: SnapshotQueue>(queue: &Q, id: &str, request_id: &str) -> Response {
    match queue.snapshot(id) {
        Ok(Some(snapshot)) => Response::Ok(snapshot),
        Ok(None) => error(404, "job_not_found", "Job not found", request_id),
        Err(QueueError::Unavailable) => error(
            503,
            "queue_unavailable",
            "Job queue unavailable",
            request_id,
        ),
    }
}

fn error(status: u16, code: &'static str, message: &'static str, request_id: &str) -> Response {
    Response::Error {
        status,
        body: ErrorBody {
            code,
            message,
            request_id: request_id.into(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;

    #[derive(Default)]
    struct FakeQueue {
        jobs: RefCell<HashMap<String, Snapshot>>,
        disconnected: Cell<bool>,
    }
    impl SnapshotQueue for FakeQueue {
        fn snapshot(&self, id: &str) -> Result<Option<Snapshot>, QueueError> {
            if self.disconnected.get() {
                Err(QueueError::Unavailable)
            } else {
                Ok(self.jobs.borrow().get(id).cloned())
            }
        }
    }
    fn snapshot(
        id: &str,
        kind: JobKind,
        version: u64,
        phase: &str,
        completed: u64,
        state: State,
    ) -> Snapshot {
        Snapshot {
            id: id.into(),
            kind,
            version,
            phase: phase.into(),
            counters: Counters {
                completed,
                total: Some(10),
                failed: 0,
            },
            state,
        }
    }
    #[test]
    fn recovery_by_id_preserves_latest_phase_counters_and_terminal_state_for_both_kinds() {
        let queue = FakeQueue::default();
        for (id, kind) in [
            ("import-1", JobKind::Import),
            ("analysis-1", JobKind::Analysis),
        ] {
            queue.jobs.borrow_mut().insert(
                id.into(),
                snapshot(id, kind.clone(), 1, "queued", 0, State::Queued),
            );
            assert_eq!(
                get(&queue, id, "r1"),
                Response::Ok(snapshot(id, kind.clone(), 1, "queued", 0, State::Queued))
            );
            queue.jobs.borrow_mut().insert(
                id.into(),
                snapshot(id, kind.clone(), 2, "processing", 4, State::Running),
            );
            let saved_id = match get(&queue, id, "r2") {
                Response::Ok(s) => {
                    assert_eq!(s.counters.completed, 4);
                    s.id
                }
                _ => panic!(),
            };
            queue.disconnected.set(true);
            assert_eq!(
                get(&queue, &saved_id, "r3"),
                error(503, "queue_unavailable", "Job queue unavailable", "r3")
            );
            queue.jobs.borrow_mut().insert(
                id.into(),
                snapshot(id, kind.clone(), 3, "finished", 10, State::Succeeded),
            );
            queue.disconnected.set(false);
            let recovered = get(&queue, &saved_id, "r4");
            assert_eq!(
                recovered,
                Response::Ok(snapshot(id, kind, 3, "finished", 10, State::Succeeded))
            );
            assert_eq!(get(&queue, &saved_id, "r4"), recovered);
        }
    }
    #[test]
    fn missing_job_is_stable_and_distinct_from_queue_failure() {
        let queue = FakeQueue::default();
        let expected = error(404, "job_not_found", "Job not found", "req");
        assert_eq!(get(&queue, "missing", "req"), expected);
        assert_eq!(get(&queue, "missing", "req"), expected);
    }
}
