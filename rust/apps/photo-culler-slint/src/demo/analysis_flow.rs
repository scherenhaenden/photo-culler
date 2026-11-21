//! User-triggered D25 analysis controls for the selected catalog gallery.
//!
//! One durable job is tracked per gallery. Active work is refreshed on a
//! bounded cadence; polling stops on terminal states, a paused state, or when
//! the user navigates to another gallery. A transient transport error retains
//! the same job ID and retries its authoritative snapshot.

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use photo_culler_client::demo::{AnalysisJob, Transport};
use slint::{ComponentHandle, Timer, TimerMode};

use super::{
    controller::DemoController,
    rest::DemoRestClient,
    slint_view::{SlintDemoView, SlintDispatcher},
};
use crate::DemoWindow;

static NEXT_JOB_ID: AtomicU64 = AtomicU64::new(1);

type AppController = DemoController<SlintDemoView, SlintDispatcher>;
type JobMap = Rc<RefCell<HashMap<String, (String, String)>>>;

pub fn bind<T: Transport + 'static>(
    window: &DemoWindow,
    controller: Arc<AppController>,
    client: Arc<DemoRestClient<T>>,
) -> Rc<dyn Fn(String)> {
    let tracked_jobs: JobMap = Rc::new(RefCell::new(HashMap::new()));
    let busy = Rc::new(Cell::new(false));
    let timer = Rc::new(Timer::default());

    let weak_window = window.as_weak();
    let request_controller = controller.clone();
    let request_client = client.clone();
    let request_jobs = tracked_jobs.clone();
    let request_busy = busy.clone();
    let request_timer = timer.clone();
    window.on_analysis_requested(move || {
        if request_busy.replace(true) {
            return;
        }
        request_timer.stop();
        let Some(window) = weak_window.upgrade() else {
            request_busy.set(false);
            return;
        };
        let Some(gallery_id) = request_controller.active_gallery_id() else {
            window.set_analysis_state("".into());
            window.set_analysis_status(
                "Carga primero una galería persistente con una raíz autorizada.".into(),
            );
            request_busy.set(false);
            return;
        };
        let known_job = request_jobs.borrow().get(&gallery_id).cloned();
        window.set_analysis_busy(true);
        window.set_analysis_status("Preparando o consultando el análisis técnico…".into());

        let client = request_client.clone();
        let jobs = request_jobs.clone();
        let busy = request_busy.clone();
        let timer = request_timer.clone();
        let controller = request_controller.clone();
        let weak = window.as_weak();
        let weak_on_spawn_error = weak.clone();
        if let Err(error) = slint::spawn_local(async move {
            let result = match known_job {
                Some((catalog_id, job_id)) => match client.analysis_job(&catalog_id, &job_id).await
                {
                    Ok(job) if job.state == "queued" => {
                        client.start_analysis_job(&catalog_id, &job_id).await
                    }
                    other => other,
                },
                None => match client.active_analysis_job(&gallery_id).await {
                    Ok(Some(job)) => {
                        let catalog_id = job.catalog_id.clone();
                        let job_id = job.job_id.clone();
                        jobs.borrow_mut().insert(
                            gallery_id.clone(),
                            (catalog_id.clone(), job_id.clone()),
                        );
                        if job.state == "queued" {
                            client.start_analysis_job(&catalog_id, &job_id).await
                        } else {
                            Ok(job)
                        }
                    }
                    Ok(None) => {
                        let job_id = next_job_id();
                        match client
                            .submit_analysis_job(&gallery_id, &job_id, "technical")
                            .await
                        {
                            Ok(job) => {
                                jobs.borrow_mut().insert(
                                    gallery_id.clone(),
                                    (job.catalog_id.clone(), job.job_id.clone()),
                                );
                                client
                                    .start_analysis_job(&job.catalog_id, &job.job_id)
                                    .await
                            }
                            Err(error) => Err(error),
                        }
                    }
                    Err(error) => Err(error),
                },
            };
            busy.set(false);
            let Some(window) = weak.upgrade() else {
                return;
            };
            window.set_analysis_busy(false);
            if controller.active_gallery_id().as_deref() != Some(gallery_id.as_str()) {
                window.set_analysis_state("".into());
                window.set_analysis_status("".into());
                return;
            }
            match result {
                Ok(job) => {
                    publish_job(&window, &job);
                    if should_poll(&job.state)
                        && controller.active_gallery_id().as_deref() == Some(gallery_id.as_str())
                    {
                        start_polling(
                            timer,
                            weak.clone(),
                            controller,
                            client,
                            busy,
                            gallery_id,
                            job.catalog_id,
                            job.job_id,
                        );
                    }
                }
                Err(error) => {
                    let tracked = jobs.borrow().get(&gallery_id).cloned();
                    if let Some((catalog_id, job_id)) = tracked {
                        window.set_analysis_state("reconnecting".into());
                        window.set_analysis_status(
                            format!(
                                "No se pudo consultar el análisis: {}. Conservando el trabajo y reintentando en 2 s.",
                                error.message
                            )
                            .into(),
                        );
                        start_polling(
                            timer,
                            weak.clone(),
                            controller,
                            client,
                            busy,
                            gallery_id,
                            catalog_id,
                            job_id,
                        );
                    } else {
                        window.set_analysis_state("error".into());
                        window.set_analysis_status(
                            format!("No se pudo ejecutar el análisis: {}", error.message).into(),
                        );
                    }
                }
            }
        }) {
            request_busy.set(false);
            if let Some(window) = weak_on_spawn_error.upgrade() {
                window.set_analysis_busy(false);
                window.set_analysis_state("error".into());
                window.set_analysis_status(
                    format!("No se pudo programar el análisis: {error}").into(),
                );
            }
        }
    });

    bind_action(
        window,
        controller.clone(),
        client.clone(),
        tracked_jobs.clone(),
        busy.clone(),
        timer.clone(),
        AnalysisAction::Pause,
    );
    bind_action(
        window,
        controller.clone(),
        client.clone(),
        tracked_jobs.clone(),
        busy.clone(),
        timer.clone(),
        AnalysisAction::Resume,
    );
    bind_action(
        window,
        controller,
        client,
        tracked_jobs,
        busy,
        timer.clone(),
        AnalysisAction::Cancel,
    );

    let gallery_change_timer = timer.clone();
    let weak_window = window.as_weak();
    Rc::new(move |_gallery_id| {
        gallery_change_timer.stop();
        if let Some(window) = weak_window.upgrade() {
            window.set_analysis_state("".into());
            window.set_analysis_status("".into());
            window.set_analysis_busy(false);
        }
    })
}

