//! Own the local fixture server and display its first gallery photo.

use std::{
    io::{BufRead, BufReader, Read},
    net::SocketAddr,
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc::sync_channel,
    },
    thread,
    time::{Duration, Instant},
};

use crate::{DemoWindow, demo::flow};

use super::demo_http::NativeHttpTransport;

const MAX_JSON_BYTES: usize = 64 * 1024;
const MAX_THUMBNAIL_BYTES: usize = 2 * 1024 * 1024;
const READY_LINE_LIMIT: usize = 4096;
const READY_TIMEOUT: Duration = Duration::from_secs(10);

struct OwnedServer {
    child: Child,
    address: SocketAddr,
}

impl OwnedServer {
    fn start() -> Result<Self, String> {
        let executable = server_sibling()?;
        let mut child = Command::new(&executable)
            .args(["--port", "0"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|error| format!("Could not start the local server: {error}"))?;

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "The local server did not expose its startup channel.".to_owned())?;
        let (ready_tx, ready_rx) = sync_channel(1);
        let reader = thread::spawn(move || {
            let mut line = String::new();
            let result = BufReader::new(stdout)
                .take((READY_LINE_LIMIT + 1) as u64)
                .read_line(&mut line)
                .map_err(|error| {
                    format!("Could not read the local server startup message: {error}")
                });
            let _ = ready_tx.send(result.map(|read| (read, line)));
        });

        let address_result = (|| -> Result<SocketAddr, String> {
            let (read, line) = ready_rx
                .recv_timeout(READY_TIMEOUT)
                .map_err(|error| match error {
                    std::sync::mpsc::RecvTimeoutError::Timeout => {
                        "The local server did not report readiness within 10 seconds.".to_owned()
                    }
                    std::sync::mpsc::RecvTimeoutError::Disconnected => {
                        "The local server closed before reporting readiness.".to_owned()
                    }
                })?
                .map_err(|error| error.to_string())?;
            if read == 0 || read > READY_LINE_LIMIT {
                return Err("The local server closed before reporting readiness.".into());
            }

            let ready: serde_json::Value = serde_json::from_str(&line).map_err(|_| {
                "The local server returned an invalid readiness message.".to_owned()
            })?;
            if ready.get("event").and_then(serde_json::Value::as_str) != Some("ready")
                || ready.get("protocol").and_then(serde_json::Value::as_u64) != Some(1)
            {
                return Err("The local server uses an unsupported readiness protocol.".into());
            }
            let origin = ready
                .get("origin")
                .and_then(serde_json::Value::as_str)
                .and_then(|value| value.strip_prefix("http://"))
                .ok_or_else(|| "The local server returned an invalid origin.".to_owned())?;
            origin
                .parse::<SocketAddr>()
                .ok()
                .filter(|address| address.ip().is_loopback() && address.port() != 0)
                .ok_or_else(|| "The local server origin must be a loopback socket.".to_owned())
        })();
        let _ = reader.join();

        match address_result {
            Ok(address) => Ok(Self { child, address }),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }

    fn is_running(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for OwnedServer {
    fn drop(&mut self) {
        // Closing the piped stdin asks the server to shut down gracefully.
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(20));
                }
                Ok(None) => {
                    let _ = self.child.kill();
                    let _ = self.child.wait();
                    return;
                }
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct ServerSlot {
    server: Arc<Mutex<Option<OwnedServer>>>,
    generation: Arc<AtomicU64>,
}

pub fn load(window: slint::Weak<DemoWindow>, slot: ServerSlot) {
    let generation = slot
        .generation
        .fetch_add(1, Ordering::Relaxed)
        .wrapping_add(1);
    if let Some(window) = window.upgrade() {
        window.set_view_state(0);
    }
    let current_generation = Arc::clone(&slot.generation);
    thread::spawn(move || {
        let result = fetch_first_photo(&slot);
        let _ = slint::invoke_from_event_loop(move || {
            if current_generation.load(Ordering::Relaxed) != generation {
                return;
            }
            let Some(window) = window.upgrade() else {
                return;
            };
            match result {
                Ok(payload) => {
                    if let Err(error) = flow::render(&window, &payload.0, &payload.1, &payload.2) {
                        flow::show_error(&window, error);
                    }
                }
                Err(error) => flow::show_error(&window, error),
            }
        });
    });
}

fn fetch_first_photo(slot: &ServerSlot) -> Result<(Vec<u8>, Vec<u8>, Vec<u8>), String> {
    let address = {
        let mut slot = slot
            .server
            .lock()
            .map_err(|_| "The local server state is unavailable.".to_owned())?;
        if slot.as_mut().is_some_and(OwnedServer::is_running) {
            slot.as_ref().expect("checked above").address
        } else {
            *slot = Some(OwnedServer::start()?);
            slot.as_ref().expect("just started").address
        }
    };

    let transport = NativeHttpTransport::new(address, Duration::from_secs(2), MAX_THUMBNAIL_BYTES)
        .map_err(|error| format!("Could not configure the local connection: {error:?}"))?;
    let health = transport
        .get("/api/v1/health")
        .map_err(|error| format!("The local service is not ready: {error:?}"))?;
    let gallery = transport
        .get("/api/v1/galleries/demo/photos")
        .map_err(|error| format!("Could not load the local gallery: {error:?}"))?;
    if health.body.len() > MAX_JSON_BYTES || gallery.body.len() > MAX_JSON_BYTES {
        return Err("The local service response exceeded the gallery size limit.".into());
    }
    if !health
        .content_type
        .to_ascii_lowercase()
        .starts_with("application/json")
        || !gallery
            .content_type
            .to_ascii_lowercase()
            .starts_with("application/json")
    {
        return Err("The local service returned an unexpected response type.".into());
    }
    let photo: serde_json::Value = serde_json::from_slice(&gallery.body)
        .map_err(|_| "The local service returned invalid gallery data.".to_owned())?;
    let thumbnail_path = photo
        .get("items")
        .and_then(serde_json::Value::as_array)
        .and_then(|items| items.first())
        .and_then(|item| item.get("thumbnail_url"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "The local gallery has no preview image.".to_owned())?;
    let thumbnail = transport
        .get(thumbnail_path)
        .map_err(|error| format!("Could not load the gallery preview: {error:?}"))?;
    if !thumbnail
        .content_type
        .to_ascii_lowercase()
        .starts_with("image/jpeg")
    {
        return Err("The local gallery preview is not a JPEG image.".into());
    }
    Ok((health.body, gallery.body, thumbnail.body))
}

fn server_sibling() -> Result<PathBuf, String> {
    let executable = std::env::current_exe()
        .map_err(|error| format!("Could not locate Photo Culler Desktop: {error}"))?;
    let parent = executable
        .parent()
        .ok_or_else(|| "Photo Culler Desktop has no executable directory.".to_owned())?;
    #[cfg(target_os = "windows")]
    let name = "photo-culler-server.exe";
    #[cfg(not(target_os = "windows"))]
    let name = "photo-culler-server";
    let sibling = parent.join(name);
    if !sibling.is_file() {
        return Err(format!(
            "The local server is missing beside the desktop app: {}",
            sibling.display()
        ));
    }
    Ok(sibling)
}
