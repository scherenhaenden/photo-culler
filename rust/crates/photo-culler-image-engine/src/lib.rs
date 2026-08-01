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
    pub red_histogram: [u64; 256],
    pub green_histogram: [u64; 256],
    pub blue_histogram: [u64; 256],
    pub histogram: [u64; 256],
    pub pixel_count: u64,
    pub mean_luminance: f64,
    pub luminance_stddev: f64,
    pub luminance_entropy: f64,
    pub shadow_clipping_ratio: f64,
    pub highlight_clipping_ratio: f64,
    pub center_shadow_clipping_ratio: f64,
    pub center_highlight_clipping_ratio: f64,
    pub underexposed_probability: f64,
    pub overexposed_probability: f64,
    pub exposure_score: f64,
    pub luminance_noise_stddev: f64,
    pub chroma_noise_stddev: f64,
    pub shadow_noise_stddev: f64,
    pub estimated_noise_level: f64,
    pub gradient_energy: f64,
    pub edge_density: f64,
    pub laplacian_variance: f64,
}

impl PixelFeatures {
    /// Analyze one decoded image after bounding its longest side.
    pub fn from_image(image: DynamicImage, max_dimension: u32) -> Self {
        let normalized = normalize(image, max_dimension);
        let (width, height) = normalized.dimensions();
        let rgb = normalized.to_rgb8();
        let mut red_histogram = [0_u64; 256];
        let mut green_histogram = [0_u64; 256];
        let mut blue_histogram = [0_u64; 256];
        let mut histogram = [0_u64; 256];
        let mut shadows = 0_u64;
        let mut highlights = 0_u64;
        let mut center_shadows = 0_u64;
        let mut center_highlights = 0_u64;
        let mut underexposed = 0_u64;
        let mut overexposed = 0_u64;
        let mut luminance_sum = 0_f64;
        let mut luminance_squared_sum = 0_f64;
        let mut luminance_values = Vec::with_capacity(width as usize * height as usize);
        let mut red_green_values = Vec::with_capacity(width as usize * height as usize);
        let mut blue_green_values = Vec::with_capacity(width as usize * height as usize);
        let center_y_start = height / 4;
        let center_y_end = height * 3 / 4;
        let center_x_start = width / 4;
        let center_x_end = width * 3 / 4;

        for (x, y, pixel) in rgb.enumerate_pixels() {
            let [red, green, blue] = pixel.0;
            // Match the Python analyzers' Rec.709 conversion. Histogram bins
            // intentionally use floor(), as numpy.histogram does for 1-wide bins.
            let luminance =
                0.2126 * f64::from(red) + 0.7152 * f64::from(green) + 0.0722 * f64::from(blue);
            red_histogram[red as usize] += 1;
            green_histogram[green as usize] += 1;
            blue_histogram[blue as usize] += 1;
            histogram[luminance.floor() as usize] += 1;
            luminance_sum += luminance;
            luminance_squared_sum += luminance * luminance;
            luminance_values.push(luminance);
            red_green_values.push(f64::from(red) - f64::from(green));
            blue_green_values.push(f64::from(blue) - f64::from(green));
            shadows += u64::from(luminance <= 1.0);
            highlights += u64::from(luminance >= 254.0);
            underexposed += u64::from(luminance < 60.0);
            overexposed += u64::from(luminance > 195.0);
            if (center_x_start..center_x_end).contains(&x)
                && (center_y_start..center_y_end).contains(&y)
            {
                center_shadows += u64::from(luminance <= 1.0);
                center_highlights += u64::from(luminance >= 254.0);
            }
        }

        let pixel_count = width as u64 * height as u64;
        let count = pixel_count.max(1) as f64;
        let mean_luminance = luminance_sum / count;
        let luminance_variance = (luminance_squared_sum / count - mean_luminance.powi(2)).max(0.0);
        let center_pixel_count =
            (center_x_end - center_x_start) as u64 * (center_y_end - center_y_start) as u64;
        let center_count = center_pixel_count.max(1) as f64;
        let luminance_entropy = histogram
            .iter()
            .filter(|&&bin| bin > 0)
            .map(|&bin| {
                let probability = bin as f64 / count;
                -probability * probability.log2()
            })
            .sum();
        let exposure_score = (1.0 - (mean_luminance - 118.0).abs() / 118.0).clamp(0.0, 1.0);
        let (luminance_noise_stddev, shadow_noise_stddev) =
            noise_standard_deviations(&luminance_values, width as usize, height as usize);
        let chroma_noise_stddev =
            (standard_deviation(&red_green_values) + standard_deviation(&blue_green_values)) / 2.0;
        let estimated_noise_level =
            ((luminance_noise_stddev + chroma_noise_stddev * 0.5) / 30.0).clamp(0.0, 1.0);
        let (gradient_energy, edge_density, laplacian_variance) =
            edge_statistics(&luminance_values, width as usize, height as usize);
        Self {
            width,
            height,
            red_histogram,
            green_histogram,
            blue_histogram,
            histogram,
            pixel_count,
            mean_luminance,
            luminance_stddev: luminance_variance.sqrt(),
            luminance_entropy,
            shadow_clipping_ratio: shadows as f64 / count,
            highlight_clipping_ratio: highlights as f64 / count,
            center_shadow_clipping_ratio: center_shadows as f64 / center_count,
            center_highlight_clipping_ratio: center_highlights as f64 / center_count,
            underexposed_probability: underexposed as f64 / count,
            overexposed_probability: overexposed as f64 / count,
            exposure_score,
            luminance_noise_stddev,
            chroma_noise_stddev,
            shadow_noise_stddev,
            estimated_noise_level,
            gradient_energy,
            edge_density,
            laplacian_variance,
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
        ])
    }
}

