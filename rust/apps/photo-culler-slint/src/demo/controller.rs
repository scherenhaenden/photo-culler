//! Demo presentation controller. Platform code adapts [`UiDispatcher`] to
//! `slint::invoke_from_event_loop`; network work runs before publication.

use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};

use photo_culler_client::demo::{CatalogCursor, CatalogSummary, Decision};

pub type DemoFuture<'a> = Pin<Box<dyn Future<Output = Result<DemoGallery, DemoError>> + 'a>>;
pub type SummaryFuture<'a> = Pin<Box<dyn Future<Output = Result<CatalogSummary, DemoError>> + 'a>>;
pub type ThumbnailFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<u8>, DemoError>> + 'a>>;
pub type DecisionFuture<'a> = Pin<Box<dyn Future<Output = Result<u64, DemoError>> + 'a>>;
pub trait DemoClient {
    fn gallery<'a>(
        &'a self,
        gallery_id: &'a str,
        cursor: Option<&'a CatalogCursor>,
        decision_filter: Option<&'a str>,
    ) -> DemoFuture<'a>;
    fn summary<'a>(&'a self, gallery_id: &'a str) -> SummaryFuture<'a>;
    fn thumbnail<'a>(&'a self, photo: &'a DemoPhoto) -> ThumbnailFuture<'a>;
    fn update_decision<'a>(
        &'a self,
        photo: &'a DemoPhoto,
        decision: Decision,
        request_id: &'a str,
    ) -> DecisionFuture<'a>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoPhoto {
    pub photo_id: String,
    pub filename: String,
    pub decision: String,
    pub thumbnail_url: String,
    pub decision_revision: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoGallery {
    pub gallery_id: String,
    pub photos: Vec<DemoPhoto>,
    pub next_cursor: Option<CatalogCursor>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedPhoto {
    pub photo: DemoPhoto,
    pub jpeg: Option<Vec<u8>>,
    pub preview_error: Option<String>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DemoError {
    pub message: String,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViewState {
    Loading,
    Ready {
        gallery_id: String,
        photos: Vec<LoadedPhoto>,
        selected_index: usize,
        page_offset: usize,
        page_number: usize,
        next_cursor: Option<CatalogCursor>,
    },
    Empty,
    Error {
        message: String,
    },
}

pub trait DemoView: Send + Sync + 'static {
    fn set_state(&self, state: ViewState);

    /// Supply confirmed whole-gallery decision counts. `None` means no
    /// summary has been confirmed for the currently displayed gallery yet.
    fn set_global_summary(&self, _summary: Option<CatalogSummary>) {}

    /// Report an edge-page request failure without discarding the current page.
    /// A user can retry the same navigation action after seeing the message.
    fn set_navigation_error(&self, _message: String) {}
}
/// Dispatches a closure onto the Slint event loop. Implementations must never
/// run it inline on a worker thread.
pub trait UiDispatcher: Send + Sync + 'static {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadTicket {
    generation: u64,
    gallery_id: String,
}
struct State {
    generation: u64,
    current: ViewState,
    active_gallery_id: Option<String>,
    page_history: Vec<PageStart>,
    current_page_index: usize,
    decision_filter: Option<String>,
    summary_gallery_id: Option<String>,
    summary: Option<CatalogSummary>,
    // Confirmed writes must survive a concurrent page load whose response was
    // produced before the write completed.
    confirmed_decisions: std::collections::HashMap<(String, String), (u64, String)>,
}

#[derive(Clone, Debug)]
struct PageStart {
    cursor: Option<CatalogCursor>,
    offset: usize,
}

enum PageTransition {
    Reset,
    Append(PageStart),
    MoveBack(usize),
}

pub struct DemoController<V, D> {
    state: Arc<Mutex<State>>,
    view: Arc<V>,
    dispatcher: Arc<D>,
}

impl<V: DemoView, D: UiDispatcher> DemoController<V, D> {
    pub fn new(view: Arc<V>, dispatcher: Arc<D>) -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                generation: 0,
                current: ViewState::Loading,
                active_gallery_id: None,
                page_history: Vec::new(),
                current_page_index: 0,
                decision_filter: None,
                summary_gallery_id: None,
                summary: None,
                confirmed_decisions: std::collections::HashMap::new(),
            })),
            view,
            dispatcher,
        }
    }
    pub fn begin_load(&self, gallery_id: impl Into<String>) -> LoadTicket {
        let gallery_id = gallery_id.into();
        let mut state = self.state.lock().expect("controller mutex poisoned");
        state.generation = state.generation.wrapping_add(1);
        state.current = ViewState::Loading;
        state.active_gallery_id = None;
        state.page_history.clear();
        state.current_page_index = 0;
        let generation = state.generation;
        drop(state);
        let view = self.view.clone();
        let _ = self
            .dispatcher
            .dispatch(Box::new(move || view.set_state(ViewState::Loading)));
        LoadTicket {
            generation,
            gallery_id,
        }
    }
    pub fn retry(&self, last_gallery_id: impl Into<String>) -> LoadTicket {
        self.begin_load(last_gallery_id)
    }
    pub fn snapshot(&self) -> ViewState {
        self.state
            .lock()
            .expect("controller mutex poisoned")
            .current
            .clone()
    }

    /// Return the resolved catalog gallery, including when the gallery is
    /// empty. Import needs this ID even before the first photo is cataloged.
    pub fn active_gallery_id(&self) -> Option<String> {
        self.state
            .lock()
            .expect("controller mutex poisoned")
            .active_gallery_id
            .clone()
    }

    /// Returns the resolved gallery only if this load ticket is still current.
    /// Callers that persist ticket-specific settings must not pair a stale
    /// request's inputs with a newer request's active gallery.
    pub fn active_gallery_id_for(&self, ticket: &LoadTicket) -> Option<String> {
        let state = self.state.lock().ok()?;
        (state.generation == ticket.generation)
            .then(|| state.active_gallery_id.clone())
            .flatten()
    }

    /// Fetch one bounded catalog page and its thumbnails off the event loop.
    /// Stale completions are discarded before they reach the view.
    pub async fn execute<C: DemoClient>(
        &self,
        client: &C,
        ticket: LoadTicket,
    ) -> Result<(), DemoError> {
        let decision_filter = self
            .state
            .lock()
            .map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?
            .decision_filter
            .clone();
        let result = load_page(
            client,
            &ticket.gallery_id,
            None,
            decision_filter.as_deref(),
        )
        .await;
        let summary_result = match &result {
            Ok((gallery, _)) => Some(client.summary(&gallery.gallery_id).await),
            Err(_) => None,
        };
        if let Ok((gallery, _)) = &result {
            let mut state = self.state.lock().map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?;
            if state.generation == ticket.generation {
                state.active_gallery_id = Some(gallery.gallery_id.clone());
                if state.summary_gallery_id.as_deref() != Some(&gallery.gallery_id) {
                    state.summary_gallery_id = Some(gallery.gallery_id.clone());
                    state.summary = None;
                }
                if let Some(Ok(summary)) = &summary_result {
                    state.summary_gallery_id = Some(gallery.gallery_id.clone());
                    state.summary = Some(summary.clone());
                }
            }
        }
        let (next, transition) = match result {
            Ok((_gallery, photos)) if photos.is_empty() => {
                (ViewState::Empty, Some(PageTransition::Reset))
            }
            Ok((gallery, photos)) => (
                ViewState::Ready {
                    gallery_id: gallery.gallery_id,
                    photos,
                    selected_index: 0,
                    page_offset: 0,
                    page_number: 1,
                    next_cursor: gallery.next_cursor,
                },
                Some(PageTransition::Reset),
            ),
            Err(error) => (
                ViewState::Error {
                    message: error.message,
                },
                Some(PageTransition::Reset),
            ),
        };
        self.publish(ticket.generation, next, transition)?;
        if let Some(Err(error)) = summary_result
            && self.is_current_generation(ticket.generation)
        {
            self.publish_summary_error(ticket.generation, error.message)?;
        }
        Ok(())
    }

    /// Replace the server-side decision filter and restart pagination at its
    /// first page. A failed request is surfaced as an error state while the
    /// selected filter remains active so the normal retry path can repeat it.
    pub async fn set_decision_filter<C: DemoClient>(
        &self,
        client: &C,
        decision_filter: Option<String>,
    ) -> Result<(), DemoError> {
        let (generation, gallery_id) = {
            let mut state = self.state.lock().map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?;
            if state.decision_filter == decision_filter {
                return Ok(());
            }
            state.decision_filter = decision_filter.clone();
            state.generation = state.generation.wrapping_add(1);
            state.page_history.clear();
            state.current_page_index = 0;
            let generation = state.generation;
            (generation, state.active_gallery_id.clone())
        };
        let Some(gallery_id) = gallery_id else {
            return Ok(());
        };

        let view = self.view.clone();
        self.dispatcher
            .dispatch(Box::new(move || view.set_state(ViewState::Loading)))?;
        match load_page(
            client,
            &gallery_id,
            None,
            decision_filter.as_deref(),
        )
        .await
        {
            Ok((gallery, photos)) => {
                let next = if photos.is_empty() {
                    ViewState::Empty
                } else {
                    ViewState::Ready {
                        gallery_id: gallery.gallery_id,
                        photos,
                        selected_index: 0,
                        page_offset: 0,
                        page_number: 1,
                        next_cursor: gallery.next_cursor,
                    }
                };
                self.publish(generation, next, Some(PageTransition::Reset))
            }
            Err(error) => {
                self.publish(
                    generation,
                    ViewState::Error {
                        message: error.message.clone(),
                    },
                    Some(PageTransition::Reset),
                )?;
                Err(error)
            }
        }
    }

    pub fn select_photo(&self, photo_id: &str) -> Result<(), DemoError> {
        let (generation, next) = {
            let mut state = self.state.lock().map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?;
            let ViewState::Ready {
                gallery_id,
                photos,
                page_offset,
                page_number,
                next_cursor,
                ..
            } = state.current.clone()
            else {
                return Ok(());
            };
            let Some(index) = photos
                .iter()
                .position(|entry| entry.photo.photo_id == photo_id)
            else {
                return Ok(());
            };
            state.generation = state.generation.wrapping_add(1);
            let generation = state.generation;
            (
                generation,
                ViewState::Ready {
                    gallery_id,
                    photos,
                    selected_index: index,
                    page_offset,
                    page_number,
                    next_cursor,
                },
            )
        };
        self.publish(generation, next, None)
    }

    pub async fn next_photo<C: DemoClient>(&self, client: &C) -> Result<(), DemoError> {
        self.navigate(client, true).await
    }

    pub async fn previous_photo<C: DemoClient>(&self, client: &C) -> Result<(), DemoError> {
        self.navigate(client, false).await
    }

    pub async fn update_decisions<C: DemoClient>(
        &self,
        client: &C,
        photo_ids: &[String],
        decision: Decision,
    ) -> Result<(), DemoError> {
        if photo_ids.is_empty() {
            return Ok(());
        }
        let (generation, mut state) = {
            let mut state = self.state.lock().map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?;
            let ViewState::Ready {
                gallery_id,
                photos,
                selected_index,
                page_offset,
                page_number,
                next_cursor,
            } = state.current.clone()
            else {
                return Ok(());
            };
            if photo_ids
                .iter()
                .any(|id| !photos.iter().any(|photo| &photo.photo.photo_id == id))
            {
                return Err(DemoError {
                    message: "La selección contiene fotos que ya no están en esta página.".into(),
                });
            }
            state.generation = state.generation.wrapping_add(1);
            (
                state.generation,
                ViewState::Ready {
                    gallery_id,
                    photos,
                    selected_index,
                    page_offset,
                    page_number,
                    next_cursor,
                },
            )
        };
        let ViewState::Ready {
            photos,
            selected_index,
            gallery_id,
            page_offset,
            page_number,
            next_cursor,
        } = &mut state
        else {
            unreachable!()
        };

        let mut seen = std::collections::HashSet::new();
        let mut confirmed_write = false;
        let mut first_error = None;
        for photo_id in photo_ids.iter().filter(|id| seen.insert((*id).clone())) {
            let Some(loaded) = photos
                .iter_mut()
                .find(|loaded| loaded.photo.photo_id == *photo_id)
            else {
                continue;
            };
            let request_id = new_request_id();
            match client
                .update_decision(&loaded.photo, decision, &request_id)
                .await
            {
                Ok(revision) => {
                    confirmed_write = true;
                    loaded.photo.decision = decision_name(decision).into();
                    loaded.photo.decision_revision = revision;
                    let mut current = self.state.lock().map_err(|_| DemoError {
                        message: "Gallery state is unavailable.".into(),
                    })?;
                    let key = (gallery_id.clone(), loaded.photo.photo_id.clone());
                    let entry = current
                        .confirmed_decisions
                        .entry(key)
                        .or_insert_with(|| (revision, decision_name(decision).to_owned()));
                    if revision > entry.0 {
                        *entry = (revision, decision_name(decision).to_owned());
                    }
                }
                Err(error) => {
                    first_error = Some(error);
                    break;
                }
            }
        }

        let summary_result = if confirmed_write {
            Some(client.summary(gallery_id).await)
        } else {
            None
        };
        if let Some(Ok(summary)) = &summary_result {
            let mut current = self.state.lock().map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?;
            if current.active_gallery_id.as_deref() == Some(gallery_id.as_str()) {
                current.summary_gallery_id = Some(gallery_id.clone());
                current.summary = Some(summary.clone());
            }
        }

        let next = ViewState::Ready {
            gallery_id: gallery_id.clone(),
            photos: photos.clone(),
            selected_index: *selected_index,
            page_offset: *page_offset,
            page_number: *page_number,
            next_cursor: next_cursor.clone(),
        };
        self.publish(generation, next, None)?;
        self.publish_confirmed_decisions()?;
        if let Some(Err(error)) = summary_result {
            self.publish_summary_error_for_gallery(gallery_id, error.message)?;
        }
        if let Some(error) = first_error {
            Err(error)
        } else {
            Ok(())
        }
    }

    async fn navigate<C: DemoClient>(&self, client: &C, forward: bool) -> Result<(), DemoError> {
        enum Navigation {
            Select {
                generation: u64,
                state: ViewState,
            },
            Fetch {
                generation: u64,
                gallery_id: String,
                cursor: Option<CatalogCursor>,
                decision_filter: Option<String>,
                offset: usize,
                target_page: usize,
                selected_index: Option<usize>,
            },
        }

        let navigation = {
            let mut state = self.state.lock().map_err(|_| DemoError {
                message: "Gallery state is unavailable.".into(),
            })?;
            let ViewState::Ready {
                gallery_id,
                photos,
                selected_index,
                page_offset,
                page_number,
                next_cursor,
            } = state.current.clone()
            else {
                return Ok(());
            };

            if forward && selected_index + 1 < photos.len() {
                state.generation = state.generation.wrapping_add(1);
                Navigation::Select {
                    generation: state.generation,
                    state: ViewState::Ready {
                        gallery_id,
                        photos,
                        selected_index: selected_index + 1,
                        page_offset,
                        page_number,
                        next_cursor,
                    },
                }
            } else if !forward && selected_index > 0 {
                state.generation = state.generation.wrapping_add(1);
                Navigation::Select {
                    generation: state.generation,
                    state: ViewState::Ready {
                        gallery_id,
                        photos,
                        selected_index: selected_index - 1,
                        page_offset,
                        page_number,
                        next_cursor,
                    },
                }
            } else if forward {
                let Some(cursor) = next_cursor else {
                    return Ok(());
                };
                let current_page_index = state.current_page_index;
                let decision_filter = state.decision_filter.clone();
                state.generation = state.generation.wrapping_add(1);
                Navigation::Fetch {
                    generation: state.generation,
                    gallery_id,
                    cursor: Some(cursor),
                    decision_filter,
                    offset: page_offset + photos.len(),
                    target_page: current_page_index + 1,
                    selected_index: Some(0),
                }
            } else {
                if state.current_page_index == 0 {
                    return Ok(());
                }
                let target_page = state.current_page_index - 1;
                let Some(start) = state.page_history.get(target_page).cloned() else {
                    return Ok(());
                };
                let decision_filter = state.decision_filter.clone();
                state.generation = state.generation.wrapping_add(1);
                Navigation::Fetch {
                    generation: state.generation,
                    gallery_id,
                    cursor: start.cursor,
                    decision_filter,
                    offset: start.offset,
                    target_page,
                    selected_index: None,
                }
            }
        };

        match navigation {
            Navigation::Select { generation, state } => self.publish(generation, state, None),
            Navigation::Fetch {
                generation,
                gallery_id,
                cursor,
                decision_filter,
                offset,
                target_page,
                selected_index,
            } => {
                let (gallery, photos) = match load_page(
                    client,
                    &gallery_id,
                    cursor.as_ref(),
                    decision_filter.as_deref(),
                )
                .await
                {
                    Ok(page) => page,
                    Err(error) => {
                        self.publish_navigation_error(generation, error.message)?;
                        return Ok(());
                    }
                };
                if photos.is_empty() {
                    return Ok(());
                }
                let index = selected_index.unwrap_or(photos.len() - 1);
                let next = ViewState::Ready {
                    gallery_id: gallery.gallery_id,
                    photos,
                    selected_index: index,
                    page_offset: offset,
                    page_number: target_page + 1,
                    next_cursor: gallery.next_cursor,
                };
                let transition = if selected_index.is_some() {
                    Some(PageTransition::Append(PageStart { cursor, offset }))
                } else {
                    Some(PageTransition::MoveBack(target_page))
                };
                self.publish(generation, next, transition)
            }
        }
    }

    fn publish(
        &self,
        generation: u64,
        next: ViewState,
        transition: Option<PageTransition>,
    ) -> Result<(), DemoError> {
        let state = self.state.clone();
        let view = self.view.clone();
        self.dispatcher.dispatch(Box::new(move || {
            let Ok(mut state) = state.lock() else { return };
            if state.generation != generation {
                return;
            }
            match transition {
                Some(PageTransition::Reset) => {
                    state.page_history = vec![PageStart {
                        cursor: None,
                        offset: 0,
                    }];
                    state.current_page_index = 0;
                }
                Some(PageTransition::Append(start)) => {
                    let keep_count = state.current_page_index + 1;
                    state.page_history.truncate(keep_count);
                    state.page_history.push(start);
                    state.current_page_index += 1;
                }
                Some(PageTransition::MoveBack(index)) => {
                    state.page_history.truncate(index + 1);
                    state.current_page_index = index;
                }
                None => {}
            }
            let mut next = next;
            apply_confirmed_decisions(&mut next, &state.confirmed_decisions);
            let next_gallery_id = match &next {
                ViewState::Ready { gallery_id, .. } => Some(gallery_id.as_str()),
                ViewState::Empty => state.active_gallery_id.as_deref(),
                ViewState::Loading | ViewState::Error { .. } => None,
            };
            let summary = next_gallery_id
                .zip(state.summary_gallery_id.as_deref())
                .filter(|(gallery_id, summary_gallery_id)| gallery_id == summary_gallery_id)
                .and(state.summary.clone());
            view.set_global_summary(summary);
            state.current = next.clone();
            view.set_state(next);
        }))
    }

    fn is_current_generation(&self, generation: u64) -> bool {
        self.state
            .lock()
            .is_ok_and(|state| state.generation == generation)
    }

    fn publish_summary_error(
        &self,
        generation: u64,
        message: String,
    ) -> Result<(), DemoError> {
        let state = self.state.clone();
        let view = self.view.clone();
        self.dispatcher.dispatch(Box::new(move || {
            let Ok(state) = state.lock() else { return };
            if state.generation == generation {
                view.set_navigation_error(message);
            }
        }))
    }

    fn publish_summary_error_for_gallery(
        &self,
        gallery_id: String,
        message: String,
    ) -> Result<(), DemoError> {
        let state = self.state.clone();
        let view = self.view.clone();
        self.dispatcher.dispatch(Box::new(move || {
            let Ok(state) = state.lock() else { return };
            if state.active_gallery_id.as_deref() == Some(gallery_id.as_str()) {
                view.set_navigation_error(message);
            }
        }))
    }

    fn publish_navigation_error(&self, generation: u64, message: String) -> Result<(), DemoError> {
        let state = self.state.clone();
        let view = self.view.clone();
        self.dispatcher.dispatch(Box::new(move || {
            let Ok(state) = state.lock() else { return };
            if state.generation == generation && matches!(&state.current, ViewState::Ready { .. }) {
                view.set_navigation_error(message);
            }
        }))
    }

    fn publish_confirmed_decisions(&self) -> Result<(), DemoError> {
        let state = self.state.clone();
        let view = self.view.clone();
        self.dispatcher.dispatch(Box::new(move || {
            let Ok(mut state) = state.lock() else { return };
            let mut current = state.current.clone();
            apply_confirmed_decisions(&mut current, &state.confirmed_decisions);
            let current_gallery_id = match &current {
                ViewState::Ready { gallery_id, .. } => Some(gallery_id.as_str()),
                _ => None,
            };
            let summary = current_gallery_id
                .zip(state.summary_gallery_id.as_deref())
                .filter(|(gallery_id, summary_gallery_id)| gallery_id == summary_gallery_id)
                .and(state.summary.clone());
            view.set_global_summary(summary);
            if current != state.current {
                state.current = current.clone();
                view.set_state(current);
            }
        }))
    }
}

