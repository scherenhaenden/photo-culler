//! Minimal desktop binary entrypoint for the shared Slint demo package.

use slint::ComponentHandle;

fn main() {
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("photo-culler-desktop {}", env!("CARGO_PKG_VERSION"));
        return;
    }
    if let Err(err) = photo_culler_slint::DemoScaffold::new().and_then(|w| w.run()) {
        eprintln!("photo-culler-desktop failed: {err}");
        std::process::exit(1);
    }
}