#[derive(Clone, Copy)]
enum AnalysisAction {
    Pause,
    Resume,
    Cancel,
}

impl AnalysisAction {
    async fn send<T: Transport>(
        self,
        client: &DemoRestClient<T>,
        catalog_id: &str,
        job_id: &str,
    ) -> Result<AnalysisJob, super::controller::DemoError> {
        match self {
            Self::Pause => client.pause_analysis_job(catalog_id, job_id).await,
            Self::Resume => client.resume_analysis_job(catalog_id, job_id).await,
            Self::Cancel => client.cancel_analysis_job(catalog_id, job_id).await,
        }
    }
}

fn bind_action<T: Transport + 'static>(
    window: &DemoWindow,
    controller: Arc<AppController>,
    client: Arc<DemoRestClient<T>>,
    jobs: JobMap,
    busy: Rc<Cell<bool>>,
    timer: Rc<Timer>,
    action: AnalysisAction,
) {
    let weak = window.as_weak();
    let spawn_error_weak = weak.clone();
    let callback: Box<dyn Fn()> = Box::new(move || {
        if busy.replace(true) {
            return;
        }
        let Some(window) = weak.upgrade() else {
            busy.set(false);
            return;
        };
        let Some(gallery_id) = controller.active_gallery_id() else {
            busy.set(false);
            return;
        };
        let Some((catalog_id, job_id)) = jobs.borrow().get(&gallery_id).cloned() else {
            busy.set(false);
            window.set_analysis_status("No hay un trabajo de análisis para esta galería.".into());
            return;
        };
        window.set_analysis_busy(true);
        let client = client.clone();
        let controller = controller.clone();
        let busy = busy.clone();
        let busy_on_spawn_error = busy.clone();
        let timer = timer.clone();
        let weak = window.as_weak();
        let spawn_error_weak = spawn_error_weak.clone();
        if let Err(error) = slint::spawn_local(async move {
            match action.send(&client, &catalog_id, &job_id).await {
                Ok(job) => {
                    busy.set(false);
                    if let Some(window) = weak.upgrade() {
                        window.set_analysis_busy(false);
                        if controller.active_gallery_id().as_deref() == Some(gallery_id.as_str()) {
                            publish_job(&window, &job);
                        } else {
                            window.set_analysis_state("".into());
                            window.set_analysis_status("".into());
                        }
                    }
                    timer.stop();
                    if should_poll(&job.state)
                        && controller.active_gallery_id().as_deref() == Some(gallery_id.as_str())
                    {
                        start_polling(
                            timer,
                            weak.clone(),
                            controller,
                            client,
                            busy,
                            gallery_id,
                            job.catalog_id,
                            job.job_id,
                        );
                    }
                }
                Err(error) => {
                    busy.set(false);
                    if let Some(window) = weak.upgrade() {
                        window.set_analysis_busy(false);
                        window.set_analysis_state("reconnecting".into());
                        window.set_analysis_status(
                            format!(
                                "No se pudo confirmar la acción: {}. Consultando el mismo trabajo antes de permitir otra acción.",
                                error.message
                            )
                            .into(),
                        );
                    }
                    // An HTTP timeout can mean that the server accepted the
                    // action but its response was lost. Query the same job ID
                    // instead of resending the action.
                    start_polling(
                        timer,
                        weak.clone(),
                        controller,
                        client,
                        busy,
                        gallery_id,
                        catalog_id,
                        job_id,
                    );
                }
            }
        }) {
            busy_on_spawn_error.set(false);
            if let Some(window) = spawn_error_weak.upgrade() {
                window.set_analysis_busy(false);
                window.set_analysis_status(
                    format!("No se pudo programar el control: {error}").into(),
                );
            }
        }
    });
    match action {
        AnalysisAction::Pause => window.on_analysis_pause_requested(move || callback()),
        AnalysisAction::Resume => window.on_analysis_resume_requested(move || callback()),
        AnalysisAction::Cancel => window.on_analysis_cancel_requested(move || callback()),
    }
}

