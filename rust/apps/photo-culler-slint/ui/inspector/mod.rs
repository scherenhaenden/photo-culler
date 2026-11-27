//! Inspector presentation model for the Slint UI.
//!
//! This module deliberately has no Slint dependency yet: the crate is not part
//! of the sprint target. The integration layer can bind these stable view
//! values and actions to Slint properties and callbacks.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Representation {
    Jpeg,
    Raw,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewState {
    Ready,
    Missing,
    Corrupt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub state: PreviewState,
    /// Opaque cache or asset key. The UI never receives a filesystem path.
    pub asset_key: Option<String>,
}

impl Preview {
    pub fn ready(key: impl Into<String>) -> Self {
        Self {
            state: PreviewState::Ready,
            asset_key: Some(key.into()),
        }
    }

    pub fn missing() -> Self {
        Self {
            state: PreviewState::Missing,
            asset_key: None,
        }
    }

    pub fn corrupt() -> Self {
        Self {
            state: PreviewState::Corrupt,
            asset_key: None,
        }
    }

    fn is_ready(&self) -> bool {
        self.state == PreviewState::Ready && self.asset_key.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Photo {
    /// Stable catalog identity; switching JPEG/RAW never replaces it.
    pub photo_id: String,
    /// Existing decision remains attached to the photo identity.
    pub decision: String,
    pub jpeg: Preview,
    pub raw: Preview,
}

impl Photo {
    fn preview(&self, representation: Representation) -> &Preview {
        match representation {
            Representation::Jpeg => &self.jpeg,
            Representation::Raw => &self.raw,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    /// Normalized coordinates in [0, 1].
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Default for Crop {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1.0,
            height: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepresentationView {
    pub representation: Representation,
    pub state: PreviewState,
    pub asset_key: Option<String>,
    /// Explains why the requested source was replaced by a fallback.
    pub fallback_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InspectorView {
    pub photo_id: String,
    pub decision: String,
    pub index: usize,
    pub total: usize,
    pub strip: Vec<String>,
    pub displayed: RepresentationView,
    pub comparison: RepresentationView,
    pub zoom: f32,
    pub pan_x: f32,
    pub pan_y: f32,
    pub crop: Crop,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InspectorError {
    EmptyGallery,
    InvalidCrop,
}

/// State and actions shared by the native and browser inspector surfaces.
#[derive(Clone, Debug, PartialEq)]
pub struct Inspector {
    photos: Vec<Photo>,
    selected: usize,
    requested: Representation,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
    crop: Crop,
}

impl Inspector {
    pub fn new(photos: Vec<Photo>) -> Result<Self, InspectorError> {
        if photos.is_empty() {
            return Err(InspectorError::EmptyGallery);
        }
        Ok(Self {
            photos,
            selected: 0,
            requested: Representation::Jpeg,
            zoom: 1.0,
            pan_x: 0.0,
            pan_y: 0.0,
            crop: Crop::default(),
        })
    }

    pub fn view(&self) -> InspectorView {
        let photo = &self.photos[self.selected];
        let displayed = self.resolve(photo, self.requested);
        let alternate = match displayed.representation {
            Representation::Jpeg => Representation::Raw,
            Representation::Raw => Representation::Jpeg,
        };
        let comparison = self.resolve(photo, alternate);
        InspectorView {
            photo_id: photo.photo_id.clone(),
            decision: photo.decision.clone(),
            index: self.selected,
            total: self.photos.len(),
            strip: self.photos.iter().map(|p| p.photo_id.clone()).collect(),
            displayed,
            comparison,
            zoom: self.zoom,
            pan_x: self.pan_x,
            pan_y: self.pan_y,
            crop: self.crop,
        }
    }

    /// Select another photo without modifying its decision.
    pub fn select(&mut self, index: usize) -> bool {
        if index >= self.photos.len() {
            return false;
        }
        self.selected = index;
        true
    }

    pub fn next(&mut self) {
        self.selected = (self.selected + 1) % self.photos.len();
    }

    pub fn previous(&mut self) {
        self.selected = (self.selected + self.photos.len() - 1) % self.photos.len();
    }

    /// Request the other representation while retaining the photo and decision.
    pub fn set_representation(&mut self, representation: Representation) {
        self.requested = representation;
    }

    pub fn zoom_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(1.0, 8.0);
        }
        if self.zoom == 1.0 {
            self.pan_x = 0.0;
            self.pan_y = 0.0;
        }
    }

    /// Reset focus adjustments while retaining the selected photo and decision.
    pub fn reset_focus(&mut self) {
        self.zoom = 1.0;
        self.pan_x = 0.0;
        self.pan_y = 0.0;
        self.crop = Crop::default();
    }

    /// Pan is normalized around the image center and bounded to the visible area.
    pub fn pan_to(&mut self, x: f32, y: f32) {
        if self.zoom > 1.0 && x.is_finite() && y.is_finite() {
            let limit = (self.zoom - 1.0) / (2.0 * self.zoom);
            self.pan_x = x.clamp(-limit, limit);
            self.pan_y = y.clamp(-limit, limit);
        }
    }

    pub fn set_crop(&mut self, crop: Crop) -> Result<(), InspectorError> {
        if ![crop.x, crop.y, crop.width, crop.height]
            .iter()
            .all(|v| v.is_finite())
            || crop.x < 0.0
            || crop.y < 0.0
            || crop.width <= 0.0
            || crop.height <= 0.0
            || crop.x + crop.width > 1.0
            || crop.y + crop.height > 1.0
        {
            return Err(InspectorError::InvalidCrop);
        }
        self.crop = crop;
        Ok(())
    }

    fn resolve(&self, photo: &Photo, requested: Representation) -> RepresentationView {
        let preview = photo.preview(requested);
        if preview.is_ready() {
            return RepresentationView {
                representation: requested,
                state: preview.state,
                asset_key: preview.asset_key.clone(),
                fallback_reason: None,
            };
        }
        let fallback = match requested {
            Representation::Jpeg => Representation::Raw,
            Representation::Raw => Representation::Jpeg,
        };
        let fallback_preview = photo.preview(fallback);
        let reason = match preview.state {
            PreviewState::Missing => "Requested preview is missing",
            PreviewState::Corrupt => "Requested preview is corrupt",
            PreviewState::Ready => "Requested preview has no asset",
        }
        .to_owned();
        if fallback_preview.is_ready() {
            RepresentationView {
                representation: fallback,
                state: PreviewState::Ready,
                asset_key: fallback_preview.asset_key.clone(),
                fallback_reason: Some(reason),
            }
        } else {
            RepresentationView {
                representation: requested,
                state: preview.state,
                asset_key: None,
                fallback_reason: Some(reason),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gallery() -> Vec<Photo> {
        vec![
            Photo {
                photo_id: "p1".into(),
                decision: "KEEP".into(),
                jpeg: Preview::ready("p1-jpeg"),
                raw: Preview::ready("p1-raw"),
            },
            Photo {
                photo_id: "p2".into(),
                decision: "REJECT".into(),
                jpeg: Preview::ready("p2-jpeg"),
                raw: Preview::corrupt(),
            },
        ]
    }

    #[test]
    fn switching_jpeg_raw_keeps_selected_identity_and_decision() {
        let mut inspector = Inspector::new(gallery()).unwrap();
        inspector.select(1);
        let before = inspector.view();
        inspector.set_representation(Representation::Raw);
        let after = inspector.view();
        assert_eq!(after.photo_id, before.photo_id);
        assert_eq!(after.decision, before.decision);
        assert_eq!(after.index, before.index);
        assert_eq!(after.displayed.representation, Representation::Jpeg);
        assert_eq!(
            after.displayed.fallback_reason.as_deref(),
            Some("Requested preview is corrupt")
        );
    }

    #[test]
    fn missing_and_corrupt_preview_fall_back_and_navigation_continues() {
        let mut photos = gallery();
        photos[0].jpeg = Preview::missing();
        let mut inspector = Inspector::new(photos).unwrap();
        assert_eq!(
            inspector.view().displayed.representation,
            Representation::Raw
        );
        assert_eq!(
            inspector.view().displayed.fallback_reason.as_deref(),
            Some("Requested preview is missing")
        );
        inspector.next();
        assert_eq!(inspector.view().photo_id, "p2");
        assert_eq!(
            inspector.view().displayed.representation,
            Representation::Jpeg
        );
        inspector.next();
        assert_eq!(inspector.view().photo_id, "p1");
    }

    #[test]
    fn strip_zoom_pan_and_crop_are_normalized_and_bounded() {
        let mut inspector = Inspector::new(gallery()).unwrap();
        assert_eq!(inspector.view().strip, ["p1", "p2"]);
        inspector.zoom_by(2.0);
        inspector.pan_to(1.0, -1.0);
        assert_eq!(inspector.view().zoom, 2.0);
        assert_eq!(inspector.view().pan_x, 0.25);
        assert_eq!(inspector.view().pan_y, -0.25);
        inspector
            .set_crop(Crop {
                x: 0.1,
                y: 0.2,
                width: 0.5,
                height: 0.6,
            })
            .unwrap();
        assert_eq!(
            inspector.view().crop,
            Crop {
                x: 0.1,
                y: 0.2,
                width: 0.5,
                height: 0.6
            }
        );
        assert_eq!(
            inspector.set_crop(Crop {
                x: 0.8,
                y: 0.0,
                width: 0.3,
                height: 1.0
            }),
            Err(InspectorError::InvalidCrop)
        );
    }

    #[test]
    fn focus_adjustments_preserve_photo_decision_and_gallery_order() {
        let mut inspector = Inspector::new(gallery()).unwrap();
        inspector.select(1);
        inspector.zoom_by(2.0);
        inspector.pan_to(0.1, -0.1);
        inspector
            .set_crop(Crop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            })
            .unwrap();

        let view = inspector.view();
        assert_eq!(view.photo_id, "p2");
        assert_eq!(view.decision, "REJECT");
        assert_eq!(view.index, 1);
        assert_eq!(view.strip, ["p1", "p2"]);
        assert_eq!(view.zoom, 2.0);
    }

    #[test]
    fn reset_focus_restores_full_frame_without_changing_selection_or_decision() {
        let mut inspector = Inspector::new(gallery()).unwrap();
        inspector.select(1);
        inspector.zoom_by(2.0);
        inspector.pan_to(0.1, -0.1);
        inspector
            .set_crop(Crop {
                x: 0.1,
                y: 0.1,
                width: 0.8,
                height: 0.8,
            })
            .unwrap();

        inspector.reset_focus();
        let view = inspector.view();
        assert_eq!(view.photo_id, "p2");
        assert_eq!(view.decision, "REJECT");
        assert_eq!(view.zoom, 1.0);
        assert_eq!((view.pan_x, view.pan_y), (0.0, 0.0));
        assert_eq!(view.crop, Crop::default());
    }

    #[test]
    fn empty_gallery_is_rejected() {
        assert_eq!(
            Inspector::new(vec![]).unwrap_err(),
            InspectorError::EmptyGallery
        );
    }
}
