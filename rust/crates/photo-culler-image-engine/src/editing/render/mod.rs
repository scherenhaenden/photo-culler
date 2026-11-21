//! Deterministic, non-destructive preview rendering and RawTherapee suggestions.
//!
//! Decoding and derivative storage are ports: this module never opens or writes a
//! source path. A caller must explicitly request a derivative write.

use std::{fmt, fs, path::Path};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    /// Exposure compensation in stops.
    pub exposure_ev: f32,
    /// White-balance temperature in Kelvin.
    pub temperature_kelvin: u16,
    /// Green/magenta correction in the range -100..=100.
    pub tint: i16,
}

impl Default for Recipe {
    fn default() -> Self {
        Self {
            exposure_ev: 0.0,
            temperature_kelvin: 6500,
            tint: 0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RgbImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbImage {
    pub fn validate(&self) -> Result<(), RenderError> {
        let expected = (self.width as usize)
            .checked_mul(self.height as usize)
            .and_then(|n| n.checked_mul(3))
            .ok_or(RenderError::InvalidDimensions)?;
        if self.width == 0 || self.height == 0 || self.pixels.len() != expected {
            return Err(RenderError::InvalidDimensions);
        }
        Ok(())
    }

    /// Stable binary PPM for a derived-file adapter; the preview itself stays in memory.
    pub fn to_ppm(&self) -> Result<Vec<u8>, RenderError> {
        self.validate()?;
        let mut bytes = format!("P6\n{} {}\n255\n", self.width, self.height).into_bytes();
        bytes.extend_from_slice(&self.pixels);
        Ok(bytes)
    }
}

pub trait Decoder {
    fn decode(&self, source: &[u8]) -> Result<RgbImage, RenderError>;
}

#[derive(Debug, Clone, PartialEq)]
pub struct BeforeAfter {
    pub before: RgbImage,
    pub after: RgbImage,
}

pub fn render_before_after(
    decoder: &impl Decoder,
    source: &[u8],
    recipe: Recipe,
) -> Result<BeforeAfter, RenderError> {
    validate_recipe(recipe)?;
    let before = decoder.decode(source)?;
    before.validate()?;
    let after = apply_recipe(&before, recipe);
    Ok(BeforeAfter { before, after })
}

fn validate_recipe(recipe: Recipe) -> Result<(), RenderError> {
    if !recipe.exposure_ev.is_finite() || !(-10.0..=10.0).contains(&recipe.exposure_ev) {
        return Err(RenderError::InvalidRecipe(
            "exposure must be finite and within -10..=10 EV",
        ));
    }
    if !(1000..=50000).contains(&recipe.temperature_kelvin) {
        return Err(RenderError::InvalidRecipe(
            "temperature must be within 1000..=50000 K",
        ));
    }
    if !(-100..=100).contains(&recipe.tint) {
        return Err(RenderError::InvalidRecipe("tint must be within -100..=100"));
    }
    Ok(())
}

fn apply_recipe(before: &RgbImage, recipe: Recipe) -> RgbImage {
    let (r_gain, g_gain, b_gain) = white_balance_gains(recipe.temperature_kelvin, recipe.tint);
    let exposure = 2_f32.powf(recipe.exposure_ev);
    let pixels = before
        .pixels
        .chunks_exact(3)
        .flat_map(|rgb| {
            [rgb[0], rgb[1], rgb[2]]
                .into_iter()
                .zip([r_gain, g_gain, b_gain])
                .map(move |(channel, gain)| {
                    (f32::from(channel) * exposure * gain)
                        .round()
                        .clamp(0.0, 255.0) as u8
                })
        })
        .collect();
    RgbImage {
        width: before.width,
        height: before.height,
        pixels,
    }
}

// A compact deterministic approximation for previews. The production color-managed
// renderer can replace this adapter while preserving the recipe and output contract.
fn white_balance_gains(kelvin: u16, tint: i16) -> (f32, f32, f32) {
    let t = (f32::from(kelvin).clamp(1000.0, 40000.0) / 100.0).max(10.0);
    let red = if t <= 66.0 {
        255.0
    } else {
        (329.698_73 * (t - 60.0).powf(-0.133_204_76)).clamp(0.0, 255.0)
    };
    let green = if t <= 66.0 {
        (99.470_8 * t.ln() - 161.119_57).clamp(0.0, 255.0)
    } else {
        (288.122_16 * (t - 60.0).powf(-0.075_514_846)).clamp(0.0, 255.0)
    };
    let blue = if t >= 66.0 {
        255.0
    } else if t <= 19.0 {
        0.0
    } else {
        (138.517_73 * (t - 10.0).ln() - 305.044_8).clamp(0.0, 255.0)
    };
    let norm = |v: f32| v / 255.0;
    let tint_gain = f32::from(tint) / 500.0;
    (
        norm(red).clamp(0.25, 2.0),
        (norm(green) * (1.0 + tint_gain)).clamp(0.25, 2.0),
        norm(blue).clamp(0.25, 2.0),
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct MetricWarnings {
    pub focus_variance: f64,
    pub noise_level: f64,
    pub clipping_ratio: f64,
    pub recoverability: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WarningReport {
    pub focus: Option<&'static str>,
    pub noise: Option<&'static str>,
    pub clipping: Option<&'static str>,
    pub recoverability: Option<&'static str>,
}

pub fn warnings(metrics: MetricWarnings) -> WarningReport {
    WarningReport {
        focus: (metrics.focus_variance < 12.0).then_some("low_focus_confidence"),
        noise: (metrics.noise_level >= 0.65).then_some("high_noise"),
        clipping: (metrics.clipping_ratio >= 0.05).then_some("visible_clipping"),
        recoverability: (metrics.recoverability < 0.25).then_some("low_recoverability"),
    }
}

/// Suggest a RawTherapee PP3 document from a recipe, without touching a source.
pub fn rawtherapee_pp3(recipe: Recipe) -> Result<String, RenderError> {
    validate_recipe(recipe)?;
    Ok(format!(
        "[Version]\nVersion=500\n\n[White Balance]\nSetting=Custom\nTemperature={}\nGreen={}\n\n[Exposure]\nCompensation={:.2}\n",
        recipe.temperature_kelvin, recipe.tint, recipe.exposure_ev
    ))
}

pub trait DerivedWriter {
    fn write(&self, destination: &Path, bytes: &[u8]) -> Result<(), RenderError>;
}

pub struct FilesystemDerivedWriter;

impl DerivedWriter for FilesystemDerivedWriter {
    fn write(&self, destination: &Path, bytes: &[u8]) -> Result<(), RenderError> {
        if destination.exists() {
            return Err(RenderError::DestinationExists);
        }
        fs::write(destination, bytes).map_err(RenderError::Io)
    }
}

/// Explicitly writes a rendered derivative to a caller-selected new path.
pub fn write_derived(
    writer: &impl DerivedWriter,
    destination: &Path,
    image: &RgbImage,
) -> Result<(), RenderError> {
    writer.write(destination, &image.to_ppm()?)
}

#[derive(Debug)]
pub enum RenderError {
    InvalidDimensions,
    InvalidRecipe(&'static str),
    DestinationExists,
    Decode(String),
    Io(std::io::Error),
}

impl fmt::Display for RenderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions => write!(f, "invalid RGB dimensions or buffer length"),
            Self::InvalidRecipe(reason) => write!(f, "invalid recipe: {reason}"),
            Self::DestinationExists => write!(f, "derivative destination already exists"),
            Self::Decode(reason) => write!(f, "decode failed: {reason}"),
            Self::Io(error) => write!(f, "derivative write failed: {error}"),
        }
    }
}

impl std::error::Error for RenderError {}

#[cfg(test)]
mod tests;
