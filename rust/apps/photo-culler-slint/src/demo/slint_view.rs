//! Bind the platform-neutral controller state and the Pro Library interactions
//! to the shared Slint window. The normal view only uses fields supplied by the
//! Sprint 01 REST DTO. Synthetic EXIF/analysis values exist only in explicit
//! visual-smoke mode.

use crate::demo::{
    controller::{DemoError, DemoView, UiDispatcher, ViewState},
    thumbnail_image::decode_jpeg,
};
use slint::ComponentHandle;
use std::{
    cmp::Ordering,
    collections::{BTreeSet, HashSet},
    sync::{Arc, Mutex},
};

#[derive(Clone, Debug, PartialEq, Eq)]
struct PhotoMetadata {
    id: String,
    filename: String,
    decision: String,
    camera: Option<String>,
    lens: Option<String>,
    tags: Vec<String>,
    session_name: Option<String>,
    iso: Option<u32>,
    sharpness: Option<u8>,
    focus_accuracy: Option<u8>,
    rating: Option<u8>,
    is_raw: Option<bool>,
    capture_time: Option<String>,
}

#[derive(Clone, Debug)]
struct GalleryItem {
    metadata: PhotoMetadata,
    jpeg: Arc<[u8]>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct LibraryFilters {
    search_query: String,
    cull_status: String,
    cameras: BTreeSet<String>,
    max_iso: u32,
    min_sharpness: u8,
    min_rating: u8,
    raw_only: bool,
}

impl Default for LibraryFilters {
    fn default() -> Self {
        Self {
            search_query: String::new(),
            cull_status: "all".into(),
            cameras: BTreeSet::new(),
            max_iso: 12_800,
            min_sharpness: 0,
            min_rating: 0,
            raw_only: false,
        }
    }
}

impl LibraryFilters {
    fn matches(&self, photo: &PhotoMetadata) -> bool {
        let query = self.search_query.trim().to_lowercase();
        let search_matches = std::iter::once(photo.filename.as_str())
            .chain(photo.camera.iter().map(String::as_str))
            .chain(photo.lens.iter().map(String::as_str))
            .chain(photo.tags.iter().map(String::as_str))
            .chain(photo.session_name.iter().map(String::as_str))
            .any(|field| field.to_lowercase().contains(&query));
        if !query.is_empty() && !search_matches {
            return false;
        }

        let decision = photo.decision.to_ascii_lowercase();
        let decision_matches = match self.cull_status.as_str() {
            "keep" => matches!(decision.as_str(), "keep" | "best"),
            "alt" => matches!(decision.as_str(), "alt" | "alternate" | "review"),
            "reject" => decision.starts_with("reject"),
            "unrated" => matches!(decision.as_str(), "" | "unrated" | "undecided"),
            _ => true,
        };
        if !decision_matches {
            return false;
        }

        if !self.cameras.is_empty()
            && !photo
                .camera
                .as_ref()
                .is_some_and(|camera| self.cameras.contains(camera))
        {
            return false;
        }

        if self.max_iso < 12_800 && !photo.iso.is_some_and(|iso| iso <= self.max_iso) {
            return false;
        }
        if self.min_sharpness > 0
            && !photo
                .sharpness
                .is_some_and(|value| value >= self.min_sharpness)
        {
            return false;
        }
        if self.min_rating > 0 && !photo.rating.is_some_and(|value| value >= self.min_rating) {
            return false;
        }
        if self.raw_only && photo.is_raw != Some(true) {
            return false;
        }

        true
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridDensity {
    Small,
    Medium,
    Large,
}

impl Default for GridDensity {
    fn default() -> Self {
        Self::Medium
    }
}

impl GridDensity {
    fn from_index(value: i32) -> Self {
        match value {
            0 => Self::Small,
            2 => Self::Large,
            _ => Self::Medium,
        }
    }

    fn index(self) -> i32 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
        }
    }

