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
const BG_MAIN: Color32 = Color32::from_rgb(15, 16, 18);       // #0F1012
const BG_CARD: Color32 = Color32::from_rgb(17, 17, 17);       // #111111
const BG_CARD_HOVER: Color32 = Color32::from_rgb(28, 29, 33); // #1C1D21
const BG_SIDEBAR: Color32 = Color32::from_rgb(5, 5, 5);       // #050505
const BORDER_COLOR: Color32 = Color32::from_rgb(25, 25, 25);  // #191919 (matches white/10 visually on #050505)

const TEXT_PRIMARY: Color32 = Color32::from_rgb(227, 226, 228);   // #e3e2e4
const TEXT_SECONDARY: Color32 = Color32::from_rgb(150, 150, 150); // rgba(255, 255, 255, 0.4)
const TEXT_MUTED: Color32 = Color32::from_rgb(80, 80, 80);        // rgba(255, 255, 255, 0.25)

const ACCENT_ORANGE: Color32 = Color32::from_rgb(255, 107, 53);  // #FF6B35 (Active/Orange)
const ACCENT_TEAL: Color32 = Color32::from_rgb(37, 161, 142);    // #25A18E (Kept/Teal)
const ACCENT_YELLOW: Color32 = Color32::from_rgb(247, 197, 159); // #F7C59F (Alt/Yellow)
const ACCENT_RED: Color32 = Color32::from_rgb(255, 107, 53);     // #FF6B35 (Rejected/Red)

#[derive(Clone, Copy, Debug, PartialEq)]
enum ViewMode {
    Library,
    Dashboard,
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
    cpu_app: f32,
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

struct NativeApp {
    server_url: String,
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
}

impl Default for NativeApp {
    fn default() -> Self {
        Self {
            server_url: std::env::var("PHOTO_CULLER_SERVER")
                .unwrap_or_else(|_| DEFAULT_SERVER.into()),
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
        }
    }
}

impl NativeApp {
    fn endpoint(&self, path: &str) -> String {
        format!("{}{}", self.server_url.trim_end_matches('/'), path)
    }

    fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let response = ureq::get(&self.endpoint(path))
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
            "POST" => ureq::post(&url).send_json(&body),
            "PUT" => ureq::put(&url).send_json(&body),
            "PATCH" => ureq::patch(&url).send_json(&body),
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

    fn select_photo(&mut self, ctx: &egui::Context, photo: &Photo) {
        self.selected_photo = Some(photo.id.clone());
        match ureq::get(&self.endpoint(&photo.thumbnail_url)).call() {
            Ok(response) => match response.into_body().read_to_vec() {
                Ok(bytes) => match image::load_from_memory(&bytes) {
                    Ok(image) => {
                        let image = image.to_rgb8();
                        let size = [image.width() as usize, image.height() as usize];
                        self.selected_texture = Some(ctx.load_texture(
                            "selected-photo-thumbnail",
                            ColorImage::from_rgb(size, image.as_raw()),
                            TextureOptions::LINEAR,
                        ));
                    }
                    Err(error) => {
                        self.status = format!("No se pudo decodificar la miniatura: {error}")
                    }
                },
                Err(error) => self.status = format!("No se pudo leer la miniatura: {error}"),
            },
            Err(error) => self.status = format!("No se pudo cargar la miniatura: {error}"),
        }
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
            serde_json::json!({"exposure": self.exposure}),
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
            Ok(_) => self.status = format!("Edición: {action}."),
            Err(error) => self.status = format!("No se pudo {action}: {error}"),
        }
    }

