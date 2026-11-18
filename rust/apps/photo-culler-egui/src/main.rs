// allow-large-file
//! Native egui/wgpu delivery adapter for the local Photo Culler application API.
//!
//! The UI intentionally does not read SQLite or implement selection/analysis
//! policy.  The local Python service remains the owner of catalog, jobs and
//! non-destructive decisions, so this adapter and the web UI stay consistent.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

const DEFAULT_SERVER: &str = "http://127.0.0.1:8765";

// NEW High-Fidelity Chromium Design Theme Colors
const BG_MAIN: Color32 = Color32::from_rgb(15, 16, 18); // #0F1012
const BG_CARD: Color32 = Color32::from_rgb(17, 17, 17); // #111111
const BG_CARD_HOVER: Color32 = Color32::from_rgb(28, 29, 33); // #1C1D21
const BG_SIDEBAR: Color32 = Color32::from_rgb(5, 5, 5); // #050505
const BORDER_COLOR: Color32 = Color32::from_rgb(25, 25, 25); // #191919 (matches white/10 visually on #050505)

const TEXT_PRIMARY: Color32 = Color32::from_rgb(227, 226, 228); // #e3e2e4
const TEXT_SECONDARY: Color32 = Color32::from_rgb(150, 150, 150); // rgba(255, 255, 255, 0.4)
const TEXT_MUTED: Color32 = Color32::from_rgb(80, 80, 80); // rgba(255, 255, 255, 0.25)

const ACCENT_ORANGE: Color32 = Color32::from_rgb(255, 107, 53); // #FF6B35 (Active/Orange)
const ACCENT_TEAL: Color32 = Color32::from_rgb(37, 161, 142); // #25A18E (Kept/Teal)
const ACCENT_YELLOW: Color32 = Color32::from_rgb(247, 197, 159); // #F7C59F (Alt/Yellow)
const ACCENT_RED: Color32 = Color32::from_rgb(230, 57, 70); // #E63946 (Rejected/Red)

#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewMode {
    Library,
    Dashboard,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum DecisionCategory {
    Kept,
    Alternate,
    Rejected,
    Unrated,
}

impl DecisionCategory {
    fn from_decision(decision: &str) -> Self {
        match decision.to_uppercase().as_str() {
            "BEST" | "KEEP" => Self::Kept,
            "ALTERNATE" | "REVIEW" => Self::Alternate,
            decision if decision.starts_with("REJECT") => Self::Rejected,
            _ => Self::Unrated,
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Kept => ACCENT_TEAL,
            Self::Alternate => ACCENT_YELLOW,
            Self::Rejected => ACCENT_RED,
            Self::Unrated => TEXT_MUTED,
        }
    }

    fn matches_filter(self, filter: &str) -> bool {
        match filter {
            "keep" => self == Self::Kept,
            "alt" => self == Self::Alternate,
            "reject" => self == Self::Rejected,
            "unrated" => self == Self::Unrated,
            _ => true,
        }
    }
}

#[derive(Default)]
struct SelectionStats {
    kept: usize,
    alternate: usize,
    rejected: usize,
    unrated: usize,
}

impl SelectionStats {
    fn add(&mut self, category: DecisionCategory) {
        match category {
            DecisionCategory::Kept => self.kept += 1,
            DecisionCategory::Alternate => self.alternate += 1,
            DecisionCategory::Rejected => self.rejected += 1,
            DecisionCategory::Unrated => self.unrated += 1,
        }
    }
}

fn card_frame(fill: Color32, corner_radius: u8, inner_margin: egui::Margin) -> egui::Frame {
    egui::Frame::new()
        .fill(fill)
        .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR))
        .corner_radius(corner_radius)
        .inner_margin(inner_margin)
}

#[derive(Clone, Debug, Deserialize)]
struct Gallery {
    id: String,
    name: String,
    #[serde(default)]
    photo_count: usize,
}

#[derive(Debug, Deserialize)]
struct GalleryList {
    items: Vec<Gallery>,
}

#[derive(Clone, Debug, Deserialize)]
struct Photo {
    id: String,
    name: String,
    decision: String,
    score: Option<f64>,
    quality_tier: String,
    thumbnail_url: String,
}

#[derive(Debug, Deserialize)]
struct CatalogPage {
    items: Vec<Photo>,
    total: usize,
}

#[derive(Debug, Deserialize)]
struct AnalysisProgress {
    status: String,
    progress: u8,
    processed: usize,
    total: usize,
    profile_name: String,
    message: String,
}

#[derive(Clone, Debug, Deserialize)]
struct SystemUsage {
    cpu_system: f32,
    cpu_app_capacity: f32,
    gpu_system: f32,
    gpu_name: String,
}

#[derive(Serialize)]
struct CreateGallery<'a> {
    name: &'a str,
}

#[derive(Serialize)]
struct ImportRequest<'a> {
    path: &'a str,
    recursive: bool,
    exclude_patterns: Vec<&'a str>,
}

#[derive(Serialize)]
struct DecisionRequest<'a> {
    decision: &'a str,
}

#[derive(Serialize)]
struct AnalysisStart<'a> {
    profile: &'a str,
    scope: &'a str,
}

#[derive(Clone, Debug, Deserialize)]
struct ComponentSummary {
    label: String,
    score_percent: f64,
    weight_percent: f64,
    contribution_points: f64,
    measurement: String,
}

#[derive(Clone, Debug, Deserialize)]
struct AnalysisSummary {
    profile_name: String,
    confidence_percent: f64,
    final_score_percent: f64,
    formula: String,
    components: Option<Vec<ComponentSummary>>,
}

#[derive(Clone, Debug, Deserialize)]
struct CameraMetadata {
    camera_model: Option<String>,
    lens: Option<String>,
    iso: Option<i32>,
    aperture: Option<f64>,
    shutter_speed: Option<String>,
    focal_length: Option<f64>,
    capture_time: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
struct PhotoDetail {
    id: String,
    name: String,
    decision: String,
    score: Option<f64>,
    quality_tier: String,
    analysis_summary: Option<AnalysisSummary>,
    metadata: Option<CameraMetadata>,
}

struct NativeApp {
    server_url: String,
    server_token: Option<String>,
    http: ureq::Agent,
    galleries: Vec<Gallery>,
    active_gallery: Option<String>,
    photos: Vec<Photo>,
    selected_photo: Option<String>,
    selected_texture: Option<TextureHandle>,
    import_path: String,
    new_gallery_name: String,
    create_new_gallery: bool,
    import_recursive: bool,
    analysis_profile: String,
    analysis: Option<AnalysisProgress>,
    sessions_count: usize,
    groups_count: usize,
    exposure: f32,
    status: String,
    last_progress_poll: Instant,

    // NEW Properties matching the Chromium version
    view_mode: ViewMode,
    search_query: String,
    filter_decision: String, // "all", "keep", "alt", "reject", "unrated"
    raw_only: bool,
    system_usage: Option<SystemUsage>,
    last_system_poll: Instant,
    last_import_poll: Instant,
    import_job_active: bool,