    #[cfg(test)]
    fn columns(self, width: u32) -> u8 {
        match self {
            Self::Small => {
                if width >= 1024 {
                    6
                } else if width >= 768 {
                    4
                } else if width >= 640 {
                    3
                } else {
                    2
                }
            }
            Self::Medium => {
                if width >= 1280 {
                    5
                } else if width >= 1024 {
                    4
                } else if width >= 768 {
                    3
                } else if width >= 640 {
                    2
                } else {
                    1
                }
            }
            Self::Large => {
                if width >= 1024 {
                    3
                } else if width >= 640 {
                    2
                } else {
                    1
                }
            }
        }
    }
}

struct LibraryInteractionState {
    filters: LibraryFilters,
    items: Vec<GalleryItem>,
    selected: HashSet<String>,
    density: GridDensity,
    sort_descending: bool,
    metadata_available: bool,
}

impl Default for LibraryInteractionState {
    fn default() -> Self {
        Self {
            filters: LibraryFilters::default(),
            items: Vec::new(),
            selected: HashSet::new(),
            density: GridDensity::Medium,
            sort_descending: true,
            metadata_available: false,
        }
    }
}

impl LibraryInteractionState {
    fn visible_indices(&self) -> Vec<usize> {
        let mut visible: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| self.filters.matches(&item.metadata).then_some(index))
            .collect();
        visible.sort_by(|left, right| {
            compare_capture_time(
                &self.items[*left].metadata,
                &self.items[*right].metadata,
                self.sort_descending,
            )
        });
        visible
    }

    fn request_selection(&mut self, photo_id: &str, multi: bool) {
        if photo_id.is_empty() {
            self.selected.clear();
        } else if multi {
            if !self.selected.remove(photo_id) {
                self.selected.insert(photo_id.to_owned());
            }
        } else {
            self.selected.clear();
            self.selected.insert(photo_id.to_owned());
        }
    }

    fn toggle_all_visible(&mut self) {
        let ids: Vec<_> = self
            .visible_indices()
            .into_iter()
            .map(|index| self.items[index].metadata.id.clone())
            .collect();
        if !ids.is_empty() && ids.iter().all(|id| self.selected.contains(id)) {
            for id in ids {
                self.selected.remove(&id);
            }
        } else {
            self.selected.extend(ids);
        }
    }

    fn reset_filters(&mut self) {
        self.filters = LibraryFilters::default();
        self.sort_descending = true;
    }
}

