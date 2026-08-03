use std::{env, path::Path, process::ExitCode};

use photo_culler_core::{Frontend, StorageBackend};
use photo_culler_image_engine::analyze_path;

fn usage() {
    println!("photo-culler-rs <status|backends|frontends|analyze PATH [MAX_DIMENSION]>");
    println!("MAX_DIMENSION defaults to 1920; 0 also uses the default bound.");
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
        Some("analyze") => {
            let Some(path) = env::args().nth(2) else {
                usage();
                return ExitCode::from(2);
            };
            let max_dimension = env::args()
                .nth(3)
                .map(|value| value.parse::<u32>())
                .transpose()
                .unwrap_or_else(|error| {
                    eprintln!("invalid MAX_DIMENSION: {error}");
                    std::process::exit(2);
                })
                .unwrap_or(1920);
            match analyze_path(Path::new(&path), max_dimension) {
                Ok(features) => match serde_json::to_string(&features) {
                    Ok(json) => {
                        println!("{json}");
                        ExitCode::SUCCESS
                    }
                    Err(error) => {
                        eprintln!("unable to serialize analysis: {error}");
                        ExitCode::FAILURE
                    }
                },
                Err(error) => {
                    eprintln!("{error}");
                    ExitCode::FAILURE
                }
            }
        }
        _ => {
            usage();
            ExitCode::from(2)
        }
    }
}
