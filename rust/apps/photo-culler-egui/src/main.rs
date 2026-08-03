//! Native egui/wgpu delivery adapter for the local Photo Culler application API.
//!
//! The UI intentionally does not read SQLite or implement selection/analysis
//! policy.  The local Python service remains the owner of catalog, jobs and
//! non-destructive decisions, so this adapter and the web UI stay consistent.

use std::time::{Duration, Instant};

use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

const DEFAULT_SERVER: &str = "http://127.0.0.1:8765";
const PANEL: Color32 = Color32::from_rgb(25, 28, 33);
const PANEL_RAISED: Color32 = Color32::from_rgb(34, 38, 45);
const CANVAS: Color32 = Color32::from_rgb(17, 19, 23);
const ACCENT: Color32 = Color32::from_rgb(87, 196, 184);
const MUTED: Color32 = Color32::from_rgb(154, 164, 178);

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
}

impl eframe::App for NativeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_analysis();
        egui::TopBottomPanel::top("top")
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(20, 14)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("PHOTO CULLER")
                            .size(21.0)
                            .strong()
                            .color(ACCENT),
                    );
                    ui.label(RichText::new("NATIVE").size(12.0).color(MUTED));
                    ui.add_space(20.0);
                    ui.label(RichText::new(&self.status).size(14.0).color(MUTED));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button(RichText::new("↻  Actualizar").strong()).clicked() {
                            self.refresh();
                        }
                        ui.add_sized(
                            [230.0, 26.0],
                            egui::TextEdit::singleline(&mut self.server_url)
                                .hint_text("Servicio local"),
                        );
                    });
                });
            });

        egui::SidePanel::left("galleries")
            .default_width(275.0)
            .min_width(235.0)
            .max_width(380.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.label(RichText::new("BIBLIOTECA").size(12.0).strong().color(MUTED));
                ui.add_space(6.0);
                ui.heading("Galerías");
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "{} sesiones  ·  {} grupos similares",
                        self.sessions_count, self.groups_count
                    ))
                    .size(13.0)
                    .color(MUTED),
                );
                ui.add_space(14.0);
                egui::ScrollArea::vertical()
                    .max_height(230.0)
                    .show(ui, |ui| {
                        for gallery in self.galleries.clone() {
                            let selected =
                                self.active_gallery.as_deref() == Some(gallery.id.as_str());
                            let text = format!("{}\n{} fotos", gallery.name, gallery.photo_count);
                            if ui
                                .add_sized(
                                    [ui.available_width(), 46.0],
                                    egui::Button::new(RichText::new(text).size(15.0))
                                        .selected(selected),
                                )
                                .clicked()
                            {
                                self.active_gallery = Some(gallery.id);
                                self.refresh_catalog();
                            }
                            ui.add_space(3.0);
                        }
                    });
                ui.separator();
                ui.add_space(8.0);
                ui.label(
                    RichText::new("IMPORTAR FOTOS")
                        .size(12.0)
                        .strong()
                        .color(MUTED),
                );
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
                        egui::Button::new(RichText::new("Importar carpeta").strong()).fill(ACCENT),
                    )
                    .clicked()
                {
                    self.import_gallery();
                }
            });

        egui::SidePanel::right("analysis")
            .default_width(285.0)
            .min_width(245.0)
            .max_width(360.0)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(16)),
            )
            .show(ctx, |ui| {
                ui.label(
                    RichText::new("HERRAMIENTAS")
                        .size(12.0)
                        .strong()
                        .color(MUTED),
                );
                ui.add_space(6.0);
                ui.heading("Análisis");
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Perfil").color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.analysis_profile).desired_width(100.0),
                    );
                });
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("Analizar pendientes"),
                    )
                    .clicked()
                {
                    self.start_analysis();
                }
                ui.horizontal(|ui| {
                    if ui.button("Pausar").clicked() {
                        self.control_analysis("pause");
                    }
                    if ui.button("Reanudar").clicked() {
                        self.control_analysis("resume");
                    }
                    if ui.button("Cancelar").clicked() {
                        self.control_analysis("cancel");
                    }
                });
                if let Some(progress) = &self.analysis {
                    ui.separator();
                    ui.label(format!("{} — {}", progress.profile_name, progress.status));
                    ui.add(
                        egui::ProgressBar::new(f32::from(progress.progress) / 100.0).fill(ACCENT),
                    );
                    ui.label(format!("{}/{} fotos", progress.processed, progress.total));
                    ui.label(&progress.message);
                }
                ui.separator();
                ui.heading("Decisión");
                ui.label(
                    RichText::new("Clasifica la foto seleccionada")
                        .size(13.0)
                        .color(MUTED),
                );
                egui::Grid::new("decision-grid")
                    .num_columns(2)
                    .spacing([6.0, 6.0])
                    .show(ui, |ui| {
                        for (label, value) in [
                            ("★ Best", "best"),
                            ("Keep", "keep"),
                            ("Review", "review"),
                            ("Reject", "reject"),
                        ] {
                            if ui
                                .add_sized([120.0, 30.0], egui::Button::new(label))
                                .clicked()
                            {
                                self.set_decision(value);
                            }
                            if value == "keep" {
                                ui.end_row();
                            }
                        }
                    });
                ui.separator();
                ui.heading("Edición no destructiva");
                ui.label(
                    RichText::new("La receta conserva el original intacto.")
                        .size(13.0)
                        .color(MUTED),
                );
                ui.add(egui::Slider::new(&mut self.exposure, -5.0..=5.0).text("Exposición"));
                if ui
                    .add_sized(
                        [ui.available_width(), 30.0],
                        egui::Button::new("Guardar receta"),
                    )
                    .clicked()
                {
                    self.update_edit();
                }
                ui.horizontal(|ui| {
                    if ui.button("Deshacer").clicked() {
                        self.edit_history("undo");
                    }
                    if ui.button("Rehacer").clicked() {
                        self.edit_history("redo");
                    }
                });
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Frame::new()
                .fill(CANVAS)
                .inner_margin(egui::Margin::same(22))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.heading("Catálogo");
                        ui.label(
                            RichText::new(format!("{} fotos", self.photos.len())).color(MUTED),
                        );
                    });
                    ui.add_space(12.0);
                    if self.photos.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(ui.available_height() * 0.28);
                            ui.label(RichText::new("◫").size(52.0).color(ACCENT));
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("Tu catálogo aparecerá aquí")
                                    .size(24.0)
                                    .strong(),
                            );
                            ui.label(
                                RichText::new(
                                    "Elige una galería o importa una carpeta para empezar.",
                                )
                                .size(15.0)
                                .color(MUTED),
                            );
                        });
                    } else {
                        ui.columns(2, |columns| {
                            egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                                for photo in self.photos.clone() {
                                    let selected =
                                        self.selected_photo.as_deref() == Some(photo.id.as_str());
                                    let score = photo.score.map_or_else(
                                        || "—".to_owned(),
                                        |score| format!("{score:.2}"),
                                    );
                                    if ui
                                        .add_sized(
                                            [ui.available_width(), 52.0],
                                            egui::Button::new(
                                                RichText::new(format!(
                                                    "{}\n{}  ·  {}  ·  puntuación {}",
                                                    photo.name,
                                                    photo.decision,
                                                    photo.quality_tier,
                                                    score
                                                ))
                                                .size(15.0),
                                            )
                                            .selected(selected),
                                        )
                                        .clicked()
                                    {
                                        self.select_photo(ctx, &photo);
                                    }
                                    ui.add_space(5.0);
                                }
                            });
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
                                        RichText::new("Selecciona una foto").size(20.0).strong(),
                                    );
                                    ui.label(
                                        RichText::new("Su previsualización aparecerá aquí.")
                                            .color(MUTED),
                                    );
                                });
                            }
                        });
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
                .insert(egui::TextStyle::Body, egui::FontId::proportional(16.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, egui::FontId::proportional(15.0));
            style
                .text_styles
                .insert(egui::TextStyle::Heading, egui::FontId::proportional(23.0));
            style.spacing.item_spacing = egui::vec2(8.0, 8.0);
            cc.egui_ctx.set_style(style);
            let mut visuals = egui::Visuals::dark();
            visuals.panel_fill = PANEL;
            visuals.window_fill = CANVAS;
            visuals.extreme_bg_color = Color32::from_rgb(12, 14, 17);
            visuals.widgets.active.bg_fill = ACCENT;
            visuals.widgets.hovered.bg_fill = PANEL_RAISED;
            visuals.selection.bg_fill = Color32::from_rgb(39, 102, 103);
            cc.egui_ctx.set_visuals(visuals);
            Ok(Box::<NativeApp>::default())
        }),
    )
}
