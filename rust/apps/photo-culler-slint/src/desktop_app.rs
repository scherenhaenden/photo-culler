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
    desktop::profile::{self, DesktopProfile},
    file_selection::{Intent, NativePicker, desktop_pick, native::SystemNativePicker},
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
    let command = match parse_desktop_args(std::env::args_os().skip(1)) {
        Ok(DesktopCommand::Version) => {
            println!("photo-culler-desktop {}", env!("CARGO_PKG_VERSION"));
            return;
        }
        Ok(command) => command,
        Err(error) => {
            eprintln!("photo-culler-desktop: {error}");
            std::process::exit(2);
        }
    };

    let config = match resolve_launch_config(command) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("photo-culler-desktop: {error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = run_application(config) {
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
        demo: bool,
    },
}

#[derive(Clone)]
struct LaunchConfig {
    data_dir: Option<PathBuf>,
    authorized_roots: Vec<PathBuf>,
    gallery_id: Option<String>,
    profile_file: Option<PathBuf>,
    choose_folder_first: bool,
    startup_error: Option<String>,
    folder_selection_available: bool,
}

fn resolve_launch_config(command: DesktopCommand) -> Result<LaunchConfig, String> {
    let DesktopCommand::Run {
        data_dir,
        authorized_roots,
        gallery_id,
        demo,
    } = command
    else {
        unreachable!("version command returned before launch config resolution")
    };
    let explicit =
        demo || data_dir.is_some() || !authorized_roots.is_empty() || gallery_id.is_some();
    if explicit {
        let folder_selection_available = !demo && data_dir.is_some();
        return Ok(LaunchConfig {
            data_dir,
            authorized_roots,
            gallery_id,
            profile_file: None,
            choose_folder_first: false,
            startup_error: None,
            folder_selection_available,
        });
    }

    // An explicit external API origin must not be paired with local profile
    // arguments; DemoServer rejects that ambiguous combination.
    if std::env::var_os("PHOTO_CULLER_API_ORIGIN").is_some() {
        return Ok(LaunchConfig {
            data_dir: None,
            authorized_roots: Vec::new(),
            gallery_id: None,
            profile_file: None,
            choose_folder_first: false,
            startup_error: None,
            folder_selection_available: false,
        });
    }

    let profile_dir = profile::default_profile_dir().map_err(|error| error.to_string())?;
    let profile_file = profile::profile_file_path(&profile_dir);
    let (profile, startup_error) = match profile::load(&profile_file) {
        Ok(stored) => (stored.unwrap_or_else(DesktopProfile::empty), None),
        Err(error) => (DesktopProfile::empty(), Some(error.to_string())),
    };
    let root = profile.authorized_root_path();
    Ok(LaunchConfig {
        data_dir: Some(profile_dir),
        authorized_roots: root.iter().cloned().collect(),
        gallery_id: profile.gallery_id.clone(),
        profile_file: Some(profile_file),
        choose_folder_first: root.is_none() || startup_error.is_some(),
        startup_error,
        folder_selection_available: true,
    })
}

fn parse_desktop_args<I>(arguments: I) -> Result<DesktopCommand, &'static str>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let mut data_dir = None;
    let mut authorized_roots = Vec::new();
    let mut gallery_id = None;
    let mut demo = false;
    while let Some(argument) = arguments.next() {
        if argument == OsStr::new("--version") || argument == OsStr::new("-V") {
            return Ok(DesktopCommand::Version);
        }
        if argument == OsStr::new("--demo") {
            if demo {
                return Err("--demo may only be specified once");
            }
            demo = true;
            continue;
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
            "unknown option; supported options are --demo, --data-dir, --authorized-root, and --gallery-id",
        );
    }
    if demo && (data_dir.is_some() || !authorized_roots.is_empty() || gallery_id.is_some()) {
        return Err("--demo cannot be combined with persistent catalog options");
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
        demo,
    })
}