fn apply_confirmed_decisions(
    state: &mut ViewState,
    confirmed: &std::collections::HashMap<(String, String), (u64, String)>,
) {
    if let ViewState::Ready {
        gallery_id, photos, ..
    } = state
    {
        for loaded in photos {
            if let Some((revision, decision)) =
                confirmed.get(&(gallery_id.clone(), loaded.photo.photo_id.clone()))
            {
                if *revision > loaded.photo.decision_revision {
                    loaded.photo.decision_revision = *revision;
                    loaded.photo.decision = decision.clone();
                }
            }
        }
    }
}

async fn load_page<C: DemoClient>(
    client: &C,
    gallery_id: &str,
    cursor: Option<&CatalogCursor>,
    decision_filter: Option<&str>,
) -> Result<(DemoGallery, Vec<LoadedPhoto>), DemoError> {
    let gallery = client
        .gallery(gallery_id, cursor, decision_filter)
        .await?;
    let mut photos = Vec::with_capacity(gallery.photos.len());
    for photo in &gallery.photos {
        let (jpeg, preview_error) = match client.thumbnail(photo).await {
            Ok(jpeg) => (Some(jpeg), None),
            Err(error) => (None, Some(error.message)),
        };
        photos.push(LoadedPhoto {
            photo: photo.clone(),
            jpeg,
            preview_error,
        });
    }
    Ok((gallery, photos))
}

