//! Native, deterministic pixel primitives for the incremental Rust engine.
//!
//! This crate intentionally has no UI, database, or Python dependency.  Its
//! results are the shadow-mode contract that the Python pipeline will compare
//! before Rust is promoted for any analyzer.

use image::{DynamicImage, GenericImageView, imageops::FilterType};
use photo_culler_core::{AnalysisEngine, AnalysisRequest, MetricResult};
use rustfft::{FftPlanner, num_complex::Complex};
use serde::Serialize;

const IMPLEMENTATION_VERSION: &str = "rust-pixels-0.1";
const DEFAULT_MAX_DIMENSION: u32 = 1920;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PixelFeatures {
    pub width: u32,
    pub height: u32,
    pub red_histogram: Vec<u64>,
    pub green_histogram: Vec<u64>,
    pub blue_histogram: Vec<u64>,
    pub histogram: Vec<u64>,
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
    pub center_laplacian_variance: f64,
    pub effective_focus_variance: f64,
    pub global_sharpness: f64,
    pub fft_high_frequency_ratio: f64,
}

impl PixelFeatures {
    /// Analyze one decoded image after bounding its longest side.
    pub fn from_image(image: DynamicImage, max_dimension: u32) -> Self {
        let normalized = normalize(image, max_dimension);
        let (width, height) = normalized.dimensions();
        let rgb = normalized.to_rgb8();
        let mut red_histogram = vec![0_u64; 256];
        let mut green_histogram = vec![0_u64; 256];
        let mut blue_histogram = vec![0_u64; 256];
        let mut histogram = vec![0_u64; 256];
        let mut shadows = 0_u64;
        let mut highlights = 0_u64;
        let mut center_shadows = 0_u64;
        let mut center_highlights = 0_u64;
        let mut underexposed = 0_u64;
        let mut overexposed = 0_u64;
        let mut luminance_sum = 0_f64;
        let mut luminance_squared_sum = 0_f64;
        let mut luminance_values = Vec::with_capacity(width as usize * height as usize);
        let mut red_green_sum = 0_f64;
        let mut red_green_squared_sum = 0_f64;
        let mut blue_green_sum = 0_f64;
        let mut blue_green_squared_sum = 0_f64;
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
            let red_green = f64::from(red) - f64::from(green);
            let blue_green = f64::from(blue) - f64::from(green);
            red_green_sum += red_green;
            red_green_squared_sum += red_green * red_green;
            blue_green_sum += blue_green;
            blue_green_squared_sum += blue_green * blue_green;
            shadows += u64::from(luminance <= 1.0);
            highlights += u64::from(luminance >= 254.0);
            underexposed += u64::from(luminance < 60.0);
            // Exposure consumes the cached histogram in Python, where bins
            // 195..255 are counted as overexposed.
            overexposed += u64::from(luminance >= 195.0);
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
        // Exposure in Python consumes the integer luminance histogram rather
        // than the floating-point luminance array. Preserve that contract in
        // the shadow engine while keeping the true mean for histogram output.
        let histogram_mean_luminance = histogram
            .iter()
            .enumerate()
            .map(|(bin, count)| bin as f64 * *count as f64)
            .sum::<f64>()
            / count;
        let exposure_score =
            (1.0 - (histogram_mean_luminance - 118.0).abs() / 118.0).clamp(0.0, 1.0);
        let (luminance_noise_stddev, shadow_noise_stddev) =
            noise_standard_deviations(&luminance_values, width as usize, height as usize);
        let chroma_noise_stddev =
            (standard_deviation_from_moments(red_green_sum, red_green_squared_sum, count)
                + standard_deviation_from_moments(blue_green_sum, blue_green_squared_sum, count))
                / 2.0;
        let estimated_noise_level =
            ((luminance_noise_stddev + chroma_noise_stddev * 0.5) / 30.0).clamp(0.0, 1.0);
        let (gradient_energy, edge_density, laplacian_variance, center_laplacian_variance) =
            edge_statistics(&luminance_values, width as usize, height as usize);
        let effective_focus_variance = center_laplacian_variance * 0.6 + laplacian_variance * 0.4;
        let global_sharpness = effective_focus_variance.max(1.0).log10() / 3.5;
        let fft_high_frequency_ratio =
            fft_high_frequency_ratio(&luminance_values, width as usize, height as usize);
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
            center_laplacian_variance,
            effective_focus_variance,
            global_sharpness: global_sharpness.clamp(0.0, 1.0),
            fft_high_frequency_ratio,
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

fn standard_deviation_from_moments(sum: f64, squared_sum: f64, count: f64) -> f64 {
    if count == 0.0 {
        return 0.0;
    }
    (squared_sum / count - (sum / count).powi(2))
        .max(0.0)
        .sqrt()
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

fn edge_statistics(luminance: &[f64], width: usize, height: usize) -> (f64, f64, f64, f64) {
    if width < 2 || height < 2 {
        return (0.0, 0.0, 0.0, 0.0);
    }
    let mut gradient_x = vec![0.0; luminance.len()];
    let mut gradient_y = vec![0.0; luminance.len()];
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            gradient_x[index] = if x == 0 {
                luminance[index + 1] - luminance[index]
            } else if x + 1 == width {
                luminance[index] - luminance[index - 1]
            } else {
                (luminance[index + 1] - luminance[index - 1]) / 2.0
            };
            gradient_y[index] = if y == 0 {
                luminance[index + width] - luminance[index]
            } else if y + 1 == height {
                luminance[index] - luminance[index - width]
            } else {
                (luminance[index + width] - luminance[index - width]) / 2.0
            };
        }
    }
    let mut gradient_energy_sum = 0.0;
    let mut edge_count = 0_u64;
    let mut laplacians = vec![0.0; luminance.len()];
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let energy = gradient_x[index].powi(2) + gradient_y[index].powi(2);
            gradient_energy_sum += energy;
            edge_count += u64::from(energy.sqrt() > 12.0);
            let gxx = if x == 0 {
                gradient_x[index + 1] - gradient_x[index]
            } else if x + 1 == width {
                gradient_x[index] - gradient_x[index - 1]
            } else {
                (gradient_x[index + 1] - gradient_x[index - 1]) / 2.0
            };
            let gyy = if y == 0 {
                gradient_y[index + width] - gradient_y[index]
            } else if y + 1 == height {
                gradient_y[index] - gradient_y[index - width]
            } else {
                (gradient_y[index + width] - gradient_y[index - width]) / 2.0
            };
            laplacians[index] = gxx + gyy;
        }
    }
    let count = luminance.len() as f64;
    let center_laplacians = laplacians
        .chunks(width)
        .skip(height / 4)
        .take(height * 3 / 4 - height / 4)
        .flat_map(|row| row[width / 4..width * 3 / 4].iter().copied())
        .collect::<Vec<_>>();
    (
        gradient_energy_sum / count,
        edge_count as f64 / count,
        standard_deviation(&laplacians).powi(2),
        standard_deviation(&center_laplacians).powi(2),
    )
}

