//! Native desktop server lifecycle and same-origin API transport boundary.
//!
//! The Slint crate, launcher and shared REST client are not present in the
//! sprint base. This module keeps their ownership contract testable without
//! importing unregistered workspace crates; see README.md for adapters.

use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApiConnection {
    /// Base URL, for example `http://127.0.0.1:43127`.
    pub base_url: String,
    /// Short-lived credential issued for this desktop session.
    pub credential: String,
    pub expires_at_unix_seconds: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectionKind {
    OwnedLocal,
    External,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Availability {
    Starting,
    Ready,
    Unavailable,
    Closed,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopError<E> {
    Launcher(E),
    InvalidLocalEndpoint,
    InvalidSession,
    NotConnected,
    SessionExpired,
    InvalidApiPath,
    Transport(String),
}

/// Process ownership is explicit. Only a process returned by `start_local`
/// is held and stopped by this runtime.
pub trait ManagedServerProcess {
    fn is_running(&mut self) -> bool;
    fn shutdown(&mut self);
}

pub struct RunningServer<P> {
    pub connection: ApiConnection,
    pub process: P,
}

/// Platform adapter: starts this window's loopback server or supplies a
/// connection to an explicitly selected external server.
pub trait ServerLauncher {
    type Error;
    type Process: ManagedServerProcess;

    fn start_local(&mut self) -> Result<RunningServer<Self::Process>, Self::Error>;
    fn connect_external(&mut self) -> Result<ApiConnection, Self::Error>;
}

pub struct DesktopRuntime<L: ServerLauncher> {
    launcher: L,
    connection: Option<ApiConnection>,
    process: Option<L::Process>,
    kind: Option<ConnectionKind>,
    availability: Availability,
}

impl<L: ServerLauncher> DesktopRuntime<L> {
    pub fn new(launcher: L) -> Self {
        Self {
            launcher,
            connection: None,
            process: None,
            kind: None,
            availability: Availability::Unavailable,
        }
    }

    pub fn availability(&self) -> Availability {
        self.availability
    }

    pub fn connection_kind(&self) -> Option<ConnectionKind> {
        self.kind
    }

    pub fn connection(&self) -> Option<&ApiConnection> {
        self.connection.as_ref()
    }

    /// Starts a server owned by this desktop window on an assigned loopback
    /// port. A failed launch leaves the runtime retryable.
    pub fn start_local(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<&ApiConnection, DesktopError<L::Error>> {
        self.availability = Availability::Starting;
        match self.launcher.start_local() {
            Ok(running) => {
                if !valid_loopback_endpoint(&running.connection.base_url) {
                    let mut running = running;
                    running.process.shutdown();
                    self.availability = Availability::Unavailable;
                    return Err(DesktopError::InvalidLocalEndpoint);
                }
                if !valid_session(&running.connection, now_unix_seconds) {
                    let mut running = running;
                    running.process.shutdown();
                    self.availability = Availability::Unavailable;
                    return Err(DesktopError::InvalidSession);
                }
                self.stop_owned_process();
                self.connection = Some(running.connection);
                self.process = Some(running.process);
                self.kind = Some(ConnectionKind::OwnedLocal);
                self.availability = Availability::Ready;
                Ok(self.connection.as_ref().expect("connection just assigned"))
            }
            Err(error) => {
                self.availability = Availability::Unavailable;
                Err(DesktopError::Launcher(error))
            }
        }
    }

    /// Attaches to an external instance. It never creates an owned process.
    pub fn connect_external(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<&ApiConnection, DesktopError<L::Error>> {
        self.availability = Availability::Starting;
        match self.launcher.connect_external() {
            Ok(connection) => {
                if !valid_session(&connection, now_unix_seconds) {
                    self.availability = Availability::Unavailable;
                    return Err(DesktopError::InvalidSession);
                }
                self.stop_owned_process();
                self.connection = Some(connection);
                self.kind = Some(ConnectionKind::External);
                self.availability = Availability::Ready;
                Ok(self.connection.as_ref().expect("connection just assigned"))
            }
            Err(error) => {
                self.availability = Availability::Unavailable;
                Err(DesktopError::Launcher(error))
            }
        }
    }

    /// Checks the owned process and restarts it when it has exited. External
    /// services are never relaunched by this method.
    pub fn recover_if_stopped(
        &mut self,
        now_unix_seconds: u64,
    ) -> Result<Option<&ApiConnection>, DesktopError<L::Error>> {
        if self.kind != Some(ConnectionKind::OwnedLocal) {
            return Ok(self.connection.as_ref());
        }
        let stopped = self.process.as_mut().is_some_and(|process| !process.is_running());
        if !stopped {
            return Ok(self.connection.as_ref());
        }
        self.stop_owned_process();
        self.connection = None;
        self.kind = None;
        self.availability = Availability::Unavailable;
        self.start_local(now_unix_seconds).map(Some)
    }

    /// Closes the desktop connection and terminates only its own server.
    pub fn close(&mut self) {
        self.stop_owned_process();
        self.connection = None;
        self.kind = None;
        self.availability = Availability::Closed;
    }

    fn stop_owned_process(&mut self) {
        if let Some(mut process) = self.process.take() {
            process.shutdown();
        }
    }
}

impl<L: ServerLauncher> Drop for DesktopRuntime<L> {
    fn drop(&mut self) {
        self.stop_owned_process();
    }
}

fn valid_loopback_endpoint(url: &str) -> bool {
    let authority = url.strip_prefix("http://").unwrap_or("");
    if authority.contains('/') || authority.contains('@') || authority.contains('?') {
        return false;
    }
    let port = if let Some(rest) = authority.strip_prefix("127.0.0.1:") {
        rest
    } else if let Some(rest) = authority.strip_prefix("[::1]:") {
        rest
    } else {
        return false;
    };
    port.parse::<u16>().is_ok_and(|port| port != 0)
}

fn valid_session(connection: &ApiConnection, now: u64) -> bool {
    !connection.credential.trim().is_empty() && connection.expires_at_unix_seconds > now
}

/// Minimal HTTP request contract shared by the desktop adapter and a mock.
/// The adapter must preserve the session credential and use the configured
/// server origin for every `/api/v1` request.
pub trait HttpTransport {
    type Error: fmt::Display;
    fn send(
        &mut self,
        url: &str,
        credential: &str,
    ) -> Result<Vec<u8>, Self::Error>;
}

pub struct DesktopApiClient<T> {
    transport: T,
}

impl<T: HttpTransport> DesktopApiClient<T> {
    pub fn new(transport: T) -> Self {
        Self { transport }
    }

    pub fn get<L: ServerLauncher>(
        &mut self,
        runtime: &DesktopRuntime<L>,
        path: &str,
        now_unix_seconds: u64,
    ) -> Result<Vec<u8>, DesktopError<L::Error>> {
        if !path.starts_with("/api/v1/") || path.contains("..") || path.contains('#') {
            return Err(DesktopError::InvalidApiPath);
        }
        let connection = runtime.connection().ok_or(DesktopError::NotConnected)?;
        if !valid_session(connection, now_unix_seconds) {
            return Err(DesktopError::SessionExpired);
        }
        let url = format!("{}{}", connection.base_url.trim_end_matches('/'), path);
        self.transport
            .send(&url, &connection.credential)
            .map_err(|error| DesktopError::Transport(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Clone, Default)]
    struct FakeProcess(Rc<RefCell<ProcessState>>);
    #[derive(Default)]
    struct ProcessState {
        running: bool,
        shutdowns: usize,
    }
    impl ManagedServerProcess for FakeProcess {
        fn is_running(&mut self) -> bool {
            self.0.borrow().running
        }
        fn shutdown(&mut self) {
            let mut state = self.0.borrow_mut();
            state.running = false;
            state.shutdowns += 1;
        }
    }

    #[derive(Default)]
    struct FakeLauncher {
        attempts: usize,
        fail_first: bool,
        local: Vec<FakeProcess>,
        external: Option<ApiConnection>,
        external_calls: usize,
    }
    impl ServerLauncher for FakeLauncher {
        type Error = &'static str;
        type Process = FakeProcess;
        fn start_local(&mut self) -> Result<RunningServer<Self::Process>, Self::Error> {
            self.attempts += 1;
            if self.fail_first && self.attempts == 1 {
                return Err("startup failed");
            }
            let process = FakeProcess::default();
            process.0.borrow_mut().running = true;
            self.local.push(process.clone());
            Ok(RunningServer {
                connection: ApiConnection {
                    base_url: format!("http://127.0.0.1:{}", 43000 + self.attempts),
                    credential: format!("local-session-{}", self.attempts),
                    expires_at_unix_seconds: 10_000,
                },
                process,
            })
        }
        fn connect_external(&mut self) -> Result<ApiConnection, Self::Error> {
            self.external_calls += 1;
            Ok(self.external.clone().unwrap_or(ApiConnection {
                base_url: "http://nas.example:43000".into(),
                credential: "external-session".into(),
                expires_at_unix_seconds: 10_000,
            }))
        }
    }

    #[derive(Default)]
    struct FakeTransport {
        requests: Vec<(String, String)>,
    }
    impl HttpTransport for FakeTransport {
        type Error = &'static str;
        fn send(&mut self, url: &str, credential: &str) -> Result<Vec<u8>, Self::Error> {
            self.requests.push((url.to_owned(), credential.to_owned()));
            Ok(b"ok".to_vec())
        }
    }

    #[test]
    fn starts_on_assigned_loopback_port_and_sends_same_origin_api_request() {
        let mut runtime = DesktopRuntime::new(FakeLauncher::default());
        let conn = runtime.start_local(5).unwrap().clone();
        assert_eq!(runtime.availability(), Availability::Ready);
        assert_eq!(runtime.connection_kind(), Some(ConnectionKind::OwnedLocal));
        assert!(valid_loopback_endpoint(&conn.base_url));
        assert_ne!(conn.base_url, "http://127.0.0.1:0");
        let mut client = DesktopApiClient::new(FakeTransport::default());
        assert_eq!(client.get(&runtime, "/api/v1/health", 5).unwrap(), b"ok");
        assert_eq!(client.transport.requests[0], ("http://127.0.0.1:43001/api/v1/health".into(), "local-session-1".into()));
    }

    #[test]
    fn startup_failure_is_recoverable_and_process_restart_is_controlled() {
        let mut runtime = DesktopRuntime::new(FakeLauncher { fail_first: true, ..Default::default() });
        assert_eq!(runtime.start_local(5), Err(DesktopError::Launcher("startup failed")));
        assert_eq!(runtime.availability(), Availability::Unavailable);
        runtime.start_local(5).unwrap();
        let old_process = runtime.process.as_ref().unwrap().clone();
        old_process.0.borrow_mut().running = false;
        let restarted = runtime.recover_if_stopped(5).unwrap().unwrap();
        assert_eq!(restarted.credential, "local-session-3");
        assert_eq!(runtime.launcher.attempts, 3);
        assert_eq!(runtime.availability(), Availability::Ready);
    }

    #[test]
    fn closing_external_connection_never_starts_or_stops_server_process() {
        let mut runtime = DesktopRuntime::new(FakeLauncher::default());
        runtime.connect_external(5).unwrap();
        assert_eq!(runtime.connection_kind(), Some(ConnectionKind::External));
        assert!(runtime.process.is_none());
        runtime.close();
        assert_eq!(runtime.availability(), Availability::Closed);
        assert_eq!(runtime.launcher.external_calls, 1);
        assert!(runtime.launcher.local.is_empty());
    }

    #[test]
    fn closing_owned_connection_stops_only_its_process() {
        let mut runtime = DesktopRuntime::new(FakeLauncher::default());
        runtime.start_local(5).unwrap();
        let process = runtime.launcher.local[0].clone();
        runtime.close();
        assert!(!process.0.borrow().running);
        assert_eq!(process.0.borrow().shutdowns, 1);
    }

    #[test]
    fn session_expiry_and_non_api_paths_are_rejected_before_transport() {
        let mut runtime = DesktopRuntime::new(FakeLauncher::default());
        runtime.start_local(5).unwrap();
        let mut client = DesktopApiClient::new(FakeTransport::default());
        assert_eq!(client.get(&runtime, "/api/v1/health", 10_000), Err(DesktopError::SessionExpired));
        assert_eq!(client.get(&runtime, "/other", 1), Err(DesktopError::InvalidApiPath));
        assert!(client.transport.requests.is_empty());
    }
}