fn compare_capture_time(left: &PhotoMetadata, right: &PhotoMetadata, descending: bool) -> Ordering {
    match (left.capture_time.as_deref(), right.capture_time.as_deref()) {
        (Some(left_time), Some(right_time)) => {
            let order = left_time
                .cmp(right_time)
                .then_with(|| left.filename.cmp(&right.filename));
            if descending { order.reverse() } else { order }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => {
            let order = left.filename.cmp(&right.filename);
            if descending { order.reverse() } else { order }
        }
    }
}

pub struct SlintDemoView {
    window: slint::Weak<crate::DemoWindow>,
    web_layout: bool,
    smoke_fixture: bool,
    library: Arc<Mutex<LibraryInteractionState>>,
}

impl SlintDemoView {
    pub fn new(window: slint::Weak<crate::DemoWindow>, web_layout: bool) -> Self {
        let smoke_fixture = explicit_smoke_mode(web_layout);
        let library = Arc::new(Mutex::new(LibraryInteractionState::default()));
        if let Some(window) = window.upgrade() {
            window
                .global::<crate::DesignTokens>()
                .set_web_layout(web_layout);
            window.set_web_layout(web_layout);
            #[cfg(target_arch = "wasm32")]
            if web_layout {
                bind_browser_window_resize(window.as_weak());
            }
            wire_library_callbacks(&window, library.clone(), web_layout);
        }
        Self {
            window,
            web_layout,
            smoke_fixture,
            library,
        }
    }
}

impl DemoView for SlintDemoView {
    fn set_state(&self, state: ViewState) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        match state {
            ViewState::Loading => {
                window.set_error_message("Connecting to the local Photo Culler service.".into());
                window.set_service_status("Conectando con el servicio local…".into());
                window.set_view_state(0);
                set_browser_status("loading", "Loading gallery");
            }
            ViewState::Empty => {
                if let Ok(mut library) = self.library.lock() {
                    library.items.clear();
                    library.selected.clear();
                }
                refresh_library(&self.window, &self.library, self.web_layout);
                window.set_photo_count(0);
                window.set_photo_index(0);
                window.set_navigation_label("0 fotos".into());
                window.set_service_status("Galería vacía".into());
                window.set_view_state(2);
                set_browser_status("empty", "The selected gallery contains no photos.");
            }
            ViewState::Error { message } => {
                set_browser_status("error", &message);
                window.set_service_status("Servicio no disponible".into());
                window.set_error_message(message.into());
                window.set_view_state(3);
            }
            ViewState::Ready {
                photo,
                jpeg,
                preview_error,
                ..
            } => {
                let filename = photo.filename;
                let photo_id = photo.photo_id;
                let decision = photo.decision;
                window.set_photo_name(filename.clone().into());
                window.set_gallery_title("Sesión de muestra".into());
                window.set_fallback_photo_id(photo_id.clone().into());
                window.set_selected_photo_id(photo_id.clone().into());
                window.set_photo_index(0);
                window.set_navigation_label("Foto 1 de 1".into());
                if let Some(bytes) = jpeg {
                    match decode_jpeg(&bytes, "image/jpeg") {
                        Ok(image) => {
                            window.set_photo_thumbnail(image);
                            let jpeg: Arc<[u8]> = Arc::from(bytes.into_boxed_slice());
                            let records = if self.smoke_fixture {
                                smoke_fixture_metadata()
                                    .into_iter()
                                    .map(|metadata| GalleryItem {
                                        metadata,
                                        jpeg: jpeg.clone(),
                                    })
                                    .collect()
                            } else {
                                vec![GalleryItem {
                                    metadata: PhotoMetadata {
                                        id: photo_id.clone(),
                                        filename: filename.clone(),
                                        decision: decision.clone(),
                                        camera: None,
                                        lens: None,
                                        tags: Vec::new(),
                                        session_name: None,
                                        iso: None,
                                        sharpness: None,
                                        focus_accuracy: None,
                                        rating: None,
                                        is_raw: None,
                                        capture_time: None,
                                    },
                                    jpeg,
                                }]
                            };
                            if let Ok(mut library) = self.library.lock() {
                                library.items = records;
                                library.metadata_available = self.smoke_fixture;
                                let valid_ids: HashSet<_> = library
                                    .items
                                    .iter()
                                    .map(|item| item.metadata.id.clone())
                                    .collect();
                                library.selected.retain(|id| valid_ids.contains(id));
                                if library.selected.is_empty()
                                    && let Some(first_id) =
                                        library.items.first().map(|item| item.metadata.id.clone())
                                {
                                    library.selected.insert(first_id);
                                }
                            }
                            window.set_service_status(if self.smoke_fixture {
                                "SMOKE FIXTURE · REST JPEG, synthetic metadata".into()
                            } else {
                                "Servicio local listo".into()
                            });
                            window.set_error_message("".into());
                            window.set_view_state(1);
                            refresh_library(&self.window, &self.library, self.web_layout);
                            let browser_message = if self.smoke_fixture {
                                format!("{filename} · 10-card smoke fixture")
                            } else {
                                filename
                            };
                            set_browser_status("ready", &browser_message);
                        }
                        Err(error) => {
                            let message = format!("Preview unavailable: {error:?}");
                            set_browser_status("error", &message);
                            window.set_error_message(message.into());
                            window.set_view_state(3);
                        }
                    }
                } else {
                    let message =
                        preview_error.unwrap_or_else(|| "The photo preview is unavailable.".into());
                    set_browser_status("error", &message);
                    window.set_service_status("Vista previa no disponible".into());
                    window.set_error_message(message.into());
                    window.set_view_state(3);
                }
            }
        }
    }
}