fn standard_deviation(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let variance = values
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn noise_standard_deviations(luminance: &[f64], width: usize, height: usize) -> (f64, f64) {
    if width < 2 || height < 2 {
        return (0.0, 0.0);
    }
    let mut horizontal_differences = Vec::with_capacity((height - 1) * width);
    let mut vertical_differences = Vec::with_capacity(height * (width - 1));
    let mut shadow_differences = Vec::new();
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            if y > 0 {
                let difference = (luminance[index] - luminance[index - width]).abs();
                horizontal_differences.push(difference);
                if luminance[index - width] < 40.0 {
                    shadow_differences.push(difference);
                }
            }
            if x > 0 {
                vertical_differences.push((luminance[index] - luminance[index - 1]).abs());
            }
        }
    }
    let luminance_noise = (standard_deviation(&horizontal_differences)
        + standard_deviation(&vertical_differences))
        / 2.0;
    let shadow_noise = if shadow_differences.len() > 100 {
        standard_deviation(&shadow_differences)
    } else {
        luminance_noise
    };
    (luminance_noise, shadow_noise)
}

fn edge_statistics(luminance: &[f64], width: usize, height: usize) -> (f64, f64, f64) {
    if width < 3 || height < 3 {
        return (0.0, 0.0, 0.0);
    }
    let mut gradient_energy_sum = 0.0;
    let mut edge_count = 0_u64;
    let mut laplacians = Vec::with_capacity((width - 2) * (height - 2));
    let mut gradient_count = 0_u64;
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let index = y * width + x;
            let gradient_x = (luminance[index + 1] - luminance[index - 1]) / 2.0;
            let gradient_y = (luminance[index + width] - luminance[index - width]) / 2.0;
            let energy = gradient_x.powi(2) + gradient_y.powi(2);
            gradient_energy_sum += energy;
            edge_count += u64::from(energy.sqrt() > 12.0);
            laplacians.push(
                luminance[index - 1]
                    + luminance[index + 1]
                    + luminance[index - width]
                    + luminance[index + width]
                    - 4.0 * luminance[index],
            );
            gradient_count += 1;
        }
    }
    let count = gradient_count.max(1) as f64;
    (
        gradient_energy_sum / count,
        edge_count as f64 / count,
        standard_deviation(&laplacians).powi(2),
    )
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
}
