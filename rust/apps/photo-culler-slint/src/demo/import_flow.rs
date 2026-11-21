//! Bind the API v2 import workflow to the shared Slint dialog.
//!
//! The component receives only opaque source/job IDs and safe DTO summaries.
//! Filesystem paths stay in server-local configuration and never cross this UI.

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use slint::{ComponentHandle, Timer, TimerMode};

use super::{
    controller::DemoController,
    rest::DemoRestClient,
    slint_view::{SlintDemoView, SlintDispatcher},
};
use crate::DemoWindow;
use photo_culler_client::demo::{ImportJob, Transport};

type GalleryController = DemoController<SlintDemoView, SlintDispatcher>;

#[derive(Default)]
struct PollBusy {
    next_request_id: u64,
    active_request_id: Option<u64>,
    gallery_id: Option<String>,
}

#[derive(Default)]
struct RefreshState {
    refreshed: BTreeSet<String>,
    pending: BTreeSet<String>,
    in_flight: BTreeSet<String>,
    loading_gallery: Option<String>,
    generation: u64,
}

pub fn bind<T: Transport + 'static>(
    window: &DemoWindow,
    controller: Arc<GalleryController>,
    client: Arc<DemoRestClient<T>>,
) {
    let refresh_gallery: Rc<dyn Fn(String, String)> = {
        let controller = controller.clone();
        let client = client.clone();
        let state = Rc::new(RefCell::new(RefreshState::default()));
        Rc::new(move |gallery_id, job_id| {
            let generation = {
                let mut state = state.borrow_mut();
                if state.refreshed.contains(&job_id) || state.in_flight.contains(&job_id) {
                    return;
                }
                if state.loading_gallery.as_deref() == Some(gallery_id.as_str()) {
                    state.pending.insert(job_id);
                    return;
                }
                if controller.active_gallery_id().as_deref() != Some(gallery_id.as_str()) {
                    return;
                }
                state.generation = state.generation.wrapping_add(1);
                state.loading_gallery = Some(gallery_id.clone());
                state.pending.clear();
                state.in_flight.clear();
                state.pending.insert(job_id);
                state.generation
            };
            let controller = controller.clone();
            let client = client.clone();
            let task_state = state.clone();
            if slint::spawn_local(async move {
                loop {
                    let batch = {
                        let mut state = task_state.borrow_mut();
                        if state.generation != generation {
                            break;
                        }
                        // A queued task must not undo navigation performed
                        // before it reaches the event loop.
                        if controller.active_gallery_id().as_deref() != Some(gallery_id.as_str()) {
                            state.loading_gallery = None;
                            state.pending.clear();
                            break;
                        }
                        let batch = std::mem::take(&mut state.pending);
                        state.in_flight = batch.clone();
                        batch
                    };
                    let ticket = controller.begin_load(gallery_id.clone());
                    let result = controller.execute(client.as_ref(), ticket).await;
                    let mut state = task_state.borrow_mut();
                    if state.generation != generation {
                        break;
                    }
                    state.in_flight.clear();
                    if result.is_err()
                        || controller.active_gallery_id().as_deref() != Some(gallery_id.as_str())
                    {
                        state.loading_gallery = None;
                        state.pending.clear();
                        break;
                    }
                    state.refreshed.extend(batch);
                    if state.pending.is_empty() {
                        state.loading_gallery = None;
                        break;
                    }
                }
            })
            .is_err()
            {
                let mut state = state.borrow_mut();
                if state.generation == generation {
                    state.loading_gallery = None;
                    state.pending.clear();
                    state.in_flight.clear();
                }
            }
        })
    };
    let request_busy = Rc::new(Cell::new(false));
    let estimate_source = Rc::new(RefCell::new(None::<String>));
    let poll_timer = Rc::new(Timer::default());
    let poll_busy = Rc::new(RefCell::new(PollBusy::default()));
    let active_jobs = Rc::new(RefCell::new(Vec::<ImportJob>::new()));
    let source_names = Rc::new(RefCell::new(BTreeMap::<String, String>::new()));
    let job_inventory_known = Rc::new(Cell::new(false));
    let open_generation = Rc::new(Cell::new(0_u64));
    // A start request can outlive the dialog session that sent it.
    let pending_starts = Rc::new(Cell::new(0_u32));

    let weak = window.as_weak();
    let open_client = client.clone();
    let open_refresh = refresh_gallery.clone();
    let open_controller = controller.clone();
    let open_timer = poll_timer.clone();
    let open_poll_busy = poll_busy.clone();
    let open_active_jobs = active_jobs.clone();
    let open_source_names = source_names.clone();
    let open_inventory_known = job_inventory_known.clone();
    let open_request_generation = open_generation.clone();
    let open_request_busy = request_busy.clone();
    let open_pending_starts = pending_starts.clone();
    let open_estimate_source = estimate_source.clone();
    window.on_import_open_requested(move || {
        let Some(weak_window) = weak.upgrade() else {
            return;
        };
        let generation = open_request_generation.get().wrapping_add(1);
        open_request_generation.set(generation);
        // Requests from the previous dialog session must not keep its busy
        // gate closed after the dialog is reopened.
        open_request_busy.set(false);
        open_poll_busy.borrow_mut().active_request_id = None;
        let Some(gallery_id) = open_controller.active_gallery_id() else {
            weak_window.set_import_sources_loading(false);
            weak_window.set_import_source_error(
                "Espera a que la biblioteca local termine de cargar.".into(),
            );
            return;
        };
        open_poll_busy.borrow_mut().gallery_id = Some(gallery_id.clone());
        weak_window.set_import_sources_loading(true);
        weak_window.set_import_source_error("".into());
        weak_window.set_import_estimate_state("idle".into());
        weak_window.set_import_estimate_error("".into());
        weak_window.set_import_estimate_details("".into());
        weak_window.set_import_job_id("".into());
        weak_window.set_import_job_discovered("0".into());
        weak_window.set_import_job_imported("0".into());
        weak_window.set_import_job_issues("0".into());
        weak_window.set_import_job_error("".into());
        weak_window.set_import_active_jobs(slint::ModelRc::new(slint::VecModel::from(Vec::<
            crate::ImportJobSummary,
        >::new(
        ))));
        weak_window.set_import_job_state("unknown".into());
        *open_active_jobs.borrow_mut() = Vec::new();
        *open_source_names.borrow_mut() = BTreeMap::new();
        open_inventory_known.set(false);
        open_timer.stop();
        *open_estimate_source.borrow_mut() = None;

        let client = open_client.clone();
        let refresh = open_refresh.clone();
        let weak = weak.clone();
        let timer = open_timer.clone();
        let poll_busy = open_poll_busy.clone();
        let active_jobs = open_active_jobs.clone();
        let source_names = open_source_names.clone();
        let inventory_known = open_inventory_known.clone();
        let request_generation = open_request_generation.clone();
        let pending_starts = open_pending_starts.clone();
        let spawn_failure_window = weak.clone();
        let spawn_failure_generation = request_generation.clone();
        if slint::spawn_local(async move {
            let result = client.import_sources(&gallery_id).await;
            let active_result = client.list_active_import_jobs(100).await;
            // A different job may still be active after one finished while the
            // dialog was closed. Inspect recent history independently.
            let latest_result = client.list_import_jobs(100).await;
            let Some(window) = weak.upgrade() else {
                return;
            };
            if request_generation.get() != generation || !window.get_import_open() {
                return;
            }
            window.set_import_sources_loading(false);
            match result {
                Ok(page) => {
                    let mut names = BTreeMap::new();
                    let sources: Vec<_> = page
                        .items
                        .into_iter()
                        .map(|source| {
                            names.insert(source.id.clone(), source.name.clone());
                            crate::ImportSource {
                                id: source.id.into(),
                                name: source.name.into(),
                                status: source.status.into(),
                            }
                        })
                        .collect();
                    *source_names.borrow_mut() = names;
                    window.set_import_sources(slint::ModelRc::new(slint::VecModel::from(sources)));
                    window.set_import_source_error("".into());
                }
                Err(error) => {
                    window.set_import_sources(slint::ModelRc::new(slint::VecModel::from(Vec::<
                        crate::ImportSource,
                    >::new(
                    ))));
                    window.set_import_source_error(error.message.into());
                }
            }
            if let Ok(page) = &latest_result {
                if let Some(job) = page
                    .items
                    .iter()
                    .find(|job| job.gallery_id == gallery_id && job.state == "completed")
                {
                    refresh(job.gallery_id.clone(), job.id.clone());
                }
            }
            match active_result {
                Ok(page) => {
                    let jobs: Vec<_> = page
                        .items
                        .into_iter()
                        .filter(|job| job.gallery_id == gallery_id)
                        .collect();
                    // An older request may still create a job after this snapshot.
                    inventory_known.set(pending_starts.get() == 0);
                    *active_jobs.borrow_mut() = jobs.clone();
                    publish_active_jobs(&window, &jobs, &source_names.borrow());
                    let job = jobs.first().cloned();
                    if let Some(job) = job {
                        let poll_job = jobs
                            .iter()
                            .find(|active| matches!(active.state.as_str(), "queued" | "running"))
                            .or_else(|| jobs.first())
                            .map(|active| active.id.clone());
                        publish_selected_job(&window, &job);
                        window.set_import_selected_source_id(job.source_id.clone().into());
                        if let Some(poll_job) = poll_job {
                            start_polling(
                                timer,
                                poll_busy,
                                weak.clone(),
                                client.clone(),
                                poll_job,
                                request_generation.clone(),
                                generation,
                                active_jobs,
                                source_names,
                                refresh.clone(),
                            );
                        }
                    } else {
                        window.set_import_job_id("".into());
                        window.set_import_job_state("".into());
                        window.set_import_job_discovered("0".into());
                        window.set_import_job_imported("0".into());
                        window.set_import_job_issues("0".into());
                        window.set_import_job_error("".into());
                        match latest_result {
                            Ok(page) => {
                                if let Some(job) = page
                                    .items
                                    .into_iter()
                                    .find(|job| job.gallery_id == gallery_id)
                                {
                                    publish_selected_job(&window, &job);
                                    window.set_import_selected_source_id(job.source_id.into());
                                }
                            }
                            Err(error) => {
                                window.set_import_job_error(error.message.into());
                            }
                        }
                    }
                }
                Err(error) => {
                    inventory_known.set(false);
                    window.set_import_job_id("".into());
                    window.set_import_job_state("unknown".into());
                    window.set_import_job_error(error.message.into());
                }
            }
        })
        .is_err()
        {
            if spawn_failure_generation.get() == generation
                && let Some(window) = spawn_failure_window.upgrade()
                && window.get_import_open()
            {
                window.set_import_sources_loading(false);
                window.set_import_source_error(
                    "No se pudo iniciar la carga de fuentes y trabajos.".into(),
                );
            }
        }
    });

    let weak = window.as_weak();
    let estimate_source_state = estimate_source.clone();
    let source_active_jobs = active_jobs.clone();
    let source_names_for_jobs = source_names.clone();
    let source_job_inventory_known = job_inventory_known.clone();
    let source_timer = poll_timer.clone();
    let source_poll_busy = poll_busy.clone();
    let source_client = client.clone();
    let source_refresh = refresh_gallery.clone();
    let source_generation = open_generation.clone();
    window.on_import_source_selected(move |source_id| {
        source_poll_busy.borrow_mut().active_request_id = None;
        *estimate_source_state.borrow_mut() = None;
        if let Some(window) = weak.upgrade() {
            let source_id_string = source_id.to_string();
            window.set_import_estimate_state("idle".into());
            window.set_import_estimate_error("".into());
            window.set_import_estimate_total_files("".into());
            window.set_import_estimate_total_bytes("".into());
            window.set_import_estimate_logical_photos("".into());
            window.set_import_estimate_details("".into());
            window.set_import_selected_source_id(source_id);
            if !source_job_inventory_known.get() {
                return;
            }
            let selected_job = source_active_jobs
                .borrow()
                .iter()
                .find(|job| job.source_id == source_id_string)
                .cloned();
            if let Some(job) = selected_job {
                publish_selected_job(&window, &job);
                if matches!(job.state.as_str(), "queued" | "running" | "paused") {
                    start_polling(
                        source_timer.clone(),
                        source_poll_busy.clone(),
                        weak.clone(),
                        source_client.clone(),
                        job.id,
                        source_generation.clone(),
                        source_generation.get(),
                        source_active_jobs.clone(),
                        source_names_for_jobs.clone(),
                        source_refresh.clone(),
                    );
                } else if let Some(active) = source_active_jobs
                    .borrow()
                    .iter()
                    .find(|job| matches!(job.state.as_str(), "queued" | "running" | "paused"))
                {
                    start_polling(
                        source_timer.clone(),
                        source_poll_busy.clone(),
                        weak.clone(),
                        source_client.clone(),
                        active.id.clone(),
                        source_generation.clone(),
                        source_generation.get(),
                        source_active_jobs.clone(),
                        source_names_for_jobs.clone(),
                        source_refresh.clone(),
                    );
                } else {
                    source_timer.stop();
                }
            } else {
                if let Some(active) = source_active_jobs
                    .borrow()
                    .iter()
                    .find(|job| matches!(job.state.as_str(), "queued" | "running" | "paused"))
                {
                    start_polling(
                        source_timer.clone(),
                        source_poll_busy.clone(),
                        weak.clone(),
                        source_client.clone(),
                        active.id.clone(),
                        source_generation.clone(),
                        source_generation.get(),
                        source_active_jobs.clone(),
                        source_names_for_jobs.clone(),
                        source_refresh.clone(),
                    );
                } else {
                    source_timer.stop();
                }
                clear_selected_job(&window);
            }
        }
    });

    let weak = window.as_weak();
    let selected_jobs = active_jobs.clone();
    let selected_names = source_names.clone();
    let selected_timer = poll_timer.clone();
    let selected_poll_busy = poll_busy.clone();
    let selected_client = client.clone();
    let selected_refresh = refresh_gallery.clone();
    let selected_estimate = estimate_source.clone();
    let selected_generation = open_generation.clone();
    window.on_import_job_selected(move |job_id| {
        selected_poll_busy.borrow_mut().active_request_id = None;
        let job_id = job_id.to_string();
        let selected_job = selected_jobs
            .borrow()
            .iter()
            .find(|job| job.id == job_id)
            .cloned();
        let Some(job) = selected_job else {
            return;
        };
        let Some(window) = weak.upgrade() else {
            return;
        };
        *selected_estimate.borrow_mut() = None;
        window.set_import_estimate_state("idle".into());
        window.set_import_estimate_error("".into());
        window.set_import_estimate_total_files("".into());
        window.set_import_estimate_total_bytes("".into());
        window.set_import_estimate_logical_photos("".into());
        window.set_import_estimate_details("".into());
        publish_selected_job(&window, &job);
        window.set_import_selected_source_id(job.source_id.clone().into());
        if matches!(job.state.as_str(), "queued" | "running" | "paused") {
            start_polling(
                selected_timer.clone(),
                selected_poll_busy.clone(),
                weak.clone(),
                selected_client.clone(),
                job.id,
                selected_generation.clone(),
                selected_generation.get(),
                selected_jobs.clone(),
                selected_names.clone(),
                selected_refresh.clone(),
            );
        } else if let Some(active) = selected_jobs
            .borrow()
            .iter()
            .find(|job| matches!(job.state.as_str(), "queued" | "running" | "paused"))
        {
            start_polling(
                selected_timer.clone(),
                selected_poll_busy.clone(),
                weak.clone(),
                selected_client.clone(),
                active.id.clone(),
                selected_generation.clone(),
                selected_generation.get(),
                selected_jobs.clone(),
                selected_names.clone(),
                selected_refresh.clone(),
            );
        } else {
            selected_timer.stop();
        }
    });

    let weak = window.as_weak();
    let estimate_client = client.clone();
    let estimate_busy = request_busy.clone();
    let estimate_source_state = estimate_source.clone();
    let estimate_generation = open_generation.clone();
    window.on_import_estimate_requested(move |source_id| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if source_id.is_empty() || window.get_import_selected_source_id() != source_id {
            return;
        }
        if estimate_busy.replace(true) {
            return;
        }
        window.set_import_estimate_state("loading".into());
        window.set_import_estimate_error("".into());
        let client = estimate_client.clone();
        let busy = estimate_busy.clone();
        let session = estimate_generation.clone();
        let generation = session.get();
        let weak = weak.clone();
        let spawn_failure_window = weak.clone();
        let spawn_failure_busy = busy.clone();
        let spawn_failure_session = session.clone();
        let estimate_source_state = estimate_source_state.clone();
        if slint::spawn_local(async move {
            let result = client.estimate_import(&source_id, true, &[]).await;
            if session.get() != generation {
                return;
            }
            busy.set(false);
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !window.get_import_open() || window.get_import_selected_source_id() != source_id {
                return;
            }
            match result {
                Ok(estimate) => {
                    *estimate_source_state.borrow_mut() = Some(source_id.to_string());
                    window.set_import_estimate_total_files(estimate.total_files.to_string().into());
                    window.set_import_estimate_total_bytes(estimate.total_bytes.to_string().into());
                    window.set_import_estimate_logical_photos(
                        estimate.logical_photos.to_string().into(),
                    );
                    let details = estimate
                        .extensions
                        .iter()
                        .take(8)
                        .map(|(extension, count)| format!("{extension} {count}"))
                        .collect::<Vec<_>>()
                        .join(" · ");
                    window.set_import_estimate_details(details.into());
                    window.set_import_estimate_state("ready".into());
                }
                Err(error) => {
                    *estimate_source_state.borrow_mut() = None;
                    window.set_import_estimate_state("error".into());
                    window.set_import_estimate_error(error.message.into());
                }
            }
        })
        .is_err()
        {
            spawn_failure_busy.set(false);
            if spawn_failure_session.get() == generation
                && let Some(window) = spawn_failure_window.upgrade()
                && window.get_import_open()
            {
                window.set_import_estimate_state("error".into());
                window.set_import_estimate_error(
                    "No se pudo iniciar la estimación de importación.".into(),
                );
            }
        }
    });

    let weak = window.as_weak();
    let start_client = client.clone();
    let start_refresh = refresh_gallery.clone();
    let start_controller = controller.clone();
    let start_busy = request_busy.clone();
    let start_estimate = estimate_source.clone();
    let start_timer = poll_timer.clone();
    let start_poll_busy = poll_busy.clone();
    let start_active_jobs = active_jobs.clone();
    let start_source_names = source_names.clone();
    let start_inventory_known = job_inventory_known.clone();
    let start_generation = open_generation.clone();
    let start_pending_starts = pending_starts.clone();
    window.on_import_start_requested(move |source_id| {
        let Some(window) = weak.upgrade() else {
            return;
        };
        if source_id.is_empty()
            || !start_inventory_known.get()
            || start_estimate.borrow().as_deref() != Some(source_id.as_str())
            || window.get_import_selected_source_id() != source_id
        {
            return;
        }
        let Some(gallery_id) = start_controller.active_gallery_id() else {
            window.set_import_job_error("No hay una biblioteca activa para importar.".into());
            return;
        };
        if start_busy.replace(true) {
            return;
        }
        start_pending_starts.set(start_pending_starts.get() + 1);
        window.set_import_job_id("".into());
        window.set_import_job_error("".into());
        window.set_import_job_state("queued".into());
        window.set_import_job_discovered("0".into());
        window.set_import_job_imported("0".into());
        window.set_import_job_issues("0".into());
        let client = start_client.clone();
        let refresh = start_refresh.clone();
        let busy = start_busy.clone();
        let session = start_generation.clone();
        let generation = session.get();
        let weak = weak.clone();
        let timer = start_timer.clone();
        let poll_busy = start_poll_busy.clone();
        let start_active_jobs = start_active_jobs.clone();
        let start_source_names = start_source_names.clone();
        let pending = start_pending_starts.clone();
        let spawn_pending = pending.clone();
        if slint::spawn_local(async move {
            let result = client
                .start_import(&gallery_id, &source_id, true, &[])
                .await;
            pending.set(pending.get().saturating_sub(1));
            if session.get() != generation {
                // Reload the current session after the receipt: its earlier
                // inventory snapshot may predate creation of this job.
                if let Some(window) = weak.upgrade() {
                    if window.get_import_open() {
                        window.invoke_import_open_requested();
                    }
                }
                return;
            }
            busy.set(false);
            let Some(window) = weak.upgrade() else {
                return;
            };
            if !window.get_import_open()
                || window.get_import_selected_source_id().as_str() != source_id.as_str()
            {
                return;
            }
            match result {
                Ok(receipt) => {
                    window.set_import_job_id(receipt.job_id.clone().into());
                    window.set_import_job_state(receipt.state.into());
                    window.set_import_selected_source_id(source_id.into());
                    poll_once(
                        weak.clone(),
                        client.clone(),
                        receipt.job_id.clone(),
                        poll_busy.clone(),
                        Rc::downgrade(&timer),
                        session.clone(),
                        generation,
                        start_active_jobs.clone(),
                        start_source_names.clone(),
                        refresh.clone(),
                    );
                    start_polling(
                        timer,
                        poll_busy,
                        weak.clone(),
                        client.clone(),
                        receipt.job_id,
                        session.clone(),
                        generation,
                        start_active_jobs.clone(),
                        start_source_names.clone(),
                        refresh.clone(),
                    );
                }
                Err(error) => {
                    window.set_import_job_state("".into());
                    window.set_import_job_error(error.message.into());
                }
            }
        })
        .is_err()
        {
            spawn_pending.set(spawn_pending.get().saturating_sub(1));
            start_busy.set(false);
            window.set_import_job_state("".into());
            window.set_import_job_error("No se pudo iniciar la solicitud de importación.".into());
        }
    });

    bind_control(
        window,
        "pause",
        client.clone(),
        poll_timer.clone(),
        poll_busy.clone(),
        request_busy.clone(),
        open_generation.clone(),
        active_jobs.clone(),
        source_names.clone(),
        refresh_gallery.clone(),
    );
    bind_control(
        window,
        "resume",
        client.clone(),
        poll_timer.clone(),
        poll_busy.clone(),
        request_busy.clone(),
        open_generation.clone(),
        active_jobs.clone(),
        source_names.clone(),
        refresh_gallery.clone(),
    );
    bind_control(
        window,
        "cancel",
        client,
        poll_timer.clone(),
        poll_busy.clone(),
        request_busy.clone(),
        open_generation.clone(),
        active_jobs,
        source_names,
        refresh_gallery,
    );

    let dismissed_timer = poll_timer;
    let dismissed_generation = open_generation;
    let dismissed_request_busy = request_busy;
    let dismissed_poll_busy = poll_busy;
    window.on_import_dismissed(move || {
        dismissed_generation.set(dismissed_generation.get().wrapping_add(1));
        dismissed_request_busy.set(false);
        dismissed_poll_busy.borrow_mut().active_request_id = None;
        dismissed_timer.stop();
    });
}