fn wire_library_callbacks(
    window: &crate::DemoWindow,
    library: Arc<Mutex<LibraryInteractionState>>,
    web_layout: bool,
) {
    let weak = window.as_weak();
    let callback_library = library.clone();
    window.on_search_changed(move |query| {
        if let Ok(mut state) = callback_library.lock() {
            state.filters.search_query = query.to_string();
        }
        refresh_library(&weak, &callback_library, web_layout);
    });

    let weak = window.as_weak();
    let callback_library = library.clone();
    window.on_filter_requested(move |name, value| {
        if let Ok(mut state) = callback_library.lock() {
            match name.as_str() {
                "reset" => state.reset_filters(),
                "cull-status" => state.filters.cull_status = value.to_string(),
                "camera-body" => {
                    let camera = value.to_string();
                    if !state.filters.cameras.remove(&camera) {
                        state.filters.cameras.insert(camera);
                    }
                }
                "max-iso" => {
                    if let Ok(iso) = value.parse::<u32>() {
                        state.filters.max_iso = iso.min(12_800);
                    }
                }
                "min-sharpness" => {
                    if let Ok(sharpness) = value.parse::<u8>() {
                        state.filters.min_sharpness = sharpness.min(95);
                    }
                }
                "min-rating" => {
                    if let Ok(rating) = value.parse::<u8>() {
                        state.filters.min_rating = rating.min(5);
                    }
                }
                "raw-only" => state.filters.raw_only = value.as_str() == "true",
                _ => {}
            }
        }
        if name.as_str() == "reset"
            && let Some(window) = weak.upgrade()
        {
            window.set_search_query("".into());
        }
        refresh_library(&weak, &callback_library, web_layout);
    });

    let weak = window.as_weak();
    let callback_library = library.clone();
    window.on_selection_requested(move |photo_id, multi| {
        if let Ok(mut state) = callback_library.lock() {
            state.request_selection(photo_id.as_str(), multi);
        }
        if let Some(window) = weak.upgrade() {
            if !photo_id.is_empty() {
                window.set_selected_photo_id(photo_id.clone());
            }
        }
        refresh_library(&weak, &callback_library, web_layout);
    });

    let weak = window.as_weak();
    let callback_library = library.clone();
    window.on_select_all_requested(move || {
        if let Ok(mut state) = callback_library.lock() {
            state.toggle_all_visible();
        }
        refresh_library(&weak, &callback_library, web_layout);
    });

    let weak = window.as_weak();
    let callback_library = library.clone();
    window.on_sort_direction_changed(move |descending| {
        if let Ok(mut state) = callback_library.lock() {
            state.sort_descending = descending;
        }
        refresh_library(&weak, &callback_library, web_layout);
    });

    let weak = window.as_weak();
    window.on_density_changed(move |density| {
        if let Ok(mut state) = library.lock() {
            state.density = GridDensity::from_index(density);
        }
        if let Some(window) = weak.upgrade() {
            window.set_grid_density(GridDensity::from_index(density).index());
        }
    });
}

fn refresh_library(
    window: &slint::Weak<crate::DemoWindow>,
    library: &Arc<Mutex<LibraryInteractionState>>,
    web_layout: bool,
) {
    let Some(window) = window.upgrade() else {
        return;
    };
    let Ok(state) = library.lock() else {
        return;
    };
    let visible = state.visible_indices();
    let gallery_photos: Vec<_> = visible
        .iter()
        .filter_map(|index| {
            let item = &state.items[*index];
            let thumbnail = decode_jpeg(&item.jpeg, "image/jpeg").ok()?;
            let metadata = &item.metadata;
            let decision = decision_label(&metadata.decision, web_layout);
            let score = metadata
                .focus_accuracy
                .map(|score| format!("{:.1}", score as f32 / 10.0))
                .unwrap_or_else(|| "—".into());
            Some(crate::GalleryPhoto {
                id: metadata.id.clone().into(),
                filename: metadata.filename.clone().into(),
                decision: decision.into(),
                quality: "—".into(),
                score: score.into(),
                thumbnail,
                rating: metadata.rating.map(i32::from).unwrap_or(-1),
                iso: metadata.iso.map(|iso| iso as i32).unwrap_or(0),
                is_raw: metadata.is_raw.unwrap_or(false),
                camera: metadata.camera.clone().unwrap_or_default().into(),
                capture_time: metadata.capture_time.clone().unwrap_or_default().into(),
                sharpness: metadata.sharpness.map(i32::from).unwrap_or(-1),
                metadata_known: state.metadata_available,
                selected: state.selected.contains(&metadata.id),
            })
        })
        .collect();
    let camera_options: Vec<_> = ordered_camera_names(&state.items)
        .into_iter()
        .map(|camera| crate::CameraFilterOption {
            label: camera.clone().into(),
            selected: state.filters.cameras.contains(&camera),
        })
        .collect();
    let selected_visible_count = visible
        .iter()
        .filter(|index| state.selected.contains(&state.items[**index].metadata.id))
        .count();
    let all_visible_selected = !visible.is_empty() && selected_visible_count == visible.len();
    let kept = state
        .items
        .iter()
        .filter(|item| decision_category(&item.metadata.decision) == "keep")
        .count() as i32;
    let alternate = state
        .items
        .iter()
        .filter(|item| decision_category(&item.metadata.decision) == "alt")
        .count() as i32;
    let rejected = state
        .items
        .iter()
        .filter(|item| decision_category(&item.metadata.decision) == "reject")
        .count() as i32;
    let unrated = state
        .items
        .iter()
        .filter(|item| decision_category(&item.metadata.decision) == "unrated")
        .count() as i32;
    let visible_count = gallery_photos.len() as i32;
    let selected_count = state.selected.len() as i32;
    window.set_gallery_photos(slint::ModelRc::new(slint::VecModel::from(gallery_photos)));
    window.set_camera_filter_options(slint::ModelRc::new(slint::VecModel::from(camera_options)));
    window.set_photo_count(visible_count);
    window.set_photo_index(0);
    window.set_navigation_label(if visible_count == 0 {
        "0 fotos".into()
    } else {
        format!("Foto 1 de {visible_count}").into()
    });
    window.set_selected_photo_count(selected_count);
    window.set_all_visible_selected(all_visible_selected);
    window.set_gallery_metadata_available(state.metadata_available);
    window.set_cull_status(state.filters.cull_status.clone().into());
    window.set_max_iso(state.filters.max_iso as i32);
    window.set_min_sharpness(state.filters.min_sharpness as i32);
    window.set_min_rating(state.filters.min_rating as i32);
    window.set_raw_only(state.filters.raw_only);
    window.set_search_query(state.filters.search_query.clone().into());
    window.set_sort_descending(state.sort_descending);
    window.set_grid_density(state.density.index());
    window.set_kept_count(kept);
    window.set_alternate_count(alternate);
    window.set_rejected_count(rejected);
    window.set_unrated_count(unrated);
}