    // NEW Auto-refresh, high-fidelity integration, and dashboard stats
    first_refresh: bool,
    total_photos: usize,
    total_files: usize,
    selected_count: usize,
    pending_count: usize,
    temperature: i32,
    tint: f32,
    showing_original: bool,
    selected_photo_detail: Option<PhotoDetail>,
}

impl Default for NativeApp {
    fn default() -> Self {
        Self {
            server_url: std::env::var("PHOTO_CULLER_SERVER")
                .unwrap_or_else(|_| DEFAULT_SERVER.into()),
            server_token: std::env::var("PHOTO_CULLER_SERVER_TOKEN").ok(),
            http: ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(2)))
                .timeout_recv_response(Some(Duration::from_secs(3)))
                .timeout_recv_body(Some(Duration::from_secs(5)))
                .timeout_global(Some(Duration::from_secs(8)))
                .build()
                .new_agent(),
            galleries: Vec::new(),
            active_gallery: None,
            photos: Vec::new(),
            selected_photo: None,
            selected_texture: None,
            import_path: String::new(),
            new_gallery_name: String::new(),
            create_new_gallery: false,
            import_recursive: true,
            analysis_profile: "fast".into(),
            analysis: None,
            sessions_count: 0,
            groups_count: 0,
            exposure: 0.0,
            status: "Conecta con el servicio local para cargar el catálogo.".into(),
            last_progress_poll: Instant::now() - Duration::from_secs(2),

            // NEW Defaults
            view_mode: ViewMode::Library,
            search_query: String::new(),
            filter_decision: "all".into(),
            raw_only: false,
            system_usage: None,
            last_system_poll: Instant::now() - Duration::from_secs(5),
            last_import_poll: Instant::now() - Duration::from_secs(5),
            import_job_active: false,

            // NEW Extended state
            first_refresh: true,
            total_photos: 0,
            total_files: 0,
            selected_count: 0,
            pending_count: 0,
            temperature: 6500,
            tint: 0.0,
            showing_original: false,
            selected_photo_detail: None,
        }
    }
}

impl NativeApp {
    fn endpoint(&self, path: &str) -> String {
        let endpoint = format!("{}{}", self.server_url.trim_end_matches('/'), path);
        match &self.server_token {
            Some(token) => format!(
                "{endpoint}{}token={token}",
                if path.contains('?') { '&' } else { '?' }
            ),
            None => endpoint,
        }
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let response = self
            .http
            .get(&self.endpoint(path))
            .call()
            .map_err(|error| error.to_string())?;
        response
            .into_body()
            .read_json::<T>()
            .map_err(|error| error.to_string())
    }

    fn send_json<T: serde::de::DeserializeOwned, B: Serialize>(
        &self,
        method: &str,
        path: &str,
        body: B,
    ) -> Result<T, String> {
        let url = self.endpoint(path);
        let response = match method {
            "POST" => self.http.post(&url).send_json(&body),
            "PUT" => self.http.put(&url).send_json(&body),
            "PATCH" => self.http.patch(&url).send_json(&body),
            unsupported => return Err(format!("unsupported HTTP method: {unsupported}")),
        }
        .map_err(|error| error.to_string())?;
        response
            .into_body()
            .read_json::<T>()
            .map_err(|error| error.to_string())
    }

    fn refresh(&mut self) {
        match self.get_json::<GalleryList>("/api/v1/galleries") {
            Ok(galleries) => {
                self.galleries = galleries.items;
                if self
                    .active_gallery
                    .as_ref()
                    .is_some_and(|id| !self.galleries.iter().any(|gallery| &gallery.id == id))
                {
                    self.active_gallery = None;
                }
                if self.active_gallery.is_none() {
                    self.active_gallery = self.galleries.first().map(|gallery| gallery.id.clone());
                }
                self.refresh_catalog();
                self.sessions_count = self
                    .get_json::<serde_json::Value>("/api/v1/sessions")
                    .ok()
                    .and_then(|value| value["items"].as_array().map(Vec::len))
                    .unwrap_or(0);
                self.groups_count = self
                    .get_json::<serde_json::Value>("/api/v1/groups")
                    .ok()
                    .and_then(|value| value["items"].as_array().map(Vec::len))
                    .unwrap_or(0);

                if let Ok(summary) = self.get_json::<serde_json::Value>("/api/v1/summary") {
                    self.total_photos = summary["total_photos"].as_u64().unwrap_or(0) as usize;
                    self.total_files = summary["total_files"].as_u64().unwrap_or(0) as usize;
                    self.selected_count = summary["selected_count"].as_u64().unwrap_or(0) as usize;
                    self.pending_count = summary["pending_count"].as_u64().unwrap_or(0) as usize;
                }

                self.status = "Catálogo actualizado.".into();
            }
            Err(error) => self.status = format!("No se pudo conectar: {error}"),
        }
    }

    fn refresh_catalog(&mut self) {
        let suffix = self
            .active_gallery
            .as_ref()
            .map(|gallery| format!("?gallery_id={gallery}"))
            .unwrap_or_default();
        match self.get_json::<CatalogPage>(&format!("/api/v1/catalog{suffix}")) {
            Ok(page) => {
                self.photos = page.items;
                self.total_photos = page.total;
                self.status = format!("{} fotos cargadas.", page.total);
            }
            Err(error) => self.status = format!("No se pudo cargar el catálogo: {error}"),
        }
    }

    fn import_gallery(&mut self) {
        let gallery_id = if self.create_new_gallery || self.active_gallery.is_none() {
            let name = self.new_gallery_name.trim();
            if name.is_empty() {
                self.status = "Indica un nombre para la primera galería.".into();
                return;
            }
            match self.send_json::<serde_json::Value, _>(
                "POST",
                "/api/v1/galleries",
                CreateGallery { name },
            ) {
                Ok(value) => value["id"].as_str().unwrap_or_default().to_owned(),
                Err(error) => {
                    self.status = format!("No se pudo crear la galería: {error}");
                    return;
                }
            }
        } else {
            self.active_gallery.clone().expect("checked above")
        };
        let path = self.import_path.trim();
        if path.is_empty() {
            self.status = "Indica la carpeta que quieres importar.".into();
            return;
        }
        match self.send_json::<serde_json::Value, _>(
            "POST",
            &format!("/api/v1/galleries/{gallery_id}/imports"),
            ImportRequest {
                path,
                recursive: self.import_recursive,
                exclude_patterns: Vec::new(),
            },
        ) {
            Ok(_) => {
                self.status =
                    "Importación encolada; el catálogo se actualizará al terminar.".into();
                self.refresh();
            }
            Err(error) => self.status = format!("No se pudo iniciar la importación: {error}"),
        }
    }

    fn load_preview_texture(&mut self, ctx: &egui::Context) {
        let Some(photo_id) = self.selected_photo.clone() else { return };
        let url = if self.showing_original {
            format!("/thumbnails/{photo_id}/800")
        } else {
            format!("/api/v1/photos/{photo_id}/edit-preview?max_size=800")
        };

        match self.http.get(&self.endpoint(&url)).call() {
            Ok(response) => match response.into_body().read_to_vec() {
                Ok(bytes) => match image::load_from_memory(&bytes) {
                    Ok(image) => {
                        let image = image.to_rgb8();
                        let size = [image.width() as usize, image.height() as usize];
                        self.selected_texture = Some(ctx.load_texture(
                            "selected-photo-preview",
                            ColorImage::from_rgb(size, image.as_raw()),
                            TextureOptions::LINEAR,
                        ));
                    }
                    Err(error) => {
                        self.status = format!("No se pudo decodificar la vista previa: {error}")
                    }
                },
                Err(error) => self.status = format!("No se pudo leer la vista previa: {error}"),
            },
            Err(error) => self.status = format!("No se pudo cargar la vista previa: {error}"),
        }
    }

