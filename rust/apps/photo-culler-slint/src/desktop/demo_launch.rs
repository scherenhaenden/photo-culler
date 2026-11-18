//! Native demo launcher: consume the child `ready` line and verify HTTP health
//! before exposing a connection to the window.
//!
//! Integration: adapt `ProcessFactory`, `HealthProbe`, and `DemoWindow` to the
//! concrete D28/D15/D13 types when D33 registers the Slint crate. The base
//! branch has no package manifest or root module tree for this app.

use std::time::Duration;

pub const READY_TIMEOUT: Duration = Duration::from_secs(10);
pub const READY_PROTOCOL: u64 = 1;
pub const HEALTH_PATH: &str = "/api/v1/health";
const MAX_READY_LINE: usize = 4096;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReadyMessage {
    pub origin: String,
    pub port: u16,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HealthResponse {
    pub status_code: u16,
    pub status: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InstanceKind {
    Owned,
    External,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WindowConnection {
    pub origin: String,
    pub kind: InstanceKind,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchError {
    Spawn(String),
    ChildExited(String),
    ReadyTimeout,
    ReadyProtocol(String),
    HealthUnavailable(String),
    HealthNotReady,
    Window(String),
}

pub trait ChildProcess {
    fn read_stdout_line(
        &mut self,
        timeout: Duration,
        max_bytes: usize,
    ) -> Result<Option<String>, String>;
    fn stop_after_startup_failure(&mut self);
}

pub trait ProcessFactory {
    type Child: ChildProcess;
    fn spawn(&mut self) -> Result<Self::Child, String>;
}

pub trait HealthProbe {
    fn get(
        &mut self,
        origin: &str,
        path: &str,
        timeout: Duration,
    ) -> Result<HealthResponse, String>;
}

pub trait DemoWindow {
    fn open(&mut self, connection: WindowConnection) -> Result<(), String>;
    fn show_startup_failure(&mut self, message: &str);
}

pub struct DemoLauncher<F, H, W> {
    process_factory: F,
    health: H,
    window: W,
    startup_timeout: Duration,
    request_timeout: Duration,
}

impl<F: ProcessFactory, H: HealthProbe, W: DemoWindow> DemoLauncher<F, H, W> {
    pub fn new(process_factory: F, health: H, window: W) -> Self {
        Self {
            process_factory,
            health,
            window,
            startup_timeout: READY_TIMEOUT,
            request_timeout: Duration::from_secs(2),
        }
    }

    pub fn start_owned(&mut self) -> Result<WindowConnection, LaunchError> {
        let mut child = match self.process_factory.spawn() {
            Ok(child) => child,
            Err(error) => return self.fail(LaunchError::Spawn(error)),
        };
        let result: Result<WindowConnection, LaunchError> = (|| {
            let line = child
                .read_stdout_line(self.startup_timeout, MAX_READY_LINE)
                .map_err(LaunchError::ChildExited)?
                .ok_or(LaunchError::ReadyTimeout)?;
            let ready = parse_ready_line(&line).map_err(LaunchError::ReadyProtocol)?;
            self.verify_health(&ready.origin)?;
            let connection = WindowConnection {
                origin: ready.origin,
                kind: InstanceKind::Owned,
            };
            self.window
                .open(connection.clone())
                .map_err(LaunchError::Window)?;
            Ok(connection)
        })();
        if result.is_err() {
            child.stop_after_startup_failure();
        }
        result.map_err(|error| {
            self.window.show_startup_failure(&error.to_string());
            error
        })
    }

    /// Explicit external connections are health-checked, but never spawned or
    /// treated as owned children by this launcher.
    pub fn connect_external(&mut self, origin: &str) -> Result<WindowConnection, LaunchError> {
        let ready = validate_origin(origin).map_err(LaunchError::ReadyProtocol)?;
        self.verify_health(&ready.origin).map_err(|error| {
            self.window.show_startup_failure(&error.to_string());
            error
        })?;
        let connection = WindowConnection {
            origin: ready.origin,
            kind: InstanceKind::External,
        };
        self.window.open(connection.clone()).map_err(|message| {
            let error = LaunchError::Window(message);
            self.window.show_startup_failure(&error.to_string());
            error
        })?;
        Ok(connection)
    }

    pub fn retry(&mut self) -> Result<WindowConnection, LaunchError> {
        self.start_owned()
    }

    pub fn parts(&self) -> (&F, &H, &W) {
        (&self.process_factory, &self.health, &self.window)
    }

    fn verify_health(&mut self, origin: &str) -> Result<(), LaunchError> {
        let response = self
            .health
            .get(origin, HEALTH_PATH, self.request_timeout)
            .map_err(LaunchError::HealthUnavailable)?;
        if response.status_code != 200 || response.status.as_deref() != Some("ready") {
            return Err(LaunchError::HealthNotReady);
        }
        Ok(())
    }

    fn fail<T>(&mut self, error: LaunchError) -> Result<T, LaunchError> {
        self.window.show_startup_failure(&error.to_string());
        Err(error)
    }
}

impl std::fmt::Display for LaunchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Parse exactly one protocol-v1 stdout line. The protocol deliberately uses
/// plain ASCII JSON fields so no shell parsing or arbitrary host is involved.
pub fn parse_ready_line(line: &str) -> Result<ReadyMessage, String> {
    if line.len() > MAX_READY_LINE {
        return Err("ready line exceeds 4096 bytes".into());
    }
    let json = line
        .strip_suffix('\n')
        .ok_or("ready message must end in newline")?;
    if json.contains(['\n', '\r']) || !json.starts_with('{') || !json.ends_with('}') {
        return Err("expected one JSON object on one line".into());
    }
    let mut event = None;
    let mut protocol = None;
    let mut origin = None;
    for field in json[1..json.len() - 1].split(',') {
        let (key, value) = field.split_once(':').ok_or("malformed ready field")?;
        let key = parse_json_string(key.trim()).ok_or("invalid ready field name")?;
        match key.as_str() {
            "event" => {
                if event
                    .replace(parse_json_string(value.trim()).ok_or("invalid event")?)
                    .is_some()
                {
                    return Err("duplicate event".into());
                }
            }
            "protocol" => {
                if protocol
                    .replace(
                        value
                            .trim()
                            .parse::<u64>()
                            .map_err(|_| "invalid protocol version")?,
                    )
                    .is_some()
                {
                    return Err("duplicate protocol".into());
                }
            }
            "origin" => {
                if origin
                    .replace(parse_json_string(value.trim()).ok_or("invalid origin")?)
                    .is_some()
                {
                    return Err("duplicate origin".into());
                }
            }
            _ => return Err("unexpected ready field".into()),
        }
    }
    if event.as_deref() != Some("ready") {
        return Err("event must be ready".into());
    }
    if protocol != Some(READY_PROTOCOL) {
        return Err("unsupported ready protocol".into());
    }
    validate_origin(origin.as_deref().ok_or("missing origin")?)
}

fn parse_json_string(value: &str) -> Option<String> {
    let inner = value.strip_prefix('"')?.strip_suffix('"')?;
    if inner.bytes().any(|b| b < 0x20 || b == b'"' || b == b'\\') {
        return None;
    }
    Some(inner.to_owned())
}

fn validate_origin(origin: &str) -> Result<ReadyMessage, String> {
    let authority = origin
        .strip_prefix("http://")
        .ok_or("origin must use http")?;
    if authority.contains(['/', '?', '#', '@', '\\']) {
        return Err("origin must be an HTTP authority without path".into());
    }
    let (host, port) = authority
        .rsplit_once(':')
        .ok_or("origin must include assigned port")?;
    if host != "127.0.0.1" && host != "localhost" && host != "[::1]" {
        return Err("origin must be loopback".into());
    }
    let port = port.parse::<u16>().map_err(|_| "invalid loopback port")?;
    if port == 0 {
        return Err("port zero is not an assigned listener".into());
    }
    Ok(ReadyMessage {
        origin: origin.to_owned(),
        port,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeChild {
        line: Option<Result<Option<String>, String>>,
        stopped: bool,
    }
    impl ChildProcess for FakeChild {
        fn read_stdout_line(
            &mut self,
            timeout: Duration,
            max: usize,
        ) -> Result<Option<String>, String> {
            assert_eq!(timeout, READY_TIMEOUT);
            assert_eq!(max, MAX_READY_LINE);
            self.line.take().unwrap_or(Ok(None))
        }
        fn stop_after_startup_failure(&mut self) {
            self.stopped = true;
        }
    }
    #[derive(Default)]
    struct FakeFactory {
        children: VecDeque<FakeChild>,
        spawns: usize,
    }
    impl ProcessFactory for FakeFactory {
        type Child = FakeChild;
        fn spawn(&mut self) -> Result<FakeChild, String> {
            self.spawns += 1;
            Ok(self.children.pop_front().unwrap_or_default())
        }
    }
    #[derive(Default)]
    struct FakeHealth {
        responses: VecDeque<Result<HealthResponse, String>>,
        calls: Vec<(String, String, Duration)>,
    }
    impl HealthProbe for FakeHealth {
        fn get(
            &mut self,
            origin: &str,
            path: &str,
            timeout: Duration,
        ) -> Result<HealthResponse, String> {
            self.calls.push((origin.into(), path.into(), timeout));
            self.responses.pop_front().unwrap_or(Ok(HealthResponse {
                status_code: 200,
                status: Some("ready".into()),
            }))
        }
    }
    #[derive(Default)]
    struct FakeWindow {
        opened: Vec<WindowConnection>,
        failures: Vec<String>,
    }
    impl DemoWindow for FakeWindow {
        fn open(&mut self, connection: WindowConnection) -> Result<(), String> {
            self.opened.push(connection);
            Ok(())
        }
        fn show_startup_failure(&mut self, message: &str) {
            self.failures.push(message.into());
        }
    }
    fn line() -> String {
        "{\"event\":\"ready\",\"protocol\":1,\"origin\":\"http://127.0.0.1:43127\"}\n".into()
    }
    fn launcher(
        children: Vec<FakeChild>,
        health: Vec<Result<HealthResponse, String>>,
    ) -> DemoLauncher<FakeFactory, FakeHealth, FakeWindow> {
        DemoLauncher::new(
            FakeFactory {
                children: children.into(),
                spawns: 0,
            },
            FakeHealth {
                responses: health.into(),
                calls: vec![],
            },
            FakeWindow::default(),
        )
    }

    #[test]
    fn valid_ready_and_health_open_owned_window_only_after_both_checks() {
        let mut app = launcher(
            vec![FakeChild {
                line: Some(Ok(Some(line()))),
                stopped: false,
            }],
            vec![Ok(HealthResponse {
                status_code: 200,
                status: Some("ready".into()),
            })],
        );
        let connection = app.start_owned().unwrap();
        assert_eq!(
            connection,
            WindowConnection {
                origin: "http://127.0.0.1:43127".into(),
                kind: InstanceKind::Owned
            }
        );
        assert_eq!(app.parts().1.calls[0].1, HEALTH_PATH);
        assert_eq!(app.parts().2.opened.len(), 1);
    }

    #[test]
    fn malformed_ready_and_timeout_show_failure_and_retry_recovers() {
        let mut app = launcher(
            vec![
                FakeChild {
                    line: Some(Ok(Some("not json\n".into()))),
                    stopped: false,
                },
                FakeChild {
                    line: Some(Ok(None)),
                    stopped: false,
                },
                FakeChild {
                    line: Some(Ok(Some(line()))),
                    stopped: false,
                },
            ],
            vec![Ok(HealthResponse {
                status_code: 200,
                status: Some("ready".into()),
            })],
        );
        assert!(matches!(
            app.start_owned(),
            Err(LaunchError::ReadyProtocol(_))
        ));
        assert!(matches!(app.start_owned(), Err(LaunchError::ReadyTimeout)));
        assert!(app.retry().is_ok());
        assert_eq!(app.parts().0.spawns, 3);
        assert_eq!(app.parts().2.failures.len(), 2);
    }

    #[test]
    fn health_must_be_http_200_and_ready() {
        let mut app = launcher(
            vec![FakeChild {
                line: Some(Ok(Some(line()))),
                stopped: false,
            }],
            vec![Ok(HealthResponse {
                status_code: 503,
                status: None,
            })],
        );
        assert_eq!(app.start_owned(), Err(LaunchError::HealthNotReady));
        assert!(app.parts().2.opened.is_empty());
    }

    #[test]
    fn only_valid_loopback_origins_are_accepted_and_external_is_explicit() {
        assert!(
            parse_ready_line(
                "{\"event\":\"ready\",\"protocol\":1,\"origin\":\"https://127.0.0.1:12\"}\n"
            )
            .is_err()
        );
        assert!(
            parse_ready_line(
                "{\"event\":\"ready\",\"protocol\":1,\"origin\":\"http://example.com:12\"}\n"
            )
            .is_err()
        );
        assert!(
            parse_ready_line(
                "{\"event\":\"ready\",\"protocol\":2,\"origin\":\"http://127.0.0.1:12\"}\n"
            )
            .is_err()
        );
        let mut app = launcher(
            vec![],
            vec![Ok(HealthResponse {
                status_code: 200,
                status: Some("ready".into()),
            })],
        );
        let connection = app.connect_external("http://127.0.0.1:43128").unwrap();
        assert_eq!(connection.kind, InstanceKind::External);
        assert_eq!(app.parts().0.spawns, 0);
    }
}
