//! Native shell for the complete, local Photo Culler web experience.
//!
//! The domain application remains in the versioned FastAPI service.  This shell owns
//! its process lifetime, an unguessable loopback session and the native window, so it
//! can be distributed without relying on an installed browser or a long-running
//! Python process.

use std::{
    fs::OpenOptions,
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream},
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};

use tauri::{AppHandle, Manager, RunEvent};
use tauri_plugin_shell::{
    ShellExt,
    process::{CommandChild, CommandEvent},
};

const BACKEND_NAME: &str = "photo-culler-backend";
const BACKEND_TOKEN_ENV: &str = "PHOTO_CULLER_TAURI_TOKEN";

struct Backend(Mutex<Option<(CommandChild, tauri::async_runtime::Receiver<CommandEvent>)>>);

fn available_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|error| error.to_string())?;
    let port = listener
        .local_addr()
        .map_err(|error| error.to_string())?
        .port();
    // The backend binds immediately after its spawn.  Keeping the listener alive
    // would prevent it from doing so, therefore this is only a short reservation.
    drop(listener);
    Ok(port)
}

fn backend_url(port: u16, token: &str) -> Result<url::Url, String> {
    url::Url::parse(&format!("http://127.0.0.1:{port}/?token={token}"))
        .map_err(|error| error.to_string())
}

fn wait_for_backend(app: &AppHandle, port: u16, token: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let state = app.state::<Backend>();
        let mut backend = state.0.lock().map_err(|_| "backend state lock poisoned")?;
        let (_, events) = backend
            .as_mut()
            .ok_or_else(|| "the local backend was not running".to_owned())?;
        while let Ok(event) = events.try_recv() {
            match event {
                CommandEvent::Terminated(status) => {
                    return Err(format!(
                        "the local backend exited before becoming ready (code {:?}, signal {:?})",
                        status.code, status.signal
                    ));
                }
                CommandEvent::Error(error) => {
                    return Err(format!(
                        "the local backend failed before becoming ready: {error}"
                    ));
                }
                CommandEvent::Stdout(_) | CommandEvent::Stderr(_) => {}
                _ => {}
            }
        }
        drop(backend);
        if let Ok(mut stream) = TcpStream::connect_timeout(
            &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
            Duration::from_millis(250),
        ) {
            let request = format!(
                "GET /api/health?token={token} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
            );
            if stream.write_all(request.as_bytes()).is_ok() {
                let mut response = String::new();
                if stream.read_to_string(&mut response).is_ok()
                    && response.starts_with("HTTP/1.1 200")
                {
                    return Ok(());
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err("the local Photo Culler backend did not become ready within 10 seconds".into())
}

fn stop_backend(app: &AppHandle) {
    if let Some(state) = app.try_state::<Backend>()
        && let Ok(mut child) = state.0.lock()
        && let Some((child, _events)) = child.take()
    {
        let _ = child.kill();
    }
}

fn start(app: &AppHandle) -> Result<(), String> {
    let port = available_port()?;
    let token = uuid::Uuid::new_v4().simple().to_string();
    let command = app
        .shell()
        .sidecar(BACKEND_NAME)
        .map_err(|error| format!("could not locate packaged backend: {error}"))?
        .args(["--host", "127.0.0.1", "--port", &port.to_string()])
        .env(BACKEND_TOKEN_ENV, &token);
    let (events, child) = command
        .spawn()
        .map_err(|error| format!("could not start local backend: {error}"))?;
    *app.state::<Backend>()
        .0
        .lock()
        .map_err(|_| "backend state lock poisoned")? = Some((child, events));

    if let Err(error) = wait_for_backend(app, port, &token) {
        stop_backend(app);
        return Err(error);
    }
    let url = backend_url(port, &token)?;
    let handle = app.clone();
    app.run_on_main_thread(move || {
        let result = handle
            .get_webview_window("main")
            .ok_or_else(|| "the configured main Tauri window was not created".to_owned())
            .and_then(|window| {
                window.navigate(url).map_err(|error| {
                    format!("could not navigate native window to local backend: {error}")
                })
            });
        if let Err(error) = result {
            report_startup_error(&handle, &format!("Photo Culler navigation failed: {error}"));
        }
    })
    .map_err(|error| format!("could not schedule native window navigation: {error}"))?;
    Ok(())
}

fn report_startup_error(app: &AppHandle, message: &str) {
    eprintln!("{message}");
    let log_path = std::env::temp_dir().join("photo-culler-tauri.log");
    if let Ok(mut log) = OpenOptions::new().create(true).append(true).open(&log_path) {
        let _ = writeln!(log, "{message}");
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.set_title(&format!(
            "Photo Culler — startup failed; see {}",
            log_path.display()
        ));
    }
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(Backend(Mutex::new(None)))
        .setup(|app| {
            // The GTK/Wayland event loop must run before a toplevel receives its
            // first configure event.  Starting the local service synchronously in
            // setup can leave KWin with an unpresented window, so keep the static
            // startup page visible and initialize the sidecar just after setup.
            let handle = app.handle().clone();
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = start(&handle) {
                    report_startup_error(&handle, &format!("Photo Culler startup failed: {error}"));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Photo Culler Tauri application")
        .run(|app, event| {
            if matches!(event, RunEvent::Exit | RunEvent::ExitRequested { .. }) {
                stop_backend(app);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_url_is_loopback_and_contains_the_session_token() {
        let url = backend_url(43210, "session-token").expect("valid URL");
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(43210));
        assert_eq!(url.query(), Some("token=session-token"));
    }

    #[test]
    fn selected_port_is_available_for_the_loopback_backend() {
        assert!(available_port().expect("port selection") > 0);
    }
}
