//! Native egui/wgpu delivery adapter for the local Photo Culler application API.
//!
//! The UI intentionally does not read SQLite or implement selection/analysis
//! policy.  The local Python service remains the owner of catalog, jobs and
//! non-destructive decisions, so this adapter and the web UI stay consistent.

use std::time::{Duration, Instant};

use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use serde::{Deserialize, Serialize};

const DEFAULT_SERVER: &str = "http://127.0.0.1:8765";

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
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Photo Culler Native");
                ui.label("Servicio:");
                ui.text_edit_singleline(&mut self.server_url);
                if ui.button("Conectar / actualizar").clicked() {
                    self.refresh();
                }
            });
            ui.label(&self.status);
        });

        egui::SidePanel::left("galleries")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Galerías");
                for gallery in self.galleries.clone() {
                    let selected = self.active_gallery.as_deref() == Some(gallery.id.as_str());
                    if ui
                        .selectable_label(
                            selected,
                            format!("{} ({})", gallery.name, gallery.photo_count),
                        )
                        .clicked()
                    {
                        self.active_gallery = Some(gallery.id);
                        self.refresh_catalog();
                    }
                }
                ui.separator();
                ui.checkbox(&mut self.create_new_gallery, "Crear una galería nueva");
                ui.label("Nombre de la nueva galería");
                ui.text_edit_singleline(&mut self.new_gallery_name);
                ui.label("Carpeta local");
                ui.text_edit_singleline(&mut self.import_path);
                ui.checkbox(&mut self.import_recursive, "Incluir subdirectorios");
                if ui.button("Importar carpeta").clicked() {
                    self.import_gallery();
                }
            });

        egui::SidePanel::right("analysis")
            .resizable(true)
            .show(ctx, |ui| {
                ui.heading("Análisis");
                ui.horizontal(|ui| {
                    ui.label("Perfil");
                    ui.text_edit_singleline(&mut self.analysis_profile);
                });
                if ui.button("Analizar pendientes").clicked() {
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
                    ui.add(egui::ProgressBar::new(f32::from(progress.progress) / 100.0));
                    ui.label(format!("{}/{} fotos", progress.processed, progress.total));
                    ui.label(&progress.message);
                }
                ui.separator();
                ui.heading("Decisión");
                for (label, value) in [
                    ("Best", "best"),
                    ("Keep", "keep"),
                    ("Review", "review"),
                    ("Reject", "reject"),
                ] {
                    if ui.button(label).clicked() {
                        self.set_decision(value);
                    }
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Catálogo");
            ui.columns(2, |columns| {
                egui::ScrollArea::vertical().show(&mut columns[0], |ui| {
                    for photo in self.photos.clone() {
                        let selected = self.selected_photo.as_deref() == Some(photo.id.as_str());
                        let score = photo
                            .score
                            .map_or_else(|| "—".to_owned(), |score| format!("{score:.2}"));
                        if ui
                            .selectable_label(
                                selected,
                                format!(
                                    "{} · {} · {} · {}",
                                    photo.name, photo.decision, photo.quality_tier, score
                                ),
                            )
                            .clicked()
                        {
                            self.select_photo(ctx, &photo);
                        }
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
                    columns[1].label("Selecciona una foto para ver su miniatura.");
                }
            });
        });
        ctx.request_repaint_after(Duration::from_millis(250));
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Photo Culler Native",
        options,
        Box::new(|_| Ok(Box::<NativeApp>::default())),
    )
}
