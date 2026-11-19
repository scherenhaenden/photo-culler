//! Start the sibling Axum process, validate its ready protocol and health, and
//! retain ownership until the Slint window closes.

use super::{
    demo_http::NativeHttpTransport,
    demo_launch::{HEALTH_PATH, ReadyMessage, parse_ready_line, validate_origin},
};
use photo_culler_api::{HealthResponse, HealthStatus};
use std::{
    io::{BufRead, BufReader, Read},
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const READY_TIMEOUT: Duration = Duration::from_secs(10);
const HEALTH_TIMEOUT: Duration = Duration::from_secs(2);
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_READY_LINE: usize = 4096;

pub struct DemoServer {
    pub origin: String,
    address: SocketAddr,
    _owned: Option<OwnedChild>,
}

impl DemoServer {
    /// Uses an explicitly configured loopback service when supplied; otherwise
    /// starts and owns the server executable beside this desktop binary.
    pub fn open() -> Result<Self, String> {
        if let Some(origin) = std::env::var_os("PHOTO_CULLER_API_ORIGIN") {
            let origin = origin
                .into_string()
                .map_err(|_| "PHOTO_CULLER_API_ORIGIN is not valid UTF-8".to_owned())?;
            return Self::connect_external(&origin);
        }
        Self::start_owned()
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    fn connect_external(origin: &str) -> Result<Self, String> {
        let ready = validate_origin(origin)?;
        let host = origin
            .strip_prefix("http://")
            .and_then(|authority| authority.rsplit_once(':').map(|(host, _)| host))
            .ok_or_else(|| "external origin must contain a host and port".to_owned())?;
        let address = match host {
            "[::1]" => SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), ready.port),
            _ => SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ready.port),
        };
        verify_health(address)?;
        Ok(Self {
            origin: ready.origin,
            address,
            _owned: None,
        })
    }

    fn start_owned() -> Result<Self, String> {
        let executable = sibling_server_path()?;
        Self::start_owned_at(&executable)
    }

    fn start_owned_at(executable: &std::path::Path) -> Result<Self, String> {
        let mut child = Command::new(executable)
            .args(["--owned", "--port", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("could not start {}: {error}", executable.display()))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "server stdin was not piped".to_owned())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "server stdout was not piped".to_owned())?;
        let owned = OwnedChild {
            child,
            stdin: Some(stdin),
        };

        let (sender, receiver) = mpsc::sync_channel(1);
        thread::spawn(move || {
            let mut line = Vec::new();
            let result = BufReader::new(stdout)
                .take((MAX_READY_LINE + 1) as u64)
                .read_until(b'\n', &mut line)
                .map_err(|error| format!("could not read server ready event: {error}"))
                .and_then(|count| {
                    if count == 0 {
                        return Err("server exited before announcing readiness".into());
                    }
                    if line.len() > MAX_READY_LINE {
                        return Err("server ready event exceeds 4096 bytes".into());
                    }
                    String::from_utf8(line)
                        .map_err(|_| "server ready event is not valid UTF-8".to_owned())
                });
            let _ = sender.send(result);
        });

        let line = receiver
            .recv_timeout(READY_TIMEOUT)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => {
                    "server did not become ready within 10 seconds".to_owned()
                }
                mpsc::RecvTimeoutError::Disconnected => {
                    "server ready-event reader stopped unexpectedly".to_owned()
                }
            })??;
        let ready: ReadyMessage = parse_ready_line(&line)?;
        let expected_origin = format!("http://127.0.0.1:{}", ready.port);
        if ready.origin != expected_origin {
            return Err("owned server must announce its 127.0.0.1 loopback origin".into());
        }
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ready.port);
        verify_health(address)?;
        Ok(Self {
            origin: ready.origin,
            address,
            _owned: Some(owned),
        })
    }
}

struct OwnedChild {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        // Closing owned stdin requests graceful shutdown. Reap the process even
        // when the server does not honor that signal.
        self.stdin.take();
        let deadline = Instant::now() + SHUTDOWN_TIMEOUT;
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn sibling_server_path() -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("PHOTO_CULLER_SERVER") {
        let path = PathBuf::from(path);
        if path.is_file() {
            return path.canonicalize().map_err(|error| error.to_string());
        }
        return Err(format!(
            "configured server executable does not exist: {}",
            path.display()
        ));
    }
    let client = std::env::current_exe().map_err(|error| error.to_string())?;
    let parent = client
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| "desktop executable has no parent directory".to_owned())?;
    let filename = if cfg!(windows) {
        "photo-culler-server.exe"
    } else {
        "photo-culler-server"
    };
    let candidate = parent.join(filename);
    if !candidate.is_file() {
        return Err(format!(
            "the sibling server executable is missing: {}",
            candidate.display()
        ));
    }
    candidate.canonicalize().map_err(|error| error.to_string())
}

fn verify_health(address: SocketAddr) -> Result<(), String> {
    let response = NativeHttpTransport::new(address, HEALTH_TIMEOUT, 64 * 1024)
        .map_err(|error| format!("invalid local server endpoint: {error:?}"))?
        .get(HEALTH_PATH)
        .map_err(|error| format!("local server health request failed: {error:?}"))?;
    if response.status != 200 {
        return Err(format!(
            "local server health returned HTTP {}",
            response.status
        ));
    }
    if !response
        .content_type
        .split(';')
        .next()
        .is_some_and(|mime| mime.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err("local server health did not return application/json".into());
    }
    let health: HealthResponse = serde_json::from_slice(&response.body)
        .map_err(|error| format!("local server returned invalid health JSON: {error}"))?;
    if health.status != HealthStatus::Ready {
        return Err("local server health is not ready".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires PHOTO_CULLER_SERVER to name a built server executable"]
    fn owned_child_serves_the_gallery_and_releases_its_port_on_drop() {
        let executable = std::env::var_os("PHOTO_CULLER_SERVER")
            .map(PathBuf::from)
            .expect("CI must set PHOTO_CULLER_SERVER to the staged server binary");
        let server = DemoServer::start_owned_at(&executable).expect("owned server should start");
        let address = server.address();
        let transport =
            NativeHttpTransport::new(address, Duration::from_secs(2), 2 * 1024 * 1024).unwrap();

        let gallery = transport
            .get("/api/v1/galleries/demo/photos")
            .expect("gallery request should succeed");
        assert_eq!(gallery.status, 200);
        assert!(String::from_utf8_lossy(&gallery.body).contains("demo-001.jpg"));

        let thumbnail = transport
            .get("/api/v1/photos/demo-001/thumbnail/800")
            .expect("thumbnail request should succeed");
        assert_eq!(thumbnail.status, 200);
        assert!(thumbnail.content_type.starts_with("image/jpeg"));
        assert!(thumbnail.body.starts_with(&[0xff, 0xd8, 0xff]));

        drop(server);
        assert!(
            transport.get(HEALTH_PATH).is_err(),
            "owned server must stop on drop"
        );
    }

    #[test]
    fn external_server_requires_an_explicit_loopback_origin() {
        assert!(DemoServer::connect_external("https://127.0.0.1:1234").is_err());
        assert!(DemoServer::connect_external("http://example.com:1234").is_err());
    }
}