fn fft_high_frequency_ratio(luminance: &[f64], width: usize, height: usize) -> f64 {
    if width == 0 || height == 0 {
        return 0.0;
    }
    let mut values = luminance
        .iter()
        .map(|&real| Complex::<f32>::new(real as f32, 0.0))
        .collect::<Vec<_>>();
    let mut planner = FftPlanner::<f32>::new();
    let row_fft = planner.plan_fft_forward(width);
    for row in values.chunks_exact_mut(width) {
        row_fft.process(row);
    }
    let column_fft = planner.plan_fft_forward(height);
    let mut column = vec![Complex::<f32>::new(0.0, 0.0); height];
    for x in 0..width {
        for y in 0..height {
            column[y] = values[y * width + x];
        }
        column_fft.process(&mut column);
        for y in 0..height {
            values[y * width + x] = column[y];
        }
    }
    let center_x = width / 2;
    let center_y = height / 2;
    let radius = width.min(height) / 10;
    let mut high_frequency = 0.0;
    let mut total = 0.0;
    for y in 0..height {
        for x in 0..width {
            let magnitude = f64::from(values[y * width + x].norm());
            total += magnitude;
            let shifted_x = (x + center_x) % width;
            let shifted_y = (y + center_y) % height;
            let dx = shifted_x as i64 - center_x as i64;
            let dy = shifted_y as i64 - center_y as i64;
            if dx * dx + dy * dy > (radius * radius) as i64 {
                high_frequency += magnitude;
            }
        }
    }
    high_frequency / (total + 1e-8)
}

fn normalize(image: DynamicImage, max_dimension: u32) -> DynamicImage {
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