    fn poll_analysis(&mut self) {
        if self.last_progress_poll.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.last_progress_poll = Instant::now();
        if let Ok(progress) = self.get_json::<AnalysisProgress>("/api/v1/analysis/progress") {
            self.analysis = Some(progress);
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
        self.poll_analysis();
        self.poll_system_usage();

        // ----------------- TOP PANEL (HEADER) -----------------
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::symmetric(20, 14))
                    .stroke(egui::Stroke::new(1.0, BORDER_COLOR)),
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
                    let separator_stroke = egui::Stroke::new(1.0, BORDER_COLOR);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
                    ui.painter().line_segment([rect.left_top(), rect.left_bottom()], separator_stroke);
                    ui.add_space(20.0);

                    // View Switcher Buttons (Library, Dashboard)
                    let lib_selected = self.view_mode == ViewMode::Library;
                    let dash_selected = self.view_mode == ViewMode::Dashboard;

                    if ui
                        .selectable_label(lib_selected, RichText::new("LIBRARY").size(11.0).strong())
                        .clicked()
                    {
                        self.view_mode = ViewMode::Library;
                    }
                    ui.add_space(8.0);
                    if ui
                        .selectable_label(dash_selected, RichText::new("DASHBOARD").size(11.0).strong())
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
                    .stroke(egui::Stroke::new(1.0, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(usage) = &self.system_usage {
                        // CPU Sistema
                        ui.label(RichText::new("CPU SISTEMA:").size(9.0).strong().color(TEXT_SECONDARY));
                        ui.label(RichText::new(format!("{:.1}%", usage.cpu_system)).size(10.0).strong().color(TEXT_PRIMARY));
                        ui.add(
                            egui::ProgressBar::new(usage.cpu_system / 100.0)
                                .fill(Color32::from_rgb(56, 139, 253))
                                .desired_width(50.0),
                        );

                        ui.add_space(15.0);

                        // CPU App
                        ui.label(RichText::new("CPU APP:").size(9.0).strong().color(TEXT_SECONDARY));
                        ui.label(RichText::new(format!("{:.1}%", usage.cpu_app)).size(10.0).strong().color(TEXT_PRIMARY));
                        ui.add(
                            egui::ProgressBar::new(usage.cpu_app_capacity / 100.0)
                                .fill(ACCENT_TEAL)
                                .desired_width(50.0),
                        );

                        ui.add_space(15.0);

                        // GPU Usage
                        ui.label(RichText::new("GPU:").size(9.0).strong().color(TEXT_SECONDARY));
                        ui.label(RichText::new(format!("{:.1}%", usage.gpu_system)).size(10.0).strong().color(TEXT_PRIMARY));
                        ui.add(
                            egui::ProgressBar::new(usage.gpu_system / 100.0)
                                .fill(ACCENT_ORANGE)
                                .desired_width(50.0),
                        );

                        if !usage.gpu_name.is_empty() && usage.gpu_name != "N/A" {
                            ui.add_space(5.0);
                            ui.label(RichText::new(format!("({})", usage.gpu_name)).size(9.0).color(TEXT_MUTED));
                        }
                    } else {
                        ui.label(RichText::new("CARGANDO TELEMETRIA...").size(9.0).color(TEXT_SECONDARY));
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new("PHOTO CULLER V0.1.0").size(9.0).color(TEXT_SECONDARY));
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
                    .stroke(egui::Stroke::new(1.0, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                // Catalog / Archive Branding Block
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 6, BG_CARD);
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "📂",
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );

                    ui.vertical(|ui| {
                        ui.label(RichText::new("CATALOG_01").size(11.0).strong().italics().color(Color32::WHITE));
                        ui.label(RichText::new("Local Archive").size(8.0).strong().color(TEXT_SECONDARY));
                    });
                });

                ui.add_space(16.0);
                ui.separator();
                ui.add_space(10.0);

                // Galleries header
                ui.label(RichText::new("GALERÍAS").size(9.0).strong().color(TEXT_SECONDARY));
                ui.add_space(6.0);

                // Scroll area for galleries
                egui::ScrollArea::vertical()
                    .max_height(140.0)
                    .show(ui, |ui| {
                        for gallery in self.galleries.clone() {
                            let selected =
                                self.active_gallery.as_deref() == Some(gallery.id.as_str());

                            let text = format!("{} ({})", gallery.name, gallery.photo_count);
                            if ui
                                .selectable_label(selected, RichText::new(text).size(13.0))
                                .clicked()
                            {
                                self.active_gallery = Some(gallery.id);
                                self.refresh_catalog();
                            }
                            ui.add_space(4.0);
                        }
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // Filters panel (visible if Library View is selected)
                if self.view_mode == ViewMode::Library {
                    ui.label(RichText::new("Filtros locales").size(9.0).strong().color(TEXT_SECONDARY));
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
                            ui.selectable_value(&mut self.filter_decision, "all".to_string(), "TODOS");
                            ui.selectable_value(&mut self.filter_decision, "keep".to_string(), "KEPT (BEST/KEEP)");
                            ui.selectable_value(&mut self.filter_decision, "alt".to_string(), "ALT (REVIEW/ALT)");
                            ui.selectable_value(&mut self.filter_decision, "reject".to_string(), "REJECTED");
                            ui.selectable_value(&mut self.filter_decision, "unrated".to_string(), "UNRATED");
                        });
                    ui.add_space(6.0);

                    // RAW checkbox
                    ui.checkbox(&mut self.raw_only, "Solo RAW");

                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(10.0);
                }

                // Selection Stats Block
                ui.label(RichText::new("SELECTION STATS").size(9.0).strong().color(TEXT_MUTED));
                ui.add_space(6.0);

                // Calculate current catalog stats
                let mut keep_count = 0;
                let mut alt_count = 0;
                let mut reject_count = 0;
                let mut unrated_count = 0;
                for p in &self.photos {
                    let d = p.decision.to_uppercase();
                    if d == "BEST" || d == "KEEP" {
                        keep_count += 1;
                    } else if d == "ALTERNATE" || d == "REVIEW" {
                        alt_count += 1;
                    } else if d.starts_with("REJECT") {
                        reject_count += 1;
                    } else {
                        unrated_count += 1;
                    }
                }

                // Kept Row
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, ACCENT_TEAL);
                    ui.label(RichText::new("KEPT").size(11.0).strong().color(TEXT_SECONDARY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(keep_count.to_string()).size(11.0).strong().color(ACCENT_TEAL));
                    });
                });
                ui.add_space(4.0);

                // Alt Row
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, ACCENT_YELLOW);
                    ui.label(RichText::new("ALT (PICK)").size(11.0).strong().color(TEXT_SECONDARY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(alt_count.to_string()).size(11.0).strong().color(ACCENT_YELLOW));
                    });
                });
                ui.add_space(4.0);

                // Rejected Row
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, ACCENT_RED);
                    ui.label(RichText::new("REJECTED").size(11.0).strong().color(TEXT_SECONDARY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(reject_count.to_string()).size(11.0).strong().color(ACCENT_RED));
                    });
                });
                ui.add_space(4.0);

                // Unrated Row
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.0, TEXT_MUTED);
                    ui.label(RichText::new("UNRATED").size(11.0).strong().color(TEXT_SECONDARY));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(RichText::new(unrated_count.to_string()).size(11.0).strong().color(TEXT_PRIMARY));
                    });
                });

                // Operator Footer
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        // Round Operator Avatar Frame
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(28.0, 28.0), egui::Sense::hover());
                        ui.painter().circle_filled(rect.center(), 14.0, BG_CARD);
                        ui.painter().circle_stroke(rect.center(), 14.0, egui::Stroke::new(1.0, BORDER_COLOR));
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "OP",
                            egui::FontId::proportional(10.0),
                            Color32::WHITE,
                        );

                        ui.vertical(|ui| {
                            ui.label(RichText::new("OPERATOR_01").size(11.0).strong().italics().color(Color32::WHITE));
                            ui.horizontal(|ui| {
                                let (dot_rect, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
                                ui.painter().circle_filled(dot_rect.center(), 3.0, ACCENT_TEAL);
                                ui.label(RichText::new("ACTIVE SESSION").size(8.0).strong().color(ACCENT_TEAL));
                            });
                        });
                    });
                });
            });

        // ----------------- RIGHT PANEL (TOOLS & ANALYSIS) -----------------
        egui::SidePanel::right("right_sidebar")
            .default_width(280.0)
            .min_width(260.0)
            .max_width(320.0)
            .frame(
                egui::Frame::new()
                    .fill(BG_SIDEBAR)
                    .inner_margin(egui::Margin::same(16))
                    .stroke(egui::Stroke::new(1.0, BORDER_COLOR)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("HERRAMIENTAS").size(10.0).strong().color(TEXT_MUTED));
                ui.add_space(6.0);
                ui.heading("Análisis");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label(RichText::new("Perfil").color(TEXT_SECONDARY));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.analysis_profile).desired_width(120.0),
                    );
                });
                ui.add_space(4.0);

                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(RichText::new("Analizar pendientes").strong()),
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
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(6.0);
                    ui.label(RichText::new(format!("{} — {}", progress.profile_name, progress.status)).strong().color(ACCENT_ORANGE));
                    ui.add(
                        egui::ProgressBar::new(f32::from(progress.progress) / 100.0).fill(ACCENT_TEAL),
                    );
                    ui.label(RichText::new(format!("{}/{} fotos", progress.processed, progress.total)).size(11.0).color(TEXT_SECONDARY));
                    ui.label(RichText::new(&progress.message).size(10.0).color(TEXT_MUTED));
                }

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // --- Decision Grid ---
                ui.heading("Decisión");
                ui.label(RichText::new("Clasifica la foto seleccionada").size(12.0).color(TEXT_SECONDARY));
                ui.add_space(8.0);

                egui::Grid::new("decision-grid")
                    .num_columns(2)
                    .spacing([8.0, 8.0])
                    .show(ui, |ui| {
                        // Best Button (Teal Border/Text)
                        if ui
                            .add_sized([120.0, 32.0], egui::Button::new(RichText::new("★ Best").strong().color(ACCENT_TEAL)))
                            .clicked()
                        {
                            self.set_decision("best");
                        }
                        // Keep Button (Teal Border/Text)
                        if ui
                            .add_sized([120.0, 32.0], egui::Button::new(RichText::new("✔ Keep").strong().color(ACCENT_TEAL)))
                            .clicked()
                        {
                            self.set_decision("keep");
                        }
                        ui.end_row();

                        // Review Button (Yellow Border/Text)
                        if ui
                            .add_sized([120.0, 32.0], egui::Button::new(RichText::new("👁 Review").strong().color(ACCENT_YELLOW)))
                            .clicked()
                        {
                            self.set_decision("review");
                        }
                        // Reject Button (Red Border/Text)
                        if ui
                            .add_sized([120.0, 32.0], egui::Button::new(RichText::new("✖ Reject").strong().color(ACCENT_RED)))
                            .clicked()
                        {
                            self.set_decision("reject");
                        }
                        ui.end_row();
                    });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // --- Non-destructive recipe editor ---
                ui.heading("Edición no destructiva");
                ui.label(RichText::new("Conserva el archivo original intacto.").size(12.0).color(TEXT_SECONDARY));
                ui.add_space(8.0);

                ui.add(egui::Slider::new(&mut self.exposure, -5.0..=5.0).text("Exposición"));
                ui.add_space(6.0);

                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new(RichText::new("Guardar receta").strong()).fill(ACCENT_ORANGE),
                    )
                    .clicked()
                {
                    self.update_edit();
                }
                ui.add_space(6.0);

                ui.columns(2, |cols| {
                    if cols[0].button("Deshacer").clicked() {
                        self.edit_history("undo");
                    }
                    if cols[1].button("Rehacer").clicked() {
                        self.edit_history("redo");
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // --- Import Block ---
                ui.label(RichText::new("IMPORTAR DESDE CARPETA").size(9.0).strong().color(TEXT_MUTED));
                ui.add_space(6.0);
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
                        egui::Button::new(RichText::new("Importar carpeta").strong()).fill(ACCENT_ORANGE),
                    )
                    .clicked()
                {
                    self.import_gallery();
                }
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
                                    let frame_1 = egui::Frame::new()
                                        .fill(BG_CARD)
                                        .stroke(egui::Stroke::new(1.0, BORDER_COLOR))
                                        .corner_radius(12)
                                        .inner_margin(egui::Margin::symmetric(18, 14));
                                    frame_1.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(RichText::new("TOTAL FOTOS").size(9.0).strong().color(TEXT_SECONDARY));
                                                ui.add_space(4.0);
                                                ui.label(RichText::new(self.photos.len().to_string()).size(28.0).strong().color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(RichText::new("CATALOGADAS").size(9.0).color(ACCENT_ORANGE).strong());
                                            });
                                        });
                                    });

                                    ui.add_space(12.0);

                                    // Card 2: Sessions Count
                                    let frame_2 = egui::Frame::new()
                                        .fill(BG_CARD)
                                        .stroke(egui::Stroke::new(1.0, BORDER_COLOR))
                                        .corner_radius(12)
                                        .inner_margin(egui::Margin::symmetric(18, 14));
                                    frame_2.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(RichText::new("SESIONES").size(9.0).strong().color(TEXT_SECONDARY));
                                                ui.add_space(4.0);
                                                ui.label(RichText::new(self.sessions_count.to_string()).size(28.0).strong().color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(RichText::new("FISICAS").size(9.0).color(ACCENT_TEAL).strong());
                                            });
                                        });
                                    });

                                    ui.add_space(12.0);

                                    // Card 3: Similar Groups
                                    let frame_3 = egui::Frame::new()
                                        .fill(BG_CARD)
                                        .stroke(egui::Stroke::new(1.0, BORDER_COLOR))
                                        .corner_radius(12)
                                        .inner_margin(egui::Margin::symmetric(18, 14));
                                    frame_3.show(ui, |ui| {
                                        ui.allocate_ui(egui::vec2(160.0, 100.0), |ui| {
                                            ui.vertical(|ui| {
                                                ui.label(RichText::new("GRUPOS SIMILARES").size(9.0).strong().color(TEXT_SECONDARY));
                                                ui.add_space(4.0);
                                                ui.label(RichText::new(self.groups_count.to_string()).size(28.0).strong().color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.separator();
                                                ui.add_space(4.0);
                                                ui.label(RichText::new("CLUSTERS").size(9.0).color(ACCENT_YELLOW).strong());
                                            });
                                        });
                                    });
                                });

                                ui.add_space(20.0);

                                // System Overview section
                                ui.heading("System Performance");
                                ui.add_space(10.0);

                                let usage_frame = egui::Frame::new()
                                    .fill(BG_CARD)
                                    .stroke(egui::Stroke::new(1.0, BORDER_COLOR))
                                    .corner_radius(12)
                                    .inner_margin(egui::Margin::same(16));

                                usage_frame.show(ui, |ui| {
                                    if let Some(usage) = &self.system_usage {
                                        ui.columns(3, |cols| {
                                            // Column 1: CPU System Info
                                            cols[0].vertical(|ui| {
                                                ui.label(RichText::new("CPU SISTEMA").size(10.0).strong().color(TEXT_SECONDARY));
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(format!("{:.1}%", usage.cpu_system)).size(24.0).strong().color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.add(egui::ProgressBar::new(usage.cpu_system / 100.0).fill(Color32::from_rgb(56, 139, 253)));
                                            });

                                            // Column 2: CPU App Info
                                            cols[1].vertical(|ui| {
                                                ui.label(RichText::new("CPU APLICACION").size(10.0).strong().color(TEXT_SECONDARY));
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(format!("{:.1}%", usage.cpu_app)).size(24.0).strong().color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.add(egui::ProgressBar::new(usage.cpu_app_capacity / 100.0).fill(ACCENT_TEAL));
                                            });

                                            // Column 3: GPU Info
                                            cols[2].vertical(|ui| {
                                                ui.label(RichText::new("GPU").size(10.0).strong().color(TEXT_SECONDARY));
                                                ui.add_space(6.0);
                                                ui.label(RichText::new(format!("{:.1}%", usage.gpu_system)).size(24.0).strong().color(Color32::WHITE));
                                                ui.add_space(8.0);
                                                ui.add(egui::ProgressBar::new(usage.gpu_system / 100.0).fill(ACCENT_ORANGE));
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
                                // Apply filter algorithms locally on catalog
                                let filtered_photos: Vec<Photo> = self.photos
                                    .iter()
                                    .filter(|photo| {
                                        // 1. Search Query
                                        if !self.search_query.is_empty() {
                                            let q = self.search_query.to_lowercase();
                                            if !photo.name.to_lowercase().contains(&q)
                                                && !photo.quality_tier.to_lowercase().contains(&q)
                                            {
                                                return false;
                                            }
                                        }

                                        // 2. Decision State Filter
                                        let d = photo.decision.to_uppercase();
                                        match self.filter_decision.as_str() {
                                            "keep" => {
                                                if d != "BEST" && d != "KEEP" {
                                                    return false;
                                                }
                                            }
                                            "alt" => {
                                                if d != "ALTERNATE" && d != "REVIEW" {
                                                    return false;
                                                }
                                            }
                                            "reject" => {
                                                if !d.starts_with("REJECT") {
                                                    return false;
                                                }
                                            }
                                            "unrated" => {
                                                if d == "BEST"
                                                    || d == "KEEP"
                                                    || d == "ALTERNATE"
                                                    || d == "REVIEW"
                                                    || d.starts_with("REJECT")
                                                {
                                                    return false;
                                                }
                                            }
                                            _ => {}
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

                                ui.columns(2, |columns| {
                                    // Left Column: Scrollable Photo Cards
                                    egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                                        for photo in filtered_photos {
                                            let selected = self.selected_photo.as_deref()
                                                == Some(photo.id.as_str());

                                            // Styling cards based on selection and decision state
                                            let card_bg = if selected { BG_CARD_HOVER } else { BG_CARD };
                                            let card_stroke = if selected {
                                                egui::Stroke::new(1.5, ACCENT_ORANGE)
                                            } else {
                                                egui::Stroke::new(1.0, BORDER_COLOR)
                                            };

                                            let frame = egui::Frame::new()
                                                .fill(card_bg)
                                                .stroke(card_stroke)
                                                .corner_radius(8)
                                                .inner_margin(egui::Margin::symmetric(12, 10));

                                            let response = frame.show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    // Left part: Decision colored dot and filename
                                                    let dot_color = match photo.decision.to_uppercase().as_str() {
                                                        "BEST" | "KEEP" => ACCENT_TEAL,
                                                        "ALTERNATE" | "REVIEW" => ACCENT_YELLOW,
                                                        d if d.starts_with("REJECT") => ACCENT_RED,
                                                        _ => TEXT_MUTED,
                                                    };

                                                    // Draw small dot
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

                                                    ui.vertical(|ui| {
                                                        ui.label(
                                                            RichText::new(&photo.name)
                                                                .size(13.0)
                                                                .strong()
                                                                .color(TEXT_PRIMARY),
                                                        );
                                                        ui.horizontal(|ui| {
                                                            ui.label(
                                                                RichText::new(
                                                                    &photo.quality_tier.to_uppercase(),
                                                                )
                                                                .size(9.0)
                                                                .color(TEXT_SECONDARY),
                                                            );
                                                            ui.label(
                                                                RichText::new("·")
                                                                    .size(9.0)
                                                                    .color(TEXT_MUTED),
                                                            );
                                                            let score_str = photo.score.map_or(
                                                                "—".to_string(),
                                                                |s| format!("{:.2}", s),
                                                            );
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
                                            });

                                            // Handle clicking on the card
                                            let card_rect = response.response.rect;
                                            let click_response = ui.allocate_rect(
                                                card_rect,
                                                egui::Sense::click(),
                                            );
                                            if click_response.clicked() {
                                                self.select_photo(ctx, &photo);
                                            }
                                            ui.add_space(6.0);
                                        }
                                    });

                                    // Right Column: Full-size/Inspect View of Selected Photo
                                    if let Some(texture) = &self.selected_texture {
                                        let available = columns[1].available_size();
                                        let original = texture.size_vec2();
                                        let scale = (available.x / original.x)
                                            .min(available.y / original.y)
                                            .min(1.0);
                                        columns[1].image((texture.id(), original * scale));
                                    } else {
                                        columns[1].vertical_centered(|ui| {
                                            ui.add_space(100.0);
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