    fn select_photo(&mut self, ctx: &egui::Context, photo_id: String, _thumbnail_url: String) {
        self.selected_photo = Some(photo_id.clone());
        self.selected_photo_detail = None;
        self.showing_original = false;

        // Fetch detailed photo info (score explanation, metadata, components)
        match self.get_json::<PhotoDetail>(&format!("/api/v1/photos/{photo_id}")) {
            Ok(detail) => {
                self.selected_photo_detail = Some(detail);
            }
            Err(error) => {
                self.status = format!("No se pudo cargar los detalles de la foto: {error}");
            }
        }

        // Fetch edit recipe
        match self.get_json::<serde_json::Value>(&format!("/api/v1/photos/{photo_id}/edit")) {
            Ok(edit_doc) => {
                if let Some(recipe) = edit_doc.get("recipe") {
                    self.exposure = recipe.get("exposure").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    self.temperature = recipe.get("temperature").and_then(|v| v.as_i64()).unwrap_or(6500) as i32;
                    self.tint = recipe.get("tint").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                }
            }
            Err(error) => {
                self.status = format!("No se pudo cargar la receta de edición: {error}");
            }
        }

        // Load preview texture (with non-destructive edits applied)
        self.load_preview_texture(ctx);
    }

    fn set_decision(&mut self, decision: &str) {
        let Some(photo_id) = self.selected_photo.clone() else {
            self.status = "Selecciona una foto antes de decidir.".into();
            return;
        };
        match self.send_json::<serde_json::Value, _>(
            "PUT",
            &format!("/api/v1/photos/{photo_id}/decision"),
            DecisionRequest { decision },
        ) {
            Ok(_) => {
                self.status = format!("Decisión guardada: {decision}.");
                self.refresh_catalog();

                // Live refresh detail to immediately reflect badge change
                if let Ok(detail) = self.get_json::<PhotoDetail>(&format!("/api/v1/photos/{photo_id}")) {
                    self.selected_photo_detail = Some(detail);
                }
            }
            Err(error) => self.status = format!("No se pudo guardar la decisión: {error}"),
        }
    }

    fn start_analysis(&mut self) {
        match self.send_json::<AnalysisProgress, _>(
            "POST",
            "/api/v1/analysis/start",
            AnalysisStart {
                profile: &self.analysis_profile,
                scope: "remaining",
            },
        ) {
            Ok(progress) => {
                self.analysis = Some(progress);
                self.status = "Análisis iniciado.".into();
            }
            Err(error) => self.status = format!("No se pudo iniciar el análisis: {error}"),
        }
    }

    fn control_analysis(&mut self, action: &str) {
        match self.send_json::<AnalysisProgress, _>(
            "POST",
            &format!("/api/v1/analysis/{action}"),
            (),
        ) {
            Ok(progress) => self.analysis = Some(progress),
            Err(error) => self.status = format!("No se pudo {action} el análisis: {error}"),
        }
    }

    fn update_edit(&mut self) {
        let Some(photo_id) = self.selected_photo.clone() else {
            self.status = "Selecciona una foto para editar.".into();
            return;
        };
        match self.send_json::<serde_json::Value, _>(
            "PATCH",
            &format!("/api/v1/photos/{photo_id}/edit"),
            serde_json::json!({
                "exposure": self.exposure,
                "temperature": self.temperature,
                "tint": self.tint
            }),
        ) {
            Ok(_) => self.status = "Receta no destructiva guardada.".into(),
            Err(error) => self.status = format!("No se pudo guardar la edición: {error}"),
        }
    }

    fn edit_history(&mut self, action: &str) {
        let Some(photo_id) = self.selected_photo.clone() else {
            self.status = "Selecciona una foto para editar.".into();
            return;
        };
        match self.send_json::<serde_json::Value, _>(
            "POST",
            &format!("/api/v1/photos/{photo_id}/edit/{action}"),
            (),
        ) {
            Ok(edit_doc) => {
                self.status = format!("Edición: {action}.");
                if let Some(recipe) = edit_doc.get("recipe") {
                    self.exposure = recipe.get("exposure").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                    self.temperature = recipe.get("temperature").and_then(|v| v.as_i64()).unwrap_or(6500) as i32;
                    self.tint = recipe.get("tint").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
                }
            }
            Err(error) => self.status = format!("No se pudo {action}: {error}"),
        }
    }

    fn poll_analysis(&mut self) {
        if self.last_progress_poll.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_progress_poll = Instant::now();
        if let Ok(progress) = self.get_json::<AnalysisProgress>("/api/v1/analysis/progress") {
            let transitioned = if let Some(old) = &self.analysis {
                let old_active = old.status == "running" || old.status == "paused";
                let new_active = progress.status == "running" || progress.status == "paused";
                old_active && !new_active
            } else {
                false
            };
            self.analysis = Some(progress);
            if transitioned {
                self.refresh();
            }
        }
    }

    fn poll_import_jobs(&mut self) {
        let interval = if self.import_job_active {
            Duration::from_millis(1500)
        } else {
            Duration::from_secs(5)
        };
        if self.last_import_poll.elapsed() < interval {
            return;
        }
        self.last_import_poll = Instant::now();
        if let Ok(jobs_val) = self.get_json::<serde_json::Value>("/api/v1/import-jobs") {
            if let Some(items) = jobs_val["items"].as_array() {
                let any_active = items.iter().any(|job| {
                    let state = job["state"].as_str().unwrap_or("");
                    state == "queued" || state == "discovering" || state == "previewing" || state == "analyzing"
                });
                if self.import_job_active && !any_active {
                    self.refresh();
                }
                self.import_job_active = any_active;
            }
        }
    }

    // NEW Poll Real-Time System Telemetry
    fn poll_system_usage(&mut self) {
        if self.last_system_poll.elapsed() < Duration::from_secs(3) {
            return;
        }
        self.last_system_poll = Instant::now();
        if let Ok(usage) = self.get_json::<SystemUsage>("/api/v1/system-usage") {
            self.system_usage = Some(usage);
        }
    }
}

