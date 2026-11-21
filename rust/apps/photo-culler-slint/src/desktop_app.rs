// Desktop entry point: own a sibling Axum service for the window lifetime and
// drive the Slint gallery through the shared REST controller.

use photo_culler_client::demo::Decision;
use photo_culler_slint::{
    DemoWindow,
    demo::{
        analysis_flow,
        controller::DemoController,
        import_flow,
        native_transport::NativeDemoTransport,
        rest::DemoRestClient,
        slint_view::{SlintDemoView, SlintDispatcher},
    },
    desktop::owned_server::DemoServer,
    recipe_flow::{self, RecipePanelController},
    recipe_rest::RecipeRestClient,
};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
    ffi::{OsStr, OsString},
    fs::OpenOptions,
    io::Write,
    net::SocketAddr,
    path::PathBuf,
    rc::Rc,
    sync::{Arc, Mutex},
};

type AppController = DemoController<SlintDemoView, SlintDispatcher>;
type AppClient = DemoRestClient<NativeDemoTransport>;

fn main() {
    let (data_dir, authorized_roots, gallery_id) =
        match parse_desktop_args(std::env::args_os().skip(1)) {
            Ok(DesktopCommand::Version) => {
                println!("photo-culler-desktop {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            Ok(DesktopCommand::Run {
                data_dir,
                authorized_roots,
                gallery_id,
            }) => (data_dir, authorized_roots, gallery_id),
            Err(error) => {
                eprintln!("photo-culler-desktop: {error}");
                std::process::exit(2);
            }
        };

    if let Err(error) = run_application(data_dir, authorized_roots, gallery_id) {
        eprintln!("photo-culler-desktop failed: {error}");
        std::process::exit(1);
    }
}

enum DesktopCommand {
    Version,
    Run {
        data_dir: Option<PathBuf>,
        authorized_roots: Vec<PathBuf>,
        gallery_id: Option<String>,
    },
}

fn parse_desktop_args<I>(arguments: I) -> Result<DesktopCommand, &'static str>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let mut data_dir = None;
    let mut authorized_roots = Vec::new();
    let mut gallery_id = None;
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--version") || argument == OsStr::new("-V") {
            return Ok(DesktopCommand::Version);
        }
        if argument == OsStr::new("--data-dir") {
            if data_dir.is_some() {
                return Err("--data-dir may only be specified once");
            }
            let path = arguments.next().ok_or("--data-dir requires a path")?;
            if path.is_empty() || is_desktop_option(&path) {
                return Err("--data-dir requires a non-empty path");
            }
            data_dir = Some(PathBuf::from(path));
            continue;
        }
        if argument == OsStr::new("--authorized-root") {
            let path = arguments
                .next()
                .ok_or("--authorized-root requires a path")?;
            if path.is_empty() || is_desktop_option(&path) {
                return Err("--authorized-root requires a non-empty path");
            }
            authorized_roots.push(PathBuf::from(path));
            continue;
        }
        if argument == OsStr::new("--gallery-id") {
            if gallery_id.is_some() {
                return Err("--gallery-id may only be specified once");
            }
            let value = arguments.next().ok_or("--gallery-id requires an ID")?;
            if value.is_empty() || is_desktop_option(&value) {
                return Err("--gallery-id requires a non-empty ID");
            }
            gallery_id = Some(
                value
                    .into_string()
                    .map_err(|_| "--gallery-id must be valid UTF-8")?,
            );
            continue;
        }
        return Err(
            "unknown option; supported options are --data-dir, --authorized-root, and --gallery-id",
        );
    }
    if (!authorized_roots.is_empty() || gallery_id.is_some()) && data_dir.is_none() {
        return Err("--authorized-root and --gallery-id require --data-dir");
    }
    if gallery_id.is_some() && authorized_roots.is_empty() {
        return Err("--gallery-id requires at least one --authorized-root");
    }
    Ok(DesktopCommand::Run {
        data_dir,
        authorized_roots,
        gallery_id,
    })
}

fn is_desktop_option(value: &OsStr) -> bool {
    [
        "--data-dir",
        "--authorized-root",
        "--gallery-id",
        "--version",
        "-V",
    ]
    .iter()
    .any(|option| value == OsStr::new(option))
}