fn decision_label(decision: &str, web_layout: bool) -> &'static str {
    match decision.to_ascii_lowercase().as_str() {
        "keep" | "best" if web_layout => "KEEP",
        "keep" | "best" => "BEST",
        "alt" | "alternate" | "review" => "ALTERNATE",
        decision if decision.starts_with("reject") => "REJECTED",
        _ => "UNRATED",
    }
}

fn decision_category(decision: &str) -> &'static str {
    match decision.to_ascii_lowercase().as_str() {
        "keep" | "best" => "keep",
        "alt" | "alternate" | "review" => "alt",
        decision if decision.starts_with("reject") => "reject",
        _ => "unrated",
    }
}

#[cfg(target_arch = "wasm32")]
fn explicit_smoke_mode(_: bool) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    let Ok(location) = js_sys::Reflect::get(window.as_ref(), &"location".into()) else {
        return false;
    };
    let Ok(search) = js_sys::Reflect::get(&location, &"search".into()) else {
        return false;
    };
    search.as_string().is_some_and(|search| {
        search
            .split('&')
            .any(|part| part.trim_start_matches('?') == "photo-culler-smoke=1")
    })
}

#[cfg(not(target_arch = "wasm32"))]
fn explicit_smoke_mode(_: bool) -> bool {
    std::env::var("PHOTO_CULLER_GALLERY_SMOKE").is_ok_and(|value| value == "1")
}

#[cfg(target_arch = "wasm32")]
fn bind_browser_window_resize(window: slint::Weak<crate::DemoWindow>) {
    use wasm_bindgen::{JsCast, closure::Closure};

    let Some(browser_window) = web_sys::window() else {
        return;
    };
    let update_size = move || {
        let Some(browser_window) = web_sys::window() else {
            return;
        };
        let width = browser_window
            .inner_width()
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        let height = browser_window
            .inner_height()
            .ok()
            .and_then(|value| value.as_f64())
            .unwrap_or_default();
        if width.is_finite()
            && height.is_finite()
            && width > 0.
            && height > 0.
            && let Some(window) = window.upgrade()
        {
            window
                .window()
                .set_size(slint::LogicalSize::new(width as f32, height as f32));
        }
    };

    update_size();
    let resize_handler = Closure::<dyn FnMut()>::new(update_size);
    if browser_window
        .add_event_listener_with_callback("resize", resize_handler.as_ref().unchecked_ref())
        .is_ok()
    {
        resize_handler.forget();
    }
}

