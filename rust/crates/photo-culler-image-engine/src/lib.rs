//! Native, deterministic pixel primitives for the incremental Rust engine.
//!
//! This crate intentionally has no UI, database, or Python dependency.  Its
//! results are the shadow-mode contract that the Python pipeline will compare
//! before Rust is promoted for any analyzer.

use image::{DynamicImage, imageops::FilterType};
use photo_culler_core::{AnalysisEngine, AnalysisRequest, MetricResult};

pub mod algorithms {
    pub mod noise;
    pub mod edges;
    pub mod fft;
}
pub mod metrics;

pub use metrics::PixelFeatures;

const IMPLEMENTATION_VERSION: &str = "rust-pixels-0.1";
const DEFAULT_MAX_DIMENSION: u32 = 1920;

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
            metric("exposure.score", features.exposure_score, elapsed_micros),
            metric(
                "exposure.underexposed_probability",
                features.underexposed_probability,
                elapsed_micros,
            ),
            metric(
                "exposure.overexposed_probability",
                features.overexposed_probability,
                elapsed_micros,
            ),
            metric(
                "histogram.luminance_stddev",
                features.luminance_stddev,
                elapsed_micros,
            ),
            metric(
                "histogram.luminance_entropy",
                features.luminance_entropy,
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
            metric(
                "clipping.center_shadow_ratio",
                features.center_shadow_clipping_ratio,
                elapsed_micros,
            ),
            metric(
                "clipping.center_highlight_ratio",
                features.center_highlight_clipping_ratio,
                elapsed_micros,
            ),
            metric(
                "noise.luminance_stddev",
                features.luminance_noise_stddev,
                elapsed_micros,
            ),
            metric(
                "noise.chroma_stddev",
                features.chroma_noise_stddev,
                elapsed_micros,
            ),
            metric(
                "noise.shadow_stddev",
                features.shadow_noise_stddev,
                elapsed_micros,
            ),
            metric(
                "noise.estimated_level",
                features.estimated_noise_level,
                elapsed_micros,
            ),
            metric(
                "sharpness.gradient_energy",
                features.gradient_energy,
                elapsed_micros,
            ),
            metric(
                "sharpness.edge_density",
                features.edge_density,
                elapsed_micros,
            ),
            metric(
                "sharpness.laplacian_variance",
                features.laplacian_variance,
                elapsed_micros,
            ),
            metric(
                "sharpness.center_laplacian_variance",
                features.center_laplacian_variance,
                elapsed_micros,
            ),
            metric(
                "sharpness.effective_focus_variance",
                features.effective_focus_variance,
                elapsed_micros,
            ),
            metric(
                "sharpness.global_score",
                features.global_sharpness,
                elapsed_micros,
            ),
            metric(
                "sharpness.fft_high_frequency_ratio",
                features.fft_high_frequency_ratio,
                elapsed_micros,
            ),
        ])
    }
}

pub fn normalize(image: DynamicImage, max_dimension: u32) -> DynamicImage {
    use image::GenericImageView;
    let (width, height) = image.dimensions();
    let max_dimension = if max_dimension == 0 {
        DEFAULT_MAX_DIMENSION
    } else {
        max_dimension
    };
    if width.max(height) <= max_dimension {
        return image;
    }
    let scale = max_dimension as f64 / f64::from(width.max(height));
    let normalized_width = (f64::from(width) * scale) as u32;
    let normalized_height = (f64::from(height) * scale) as u32;
    image.resize_exact(
        normalized_width.max(1),
        normalized_height.max(1),
        FilterType::Triangle,
    )
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
        // Rec.709 floating-point white lands in bin 254, matching numpy.histogram.
        assert_eq!(features.histogram[254], 1);
        assert_eq!(features.red_histogram[0], 1);
        assert_eq!(features.red_histogram[255], 1);
        assert_eq!(features.luminance_entropy, 1.0);
        assert_eq!(features.shadow_clipping_ratio, 0.5);
        assert_eq!(features.highlight_clipping_ratio, 0.5);
        assert_eq!(features.center_shadow_clipping_ratio, 0.0);
        assert_eq!(features.center_highlight_clipping_ratio, 0.0);
    }

    #[test]
    fn measures_clipping_in_the_central_subject_region() {
        let mut image = RgbImage::from_pixel(4, 4, Rgb([128, 128, 128]));
        image.put_pixel(1, 1, Rgb([0, 0, 0]));
        image.put_pixel(2, 1, Rgb([255, 255, 255]));

        let features = PixelFeatures::from_image(DynamicImage::ImageRgb8(image), 1920);

        assert_eq!(features.center_shadow_clipping_ratio, 0.25);
        assert_eq!(features.center_highlight_clipping_ratio, 0.25);
        assert_eq!(features.underexposed_probability, 1.0 / 16.0);
        assert_eq!(features.overexposed_probability, 1.0 / 16.0);
        assert!(features.luminance_noise_stddev > 0.0);
        assert!(features.gradient_energy > 0.0);
        assert!(features.edge_density > 0.0);
        assert!(features.laplacian_variance > 0.0);
    }

    #[test]
    fn bounds_the_analysis_resolution() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(6000, 4000));
        let features = PixelFeatures::from_image(image, 1920);

        assert_eq!((features.width, features.height), (1920, 1280));
    }

    #[test]
    fn zero_dimension_uses_the_default_bound() {
        let image = DynamicImage::ImageRgb8(RgbImage::new(6000, 4000));
        let features = PixelFeatures::from_image(image, 0);

        assert_eq!((features.width, features.height), (1920, 1280));
    }
}