fn run_application(
    data_dir: Option<PathBuf>,
    authorized_roots: Vec<PathBuf>,
    gallery_id: Option<String>,
) -> Result<(), String> {
    let window = DemoWindow::new().map_err(|error| error.to_string())?;
    write_smoke_event("desktop-starting", &std::process::id().to_string(), "");
    let endpoint = Arc::new(Mutex::new(None::<SocketAddr>));
    let session_token = Arc::new(Mutex::new(None::<String>));
    let client = Arc::new(AppClient::new(NativeDemoTransport::new(
        endpoint.clone(),
        session_token.clone(),
    )));
    let view = Arc::new(SlintDemoView::new(window.as_weak(), false));
    let controller = Arc::new(DemoController::new(view.clone(), Arc::new(SlintDispatcher)));
    let server = Rc::new(RefCell::new(None));

    let analysis_gallery_changed = analysis_flow::bind(&window, controller.clone(), client.clone());
    import_flow::bind(&window, controller.clone(), client.clone());

    start_load(
        &window,
        controller.clone(),
        client.clone(),
        endpoint.clone(),
        session_token.clone(),
        server.clone(),
        data_dir.clone(),
        authorized_roots.clone(),
        gallery_id.clone(),
        false,
    );
    recipe_flow::bind(
        &window,
        Arc::new(RecipePanelController::new(RecipeRestClient::new(
            NativeDemoTransport::new(endpoint.clone(), session_token.clone()),
        ))),
    );

    let weak_window = window.as_weak();
    let retry_controller = controller.clone();
    let retry_client = client.clone();
    let retry_endpoint = endpoint.clone();
    let retry_session_token = session_token.clone();
    let retry_server = server.clone();
    let retry_data_dir = data_dir.clone();
    let retry_authorized_roots = authorized_roots.clone();
    let retry_gallery_id = gallery_id.clone();
    window.on_retry(move || {
        if let Some(window) = weak_window.upgrade() {
            start_load(
                &window,
                retry_controller.clone(),
                retry_client.clone(),
                retry_endpoint.clone(),
                retry_session_token.clone(),
                retry_server.clone(),
                retry_data_dir.clone(),
                retry_authorized_roots.clone(),
                retry_gallery_id.clone(),
                true,
            );
        }
    });
    let refresh_controller = controller.clone();
    let refresh_client = client.clone();
    window.on_refresh_gallery(move || {
        let Some(gallery_id) = refresh_controller.active_gallery_id() else {
            return;
        };
        let ticket = refresh_controller.begin_load(gallery_id);
        let controller = refresh_controller.clone();
        let client = refresh_client.clone();
        let _ = slint::spawn_local(async move {
            let _ = controller.execute(client.as_ref(), ticket).await;
        });
    });
    let previous_window = window.as_weak();
    let previous_controller = controller.clone();
    let previous_client = client.clone();
    window.on_previous_photo(move || {
        let controller = previous_controller.clone();
        let client = previous_client.clone();
        let weak_window = previous_window.clone();
        let _ = slint::spawn_local(async move {
            if let Err(error) = controller.previous_photo(client.as_ref()).await
                && let Some(window) = weak_window.upgrade()
            {
                window.set_navigation_error(error.message.into());
                window.set_service_status("No se pudo cargar la página; puedes reintentar.".into());
            }
        });
    });

    let next_window = window.as_weak();
    let next_controller = controller.clone();
    let next_client = client.clone();
    window.on_next_photo(move || {
        let controller = next_controller.clone();
        let client = next_client.clone();
        let weak_window = next_window.clone();
        let _ = slint::spawn_local(async move {
            if let Err(error) = controller.next_photo(client.as_ref()).await
                && let Some(window) = weak_window.upgrade()
            {
                window.set_navigation_error(error.message.into());
                window.set_service_status("No se pudo cargar la página; puedes reintentar.".into());
            }
        });
    });

    let selected_window = window.as_weak();
    let select_controller = controller.clone();
    window.on_select_photo(move |photo_id| {
        let _ = select_controller.select_photo(photo_id.as_str());
        if let Some(window) = selected_window.upgrade() {
            window.set_selected_photo_id(photo_id.clone());
            window.invoke_recipe_photo_selected(photo_id);
        }
    });

    let decision_controller = controller.clone();
    let decision_client = client.clone();
    let decision_view = view.clone();
    let decision_window = window.as_weak();
    let decision_busy = Arc::new(Mutex::new(false));
    window.on_decision_requested(move |value| {
        let decision = match value.as_str() {
            "keep" => Decision::Keep,
            "alt" => Decision::Alt,
            "reject" => Decision::Reject,
            "undecided" => Decision::Undecided,
            _ => return,
        };
        let photo_ids = decision_view.selected_photo_ids();
        if photo_ids.is_empty() {
            return;
        }
        let Ok(mut busy) = decision_busy.lock() else {
            return;
        };
        if *busy {
            return;
        }
        *busy = true;
        drop(busy);
        if let Some(window) = decision_window.upgrade() {
            window.set_service_status("Guardando decisiones…".into());
        }
        let controller = decision_controller.clone();
        let client = decision_client.clone();
        let busy = decision_busy.clone();
        let weak = decision_window.clone();
        if slint::spawn_local(async move {
            let result = controller
                .update_decisions(client.as_ref(), &photo_ids, decision)
                .await;
            if let Ok(mut pending) = busy.lock() {
                *pending = false;
            }
            if let Some(window) = weak.upgrade() {
                match result {
                    Ok(()) => window.set_service_status("Decisiones guardadas".into()),
                    Err(error) => {
                        window.set_service_status(format!("No se guardó: {}", error.message).into())
                    }
                }
            }
        })
        .is_err()
        {
            if let Ok(mut pending) = decision_busy.lock() {
                *pending = false;
            }
            if let Some(window) = decision_window.upgrade() {
                window.set_service_status("No se pudo iniciar el guardado de decisiones.".into());
            }
        }
    });

    let archive_controller = controller.clone();
    let archive_client = client.clone();
    let archive_analysis_gallery_changed = analysis_gallery_changed.clone();
    window.on_select_archive(move |gallery_id| {
        archive_analysis_gallery_changed(gallery_id.to_string());
        let ticket = archive_controller.retry(gallery_id.to_string());
        let controller = archive_controller.clone();
        let client = archive_client.clone();
        let _ = slint::spawn_local(async move {
            let _ = controller.execute(client.as_ref(), ticket).await;
        });
    });

    let result = window.run().map_err(|error| error.to_string());
    drop(window);
    drop(server);
    result
}