fn bind_control<T: Transport + 'static>(
    window: &DemoWindow,
    action: &'static str,
    client: Arc<DemoRestClient<T>>,
    timer: Rc<Timer>,
    poll_busy: Rc<RefCell<PollBusy>>,
    request_busy: Rc<Cell<bool>>,
    open_generation: Rc<Cell<u64>>,
    active_jobs: Rc<RefCell<Vec<ImportJob>>>,
    source_names: Rc<RefCell<BTreeMap<String, String>>>,
    refresh_gallery: Rc<dyn Fn(String, String)>,
) {
    let weak = window.as_weak();
    let handler = move |job_id: slint::SharedString| {
        if job_id.is_empty() {
            return;
        }
        let weak = weak.clone();
        let client = client.clone();
        let timer = timer.clone();
        let poll_busy = poll_busy.clone();
        let request_busy = request_busy.clone();
        let session = open_generation.clone();
        let generation = session.get();
        let active_jobs = active_jobs.clone();
        let source_names = source_names.clone();
        let refresh = refresh_gallery.clone();
        let job_id = job_id.to_string();
        if request_busy.replace(true) {
            return;
        }
        let previous_state = if let Some(window) = weak.upgrade() {
            let previous_state = window.get_import_job_state();
            window.set_import_job_state(
                match action {
                    "pause" => "pausing",
                    "resume" => "resuming",
                    _ => "cancelling",
                }
                .into(),
            );
            previous_state
        } else {
            request_busy.set(false);
            return;
        };
        let spawn_failure_window = weak.clone();
        let spawn_failure_busy = request_busy.clone();
        let spawn_failure_state = previous_state.clone();
        let spawn_failure_job_id = job_id.clone();
        if slint::spawn_local(async move {
            let result = match action {
                "pause" => client
                    .pause_import_job(&job_id)
                    .await
                    .map(|_| job_id.clone()),
                "resume" => client
                    .resume_import_job(&job_id)
                    .await
                    .map(|_| job_id.clone()),
                _ => client
                    .cancel_import_job(&job_id)
                    .await
                    .map(|_| job_id.clone()),
            };
            if session.get() != generation {
                return;
            }
            request_busy.set(false);
            if let Some(window) = weak.upgrade() {
                if !window.get_import_open()
                    || window.get_import_job_id().as_str() != job_id.as_str()
                {
                    return;
                }
                match result {
                    Ok(job_id) => {
                        // A response from an earlier poll can contain a
                        // pre-control snapshot and must not stop the new timer.
                        poll_busy.borrow_mut().active_request_id = None;
                        poll_once(
                            weak.clone(),
                            client.clone(),
                            job_id.clone(),
                            poll_busy.clone(),
                            Rc::downgrade(&timer),
                            session.clone(),
                            generation,
                            active_jobs.clone(),
                            source_names.clone(),
                            refresh.clone(),
                        );
                        start_polling(
                            timer,
                            poll_busy,
                            weak.clone(),
                            client.clone(),
                            job_id,
                            session.clone(),
                            generation,
                            active_jobs,
                            source_names,
                            refresh.clone(),
                        );
                    }
                    Err(error) => {
                        window.set_import_job_state(previous_state);
                        window.set_import_job_error(error.message.into());
                    }
                }
            }
        })
        .is_err()
        {
            spawn_failure_busy.set(false);
            if let Some(window) = spawn_failure_window.upgrade()
                && window.get_import_open()
                && window.get_import_job_id().as_str() == spawn_failure_job_id.as_str()
            {
                window.set_import_job_state(spawn_failure_state);
                window
                    .set_import_job_error("No se pudo iniciar la acción sobre el trabajo.".into());
            }
        }
    };
    match action {
        "pause" => window.on_import_pause_requested(handler),
        "resume" => window.on_import_resume_requested(handler),
        _ => window.on_import_cancel_requested(handler),
    }
}