fn smoke_fixture_metadata() -> Vec<PhotoMetadata> {
    vec![
        metadata(
            "photo-01",
            "DSC_8492.NEF",
            "keep",
            "NIKON Z9",
            "NIKKOR Z 50mm f/1.2 S",
            &["landscape", "mountains", "golden hour", "raw"],
            "ICELAND_EXPEDITION_RAW",
            400,
            98,
            98,
            5,
            true,
            "2026-07-28 06:14:22",
        ),
        metadata(
            "photo-02",
            "DSC_4021.ARW",
            "keep",
            "Sony A7R IV",
            "FE 24-70mm f/2.8 GM II",
            &["architecture", "monochrome", "geometry", "raw"],
            "BERLIN_ARCH_STUDY",
            100,
            95,
            95,
            4,
            true,
            "2026-07-27 13:45:10",
        ),
        metadata(
            "photo-03",
            "DSC_8492.ARW",
            "keep",
            "Sony A7R IV",
            "FE 85mm f/1.4 GM",
            &["portrait", "studio", "headshot", "burst"],
            "STUDIO_PORTRAIT_TEST",
            100,
            94,
            94,
            5,
            true,
            "2026-07-29 15:20:01",
        ),
        metadata(
            "photo-04",
            "DSC_8493.ARW",
            "alt",
            "Sony A7R IV",
            "FE 85mm f/1.4 GM",
            &["portrait", "studio", "eye-af", "winner"],
            "STUDIO_PORTRAIT_TEST",
            100,
            98,
            98,
            5,
            true,
            "2026-07-29 15:20:02",
        ),
        metadata(
            "photo-05",
            "DSC_8494.ARW",
            "reject",
            "Sony A7R IV",
            "FE 85mm f/1.4 GM",
            &["portrait", "studio", "out of focus"],
            "STUDIO_PORTRAIT_TEST",
            100,
            62,
            62,
            2,
            true,
            "2026-07-29 15:20:03",
        ),
        metadata(
            "photo-06",
            "DSC_8495.ARW",
            "unrated",
            "Sony A7R IV",
            "FE 85mm f/1.4 GM",
            &["portrait", "profile", "studio"],
            "STUDIO_PORTRAIT_TEST",
            100,
            91,
            91,
            4,
            true,
            "2026-07-29 15:20:04",
        ),
        metadata(
            "photo-07",
            "IMG_9102.CR3",
            "keep",
            "Canon EOS R5",
            "RF 100mm f/2.8 L MACRO IS USM",
            &["macro", "horology", "watch", "technical"],
            "LUXURY_PRODUCT_CATALOG",
            200,
            97,
            97,
            5,
            true,
            "2026-07-25 11:12:00",
        ),
        metadata(
            "photo-08",
            "DSC_9012.NEF",
            "unrated",
            "NIKON Z9",
            "NIKKOR Z 70-200mm f/2.8 VR S",
            &["landscape", "aerial", "forest", "fog"],
            "ICELAND_EXPEDITION_RAW",
            400,
            92,
            92,
            4,
            true,
            "2026-07-28 07:02:18",
        ),
        metadata(
            "photo-09",
            "DSC_7781.ARW",
            "keep",
            "Sony A7R IV",
            "FE 35mm f/1.4 GM",
            &["street", "tokyo", "neon", "rain", "night"],
            "TOKYO_NIGHT_SERIES",
            1600,
            90,
            89,
            5,
            true,
            "2026-07-24 22:18:40",
        ),
        metadata(
            "photo-10",
            "IMG_4412.CR3",
            "alt",
            "Canon EOS R5",
            "RF 85mm f/1.2 L USM DS",
            &["portrait", "cyan", "lighting", "dramatic"],
            "STUDIO_PORTRAIT_TEST",
            400,
            95,
            96,
            4,
            true,
            "2026-07-26 18:30:15",
        ),
    ]
}

fn metadata(
    id: &str,
    filename: &str,
    decision: &str,
    camera: &str,
    lens: &str,
    tags: &[&str],
    session_name: &str,
    iso: u32,
    sharpness: u8,
    focus_accuracy: u8,
    rating: u8,
    is_raw: bool,
    capture_time: &str,
) -> PhotoMetadata {
    PhotoMetadata {
        id: id.into(),
        filename: filename.into(),
        decision: decision.into(),
        camera: Some(camera.into()),
        lens: Some(lens.into()),
        tags: tags.iter().map(|tag| (*tag).into()).collect(),
        session_name: Some(session_name.into()),
        iso: Some(iso),
        sharpness: Some(sharpness),
        focus_accuracy: Some(focus_accuracy),
        rating: Some(rating),
        is_raw: Some(is_raw),
        capture_time: Some(capture_time.into()),
    }
}

