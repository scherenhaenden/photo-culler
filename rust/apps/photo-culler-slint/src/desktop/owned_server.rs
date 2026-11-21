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
    path::{Path, PathBuf},
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
    /// Session bearer kept in process memory and never included in diagnostics.
    pub session_token: Option<String>,
    address: SocketAddr,
    _owned: Option<OwnedChild>,
}

impl DemoServer {
    /// Uses an explicitly configured loopback service when supplied; otherwise
    /// starts and owns the server executable beside this desktop binary.
    pub fn open(
        data_dir: Option<&Path>,
        authorized_roots: &[PathBuf],
        gallery_id: Option<&str>,
    ) -> Result<Self, String> {
        if let Some(origin) = std::env::var_os("PHOTO_CULLER_API_ORIGIN") {
            if data_dir.is_some() || !authorized_roots.is_empty() || gallery_id.is_some() {
                return Err(
                    "persistent catalog and import options require the launcher-owned local server; external API mode is active"
                        .to_owned(),
                );
            }
            let origin = origin
                .into_string()
                .map_err(|_| "PHOTO_CULLER_API_ORIGIN is not valid UTF-8".to_owned())?;
            return Self::connect_external(&origin);
        }
        Self::start_owned(data_dir, authorized_roots, gallery_id)
    }

    pub fn address(&self) -> SocketAddr {
        self.address
    }

    /// Returns the process ID only when this launcher owns the server child.
    pub fn owned_process_id(&self) -> Option<u32> {
        self._owned.as_ref().map(|owned| owned.child.id())
    }

    fn connect_external(origin: &str) -> Result<Self, String> {
        let mut ready = validate_origin(origin)?;
        ready.session_token = match std::env::var("PHOTO_CULLER_BEARER") {
            Ok(token) if valid_session_token(&token) => Some(token),
            Ok(_) => return Err("PHOTO_CULLER_BEARER has an invalid session token format".into()),
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err("PHOTO_CULLER_BEARER is not valid UTF-8".into());
            }
        };
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
            session_token: ready.session_token,
            address,
            _owned: None,
        })
    }

    fn start_owned(
        data_dir: Option<&Path>,
        authorized_roots: &[PathBuf],
        gallery_id: Option<&str>,
    ) -> Result<Self, String> {
        let executable = sibling_server_path()?;
        Self::start_owned_at_with_data_dir(&executable, data_dir, authorized_roots, gallery_id)
    }

    #[cfg(test)]
    fn start_owned_at(executable: &std::path::Path) -> Result<Self, String> {
        Self::start_owned_at_with_data_dir(executable, None, &[], None)
    }

    fn start_owned_at_with_data_dir(
        executable: &std::path::Path,
        data_dir: Option<&Path>,
        authorized_roots: &[PathBuf],
        gallery_id: Option<&str>,
    ) -> Result<Self, String> {
        if data_dir.is_some_and(|path| path.as_os_str().is_empty()) {
            return Err("--data-dir requires a non-empty path".to_owned());
        }
        if data_dir.is_some_and(|path| path.exists() && !path.is_dir()) {
            return Err("--data-dir must point to a directory".to_owned());
        }
        if (!authorized_roots.is_empty() || gallery_id.is_some()) && data_dir.is_none() {
            return Err("--authorized-root and --gallery-id require --data-dir".to_owned());
        }
        if gallery_id.is_some() && authorized_roots.is_empty() {
            return Err("--gallery-id requires at least one --authorized-root".to_owned());
        }
        if gallery_id.is_some_and(str::is_empty) {
            return Err("--gallery-id requires a non-empty ID".to_owned());
        }
        if authorized_roots
            .iter()
            .any(|path| path.as_os_str().is_empty())
        {
            return Err("--authorized-root requires a non-empty path".to_owned());
        }

        let mut command = Command::new(executable);
        command.args(["--owned", "--port", "0"]);
        if let Some(data_dir) = data_dir {
            command.arg("--data-dir").arg(data_dir.as_os_str());
        }
        for root in authorized_roots {
            command.arg("--authorized-root").arg(root.as_os_str());
        }
        if let Some(gallery_id) = gallery_id {
            command.arg("--gallery-id").arg(gallery_id);
        }
        let mut child = command
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
        let session_token = ready
            .session_token
            .filter(|token| valid_session_token(token))
            .ok_or_else(|| "owned server omitted its session credential".to_owned())?;
        let expected_origin = format!("http://127.0.0.1:{}", ready.port);
        if ready.origin != expected_origin {
            return Err("owned server must announce its 127.0.0.1 loopback origin".into());
        }
        let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), ready.port);
        verify_health(address)?;
        Ok(Self {
            origin: ready.origin,
            session_token: Some(session_token),
            address,
            _owned: Some(owned),
        })
    }
}

fn valid_session_token(token: &str) -> bool {
    token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit())
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
        let transport = NativeHttpTransport::new_authenticated(
            address,
            Duration::from_secs(2),
            2 * 1024 * 1024,
            server
                .session_token
                .clone()
                .expect("owned server must provide its editor credential"),
        )
        .unwrap();

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