fn decision_name(decision: Decision) -> &'static str {
    match decision {
        Decision::Keep => "keep",
        Decision::Alt => "alt",
        Decision::Reject => "reject",
        Decision::Undecided => "undecided",
    }
}

fn new_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};
    static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "photo-culler-{timestamp:x}-{}",
        NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed)
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::VecDeque,
        task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
    };
    #[derive(Default)]
    struct FakeView {
        states: Mutex<Vec<ViewState>>,
    }
    impl DemoView for FakeView {
        fn set_state(&self, state: ViewState) {
            self.states.lock().unwrap().push(state);
        }
    }
    #[derive(Default)]
    struct Queue {
        tasks: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
    }
    impl UiDispatcher for Queue {
        fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError> {
            self.tasks.lock().unwrap().push_back(task);
            Ok(())
        }
    }
    impl Queue {
        fn run(&self) {
            self.tasks.lock().unwrap().pop_front().unwrap()();
        }
        fn run_all(&self) {
            while !self.tasks.lock().unwrap().is_empty() {
                self.run();
            }
        }
    }

    #[test]
    fn ticket_specific_gallery_lookup_ignores_superseded_loads() {
        let c = DemoController::new(Arc::new(FakeView::default()), Arc::new(Queue::default()));
        let stale = c.begin_load("old-root");
        let current = c.begin_load("new-root");
        c.state.lock().unwrap().active_gallery_id = Some("new-gallery".into());

        assert_eq!(c.active_gallery_id_for(&stale), None);
        assert_eq!(
            c.active_gallery_id_for(&current).as_deref(),
            Some("new-gallery")
        );
    }

    struct FakeClient {
        photos: Vec<DemoPhoto>,
        image: Result<Vec<u8>, DemoError>,
    }
    impl DemoClient for FakeClient {
        fn gallery<'a>(&'a self, _: &'a str, _: Option<&'a CatalogCursor>) -> DemoFuture<'a> {
            let photos = self.photos.clone();
            Box::pin(async move {
                Ok(DemoGallery {
                    gallery_id: "demo".into(),
                    photos,
                    next_cursor: None,
                })
            })
        }
        fn thumbnail<'a>(&'a self, _: &'a DemoPhoto) -> ThumbnailFuture<'a> {
            let image = self.image.clone();
            Box::pin(async move { image })
        }
        fn update_decision<'a>(
            &'a self,
            photo: &'a DemoPhoto,
            _: Decision,
            _: &'a str,
        ) -> DecisionFuture<'a> {
            let next_revision = photo.decision_revision + 1;
            Box::pin(async move { Ok(next_revision) })
        }
    }
    fn photo() -> DemoPhoto {
        DemoPhoto {
            photo_id: "demo-001".into(),
            filename: "demo-001.jpg".into(),
            decision: "undecided".into(),
            thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800".into(),
            decision_revision: 0,
        }
    }
    fn block_on<F: Future>(f: F) -> F::Output {
        unsafe fn clone(_: *const ()) -> RawWaker {
            raw()
        }
        unsafe fn nop(_: *const ()) {}
        fn raw() -> RawWaker {
            RawWaker::new(std::ptr::null(), &RawWakerVTable::new(clone, nop, nop, nop))
        }
        let w = unsafe { Waker::from_raw(raw()) };
        let mut c = Context::from_waker(&w);
        let mut f = std::pin::pin!(f);
        match f.as_mut().poll(&mut c) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("pending"),
        }
    }
    #[test]
    fn loads_ready_and_publishes_only_when_dispatcher_runs() {
        let view = Arc::new(FakeView::default());
        let q = Arc::new(Queue::default());
        let c = DemoController::new(view.clone(), q.clone());
        let t = c.begin_load("demo");
        let api = FakeClient {
            photos: vec![photo()],
            image: Ok(vec![1, 2]),
        };
        block_on(c.execute(&api, t)).unwrap();
        assert_eq!(c.snapshot(), ViewState::Loading);
        q.run_all();
        assert!(matches!(
            c.snapshot(),
            ViewState::Ready { photos, selected_index: 0, .. }
                if photos[0].photo.photo_id == "demo-001"
                    && photos[0].jpeg.as_deref() == Some(&[1, 2][..])
                    && photos[0].preview_error.is_none()
        ));
    }
    #[test]
    fn empty_gallery_and_thumbnail_error_keep_gallery_ready() {
        let view = Arc::new(FakeView::default());
        let q = Arc::new(Queue::default());
        let c = DemoController::new(view, q.clone());
        let t = c.begin_load("demo");
        block_on(c.execute(
            &FakeClient {
                photos: vec![],
                image: Ok(vec![]),
            },
            t,
        ))
        .unwrap();
        q.run_all();
        assert_eq!(c.snapshot(), ViewState::Empty);
        let t = c.retry("demo");
        block_on(c.execute(
            &FakeClient {
                photos: vec![photo()],
                image: Err(DemoError {
                    message: "bad preview".into(),
                }),
            },
            t,
        ))
        .unwrap();
        q.run_all();
        assert!(matches!(
            c.snapshot(),
            ViewState::Ready { photos, selected_index: 0, .. }
                if photos[0].photo.photo_id == "demo-001"
                    && photos[0].jpeg.is_none()
                    && photos[0].preview_error.as_deref() == Some("bad preview")
        ));
    }
    #[test]
    fn delayed_old_gallery_result_cannot_replace_new_load() {
        let view = Arc::new(FakeView::default());
        let q = Arc::new(Queue::default());
        let c = DemoController::new(view, q.clone());
        let old = c.begin_load("old");
        let fresh = c.begin_load("demo");
        block_on(c.execute(
            &FakeClient {
                photos: vec![photo()],
                image: Ok(vec![1]),
            },
            old,
        ))
        .unwrap();
        block_on(c.execute(
            &FakeClient {
                photos: vec![photo()],
                image: Ok(vec![2]),
            },
            fresh,
        ))
        .unwrap();
        q.run_all();
        assert!(matches!(
            c.snapshot(),
            ViewState::Ready { photos, .. }
                if photos[0].jpeg.as_deref() == Some(&[2][..])
        ));
    }
}

#[cfg(test)]
#[path = "controller_state_tests.rs"]
mod state_flow_tests;
