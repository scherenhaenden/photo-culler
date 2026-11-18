//! Orderly shutdown for the desktop-owned demo server.
//!
//! Integration: the concrete native child handle from D16 implements
//! `ChildProcess`; the base branch has no Slint package/module tree to wire it
//! into yet.

use std::time::Duration;

pub const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceKind {
    Owned,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WaitResult {
    Exited,
    StillRunning,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShutdownReport {
    pub stdin_closed: bool,
    pub graceful_exit: bool,
    pub force_terminated: bool,
    pub reaped: bool,
}

pub trait ChildProcess {
    fn close_stdin(&mut self) -> Result<(), String>;
    fn wait_timeout(&mut self, timeout: Duration) -> Result<WaitResult, String>;
    /// Must terminate and collect/wait for this child before returning.
    fn terminate_and_reap(&mut self) -> Result<(), String>;
}

enum Ownership<P> {
    Owned(P),
    External,
    Closed,
}

pub struct ShutdownGuard<P: ChildProcess> {
    ownership: Ownership<P>,
}

impl<P: ChildProcess> ShutdownGuard<P> {
    pub fn owned(child: P) -> Self {
        Self {
            ownership: Ownership::Owned(child),
        }
    }
    pub fn external() -> Self {
        Self {
            ownership: Ownership::External,
        }
    }
    pub fn kind(&self) -> InstanceKind {
        match &self.ownership {
            Ownership::Owned(_) => InstanceKind::Owned,
            Ownership::External | Ownership::Closed => InstanceKind::External,
        }
    }

    /// Closing is idempotent. It signals EOF on stdin, allows three seconds for
    /// the server to stop, then force-terminates and reaps only our own child.
    pub fn close(&mut self) -> ShutdownReport {
        let ownership = std::mem::replace(&mut self.ownership, Ownership::Closed);
        match ownership {
            Ownership::Owned(mut child) => {
                let stdin_closed = child.close_stdin().is_ok();
                match child.wait_timeout(GRACEFUL_SHUTDOWN_TIMEOUT) {
                    Ok(WaitResult::Exited) => ShutdownReport {
                        stdin_closed,
                        graceful_exit: true,
                        force_terminated: false,
                        reaped: true,
                    },
                    Ok(WaitResult::StillRunning) | Err(_) => {
                        let reaped = child.terminate_and_reap().is_ok();
                        ShutdownReport {
                            stdin_closed,
                            graceful_exit: false,
                            force_terminated: true,
                            reaped,
                        }
                    }
                }
            }
            Ownership::External | Ownership::Closed => ShutdownReport::default(),
        }
    }
}

impl<P: ChildProcess> Drop for ShutdownGuard<P> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct State {
        closes: usize,
        waits: Vec<Duration>,
        terminate_reap: usize,
        exits: bool,
    }
    #[derive(Clone, Default)]
    struct FakeChild(Rc<RefCell<State>>);
    impl ChildProcess for FakeChild {
        fn close_stdin(&mut self) -> Result<(), String> {
            self.0.borrow_mut().closes += 1;
            Ok(())
        }
        fn wait_timeout(&mut self, timeout: Duration) -> Result<WaitResult, String> {
            let mut state = self.0.borrow_mut();
            state.waits.push(timeout);
            Ok(if state.exits {
                WaitResult::Exited
            } else {
                WaitResult::StillRunning
            })
        }
        fn terminate_and_reap(&mut self) -> Result<(), String> {
            self.0.borrow_mut().terminate_reap += 1;
            Ok(())
        }
    }

    #[test]
    fn closes_stdin_and_exits_gracefully_within_three_seconds() {
        let child = FakeChild::default();
        child.0.borrow_mut().exits = true;
        let state = child.0.clone();
        let mut guard = ShutdownGuard::owned(child);
        assert_eq!(
            guard.close(),
            ShutdownReport {
                stdin_closed: true,
                graceful_exit: true,
                force_terminated: false,
                reaped: true
            }
        );
        assert_eq!(state.borrow().closes, 1);
        assert_eq!(state.borrow().waits, vec![Duration::from_secs(3)]);
        assert_eq!(state.borrow().terminate_reap, 0);
    }

    #[test]
    fn stuck_child_is_terminated_and_reaped_then_repeated_close_is_safe() {
        let child = FakeChild::default();
        let state = child.0.clone();
        let mut guard = ShutdownGuard::owned(child);
        assert_eq!(
            guard.close(),
            ShutdownReport {
                stdin_closed: true,
                graceful_exit: false,
                force_terminated: true,
                reaped: true
            }
        );
        assert_eq!(state.borrow().terminate_reap, 1);
        assert_eq!(guard.close(), ShutdownReport::default());
        assert_eq!(state.borrow().terminate_reap, 1);
    }

    #[test]
    fn external_service_is_never_signaled_or_terminated() {
        let mut guard: ShutdownGuard<FakeChild> = ShutdownGuard::external();
        assert_eq!(guard.kind(), InstanceKind::External);
        assert_eq!(guard.close(), ShutdownReport::default());
        assert_eq!(guard.close(), ShutdownReport::default());
    }
}