fn start_polling<T: Transport + 'static>(
    timer: Rc<Timer>,
    weak_window: slint::Weak<DemoWindow>,
    controller: Arc<AppController>,
    client: Arc<DemoRestClient<T>>,
    busy: Rc<Cell<bool>>,
    gallery_id: String,
    catalog_id: String,
    job_id: String,
) {
    let weak_timer = Rc::downgrade(&timer);
    timer.start(TimerMode::Repeated, Duration::from_secs(2), move || {
        if weak_window.upgrade().is_none() {
            if let Some(timer) = weak_timer.upgrade() {
                timer.stop();
            }
            return;
        };
        if controller.active_gallery_id().as_deref() != Some(gallery_id.as_str()) {
            if let Some(window) = weak_window.upgrade() {
                window.set_analysis_state("".into());
                window.set_analysis_status("".into());
            }
            if let Some(timer) = weak_timer.upgrade() {
                timer.stop();
            }
            return;
        }
        if busy.replace(true) {
            return;
        }

        let client = client.clone();
        let controller = controller.clone();
        let busy = busy.clone();
        let busy_on_spawn_error = busy.clone();
        let weak = weak_window.clone();
        let timer_weak = weak_timer.clone();
        let gallery_id = gallery_id.clone();
        let catalog_id = catalog_id.clone();
        let job_id = job_id.clone();
        if slint::spawn_local(async move {
            let result = client.analysis_job(&catalog_id, &job_id).await;
            busy.set(false);
            match result {
                Ok(job) => {
                    let selected_gallery =
                        controller.active_gallery_id().as_deref() == Some(gallery_id.as_str());
                    if let Some(window) = weak.upgrade() {
                        if selected_gallery {
                            publish_job(&window, &job);
                        } else {
                            window.set_analysis_state("".into());
                            window.set_analysis_status("".into());
                        }
                    }
                    if (!should_poll(&job.state) || !selected_gallery)
                        && let Some(timer) = timer_weak.upgrade()
                    {
                        timer.stop();
                    }
                }
                Err(error) => {
                    if let Some(window) = weak.upgrade() {
                        window.set_analysis_state("reconnecting".into());
                        window.set_analysis_status(
                            format!(
                                "No se pudo actualizar el análisis: {}. Se conserva el mismo trabajo y se volverá a consultar en 2 s.",
                                error.message
                            )
                            .into(),
                        );
                    }
                    // Keep the repeated timer active. It asks for the same
                    // catalog/job snapshot and never submits a replacement.
                }
            }
            // Do not leave a completed job's timer alive if navigation changed
            // while the request was in flight.
            if controller.active_gallery_id().as_deref() != Some(gallery_id.as_str())
                && let Some(timer) = timer_weak.upgrade()
            {
                timer.stop();
            }
        })
        .is_err()
        {
            busy_on_spawn_error.set(false);
            if let Some(window) = weak_window.upgrade() {
                window.set_analysis_state("error".into());
                window.set_analysis_status(
                    "No se pudo programar la consulta de progreso. Pulsa Procesamiento para reintentar.".into(),
                );
            }
            if let Some(timer) = weak_timer.upgrade() {
                timer.stop();
            }
        }
    });
}

fn publish_job(window: &DemoWindow, job: &AnalysisJob) {
    window.set_analysis_state(job.state.clone().into());
    window.set_analysis_status(format_job(job).into());
}

fn should_poll(state: &str) -> bool {
    matches!(state, "queued" | "running")
}

fn format_job(job: &AnalysisJob) -> String {
    let total = if job.total == 0 {
        "sin fotos procesables".to_owned()
    } else {
        format!("{} de {} fotos", job.completed, job.total)
    };
    format!(
        "Análisis {} · {}% · {} · {} fallidas",
        job.state, job.percent, total, job.failed
    )
}

fn next_job_id() -> String {
    let stamp = clock_millis();
    let sequence = NEXT_JOB_ID.fetch_add(1, Ordering::Relaxed);
    format!("analysis-{stamp}-{sequence}")
}

#[cfg(not(target_arch = "wasm32"))]
fn clock_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(target_arch = "wasm32")]
fn clock_millis() -> u128 {
    js_sys::Date::now().max(0.0) as u128
}