fn start_polling<T: Transport + 'static>(
    timer: Rc<Timer>,
    poll_busy: Rc<RefCell<PollBusy>>,
    weak_window: slint::Weak<DemoWindow>,
    client: Arc<DemoRestClient<T>>,
    job_id: String,
    open_generation: Rc<Cell<u64>>,
    generation: u64,
    active_jobs: Rc<RefCell<Vec<ImportJob>>>,
    source_names: Rc<RefCell<BTreeMap<String, String>>>,
    refresh_gallery: Rc<dyn Fn(String, String)>,
) {
    let weak_timer = Rc::downgrade(&timer);
    timer.start(TimerMode::Repeated, Duration::from_secs(2), move || {
        let Some(window) = weak_window.upgrade() else {
            if let Some(timer) = weak_timer.upgrade() {
                timer.stop();
            }
            return;
        };
        if !window.get_import_open() {
            if let Some(timer) = weak_timer.upgrade() {
                timer.stop();
            }
            return;
        }
        if open_generation.get() != generation {
            if let Some(timer) = weak_timer.upgrade() {
                timer.stop();
            }
            return;
        }
        poll_once(
            weak_window.clone(),
            client.clone(),
            job_id.clone(),
            poll_busy.clone(),
            weak_timer.clone(),
            open_generation.clone(),
            generation,
            active_jobs.clone(),
            source_names.clone(),
            refresh_gallery.clone(),
        );
    });
}

