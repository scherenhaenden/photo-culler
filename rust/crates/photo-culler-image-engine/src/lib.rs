//! Native, deterministic pixel primitives for the incremental Rust engine.
//!
//! This crate intentionally has no UI, database, or Python dependency.  Its
//! results are the shadow-mode contract that the Python pipeline will compare
//! before Rust is promoted for any analyzer.

use image::{DynamicImage, GenericImageView, imageops::FilterType};
use photo_culler_core::{AnalysisEngine, AnalysisRequest, MetricResult};

const IMPLEMENTATION_VERSION: &str = "rust-pixels-0.1";

#[derive(Debug, Clone, PartialEq)]
pub struct PixelFeatures {
    pub width: u32,
    pub height: u32,
    pub histogram: [u64; 256],
    pub pixel_count: u64,
    pub mean_luminance: f64,
    pub shadow_clipping_ratio: f64,
    pub highlight_clipping_ratio: f64,
}

impl PixelFeatures {
    /// Analyze one decoded image after bounding its longest side.
    pub fn from_image(image: DynamicImage, max_dimension: u32) -> Self {
        let normalized = normalize(image, max_dimension);
        let (width, height) = normalized.dimensions();
        let mut histogram = [0_u64; 256];
        let mut shadows = 0_u64;
        let mut highlights = 0_u64;
        let mut luminance_sum = 0_u64;

        for pixel in normalized.to_rgb8().pixels() {
            let [red, green, blue] = pixel.0;
            // Integer Rec. 709 approximation, deliberately documented so the
            // Python comparison can apply an explicit numeric tolerance.
            let luminance =
                ((54_u32 * red as u32 + 183_u32 * green as u32 + 19_u32 * blue as u32) / 256) as u8;
            histogram[luminance as usize] += 1;
            luminance_sum += luminance as u64;
            shadows += u64::from(luminance <= 1);
            highlights += u64::from(luminance >= 254);
        }

        let pixel_count = width as u64 * height as u64;
        Self {
            width,
            height,
            histogram,
            pixel_count,
            mean_luminance: luminance_sum as f64 / pixel_count.max(1) as f64,
            shadow_clipping_ratio: shadows as f64 / pixel_count.max(1) as f64,
            highlight_clipping_ratio: highlights as f64 / pixel_count.max(1) as f64,
        }
    }
}

/// Decode and normalize a supported browser-viewable source.
pub fn analyze_path(path: &std::path::Path, max_dimension: u32) -> Result<PixelFeatures, String> {
    image::open(path)
        .map(|image| PixelFeatures::from_image(image, max_dimension))
        .map_err(|error| format!("unable to decode {}: {error}", path.display()))
}

/// Initial Rust adapter. It is deliberately limited to inexpensive metrics;
/// Python remains the selected engine until shadow comparisons promote it.
pub struct NativePixelEngine {
    pub max_dimension: u32,
}

impl Default for NativePixelEngine {
    fn default() -> Self {
        Self {
            max_dimension: 1920,
        }
    }
}

impl AnalysisEngine for NativePixelEngine {
    fn name(&self) -> &'static str {
        "native-pixels"
    }

    fn analyze(&self, request: &AnalysisRequest) -> Result<Vec<MetricResult>, String> {
        let started = std::time::Instant::now();
        let features = analyze_path(&request.source, self.max_dimension)?;
        let elapsed_micros = started.elapsed().as_micros() as u64;
        Ok(vec![
            metric("dimensions.width", features.width as f64, elapsed_micros),
            metric("dimensions.height", features.height as f64, elapsed_micros),
            metric(
                "exposure.mean_luminance",
                features.mean_luminance,
                elapsed_micros,
            ),
            metric(
                "clipping.shadow_ratio",
                features.shadow_clipping_ratio,
                elapsed_micros,
            ),
            metric(
                "clipping.highlight_ratio",
                features.highlight_clipping_ratio,
                elapsed_micros,
            ),
        ])
    }
}

fn normalize(image: DynamicImage, max_dimension: u32) -> DynamicImage {
    let (width, height) = image.dimensions();
    if max_dimension == 0 || width.max(height) <= max_dimension {
        return image;
    }
    image.resize(max_dimension, max_dimension, FilterType::Triangle)
}

fn metric(analyzer: &str, value: f64, elapsed_micros: u64) -> MetricResult {
    MetricResult {
        analyzer: analyzer.to_owned(),
        implementation_version: IMPLEMENTATION_VERSION.to_owned(),
        value,
        confidence: 1.0,
        elapsed_micros,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Rgb, RgbImage};

    #[test]
    fn calculates_luminance_histogram_and_clipping_once() {
        let mut image = RgbImage::new(2, 1);
        image.put_pixel(0, 0, Rgb([0, 0, 0]));
        image.put_pixel(1, 0, Rgb([255, 255, 255]));

        let features = PixelFeatures::from_image(DynamicImage::ImageRgb8(image), 1920);

        assert_eq!(features.pixel_count, 2);
        assert_eq!(features.histogram[0], 1);
        assert_eq!(features.histogram[255], 1);
        assert_eq!(features.shadow_clipping_ratio, 0.5);
        assert_eq!(features.highlight_clipping_ratio, 0.5);
    }

    #[test]
    fn bounds_the_analysis_resolution() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(6000, 4000));
        let features = PixelFeatures::from_image(image, 1920);

        assert_eq!((features.width, features.height), (1920, 1280));
    }
}