fn start_load(
    window: &DemoWindow,
    controller: Arc<AppController>,
    client: Arc<AppClient>,
    endpoint: Arc<Mutex<Option<SocketAddr>>>,
    session_token: Arc<Mutex<Option<String>>>,
    server: Rc<RefCell<Option<DemoServer>>>,
    data_dir: Option<PathBuf>,
    authorized_roots: Vec<PathBuf>,
    gallery_id: Option<String>,
    retry: bool,
) {
    if server.borrow().is_none() {
        match DemoServer::open(
            data_dir.as_deref(),
            &authorized_roots,
            gallery_id.as_deref(),
        ) {
            Ok(connection) => {
                if let Ok(mut current) = endpoint.lock() {
                    *current = Some(connection.address());
                }
                if let Ok(mut current) = session_token.lock() {
                    *current = connection.session_token.clone();
                }
                write_smoke_event(
                    "server-ready",
                    &connection.origin,
                    &connection
                        .owned_process_id()
                        .map_or_else(|| "external".to_owned(), |pid| pid.to_string()),
                );
                *server.borrow_mut() = Some(connection);
            }
            Err(error) => {
                write_smoke_event("gallery-error", &error, "");
                window.set_error_message(error.into());
                window.set_service_status("Servicio local no disponible".into());
                window.set_view_state(3);
                return;
            }
        }
    }

    let ticket = if retry {
        controller.retry("active")
    } else {
        controller.begin_load("active")
    };
    let task_controller = controller.clone();
    let smoke_context = server.borrow().as_ref().map(|connection| {
        (
            connection.origin.clone(),
            connection
                .owned_process_id()
                .map_or_else(|| "external".to_owned(), |pid| pid.to_string()),
        )
    });
    if let Err(error) = slint::spawn_local(async move {
        let _ = task_controller.execute(client.as_ref(), ticket).await;
        if let (Some(status_file), Some((origin, server_pid))) =
            (smoke_status_file(), smoke_context)
        {
            let controller = task_controller.clone();
            let _ = slint::invoke_from_event_loop(move || match controller.snapshot() {
                photo_culler_slint::demo::controller::ViewState::Ready {
                    photos,
                    selected_index,
                    ..
                } => {
                    if let Some(loaded) = photos.get(selected_index) {
                        write_smoke_event_to(
                            &status_file,
                            "gallery-ready",
                            &loaded.photo.photo_id,
                            &format!("{}\t{origin}\t{server_pid}", loaded.photo.filename),
                        );
                    }
                }
                photo_culler_slint::demo::controller::ViewState::Empty => {
                    write_smoke_event_to(&status_file, "gallery-empty", "", "");
                }
                photo_culler_slint::demo::controller::ViewState::Error { message } => {
                    write_smoke_event_to(&status_file, "gallery-error", &message, "");
                }
                photo_culler_slint::demo::controller::ViewState::Loading => {
                    write_smoke_event_to(&status_file, "gallery-loading", "", "");
                }
            });
        }
    }) {
        write_smoke_event("gallery-error", &error.to_string(), "");
        window.set_error_message(format!("Could not start the gallery request: {error}").into());
        window.set_view_state(3);
    }
}

fn smoke_status_file() -> Option<PathBuf> {
    std::env::var_os("PHOTO_CULLER_SMOKE_STATUS_FILE").map(PathBuf::from)
}

fn write_smoke_event(event: &str, first: &str, second: &str) {
    let Some(path) = smoke_status_file() else {
        return;
    };
    write_smoke_event_to(&path, event, first, second);
}

fn write_smoke_event_to(path: &std::path::Path, event: &str, first: &str, second: &str) {
    match OpenOptions::new().create(true).append(true).open(path) {
        Ok(mut file) => {
            if let Err(error) = writeln!(file, "{event}\t{first}\t{second}") {
                eprintln!("could not record native smoke event: {error}");
            }
        }
        Err(error) => eprintln!("could not open native smoke status file: {error}"),
    }
}