#[cfg(target_arch = "wasm32")]
fn set_browser_status(state: &str, message: &str) {
    let Some(document) = web_sys::window().and_then(|window| window.document()) else {
        return;
    };
    if let Some(root) = document.document_element() {
        let _ = root.set_attribute("data-photo-culler-state", state);
    }
    if let Some(status) = document.get_element_by_id("application-status") {
        status.set_text_content(Some(message));
        let _ = status.set_attribute("data-state", state);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn set_browser_status(_: &str, _: &str) {}

pub struct SlintDispatcher;

impl UiDispatcher for SlintDispatcher {
    fn dispatch(&self, task: Box<dyn FnOnce() + Send>) -> Result<(), DemoError> {
        slint::invoke_from_event_loop(move || task()).map_err(|error| DemoError {
            message: format!("Could not update the Photo Culler window: {error}"),
        })
    }
}

fn ordered_camera_names(items: &[GalleryItem]) -> Vec<String> {
    let mut seen = HashSet::new();
    items
        .iter()
        .filter_map(|item| item.metadata.camera.as_ref())
        .filter(|camera| seen.insert((*camera).clone()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn records() -> Vec<GalleryItem> {
        smoke_fixture_metadata()
            .into_iter()
            .map(|metadata| GalleryItem {
                metadata,
                jpeg: Arc::from([]),
            })
            .collect()
    }

    fn visible_ids(state: &LibraryInteractionState) -> Vec<String> {
        state
            .visible_indices()
            .into_iter()
            .map(|index| state.items[index].metadata.id.clone())
            .collect()
    }

    #[test]
    fn search_and_cull_filters_are_case_insensitive_and_reversible() {
        let mut state = LibraryInteractionState {
            items: records(),
            ..Default::default()
        };
        state.filters.search_query = "dsc_4021".into();
        assert_eq!(visible_ids(&state), ["photo-02"]);
        state.filters.search_query = "BERLIN_ARCH_STUDY".into();
        assert_eq!(visible_ids(&state), ["photo-02"]);
        state.filters.search_query = "NIKON Z9".into();
        assert_eq!(visible_ids(&state), ["photo-08", "photo-01"]);
        state.filters.search_query = "FE 85mm f/1.4".into();
        assert_eq!(visible_ids(&state).len(), 4);
        state.filters.search_query = "tokyo".into();
        assert_eq!(visible_ids(&state), ["photo-09"]);
        state.filters.search_query.clear();
        state.filters.cull_status = "keep".into();
        assert_eq!(visible_ids(&state).len(), 5);
        state.filters.cull_status = "unrated".into();
        assert_eq!(visible_ids(&state), ["photo-06", "photo-08"]);
    }

    #[test]
    fn camera_iso_sharpness_rating_and_raw_filters_compose() {
        let mut state = LibraryInteractionState {
            items: records(),
            ..Default::default()
        };
        state.filters.cameras.insert("NIKON Z9".into());
        state.filters.max_iso = 3_200;
        state.filters.min_sharpness = 90;
        state.filters.min_rating = 4;
        state.filters.raw_only = true;
        assert_eq!(visible_ids(&state), ["photo-08", "photo-01"]);
        state.filters.min_rating = 5;
        assert_eq!(visible_ids(&state), ["photo-01"]);
        state.filters.max_iso = 300;
        assert!(visible_ids(&state).is_empty());
    }

    #[test]
    fn reset_clears_search_and_every_filter_but_keeps_density_and_selection() {
        let mut state = LibraryInteractionState {
            items: records(),
            density: GridDensity::Large,
            ..Default::default()
        };
        state.filters.search_query = "night".into();
        state.filters.cull_status = "reject".into();
        state.filters.cameras.insert("NIKON Z9".into());
        state.filters.max_iso = 3_200;
        state.filters.min_sharpness = 50;
        state.filters.min_rating = 3;
        state.filters.raw_only = true;
        state.selected.insert("photo-03".into());
        state.reset_filters();
        assert_eq!(state.filters, LibraryFilters::default());
        assert_eq!(state.density, GridDensity::Large);
        assert!(state.selected.contains("photo-03"));
        assert_eq!(visible_ids(&state).len(), 10);
    }

    #[test]
    fn camera_filter_order_matches_first_appearance_in_the_reference() {
        assert_eq!(
            ordered_camera_names(&records()),
            ["NIKON Z9", "Sony A7R IV", "Canon EOS R5"]
        );
    }

    #[test]
    fn selection_supports_single_toggle_multiselect_and_visible_select_all() {
        let mut state = LibraryInteractionState {
            items: records(),
            ..Default::default()
        };
        state.request_selection("photo-01", false);
        state.request_selection("photo-02", true);
        assert!(state.selected.contains("photo-01"));
        assert!(state.selected.contains("photo-02"));
        state.request_selection("photo-02", true);
        assert!(!state.selected.contains("photo-02"));

        state.filters.cull_status = "reject".into();
        state.toggle_all_visible();
        assert_eq!(
            state.selected,
            HashSet::from(["photo-01".into(), "photo-05".into()])
        );
        state.toggle_all_visible();
        assert_eq!(state.selected, HashSet::from(["photo-01".into()]));
    }

    #[test]
    fn sort_direction_and_density_match_the_reference_breakpoints() {
        let mut state = LibraryInteractionState {
            items: records(),
            ..Default::default()
        };
        let descending = visible_ids(&state);
        state.sort_descending = false;
        let ascending = visible_ids(&state);
        assert_eq!(
            descending,
            [
                "photo-06", "photo-05", "photo-04", "photo-03", "photo-08", "photo-01", "photo-02",
                "photo-10", "photo-07", "photo-09"
            ]
        );
        assert_eq!(ascending.first().map(String::as_str), Some("photo-09"));
        assert_eq!(GridDensity::Small.columns(1440), 6);
        assert_eq!(GridDensity::Medium.columns(1440), 5);
        assert_eq!(GridDensity::Large.columns(1440), 3);
        assert_eq!(GridDensity::Small.columns(1100), 6);
        assert_eq!(GridDensity::Medium.columns(1100), 4);
        assert_eq!(GridDensity::Large.columns(1100), 3);
        assert_eq!(GridDensity::Medium.columns(700), 2);
    }

    #[test]
    fn smoke_fixture_matches_reference_decisions_cameras_and_distinct_analysis_scores() {
        let records = smoke_fixture_metadata();
        assert_eq!(records.len(), 10);
        assert_eq!(
            records
                .iter()
                .filter(|photo| photo.decision == "keep")
                .count(),
            5
        );
        assert_eq!(
            records
                .iter()
                .filter(|photo| photo.decision == "alt")
                .count(),
            2
        );
        assert_eq!(
            records
                .iter()
                .filter(|photo| photo.decision == "reject")
                .count(),
            1
        );
        assert_eq!(
            records
                .iter()
                .filter(|photo| photo.decision == "unrated")
                .count(),
            2
        );
        assert!(records.iter().all(|photo| photo.is_raw == Some(true)));
        assert_eq!(
            records
                .iter()
                .filter_map(|photo| photo.camera.as_deref())
                .collect::<BTreeSet<_>>(),
            BTreeSet::from(["Canon EOS R5", "NIKON Z9", "Sony A7R IV"])
        );
        assert_eq!(records[0].id, "photo-01");
        assert_eq!(records[0].filename, "DSC_8492.NEF");
        assert_eq!(records[0].focus_accuracy, Some(98));
        assert_eq!(records[0].sharpness, Some(98));
        assert_eq!(records[8].id, "photo-09");
        assert_eq!(records[8].sharpness, Some(90));
        assert_eq!(records[8].focus_accuracy, Some(89));
        assert_eq!(records[9].id, "photo-10");
        assert_eq!(records[9].sharpness, Some(95));
        assert_eq!(records[9].focus_accuracy, Some(96));
    }

    #[test]
    fn live_metadata_stays_unknown_instead_of_becoming_synthetic_defaults() {
        let live = PhotoMetadata {
            id: "demo-001".into(),
            filename: "demo-001.jpg".into(),
            decision: "undecided".into(),
            camera: None,
            lens: None,
            tags: Vec::new(),
            session_name: None,
            iso: None,
            sharpness: None,
            focus_accuracy: None,
            rating: None,
            is_raw: None,
            capture_time: None,
        };
        assert_eq!(live.camera, None);
        assert_eq!(live.lens, None);
        assert!(live.tags.is_empty());
        assert_eq!(live.session_name, None);
        assert_eq!(live.iso, None);
        assert_eq!(live.rating, None);
        assert_eq!(live.is_raw, None);
        let mut filters = LibraryFilters::default();
        filters.search_query = "85mm".into();
        assert!(!filters.matches(&live));
        filters.search_query = "demo-001".into();
        assert!(filters.matches(&live));
    }
}
