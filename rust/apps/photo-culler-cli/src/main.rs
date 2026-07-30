use std::{env, process::ExitCode};

use photo_culler_core::{Frontend, StorageBackend};

fn usage() {
    println!("photo-culler-rs <status|backends|frontends>");
}

fn main() -> ExitCode {
    match env::args().nth(1).as_deref() {
        Some("status") => {
            println!("Rust foundation: experimental");
            println!("Default catalog: sqlite");
            ExitCode::SUCCESS
        }
        Some("backends") => {
            for backend in [StorageBackend::Sqlite, StorageBackend::PostgreSql] {
                println!("{backend}: {}% ready", backend.readiness());
            }
            ExitCode::SUCCESS
        }
        Some("frontends") => {
            println!("tauri-webgl: {}% ready", Frontend::TauriWebGl.readiness());
            println!("egui-wgpu: {}% ready", Frontend::EguiWgpu.readiness());
            ExitCode::SUCCESS
        }
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}