impl eframe::App for NativeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Run automatic initial load/refresh on startup
        if self.first_refresh {
            self.first_refresh = false;
            self.refresh();
        }

        self.poll_analysis();
        self.poll_import_jobs();
        self.poll_system_usage();

        // ----------------- TOP PANEL (HEADER) -----------------
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // CULLER / 01 Brand Logo
                    ui.label(
                        RichText::new("CULLER")
                            .size(17.0)
                            .strong()
                            .color(Color32::WHITE)
                            .italics(),
                    );
                    ui.label(
                        RichText::new("/ 01")
                            .size(17.0)
                            .strong()
                            .color(ACCENT_ORANGE),
                    );

                    ui.add_space(20.0);
                    let separator_stroke = egui::Stroke::new(1.0_f32, BORDER_COLOR);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
                    ui.painter()
                        .line_segment([rect.left_top(), rect.left_bottom()], separator_stroke);
                    ui.add_space(20.0);

                    // View Switcher Buttons (Library, Dashboard)
                    let lib_selected = self.view_mode == ViewMode::Library;
                    let dash_selected = self.view_mode == ViewMode::Dashboard;

                    if ui
                        .selectable_label(
                            lib_selected,
                            RichText::new("LIBRARY").size(11.0).strong(),
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::Library;
                    }
                    ui.add_space(8.0);
                    if ui
                        .selectable_label(
                            dash_selected,
                            RichText::new("DASHBOARD").size(11.0).strong(),
                        )
                        .clicked()
                    {
                        self.view_mode = ViewMode::Dashboard;
                    }

                    ui.add_space(20.0);
                    ui.label(RichText::new(&self.status).size(12.0).color(TEXT_SECONDARY));

                    // Server endpoint settings
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("↻ Actualizar").strong()).clicked() {
                            self.refresh();
                        }
                        ui.add_sized(
                            [200.0, 26.0],
                            egui::TextEdit::singleline(&mut self.server_url)
                                .hint_text("Servicio local"),
                        );
                    });
                });
            });

        // ----------------- BOTTOM PANEL (TELEMETRY) -----------------
        egui::TopBottomPanel::bottom("bottom")
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(20, 8))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(usage) = &self.system_usage {
                        // CPU Sistema
                        ui.label(
                            RichText::new("CPU SISTEMA:")
                                .size(9.0)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(format!("{:.1}%", usage.cpu_system))
                                .size(10.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.add(
                            egui::ProgressBar::new(usage.cpu_system / 100.0)
                                .fill(Color32::from_rgb(56, 139, 253))
                                .desired_width(50.0),
                        );

                        ui.add_space(15.0);

                        // CPU App
                        ui.label(
                            RichText::new("CPU APP (CAP.):")
                                .size(9.0)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(format!("{:.1}%", usage.cpu_app_capacity))
                                .size(10.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.add(
                            egui::ProgressBar::new(usage.cpu_app_capacity / 100.0)
                                .fill(ACCENT_TEAL)
                                .desired_width(50.0),
                        );

                        ui.add_space(15.0);

                        // GPU Usage
                        ui.label(
                            RichText::new("GPU:")
                                .size(9.0)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                        ui.label(
                            RichText::new(format!("{:.1}%", usage.gpu_system))
                                .size(10.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                        ui.add(
                            egui::ProgressBar::new(usage.gpu_system / 100.0)
                                .fill(ACCENT_ORANGE)
                                .desired_width(50.0),
                        );

                        if !usage.gpu_name.is_empty() && usage.gpu_name != "N/A" {
                            ui.add_space(5.0);
                            ui.label(
                                RichText::new(format!("({})", usage.gpu_name))
                                    .size(9.0)
                                    .color(TEXT_MUTED),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("CARGANDO TELEMETRIA...")
                                .size(9.0)
                                .color(TEXT_SECONDARY),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new("PHOTO CULLER V0.1.0")
                                .size(9.0)
                                .color(TEXT_SECONDARY),
                        );
                    });
                });
            });

        // ----------------- LEFT PANEL (NAVIGATION & STATISTICS) -----------------
        egui::SidePanel::left("left_sidebar")
            .default_width(220.0)
            .min_width(200.0)
            .max_width(260.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::same(16))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                // Catalog / Archive Branding Block
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 6, BG_CARD);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "📂",
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );

                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("CATALOG_01")
                                .size(11.0)
                                .strong()
                                .italics()
                                .color(Color32::WHITE),
                        );
                        ui.label(
                            RichText::new("Local Archive")
                                .size(8.0)
                                .strong()
                                .color(TEXT_SECONDARY),
                        );
                    });
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                // Galleries header
                ui.label(
                    RichText::new("GALERÍAS")
                        .size(9.0)
                        .strong()
                        .color(TEXT_SECONDARY),
                );
                ui.add_space(6.0);

                // Scroll area for galleries
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        let mut selected_gallery_id = None;
                        for gallery in &self.galleries {
                            let selected =
                                self.active_gallery.as_deref() == Some(gallery.id.as_str());

                            let text = format!("{} ({})", gallery.name, gallery.photo_count);
                            if ui
                                .selectable_label(selected, RichText::new(text).size(13.0))
                                .clicked()
                            {
                                selected_gallery_id = Some(gallery.id.clone());
                            }
                            ui.add_space(4.0);
                        }
                        if let Some(gallery_id) = selected_gallery_id {
                            self.active_gallery = Some(gallery_id);
                            self.refresh_catalog();
                        }
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // Filters panel (visible if Library View is selected)
                if self.view_mode == ViewMode::Library {
                    ui.label(
                        RichText::new("Filtros locales")
                            .size(9.0)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.add_space(6.0);

                    // Search box
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("🔍").size(10.0));
                        ui.add(
                            egui::TextEdit::singleline(&mut self.search_query)
                                .hint_text("Buscar nombre o etiqueta...")
                                .desired_width(ui.available_width()),
                        );
                    });
                    ui.add_space(6.0);

                    // Decision State filters dropdown/buttons
                    ui.label(RichText::new("Decisión").size(9.0).color(TEXT_SECONDARY));
                    egui::ComboBox::from_id_salt("decision_filter_combo")
                        .selected_text(self.filter_decision.to_uppercase())
                        .width(ui.available_width())
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.filter_decision,
                                "all".to_string(),
                                "TODOS",
                            );
                            ui.selectable_value(
                                &mut self.filter_decision,
                                "keep".to_string(),
                                "KEPT (BEST/KEEP)",
                            );
                            ui.selectable_value(
                                &mut self.filter_decision,
                                "alt".to_string(),
                                "ALT (REVIEW/ALT)",
                            );
                            ui.selectable_value(
                                &mut self.filter_decision,
                                "reject".to_string(),
                                "REJECTED",
                            );
                            ui.selectable_value(
                                &mut self.filter_decision,
                                "unrated".to_string(),
                                "UNRATED",
                            );
                        });
                    ui.add_space(6.0);

                    // RAW checkbox
                    ui.checkbox(&mut self.raw_only, "Solo RAW");

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                }

                // Selection Stats Block
                ui.label(
                    RichText::new("SELECTION STATS")
                        .size(9.0)
                        .strong()
                        .color(TEXT_MUTED),
                );
                ui.add_space(6.0);

                // Calculate current catalog stats
                let mut selection_stats = SelectionStats::default();
                for p in &self.photos {
                    selection_stats.add(DecisionCategory::from_decision(&p.decision));
                }

                // Kept Row
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, ACCENT_TEAL);
                    ui.label(
                        RichText::new("KEPT")
                            .size(11.0)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(selection_stats.kept.to_string())
                                .size(11.0)
                                .strong()
                                .color(ACCENT_TEAL),
                        );
                    });
                });
                ui.add_space(4.0);

                // Alt Row
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter()
                        .circle_filled(rect.center(), 3.0, ACCENT_YELLOW);
                    ui.label(
                        RichText::new("ALT (PICK)")
                            .size(11.0)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(selection_stats.alternate.to_string())
                                .size(11.0)
                                .strong()
                                .color(ACCENT_YELLOW),
                        );
                    });
                });
                ui.add_space(4.0);

                // Rejected Row
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, ACCENT_RED);
                    ui.label(
                        RichText::new("REJECTED")
                            .size(11.0)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(selection_stats.rejected.to_string())
                                .size(11.0)
                                .strong()
                                .color(ACCENT_RED),
                        );
                    });
                });
                ui.add_space(4.0);

                // Unrated Row
                ui.horizontal(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, TEXT_MUTED);
                    ui.label(
                        RichText::new("UNRATED")
                            .size(11.0)
                            .strong()
                            .color(TEXT_SECONDARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(selection_stats.unrated.to_string())
                                .size(11.0)
                                .strong()
                                .color(TEXT_PRIMARY),
                        );
                    });
                });

                // Operator Footer
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        // Round Operator Avatar Frame
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 14.0, BG_CARD);
                        ui.painter().circle_stroke(
                            rect.center(),
                            14.0,
                            egui::Stroke::new(1.0_f32, BORDER_COLOR),
                        );
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "OP",
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );

                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("OPERATOR_01")
                                    .size(11.0)
                                    .strong()
                                    .italics()
                                    .color(Color32::WHITE),
                            );
                            ui.horizontal(|ui| {
                                let (dot_rect, _) = ui.allocate_exact_size(
                                    egui::vec2(6.0, 6.0),
                                    egui::Sense::hover(),
                                );
                                ui.painter()
                                    .circle_filled(dot_rect.center(), 3.0, ACCENT_TEAL);
                                ui.label(
                                    RichText::new("ACTIVE SESSION")
                                        .size(8.0)
                                        .strong()
                                        .color(ACCENT_TEAL),
                                );
                            });
                        });
                    });
                });
            });

        // ----------------- RIGHT PANEL (TOOLS & ANALYSIS & PHOTO DETAILS) -----------------
        egui::SidePanel::right("right_sidebar")
            .default_width(280.0)
            .min_width(260.0)
            .max_width(320.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::same(16))
                    .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(detail) = self.selected_photo_detail.clone() {
                        // ----------------- SELECTED PHOTO INSPECTOR -----------------
                        // 1. EVALUACIÓN TÉCNICA
                        ui.label(
                            RichText::new("EVALUACIÓN TÉCNICA")
                                .size(9.0)
                                .strong()
                                .color(TEXT_MUTED),
                        );
                        ui.add_space(4.0);

                        let score_val = detail.score.map_or(0, |s| (s * 100.0).round() as i32);
                        let frame = egui::Frame::new()
                            .fill(BG_CARD)
                            .stroke(egui::Stroke::new(1.0_f32, BORDER_COLOR))
                            .corner_radius(8)
                            .inner_margin(egui::Margin::symmetric(14, 10));

                        frame.show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(score_val.to_string())
                                        .size(28.0)
                                        .strong()
                                        .color(ACCENT_TEAL),
                                );
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new("PUNTUACIÓN TOTAL / 100")
                                            .size(8.0)
                                            .color(TEXT_SECONDARY)
                                            .strong(),
                                    );
                                    ui.label(
                                        RichText::new(detail.quality_tier.to_uppercase())
                                            .size(10.0)
                                            .strong()
                                            .color(ACCENT_TEAL),
                                    );
                                });
                            });
                        });
                        ui.add_space(8.0);

                        // POR QUÉ ESTA PUNTUACIÓN (detailed components breakdown)
                        if let Some(summary) = &detail.analysis_summary {
                            ui.collapsing(
                                RichText::new("POR QUÉ ESTA PUNTUACIÓN")
                                    .size(10.0)
                                    .strong()
                                    .color(TEXT_SECONDARY),
                                |ui| {
                                    ui.label(
                                        RichText::new(format!(
                                            "Perfil {} · confianza {}%",
                                            summary.profile_name,
                                            summary.confidence_percent.round() as i32
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
                                    );
                                    ui.label(
                                        RichText::new(format!(
                                            "{} = {}",
                                            summary.final_score_percent.round() as i32,
                                            summary.formula
                                        ))
                                        .size(10.0)
                                        .color(TEXT_MUTED),
                                    );
                                    ui.add_space(4.0);

                                    if let Some(components) = &summary.components {
                                        for comp in components {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{}: {}/100",
                                                        comp.label,
                                                        comp.score_percent.round() as i32
                                                    ))
                                                    .size(11.0)
                                                    .strong()
                                                    .color(TEXT_PRIMARY),
                                                );
                                                ui.label(
                                                    RichText::new(format!(
                                                        "peso {}% · aporta {:.1} puntos",
                                                        comp.weight_percent.round() as i32,
                                                        comp.contribution_points
                                                    ))
                                                    .size(9.0)
                                                    .color(TEXT_SECONDARY),
                                                );
                                                ui.label(
                                                    RichText::new(&comp.measurement)
                                                        .size(9.0)
                                                        .color(TEXT_MUTED),
                                                );
                                                ui.add_space(4.0);
                                            });
                                        }
                                    }
                                },
                            );
                        } else {
                            ui.label(
                                RichText::new("Aún no hay mediciones guardadas para explicar esta nota.")
                                    .size(10.0)
                                    .color(TEXT_MUTED)
                                    .italics(),
                            );
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // 2. EDICIÓN NO DESTRUCTIVA
                        ui.heading("Edición no destructiva");
                        ui.label(
                            RichText::new("Conserva el archivo original intacto.")
                                .size(11.0)
                                .color(TEXT_SECONDARY),
                        );
                        ui.add_space(6.0);

                        let exposure_val = self.exposure;
                        let temp_val = self.temperature;
                        let tint_val = self.tint;

                        let mut changed = false;
                        if ui
                            .add(
                                egui::Slider::new(&mut self.exposure, -5.0..=5.0)
                                    .text(format!("Exposición ({:.1} EV)", exposure_val)),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .add(
                                egui::Slider::new(&mut self.temperature, 2000..=12000)
                                    .text(format!("Temperatura ({} K)", temp_val)),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .add(
                                egui::Slider::new(&mut self.tint, -100.0..=100.0)
                                    .text(format!("Tint ({:.0})", tint_val)),
                            )
                            .changed()
                        {
                            changed = true;
                        }

                        if changed {
                            self.update_edit();
                            self.load_preview_texture(ctx);
                        }
                        ui.add_space(6.0);

                        ui.columns(3, |cols| {
                            if cols[0].button("Deshacer").clicked() {
                                self.edit_history("undo");
                                self.load_preview_texture(ctx);
                            }
                            if cols[1].button("Rehacer").clicked() {
                                self.edit_history("redo");
                                self.load_preview_texture(ctx);
                            }

                            let before_btn_text = if self.showing_original {
                                "Volver a edición"
                            } else {
                                "Ver original"
                            };
                            if cols[2].button(before_btn_text).clicked() {
                                self.showing_original = !self.showing_original;
                                self.load_preview_texture(ctx);
                            }
                        });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // 3. CAMERA SPECIFICATIONS
                        ui.heading("Camera Specifications");
                        ui.add_space(6.0);

                        if let Some(meta) = &detail.metadata {
                            egui::Grid::new("camera-meta-grid")
                                .num_columns(2)
                                .spacing([10.0, 6.0])
                                .show(ui, |ui| {
                                    ui.label(RichText::new("Camera").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(
                                            meta.camera_model.as_deref().unwrap_or("N/A"),
                                        )
                                        .strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("Lens").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(meta.lens.as_deref().unwrap_or("N/A"))
                                            .strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("ISO").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(
                                            meta.iso
                                                .map_or("N/A".to_string(), |i| i.to_string()),
                                        )
                                        .strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("Aperture").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(
                                            meta.aperture.map_or("N/A".to_string(), |a| {
                                                format!("f/{:.1}", a)
                                            }),
                                        )
                                        .strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("Exposure").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(
                                            meta.shutter_speed.as_deref().unwrap_or("N/A"),
                                        )
                                        .strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("Focal Length").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(
                                            meta.focal_length.map_or("N/A".to_string(), |f| {
                                                format!("{:.1} mm", f)
                                            }),
                                        )
                                        .strong(),
                                    );
                                    ui.end_row();

                                    ui.label(RichText::new("Captured").color(TEXT_SECONDARY));
                                    ui.label(
                                        RichText::new(
                                            meta.capture_time.as_deref().unwrap_or("N/A"),
                                        )
                                        .strong(),
                                    );
                                    ui.end_row();
                                });
                        } else {
                            ui.label(
                                RichText::new("No hay especificaciones disponibles.")
                                    .size(10.0)
                                    .color(TEXT_MUTED)
                                    .italics(),
                            );
                        }

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);

                        // 4. SET STATUS DECISION
                        ui.heading("Set Status Decision");
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Current State:").size(11.0).color(TEXT_SECONDARY));
                            let category = DecisionCategory::from_decision(&detail.decision);
                            ui.label(
                                RichText::new(detail.decision.to_uppercase())
                                    .size(11.0)
                                    .strong()
                                    .color(category.color()),
                            );
                        });
                        ui.add_space(8.0);

                        egui::Grid::new("decision-grid-inspector")
                            .num_columns(2)
                            .spacing([8.0, 8.0])
                            .show(ui, |ui| {
                                if ui
                                    .add_sized(
                                        [120.0, 32.0],
                                        egui::Button::new(
                                            RichText::new("[1] Mark as Best")
                                                .strong()
                                                .color(ACCENT_TEAL),
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.set_decision("best");
                                }
                                if ui
                                    .add_sized(
                                        [120.0, 32.0],
                                        egui::Button::new(
                                            RichText::new("[2] Mark as Keep")
                                                .strong()
                                                .color(ACCENT_TEAL),
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.set_decision("keep");
                                }
                                ui.end_row();
                                if ui
                                    .add_sized(
                                        [120.0, 32.0],
                                        egui::Button::new(
                                            RichText::new("[4] Mark for Review")
                                                .strong()
                                                .color(ACCENT_YELLOW),
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.set_decision("review");
                                }
                                if ui
                                    .add_sized(
                                        [120.0, 32.0],
                                        egui::Button::new(
                                            RichText::new("[X] Reject Photo")
                                                .strong()
                                                .color(ACCENT_RED),
                                        ),
                                    )
                                    .clicked()
                                {
                                    self.set_decision("reject");
                                }
                                ui.end_row();
                            });

                        ui.add_space(10.0);
                        ui.separator();
                        ui.add_space(10.0);
                    } else {
                        // Show warning that no photo is selected
                        ui.label(
                            RichText::new("Selecciona una foto para ver sus detalles.")
                                .size(12.0)
                                .color(TEXT_SECONDARY)
                                .italics(),
                        );
                        ui.add_space(10.0);
                    }

                    // --- General Collapsible Tools ---
                    ui.collapsing(
                        RichText::new("Herramientas de Análisis")
                            .size(12.0)
                            .strong(),
                        |ui| {
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Perfil").color(TEXT_SECONDARY));
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.analysis_profile)
                                        .desired_width(120.0),
                                );
                            });
                            ui.add_space(4.0);

                            if ui
                                .add_sized(
                                    [ui.available_width(), 32.0],
                                    egui::Button::new(
                                        RichText::new("Analizar pendientes").strong(),
                                    ),
                                )
                                .clicked()
                            {
                                self.start_analysis();
                            }
                            ui.add_space(6.0);

                            ui.columns(3, |cols| {
                                if cols[0].button("Pausar").clicked() {
                                    self.control_analysis("pause");
                                }
                                if cols[1].button("Reanudar").clicked() {
                                    self.control_analysis("resume");
                                }
                                if cols[2].button("Cancelar").clicked() {
                                    self.control_analysis("cancel");
                                }
                            });

                            if let Some(progress) = &self.analysis {
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(format!(
                                        "{} — {}",
                                        progress.profile_name, progress.status
                                    ))
                                    .strong()
                                    .color(ACCENT_ORANGE),
                                );
                                ui.add(
                                    egui::ProgressBar::new(f32::from(progress.progress) / 100.0)
                                        .fill(ACCENT_TEAL),
                                );
                                ui.label(
                                    RichText::new(format!(
                                        "{}/{} fotos",
                                        progress.processed, progress.total
                                    ))
                                    .size(11.0)
                                    .color(TEXT_SECONDARY),
                                );
                                ui.label(
                                    RichText::new(&progress.message)
                                        .size(10.0)
                                        .color(TEXT_MUTED),
                                );
                            }
                        },
                    );

                    ui.add_space(6.0);

                    ui.collapsing(
                        RichText::new("Importar desde Carpeta")
                            .size(12.0)
                            .strong(),
                        |ui| {
                            ui.add_space(4.0);
                            ui.checkbox(&mut self.create_new_gallery, "Crear galería nueva");
                            if self.create_new_gallery {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.new_gallery_name)
                                        .hint_text("Nombre de la galería"),
                                );
                                ui.add_space(4.0);
                            }
                            ui.add(
                                egui::TextEdit::singleline(&mut self.import_path)
                                    .hint_text("Carpeta local de fotos"),
                            );
                            ui.checkbox(&mut self.import_recursive, "Incluir subdirectorios");
                            ui.add_space(6.0);
                            if ui
                                .add_sized(
                                    [ui.available_width(), 32.0],
                                    egui::Button::new(
                                        RichText::new("Importar carpeta").strong(),
                                    )
                                    .fill(ACCENT_ORANGE),
                                )
                                .clicked()
                            {
                                self.import_gallery();
                            }
                        },
                    );
                });
            });

        // ----------------- CENTRAL PANEL (MAIN ACTIVE VIEW) -----------------
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::new()
                .fill(BG_MAIN)
                .inner_margin(egui::Margin::same(20))
                .show(ui, |ui| {
                    match self.view_mode {
                        ViewMode::Dashboard => {
                            // ----------------- DASHBOARD VIEW -----------------
                            ui.vertical(|ui| {
                                ui.heading("Dashboard / Resumen");
                                ui.add_space(12.0);

                                // Responsive Bento-Grid layout using vertical & horizontal stacks
                                ui.horizontal(|ui| {
                                    // Card 1: Total Photos
                                    let frame_1 =
                                        card_frame(BG_CARD, 12, egui::Margin::symmetric(18, 14));
                                    frame_1.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new("TOTAL FOTOS")
                                                        .size(9.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(self.total_photos.to_string())
                                                        .size(28.0)
                                                        .strong()
                                                        .color(Color32::WHITE),
                                                );
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{} ARCHIVOS",
                                                        self.total_files
                                                    ))
                                                    .size(9.0)
                                                    .color(ACCENT_ORANGE)
                                                    .strong(),
                                                );
                                            });
                                        });
                                    });

                                    ui.add_space(12.0);

                                    // Card 2: Selections (Best + Keep)
                                    let frame_2 =
                                        card_frame(BG_CARD, 12, egui::Margin::symmetric(18, 14));
                                    frame_2.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new("SELECCIONES")
                                                        .size(9.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(self.selected_count.to_string())
                                                        .size(28.0)
                                                        .strong()
                                                        .color(ACCENT_TEAL),
                                                );
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new("BEST + KEEP")
                                                        .size(9.0)
                                                        .color(ACCENT_TEAL)
                                                        .strong(),
                                                );
                                            });
                                        });
                                    });

                                    ui.add_space(12.0);

                                    // Card 3: Pending (Review + Unprocessed)
                                    let frame_3 =
                                        card_frame(BG_CARD, 12, egui::Margin::symmetric(18, 14));
                                    frame_3.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new("PENDIENTES")
                                                        .size(9.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new(self.pending_count.to_string())
                                                        .size(28.0)
                                                        .strong()
                                                        .color(ACCENT_YELLOW),
                                                );
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new("REVIEW + UNPROCESSED")
                                                        .size(9.0)
                                                        .color(ACCENT_YELLOW)
                                                        .strong(),
                                                );
                                            });
                                        });
                                    });

                                    ui.add_space(12.0);

                                    // Card 4: Rejected Count
                                    let frame_4 =
                                        card_frame(BG_CARD, 12, egui::Margin::symmetric(18, 14));
                                    frame_4.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(
                                                    RichText::new("RECHAZADAS")
                                                        .size(9.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(4.0);
                                                let rejected_count = self.total_photos.saturating_sub(self.selected_count + self.pending_count);
                                                ui.label(
                                                    RichText::new(rejected_count.to_string())
                                                        .size(28.0)
                                                        .strong()
                                                        .color(ACCENT_RED),
                                                );
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(
                                                    RichText::new("REJECT")
                                                        .size(9.0)
                                                        .color(ACCENT_RED)
                                                        .strong(),
                                                );
                                            });
                                        });
                                    });
                                });

                                ui.add_space(20.0);

                                // System Overview section
                                ui.heading("System Performance");
                                ui.add_space(10.0);

                                let usage_frame = card_frame(BG_CARD, 12, egui::Margin::same(16));

                                usage_frame.show(ui, |ui| {
                                    if let Some(usage) = &self.system_usage {
                                        ui.columns(3, |cols| {
                                            // Column 1: CPU System Info
                                            cols[0].vertical(|ui| {
                                                ui.label(
                                                    RichText::new("CPU SISTEMA")
                                                        .size(10.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(6.0);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{:.1}%",
                                                        usage.cpu_system
                                                    ))
                                                    .size(24.0)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                                );
                                                ui.add_space(8.0);
                                                ui.add(
                                                    egui::ProgressBar::new(
                                                        usage.cpu_system / 100.0,
                                                    )
                                                    .fill(Color32::from_rgb(56, 139, 253)),
                                                );
                                            });

                                            // Column 2: CPU App Info
                                            cols[1].vertical(|ui| {
                                                ui.label(
                                                    RichText::new("CPU APLICACIÓN (CAP.)")
                                                        .size(10.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(6.0);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{:.1}%",
                                                        usage.cpu_app_capacity
                                                    ))
                                                    .size(24.0)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                                );
                                                ui.add_space(8.0);
                                                ui.add(
                                                    egui::ProgressBar::new(
                                                        usage.cpu_app_capacity / 100.0,
                                                    )
                                                    .fill(ACCENT_TEAL),
                                                );
                                            });

                                            // Column 3: GPU Info
                                            cols[2].vertical(|ui| {
                                                ui.label(
                                                    RichText::new("GPU")
                                                        .size(10.0)
                                                        .strong()
                                                        .color(TEXT_SECONDARY),
                                                );
                                                ui.add_space(6.0);
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{:.1}%",
                                                        usage.gpu_system
                                                    ))
                                                    .size(24.0)
                                                    .strong()
                                                    .color(Color32::WHITE),
                                                );
                                                ui.add_space(8.0);
                                                ui.add(
                                                    egui::ProgressBar::new(
                                                        usage.gpu_system / 100.0,
                                                    )
                                                    .fill(ACCENT_ORANGE),
                                                );
                                            });
                                        });
                                    } else {
                                        ui.label("Cargando métricas de rendimiento real...");
                                    }
                                });
                            });
                        }
                        ViewMode::Library => {
                            // ----------------- LIBRARY VIEW -----------------
                            ui.horizontal(|ui| {
                                ui.heading("Catálogo");
                                ui.add_space(8.0);
                                ui.label(
                                    RichText::new(format!("{} fotos", self.photos.len()))
                                        .color(TEXT_SECONDARY),
                                );
                            });
                            ui.add_space(12.0);

                            if self.photos.is_empty() {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(ui.available_height() * 0.28);
                                    ui.label(RichText::new("◫").size(52.0).color(ACCENT_ORANGE));
                                    ui.add_space(10.0);
                                    ui.label(
                                        RichText::new("Tu catálogo aparecerá aquí")
                                            .size(20.0)
                                            .strong(),
                                    );
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(
                                            "Elige una galería o importa una carpeta para empezar.",
                                        )
                                        .size(13.0)
                                        .color(TEXT_SECONDARY),
                                    );
                                });
                            } else {
                                // Apply filters by reference; the catalog remains owned by self.
                                let search_query = self.search_query.to_lowercase();
                                let filtered_photos: Vec<Photo> = self
                                    .photos
                                    .iter()
                                    .filter(|photo| {
                                        // 1. Search Query
                                        if !search_query.is_empty()
                                            && !photo.name.to_lowercase().contains(&search_query)
                                            && !photo
                                                .quality_tier
                                                .to_lowercase()
                                                .contains(&search_query)
                                        {
                                            return false;
                                        }

                                        // 2. Decision State Filter
                                        if !DecisionCategory::from_decision(&photo.decision)
                                            .matches_filter(&self.filter_decision)
                                        {
                                            return false;
                                        }

                                        // 3. RAW only check
                                        if self.raw_only {
                                            let name_lower = photo.name.to_lowercase();
                                            let is_raw_ext = name_lower.ends_with(".nef")
                                                || name_lower.ends_with(".cr2")
                                                || name_lower.ends_with(".arw")
                                                || name_lower.ends_with(".dng")
                                                || name_lower.ends_with(".raf")
                                                || name_lower.ends_with(".orf");
                                            if !is_raw_ext {
                                                return false;
                                            }
                                        }

                                        true
                                    })
                                    .cloned()
                                    .collect();

                                ui.vertical(|ui| {
                                    let filmstrip_height = 110.0;
                                    let available_height = ui.available_height();
                                    let preview_height = available_height - filmstrip_height - 15.0;

                                    // 1. Large Centered Preview taking remaining space above the horizontal filmstrip
                                    if preview_height > 50.0 {
                                        ui.allocate_ui_with_layout(
                                            egui::vec2(ui.available_width(), preview_height),
                                            egui::Layout::centered_and_justified(egui::Direction::TopDown),
                                            |ui| {
                                                if let Some(texture) = &self.selected_texture {
                                                    let available = ui.available_size();
                                                    let original = texture.size_vec2();
                                                    // Allow upscale/downscale to fill available window area
                                                    let scale = (available.x / original.x)
                                                        .min(available.y / original.y);
                                                    ui.image((texture.id(), original * scale));
                                                } else {
                                                    ui.vertical_centered(|ui| {
                                                        ui.add_space(preview_height * 0.4);
                                                        ui.label(
                                                            RichText::new("Selecciona una foto")
                                                                .size(18.0)
                                                                .strong(),
                                                        );
                                                        ui.add_space(4.0);
                                                        ui.label(
                                                            RichText::new(
                                                                "Su previsualización aparecerá aquí.",
                                                            )
                                                            .color(TEXT_SECONDARY),
                                                        );
                                                    });
                                                }
                                            }
                                        );
                                    }

                                    ui.add_space(10.0);

                                    // 2. Horizontal photo strip (filmstrip) at the bottom
                                    ui.label(
                                        RichText::new("TIRA DE FOTOS")
                                            .size(9.0)
                                            .strong()
                                            .color(TEXT_MUTED),
                                    );
                                    ui.add_space(4.0);

                                    egui::ScrollArea::horizontal()
                                        .max_height(filmstrip_height)
                                        .show(ui, |ui| {
                                            ui.horizontal(|ui| {
                                                let mut photo_to_select = None;
                                                for photo in filtered_photos {
                                                    let selected = self.selected_photo.as_deref()
                                                        == Some(photo.id.as_str());

                                                    let card_bg =
                                                        if selected { BG_CARD_HOVER } else { BG_CARD };
                                                    let card_stroke = if selected {
                                                        egui::Stroke::new(1.5_f32, ACCENT_ORANGE)
                                                    } else {
                                                        egui::Stroke::new(1.0_f32, BORDER_COLOR)
                                                    };

                                                    let frame = card_frame(
                                                        card_bg,
                                                        8,
                                                        egui::Margin::symmetric(12, 10),
                                                    )
                                                    .stroke(card_stroke);

                                                    let frame_response = frame.show(ui, |ui| {
                                                        ui.vertical(|ui| {
                                                            ui.horizontal(|ui| {
                                                                let dot_color =
                                                                    DecisionCategory::from_decision(
                                                                        &photo.decision,
                                                                    )
                                                                    .color();

                                                                let (dot_rect, _) = ui.allocate_exact_size(
                                                                    egui::vec2(8.0, 8.0),
                                                                    egui::Sense::hover(),
                                                                );
                                                                ui.painter().circle_filled(
                                                                    dot_rect.center(),
                                                                    4.0,
                                                                    dot_color,
                                                                );
                                                                ui.add_space(4.0);

                                                                ui.label(
                                                                    RichText::new(&photo.name)
                                                                        .size(12.0)
                                                                        .strong()
                                                                        .color(TEXT_PRIMARY),
                                                                );
                                                            });
                                                            ui.add_space(4.0);
                                                            ui.horizontal(|ui| {
                                                                ui.label(
                                                                    RichText::new(
                                                                        photo
                                                                            .quality_tier
                                                                            .to_uppercase(),
                                                                    )
                                                                    .size(9.0)
                                                                    .color(TEXT_SECONDARY),
                                                                );
                                                                ui.label(
                                                                    RichText::new("·")
                                                                        .size(9.0)
                                                                        .color(TEXT_MUTED),
                                                                );
                                                                let score_str = photo
                                                                    .score
                                                                    .map_or("—".to_string(), |s| {
                                                                        format!("{:.1}", s)
                                                                    });
                                                                ui.label(
                                                                    RichText::new(format!(
                                                                        "SCORE: {}",
                                                                        score_str
                                                                    ))
                                                                    .size(9.0)
                                                                    .color(ACCENT_TEAL)
                                                                    .strong(),
                                                                );
                                                            });
                                                        });
                                                    });

                                                    if frame_response
                                                        .response
                                                        .interact(egui::Sense::click())
                                                        .clicked()
                                                    {
                                                        photo_to_select = Some((
                                                            photo.id.clone(),
                                                            photo.thumbnail_url.clone(),
                                                        ));
                                                    }
                                                    ui.add_space(6.0);
                                                }

                                                if let Some((photo_id, thumbnail_url)) = photo_to_select {
                                                    self.select_photo(ctx, photo_id, thumbnail_url);
                                                }
                                            });
                                        });
                                });
                            }
                        }
                    }
                });
        });

        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([1100.0, 700.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Photo Culler Native",
        options,
        Box::new(|cc| {
            let mut style = (*cc.egui_ctx.style()).clone();
            style
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(20.0));
            style.spacing.item_spacing = egui::vec2(10.0, 10.0);
            cc.egui_ctx.set_style(style);

            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = BG_SIDEBAR;
            visuals.window_fill = BG_MAIN;
            visuals.extreme_bg_color = BG_SIDEBAR;

            // Customize normal widgets
            visuals.widgets.inactive.bg_fill = BG_CARD;
            visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(8);
            visuals.widgets.inactive.fg_stroke.color = TEXT_PRIMARY;

            // Customize hovered widgets
            visuals.widgets.hovered.bg_fill = BG_CARD_HOVER;
            visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(8);
            visuals.widgets.hovered.fg_stroke.color = TEXT_PRIMARY;

            // Customize active widgets
            visuals.widgets.active.bg_fill = ACCENT_ORANGE;
            visuals.widgets.active.corner_radius = egui::CornerRadius::same(8);
            visuals.widgets.active.fg_stroke.color = Color32::WHITE;

            // Selection Highlight
            visuals.selection.bg_fill = ACCENT_ORANGE;

            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::<NativeApp>::default())
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::DecisionCategory;

    #[test]
    fn decision_categories_stay_consistent_with_filters() {
        assert_eq!(
            DecisionCategory::from_decision("best"),
            DecisionCategory::Kept
        );
        assert_eq!(
            DecisionCategory::from_decision("review"),
            DecisionCategory::Alternate
        );
        assert_eq!(
            DecisionCategory::from_decision("reject_technical"),
            DecisionCategory::Rejected
        );
        assert_eq!(
            DecisionCategory::from_decision("unprocessed"),
            DecisionCategory::Unrated
        );

        assert!(DecisionCategory::Kept.matches_filter("keep"));
        assert!(!DecisionCategory::Kept.matches_filter("reject"));
        assert!(DecisionCategory::Unrated.matches_filter("unrated"));
    }
}