fn poll_once<T: Transport + 'static>(
    weak_window: slint::Weak<DemoWindow>,
    client: Arc<DemoRestClient<T>>,
    job_id: String,
    poll_busy: Rc<RefCell<PollBusy>>,
    weak_timer: std::rc::Weak<Timer>,
    open_generation: Rc<Cell<u64>>,
    generation: u64,
    active_jobs: Rc<RefCell<Vec<ImportJob>>>,
    source_names: Rc<RefCell<BTreeMap<String, String>>>,
    refresh_gallery: Rc<dyn Fn(String, String)>,
) {
    let request_id = {
        let mut state = poll_busy.borrow_mut();
        if state.active_request_id.is_some() {
            return;
        }
        state.next_request_id = state.next_request_id.wrapping_add(1);
        let request_id = state.next_request_id;
        state.active_request_id = Some(request_id);
        request_id
    };
    let request_state = poll_busy.clone();
    let poll_gallery_id = poll_busy.borrow().gallery_id.clone();
    let selected_job_id = weak_window
        .upgrade()
        .map(|window| window.get_import_job_id().to_string())
        .filter(|selected| !selected.is_empty() && selected != &job_id);
    if slint::spawn_local(async move {
        let result = client.import_job(&job_id).await;
        // Keep observing the inventory even if the previous anchor disappears
        // or its detail endpoint fails. Another job may still be active.
        let active_result = Some(client.list_active_import_jobs(100).await);
        let history_result = if matches!(&active_result, Some(Ok(_))) {
            Some(client.list_import_jobs(100).await)
        } else {
            None
        };
        // The selected paused job may differ from the running job that keeps
        // the inventory timer alive. Observe it independently so controls do
        // not keep displaying an obsolete state after an external action.
        let selected_result = if let Some(selected_id) = &selected_job_id {
            Some(client.import_job(selected_id).await)
        } else {
            None
        };
        let inventory_available =
            matches!(&active_result, Some(Ok(_))) && matches!(&history_result, Some(Ok(_)));
        let still_active = poll_busy.borrow().active_request_id == Some(request_id);
        if still_active {
            poll_busy.borrow_mut().active_request_id = None;
        }
        if !still_active {
            return;
        }
        let Some(window) = weak_window.upgrade() else {
            return;
        };
        if !window.get_import_open() || open_generation.get() != generation {
            return;
        }
        if let (Some(selected_id), Some(result)) = (&selected_job_id, selected_result) {
            if window.get_import_job_id().as_str() == selected_id.as_str() {
                match result {
                    Ok(selected) => {
                        publish_selected_job(&window, &selected);
                        if selected.state == "completed" {
                            refresh_gallery(selected.gallery_id, selected.id);
                        }
                    }
                    Err(error) => window.set_import_job_error(error.message.into()),
                }
            }
        }
        match result {
            Ok(job) => {
                let selected_gallery_id = job.gallery_id.clone();
                let selected_stopped = matches!(
                    job.state.as_str(),
                    "paused" | "completed" | "cancelled" | "failed"
                );
                let terminal = matches!(job.state.as_str(), "completed" | "cancelled" | "failed");
                if window.get_import_job_id().as_str() == job_id {
                    publish_selected_job(&window, &job);
                }
                if job.state == "completed" {
                    refresh_gallery(job.gallery_id.clone(), job.id.clone());
                }
                if let Some(Ok(page)) = history_result {
                    for completed in page.items.into_iter().filter(|entry| {
                        entry.gallery_id == selected_gallery_id && entry.state == "completed"
                    }) {
                        refresh_gallery(completed.gallery_id, completed.id);
                    }
                }
                let mut other_running = false;
                if let Some(Ok(page)) = active_result {
                    let refreshed: Vec<_> = page
                        .items
                        .into_iter()
                        .filter(|active_job| active_job.gallery_id == selected_gallery_id)
                        .collect();
                    other_running = refreshed.iter().any(|active| {
                        active.id != job.id
                            && matches!(active.state.as_str(), "queued" | "running" | "paused")
                    });
                    let mut jobs = active_jobs.borrow_mut();
                    *jobs = refreshed;
                    if !terminal && !jobs.iter().any(|existing| existing.id == job.id) {
                        jobs.push(job.clone());
                    }
                    jobs.sort_by(|left, right| {
                        right
                            .updated_at
                            .cmp(&left.updated_at)
                            .then_with(|| right.id.cmp(&left.id))
                    });
                    publish_active_jobs(&window, &jobs, &source_names.borrow());
                } else if terminal {
                    let mut jobs = active_jobs.borrow_mut();
                    jobs.retain(|existing| existing.id != job.id);
                    publish_active_jobs(&window, &jobs, &source_names.borrow());
                }
                if selected_stopped
                    && job.state != "paused"
                    && !other_running
                    && inventory_available
                    && let Some(timer) = weak_timer.upgrade()
                {
                    timer.stop();
                }
            }
            Err(error) => {
                if window.get_import_job_id().as_str() == job_id {
                    window.set_import_job_error(error.message.into());
                }
                if let (Some(gallery), Some(Ok(page))) = (&poll_gallery_id, history_result) {
                    for completed in page
                        .items
                        .into_iter()
                        .filter(|entry| entry.gallery_id == *gallery && entry.state == "completed")
                    {
                        refresh_gallery(completed.gallery_id, completed.id);
                    }
                }
                // Switch to a known job from the same gallery if the old
                // anchor is absent from a successfully retrieved inventory.
                if let Some(Ok(page)) = active_result {
                    if let Some(gallery) = poll_gallery_id {
                        let jobs: Vec<_> = page
                            .items
                            .into_iter()
                            .filter(|job| job.gallery_id == gallery)
                            .collect();
                        let replacement = jobs
                            .iter()
                            .find(|job| job.id != job_id)
                            .map(|job| job.id.clone());
                        let anchor_present = jobs.iter().any(|job| job.id == job_id);
                        *active_jobs.borrow_mut() = jobs;
                        publish_active_jobs(&window, &active_jobs.borrow(), &source_names.borrow());
                        if !anchor_present {
                            if let (Some(replacement), Some(timer)) =
                                (replacement, weak_timer.upgrade())
                            {
                                start_polling(
                                    timer,
                                    poll_busy.clone(),
                                    weak_window.clone(),
                                    client.clone(),
                                    replacement,
                                    open_generation.clone(),
                                    generation,
                                    active_jobs.clone(),
                                    source_names.clone(),
                                    refresh_gallery.clone(),
                                );
                            }
                        }
                    }
                }
            }
        }
    })
    .is_err()
    {
        let still_active = request_state.borrow().active_request_id == Some(request_id);
        if still_active {
            request_state.borrow_mut().active_request_id = None;
        }
    }
}

