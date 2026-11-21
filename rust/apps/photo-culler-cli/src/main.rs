use std::{env, path::Path, process::ExitCode};

use photo_culler_core::{Frontend, StorageBackend};
use photo_culler_image_engine::analyze_path;

pub mod commands;

fn usage() {
    println!("photo-culler-rs <COMMAND>");
    println!("Server commands (API options precede the command):");
    println!("  [--server URL] status | doctor | diagnostics | active-gallery");
    println!("  [--server URL] galleries | photos GALLERY_ID | sources GALLERY_ID");
    println!("  [--server URL] profiles | photo PHOTO_ID | summary GALLERY_ID");
    println!("  [--server URL] sessions GALLERY_ID | groups GALLERY_ID");
    println!("  [--server URL] estimate SOURCE_ID | import GALLERY_ID SOURCE_ID");
    println!("  [--server URL] scan GALLERY_ID | jobs | job JOB_ID [pause|resume|cancel]");
    println!("  [--server URL] revisions GALLERY_ID | decision PHOTO_ID STATE REVISION");
    println!("  [--server URL] [--dry-run] <mutating server command>");
    println!("  [--server URL] --credential-file PATH auth pair|pairing-code|logout");
    println!("  auth pair reads the one-time code from stdin; no credential is accepted in argv.");
    println!("  [--catalog PATH] doctor|diagnostics (diagnostic context only)");
    println!("Local commands (no API options):");
    println!("  analyze PATH [MAX_DIMENSION] | backends | frontends");
    println!("Server default: http://127.0.0.1:8765.");
    println!("Unsupported roadmap commands return a usage error; see commands/INTEGRATION.md.");
}

fn main() -> ExitCode {
    let argv: Vec<String> = env::args().skip(1).collect();
    match argv.first().map(String::as_str) {
        Some("-h" | "--help" | "help") => {
            usage();
            ExitCode::SUCCESS
        }
        _ if has_prefixed_local_command(&argv) => {
            eprintln!(
                "API options cannot be used with local commands; run the local command directly."
            );
            usage();
            ExitCode::from(2)
        }
        Some("backends") => {
            if argv.len() != 1 {
                eprintln!("backends takes no arguments");
                return ExitCode::from(2);
            }
            for backend in [StorageBackend::Sqlite, StorageBackend::PostgreSql] {
                println!("{backend}: {}% ready", backend.readiness());
            }
            ExitCode::SUCCESS
        }
        Some("frontends") => {
            if argv.len() != 1 {
                eprintln!("frontends takes no arguments");
                return ExitCode::from(2);
            }
            println!("tauri-webgl: {}% ready", Frontend::TauriWebGl.readiness());
            println!("egui-wgpu: {}% ready", Frontend::EguiWgpu.readiness());
            ExitCode::SUCCESS
        }
        Some("analyze") => analyze(&argv),
        _ => run_operational_command(&argv),
    }
}

fn has_prefixed_local_command(argv: &[String]) -> bool {
    let mut i = 0;
    while i < argv.len() {
        match argv[i].as_str() {
            "--server" | "--catalog" | "-c" | "--credential-file" => i += 2,
            "--dry-run" => i += 1,
            "analyze" | "backends" | "frontends" => return i > 0,
            value if value.starts_with('-') => i += 1,
            _ => return false,
        }
    }
    false
}

fn analyze(argv: &[String]) -> ExitCode {
    if !(2..=3).contains(&argv.len()) {
        usage();
        return ExitCode::from(2);
    }
    let max_dimension = match argv.get(2).map(|value| value.parse::<u32>()).transpose() {
        Ok(value) => value.unwrap_or(1920),
        Err(error) => {
            eprintln!("invalid MAX_DIMENSION: {error}");
            return ExitCode::from(2);
        }
    };
    match analyze_path(Path::new(&argv[1]), max_dimension) {
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

fn run_operational_command(argv: &[String]) -> ExitCode {
    let invocation = match commands::Invocation::parse(argv) {
        Ok(invocation) => invocation,
        Err(error) => {
            eprintln!("{}", commands::format_error(&error));
            usage();
            return ExitCode::from(error.exit_code());
        }
    };
    let bearer = if invocation.command == "auth" {
        None
    } else {
        match commands::load_bearer(&invocation) {
            Ok(bearer) => bearer,
            Err(error) => {
                eprintln!("{}", commands::format_error(&error));
                return ExitCode::from(error.exit_code());
            }
        }
    };
    let mut client = commands::UreqRestClient::with_bearer(bearer);
    let output = commands::execute(&mut client, &invocation);
    if !output.stdout.is_empty() {
        println!("{}", output.stdout);
    }
    if !output.stderr.is_empty() {
        eprintln!("{}", output.stderr);
    }
    ExitCode::from(output.exit_code)
}