fn is_desktop_option(value: &OsStr) -> bool {
    [
        "--data-dir",
        "--authorized-root",
        "--gallery-id",
        "--demo",
        "--version",
        "-V",
    ]
    .iter()
    .any(|option| value == OsStr::new(option))
}

fn backup_profile_file(path: &std::path::Path) -> Result<(), String> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|error| format!("could not timestamp profile backup: {error}"))?
        .as_nanos();
    let name = path
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| "desktop profile filename is not valid UTF-8".to_owned())?;
    let backup = path.with_file_name(format!("{name}.invalid-{timestamp}-{}", std::process::id()));
    std::fs::copy(path, &backup)
        .map(|_| ())
        .map_err(|error| format!("could not preserve the unreadable profile: {error}"))
}

fn run_application(initial_config: LaunchConfig) -> Result<(), String> {
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
    let launch_config = Rc::new(RefCell::new(initial_config.clone()));
    window.set_folder_selection_available(initial_config.folder_selection_available);

    let analysis_gallery_changed = analysis_flow::bind(&window, controller.clone(), client.clone());
    import_flow::bind(&window, controller.clone(), client.clone());

    if initial_config.choose_folder_first {
        let status = initial_config.startup_error.as_deref().map_or_else(
            || "Elige una carpeta para crear tu catálogo local.".to_owned(),
            |error| format!("No se pudo leer la configuración guardada ({error}). El archivo se conservará como copia antes de continuar."),
        );
        if initial_config.startup_error.is_some() {
            window.set_error_message(status.into());
            window.set_view_state(3);
        } else {
            window.set_view_state(2);
            window.set_service_status(status.into());
        }
    } else {
        start_load(
            &window,
            controller.clone(),
            client.clone(),
            endpoint.clone(),
            session_token.clone(),
            server.clone(),
            initial_config.data_dir.clone(),
            initial_config.authorized_roots.clone(),
            initial_config.gallery_id.clone(),
            initial_config.profile_file.clone(),
            initial_config.authorized_roots.first().cloned(),
            launch_config.clone(),
            false,
            false,
        );
    }
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
    let retry_config = launch_config.clone();
    window.on_retry(move || {
        if let Some(window) = weak_window.upgrade() {
            let config = retry_config.borrow().clone();
            start_load(
                &window,
                retry_controller.clone(),
                retry_client.clone(),
                retry_endpoint.clone(),
                retry_session_token.clone(),
                retry_server.clone(),
                config.data_dir,
                config.authorized_roots.clone(),
                config.gallery_id,
                config.profile_file,
                config.authorized_roots.first().cloned(),
                retry_config.clone(),
                false,
                true,
            );
        }
    });
    let choose_window = window.as_weak();
    let choose_controller = controller.clone();
    let choose_client = client.clone();
    let choose_endpoint = endpoint.clone();
    let choose_token = session_token.clone();
    let choose_server = server.clone();
    let choose_config = launch_config.clone();
    window.on_choose_folder_requested(move || {
        let picked = match desktop_pick(&mut SystemNativePicker, Intent::Source) {
            Ok(path) => path,
            Err(photo_culler_slint::file_selection::SelectionError::Cancelled) => return,
            Err(error) => {
                if let Some(window) = choose_window.upgrade() {
                    window.set_service_status(error.to_string().into());
                    window.set_error_message(error.to_string().into());
                    window.set_view_state(3);
                }
                return;
            }
        };
        let root = PathBuf::from(picked.as_str());
        if !root.is_dir() {
            if let Some(window) = choose_window.upgrade() {
                window.set_service_status("La carpeta seleccionada ya no está disponible.".into());
                window.set_error_message("La carpeta seleccionada ya no está disponible.".into());
                window.set_view_state(3);
            }
            return;
        }
        let Ok(root) = root.canonicalize() else {
            if let Some(window) = choose_window.upgrade() {
                window.set_service_status("No se pudo acceder a la carpeta seleccionada.".into());
                window.set_error_message("No se pudo acceder a la carpeta seleccionada.".into());
                window.set_view_state(3);
            }
            return;
        };
        let profile_file = choose_config.borrow().profile_file.clone();
        if let Some(path) = profile_file.as_deref() {
            if choose_config.borrow().startup_error.is_some()
                && let Err(error) = backup_profile_file(path)
            {
                if let Some(window) = choose_window.upgrade() {
                    window.set_service_status(error.into());
                    window.set_error_message(error.clone().into());
                    window.set_view_state(3);
                }
                return;
            }
        }
        let fallback_data_dir = choose_config.borrow().data_dir.clone();
        let mut config = choose_config.borrow_mut();
        config.data_dir = Some(
            profile_file
                .as_ref()
                .and_then(|path| path.parent())
                .map(PathBuf::from)
                .or(fallback_data_dir)
                .unwrap_or_else(|| PathBuf::from(".")),
        );
        config.authorized_roots = vec![root.clone()];
        config.choose_folder_first = false;
        config.startup_error = None;
        drop(config);
        choose_server.borrow_mut().take();
        if let Ok(mut endpoint) = choose_endpoint.lock() {
            *endpoint = None;
        }
        if let Ok(mut token) = choose_token.lock() {
            *token = None;
        }
        if let Some(window) = choose_window.upgrade() {
            window.set_view_state(0);
            window.set_service_status("Preparando tu catálogo…".into());
            let config = choose_config.borrow().clone();
            start_load(
                &window,
                choose_controller.clone(),
                choose_client.clone(),
                choose_endpoint.clone(),
                choose_token.clone(),
                choose_server.clone(),
                config.data_dir,
                config.authorized_roots,
                config.gallery_id,
                config.profile_file,
                Some(root),
                choose_config.clone(),
                true,
                false,
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
    profile_file: Option<PathBuf>,
    profile_root: Option<PathBuf>,
    launch_config: Rc<RefCell<LaunchConfig>>,
    open_import_after_load: bool,
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
    let profile_controller = task_controller.clone();
    let profile_file_after_load = profile_file.clone();
    let profile_root_after_load = profile_root.clone();
    let launch_config_after_load = launch_config.clone();
    let open_import_window = window.as_weak();
    let smoke_context = server.borrow().as_ref().map(|connection| {
        (
            connection.origin.clone(),
            connection
                .owned_process_id()
                .map_or_else(|| "external".to_owned(), |pid| pid.to_string()),
        )
    });
    if let Err(error) = slint::spawn_local(async move {
        let load_result = task_controller
            .execute(client.as_ref(), ticket.clone())
            .await;
        let resolved_gallery = profile_controller.active_gallery_id_for(&ticket);
        if load_result.is_ok()
            && let Some(gallery_id) = resolved_gallery.clone()
        {
            if let Ok(mut config) = launch_config_after_load.try_borrow_mut() {
                config.gallery_id = Some(gallery_id.clone());
            }
            if let (Some(path), Some(root)) = (profile_file_after_load, profile_root_after_load) {
                if let Ok(profile) = DesktopProfile::for_selection(&root, Some(&gallery_id)) {
                    if let Err(error) = profile::save(&path, &profile) {
                        eprintln!("could not persist desktop profile: {error}");
                    }
                }
            }
        }
        if open_import_after_load
            && load_result.is_ok()
            && resolved_gallery.is_some()
            && let Some(window) = open_import_window.upgrade()
        {
            window.invoke_import_open_requested();
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<DesktopCommand, &'static str> {
        parse_desktop_args(values.iter().map(OsString::from))
    }

    #[test]
    fn demo_mode_is_explicit_and_cannot_enable_persistent_import() {
        assert!(matches!(
            parse(&["--demo"]),
            Ok(DesktopCommand::Run { demo: true, .. })
        ));
        assert!(matches!(
            parse(&["--demo", "--data-dir", "/tmp/catalog"]),
            Err("--demo cannot be combined with persistent catalog options")
        ));
    }

    #[test]
    fn no_arguments_select_normal_profile_mode_instead_of_demo_mode() {
        assert!(matches!(
            parse(&[]),
            Ok(DesktopCommand::Run { demo: false, .. })
        ));
    }
}
