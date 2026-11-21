use std::{io::IsTerminal, process::ExitCode};

use photo_culler_server::{
    catalog::sqlite::{Catalog as SqliteCatalog, http_adapter::SqliteHttpCatalog},
    demo::runtime::{build_catalog_router, build_import_catalog_router, build_router},
    demo_args::{HELP, ParseOutcome, parse},
    http::analysis::router as analysis_router,
    http::analysis_sqlite::SqliteAnalysisApi,
    http::imports::sqlite_api::SqliteImportApi,
    http::previews::router as previews_router,
    http::previews_sqlite::SqlitePreviewApi,
    http::recipe_sqlite::SqliteRecipeApi,
    http::session::runtime::Bootstrap,
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
    let bind = args.bind;
    let port = args.port;
    let owned = args.owned;
    let mut auth_bootstrap = Bootstrap::new(owned)?;
    let auth = auth_bootstrap.auth.clone();
    let web_root = args.web_root;
    let authorized_roots = args.authorized_roots;
    let gallery_id = args.gallery_id;

    // Keep fixture-only S01 behavior as the default. Supplying --data-dir is an
    // explicit opt-in to a durable, local SQLite catalogue.
    let server = match args.data_dir {
        Some(data_dir) => {
            std::fs::create_dir_all(&data_dir)
                .map_err(|_| "could not create the persistent catalog directory".to_owned())?;
            let database_path = data_dir.join("catalog.sqlite3");
            let catalog = SqliteCatalog::open_initialized(&database_path)
                .await
                .map_err(|_| "could not initialize the persistent catalog".to_owned())?;
            let writer = catalog.writer_pool();
            let recipes = SqliteRecipeApi::new(writer.clone(), tokio::runtime::Handle::current());
            let analysis = if authorized_roots.is_empty() {
                None
            } else {
                let api = SqliteAnalysisApi::initialize(
                    writer.clone(),
                    tokio::runtime::Handle::current(),
                    authorized_roots.clone(),
                    1,
                    4,
                )
                .await
                .map_err(|_| "could not initialize the analysis service".to_owned())?;
                Some(analysis_router::<SqliteAnalysisApi>().with_state(std::sync::Arc::new(api)))
            };
            let previews = Some(previews_router::<SqlitePreviewApi>().with_state(
                std::sync::Arc::new(SqlitePreviewApi::new(
                    catalog.clone(),
                    tokio::runtime::Handle::current(),
                )),
            ));
            if authorized_roots.is_empty() {
                let active_gallery = if let Some(requested) = gallery_id {
                    let galleries = catalog
                        .list_galleries()
                        .await
                        .map_err(|_| "could not read catalog galleries".to_owned())?;
                    if !galleries.iter().any(|(id, _)| id == &requested) {
                        return Err("--gallery-id does not identify a catalog gallery".into());
                    }
                    Some(requested)
                } else {
                    None
                };
                let adapter = SqliteHttpCatalog::new(catalog).with_active_gallery(active_gallery);
                lifecycle::start_on(bind, port, move |address| {
                    build_catalog_router(
                        address,
                        web_root.as_deref(),
                        adapter,
                        recipes,
                        analysis,
                        previews,
                        auth.clone(),
                    )
                })
                .await
            } else {
                let imports =
                    SqliteImportApi::new(writer.clone(), authorized_roots, gallery_id).await?;
                let adapter = SqliteHttpCatalog::new(catalog)
                    .with_active_gallery(Some(imports.gallery_id().to_owned()));
                lifecycle::start_on(bind, port, move |address| {
                    build_import_catalog_router(
                        address,
                        web_root.as_deref(),
                        adapter,
                        imports,
                        recipes,
                        analysis,
                        previews,
                        auth.clone(),
                    )
                })
                .await
            }
        }
        None if !authorized_roots.is_empty() || gallery_id.is_some() => {
            return Err("--authorized-root and --gallery-id require --data-dir".into());
        }
        None => {
            lifecycle::start_on(bind, port, move |address| {
                build_router(address, web_root.as_deref(), auth.clone())
            })
            .await
        }
    }
    .map_err(|error| format!("server startup failed: {error}"))?;

    let ready = server.ready_message(auth_bootstrap.session_token.take());
    if args.owned {
        let line = serde_json::to_string(&ready).map_err(|error| error.to_string())?;
        println!("{line}");
    } else {
        println!("{}", ready.origin);
        if std::io::stderr().is_terminal()
            && let Some(code) = auth_bootstrap.pairing_code.take()
        {
            eprintln!("Photo Culler one-time pairing code (expires in 5 minutes): {code}");
        }
    }
    drop(ready);

    let shutdown_result = if owned {
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
