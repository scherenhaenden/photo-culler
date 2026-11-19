//! Desktop entry point: own a sibling Axum service for the window lifetime and
//! drive the Slint gallery through the shared REST controller.

use photo_culler_slint::{
    DemoWindow,
    demo::{
        controller::DemoController,
        native_transport::NativeDemoTransport,
        rest::DemoRestClient,
        slint_view::{SlintDemoView, SlintDispatcher},
    },
    desktop::owned_server::DemoServer,
};
use slint::ComponentHandle;
use std::{
    cell::RefCell,
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
    if std::env::args().any(|arg| arg == "--version" || arg == "-V") {
        println!("photo-culler-desktop {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    if let Err(error) = run_application() {
        eprintln!("photo-culler-desktop failed: {error}");
        std::process::exit(1);
    }
}

fn run_application() -> Result<(), String> {
    let window = DemoWindow::new().map_err(|error| error.to_string())?;
    write_smoke_event("desktop-starting", &std::process::id().to_string(), "");
    let endpoint = Arc::new(Mutex::new(None::<SocketAddr>));
    let client = Arc::new(AppClient::new(NativeDemoTransport::new(endpoint.clone())));
    let controller = Arc::new(DemoController::new(
        Arc::new(SlintDemoView::new(window.as_weak(), false)),
        Arc::new(SlintDispatcher),
    ));
    let server = Rc::new(RefCell::new(None));

    start_load(
        &window,
        controller.clone(),
        client.clone(),
        endpoint.clone(),
        server.clone(),
        false,
    );

    let weak_window = window.as_weak();
    let retry_controller = controller.clone();
    let retry_client = client.clone();
    let retry_endpoint = endpoint.clone();
    let retry_server = server.clone();
    window.on_retry(move || {
        if let Some(window) = weak_window.upgrade() {
            start_load(
                &window,
                retry_controller.clone(),
                retry_client.clone(),
                retry_endpoint.clone(),
                retry_server.clone(),
                true,
            );
        }
    });
    window.on_previous_photo(|| {});
    window.on_next_photo(|| {});

    let selected_window = window.as_weak();
    window.on_select_photo(move |photo_id| {
        if let Some(window) = selected_window.upgrade() {
            window.set_selected_photo_id(photo_id);
        }
    });

    let archive_controller = controller.clone();
    let archive_client = client.clone();
    window.on_select_archive(move |_| {
        let ticket = archive_controller.retry("demo");
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
    server: Rc<RefCell<Option<DemoServer>>>,
    retry: bool,
) {
    if server.borrow().is_none() {
        match DemoServer::open() {
            Ok(connection) => {
                if let Ok(mut current) = endpoint.lock() {
                    *current = Some(connection.address());
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
        controller.retry("demo")
    } else {
        controller.begin_load("demo")
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
                photo_culler_slint::demo::controller::ViewState::Ready { photo, .. } => {
                    write_smoke_event_to(
                        &status_file,
                        "gallery-ready",
                        &photo.photo_id,
                        &format!("{}\t{origin}\t{server_pid}", photo.filename),
                    );
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
