//! Minimal desktop binary entrypoint for the shared Slint demo package.

#[cfg(not(target_arch = "wasm32"))]
use slint::ComponentHandle;

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("photo-culler-desktop {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    let window = match photo_culler_slint::DemoWindow::new() {
        Ok(window) => window,
        Err(error) => {
            eprintln!("photo-culler-desktop failed to open the window: {error}");
            std::process::exit(1);
        }
    };
    let server = photo_culler_slint::desktop::native_flow::ServerSlot::default();
    let retry_window = window.as_weak();
    let retry_server = server.clone();
    window.on_retry(move || {
        if let Some(window) = retry_window.upgrade() {
            window.set_view_state(0);
        }
        photo_culler_slint::desktop::native_flow::load(retry_window.clone(), retry_server.clone());
    });
    photo_culler_slint::desktop::native_flow::load(window.as_weak(), server.clone());
    if let Err(error) = window.run() {
        eprintln!("photo-culler-desktop failed: {error}");
        std::process::exit(1);
    }
    drop(server);
}

#[cfg(target_arch = "wasm32")]
fn main() {}