fn publish_selected_job(window: &DemoWindow, job: &ImportJob) {
    window.set_import_job_id(job.id.clone().into());
    window.set_import_job_state(job.state.clone().into());
    window.set_import_job_discovered(job.discovered.to_string().into());
    window.set_import_job_imported(job.imported.to_string().into());
    window.set_import_job_issues(job.issues.to_string().into());
    window.set_import_job_error(job.error.clone().unwrap_or_default().into());
}

fn clear_selected_job(window: &DemoWindow) {
    window.set_import_job_id("".into());
    window.set_import_job_state("".into());
    window.set_import_job_discovered("0".into());
    window.set_import_job_imported("0".into());
    window.set_import_job_issues("0".into());
    window.set_import_job_error("".into());
}

fn publish_active_jobs(
    window: &DemoWindow,
    jobs: &[ImportJob],
    source_names: &BTreeMap<String, String>,
) {
    let summaries = jobs
        .iter()
        .map(|job| crate::ImportJobSummary {
            id: job.id.clone().into(),
            source_name: source_names
                .get(&job.source_id)
                .cloned()
                .unwrap_or_else(|| "Fuente no disponible".into())
                .into(),
            state: job.state.clone().into(),
            discovered: job.discovered.to_string().into(),
            imported: job.imported.to_string().into(),
            issues: job.issues.to_string().into(),
        })
        .collect::<Vec<_>>();
    window.set_import_active_jobs(slint::ModelRc::new(slint::VecModel::from(summaries)));
}
