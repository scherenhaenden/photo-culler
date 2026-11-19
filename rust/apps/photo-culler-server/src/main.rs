use std::process::ExitCode;

use photo_culler_server::{
    demo::runtime::build_router,
    demo_args::{HELP, ParseOutcome, parse},
    lifecycle,
};
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() -> ExitCode {
    match parse(std::env::args().skip(1)) {
        Ok(ParseOutcome::Help) => {
            println!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(ParseOutcome::Run(args)) => match serve(args).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("{error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            eprintln!("{error}\n\n{HELP}");
            ExitCode::from(2)
        }
    }
}

async fn serve(args: photo_culler_server::demo_args::DemoArgs) -> Result<(), String> {
    let server = lifecycle::start_on(args.bind, args.port, |address| {
        build_router(address, args.web_root.as_deref())
    })
    .await
    .map_err(|error| format!("server startup failed: {error}"))?;

    let ready = server.ready_message();
    if args.owned {
        println!(
            "{}",
            serde_json::to_string(&ready).map_err(|error| error.to_string())?
        );
    } else {
        println!("{}", ready.origin);
    }

    let shutdown_result = if args.owned {
        server.wait_for_shutdown(stdin_eof(), ctrl_c()).await
    } else {
        server
            .wait_for_shutdown(std::future::pending(), ctrl_c())
            .await
    };
    shutdown_result.map_err(|error| format!("server shutdown failed: {error}"))
}

async fn stdin_eof() {
    let mut input = Vec::new();
    let _ = tokio::io::stdin().read_to_end(&mut input).await;
}

async fn ctrl_c() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        eprintln!("could not listen for Ctrl+C: {error}");
    }
}
