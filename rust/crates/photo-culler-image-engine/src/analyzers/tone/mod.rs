//! Tone and image-dimension metrics over an already decoded, normalized RGB8 image.
//!
//! The decoder owns file-level integrity/decodability. This analyzer validates
//! the decoded buffer shape and never reads or changes the source file.

/// Stable analyzer IDs used by the Rust `MetricResult` adapter.
pub const DIMENSIONS_ANALYZER_ID: &str = "dimensions";
pub const HISTOGRAM_ANALYZER_ID: &str = "histogram";
pub const EXPOSURE_ANALYZER_ID: &str = "exposure";
pub const CLIPPING_ANALYZER_ID: &str = "clipping";

/// Version of these Rust metric calculations. Bump when numerical semantics
/// change so callers can invalidate cached results explicitly.
pub const IMPLEMENTATION_VERSION: &str = "rust-tone-0.1";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Orientation {
    Portrait,
    Landscape,
    Square,
    Panorama,
}

impl Orientation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
            Self::Square => "square",
            Self::Panorama => "panorama",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToneAnalysis {
    pub width: u32,
    pub height: u32,
    pub aspect_ratio: f64,
    pub megapixels: f64,
    pub orientation: Orientation,
    pub pixel_count: u64,

    pub red_histogram: [u64; 256],
    pub green_histogram: [u64; 256],
    pub blue_histogram: [u64; 256],
    pub luminance_histogram: [u64; 256],
    pub percentile_1: f64,
    pub percentile_99: f64,
    pub median_luminance: f64,
    pub mean_luminance: f64,
    /// Histogram-bin mean used by the established exposure analyzer.
    pub exposure_mean_luminance: f64,
    pub luminance_stddev: f64,
    pub luminance_entropy_bits: f64,
    pub red_mean: f64,
    pub green_mean: f64,
    pub blue_mean: f64,

    pub exposure_score: f64,
    pub underexposed_probability: f64,
    pub overexposed_probability: f64,
    pub is_underexposed: bool,
    pub is_overexposed: bool,
    pub is_balanced: bool,

    /// Clipping fractions in `[0, 1]`, matching the Rust shadow adapter's
    /// `clipping.*_ratio` contract. Multiply by 100 for legacy Python `*_pct`.
    pub shadow_clipping_ratio: f64,
    pub highlight_clipping_ratio: f64,
    pub center_shadow_clipping_ratio: f64,
    pub center_highlight_clipping_ratio: f64,
    pub red_shadow_clipping_ratio: f64,
    pub red_highlight_clipping_ratio: f64,
    pub green_shadow_clipping_ratio: f64,
    pub green_highlight_clipping_ratio: f64,
    pub blue_shadow_clipping_ratio: f64,
    pub blue_highlight_clipping_ratio: f64,
    pub has_severe_subject_clipping: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToneInputError {
    BufferLengthOverflow,
    BufferLengthMismatch { expected: usize, actual: usize },
}

/// Analyze RGB8 bytes in row-major order. The caller supplies the same
/// orientation-corrected, longest-side-bounded representation used by the
/// other technical analyzers (the current shared bound is 1920 pixels).
///
/// Empty dimensions with an empty buffer produce finite zero-valued metrics.
/// A non-empty image with fewer than four pixels can have an empty central ROI;
/// its center clipping ratios are defined as zero rather than NaN.
pub fn analyze(rgb: &[u8], width: u32, height: u32) -> Result<ToneAnalysis, ToneInputError> {
    let pixel_count = (width as usize)
        .checked_mul(height as usize)
        .ok_or(ToneInputError::BufferLengthOverflow)?;
    let expected = pixel_count
        .checked_mul(3)
        .ok_or(ToneInputError::BufferLengthOverflow)?;
    if rgb.len() != expected {
        return Err(ToneInputError::BufferLengthMismatch {
            expected,
            actual: rgb.len(),
        });
    }

    let mut red_histogram = [0_u64; 256];
    let mut green_histogram = [0_u64; 256];
    let mut blue_histogram = [0_u64; 256];
    let mut luminance_histogram = [0_u64; 256];
    let mut luminance = Vec::with_capacity(pixel_count);
    let mut shadow_count = 0_u64;
    let mut highlight_count = 0_u64;
    let mut underexposed_count = 0_u64;
    let mut overexposed_count = 0_u64;
    let mut center_shadow_count = 0_u64;
    let mut center_highlight_count = 0_u64;
    let mut red_shadow_count = 0_u64;
    let mut red_highlight_count = 0_u64;
    let mut green_shadow_count = 0_u64;
    let mut green_highlight_count = 0_u64;
    let mut blue_shadow_count = 0_u64;
    let mut blue_highlight_count = 0_u64;

    let width_usize = width as usize;
    let height_usize = height as usize;
    let center_x_start = width_usize / 4;
    let center_x_end = three_quarters(width_usize);
    let center_y_start = height_usize / 4;
    let center_y_end = three_quarters(height_usize);

    for (index, pixel) in rgb.chunks_exact(3).enumerate() {
        let [red, green, blue] = [pixel[0], pixel[1], pixel[2]];
        red_histogram[red as usize] += 1;
        green_histogram[green as usize] += 1;
        blue_histogram[blue as usize] += 1;

        // Preserve the exact Rec.709 coefficients and floor-bin behavior of
        // the existing Rust shadow implementation and NumPy's 1-wide bins.
        let value = 0.2126 * f64::from(red) + 0.7152 * f64::from(green) + 0.0722 * f64::from(blue);
        let bin = (value.floor() as usize).min(255);
        luminance_histogram[bin] += 1;
        luminance.push(value);

        shadow_count += u64::from(value <= 1.0);
        highlight_count += u64::from(value >= 254.0);
        underexposed_count += u64::from(value < 60.0);
        // The established exposure contract counts histogram bins 195..255.
        overexposed_count += u64::from(bin >= 195);

        let x = index % width_usize.max(1);
        let y = index / width_usize.max(1);
        if (center_x_start..center_x_end).contains(&x)
            && (center_y_start..center_y_end).contains(&y)
        {
            center_shadow_count += u64::from(value <= 1.0);
            center_highlight_count += u64::from(value >= 254.0);
        }
        red_shadow_count += u64::from(red <= 1);
        red_highlight_count += u64::from(red >= 254);
        green_shadow_count += u64::from(green <= 1);
        green_highlight_count += u64::from(green >= 254);
        blue_shadow_count += u64::from(blue <= 1);
        blue_highlight_count += u64::from(blue >= 254);
    }

    let count = pixel_count as f64;
    let mean_luminance = mean(&luminance);
    let luminance_stddev = population_stddev(&luminance, mean_luminance);
    let mut sorted_luminance = luminance.clone();
    sorted_luminance.sort_by(f64::total_cmp);
    let percentile_1 = percentile(&sorted_luminance, 0.01);
    let percentile_99 = percentile(&sorted_luminance, 0.99);
    let median_luminance = percentile(&sorted_luminance, 0.50);
    let luminance_entropy_bits = entropy(&luminance_histogram, count);

    let histogram_mean_luminance = if count == 0.0 {
        0.0
    } else {
        luminance_histogram
            .iter()
            .enumerate()
            .map(|(bin, bin_count)| bin as f64 * *bin_count as f64)
            .sum::<f64>()
            / count
    };
    let underexposed_probability = probability(underexposed_count, pixel_count);
    let overexposed_probability = probability(overexposed_count, pixel_count);
    let is_underexposed = underexposed_probability > 0.40 && histogram_mean_luminance < 80.0;
    let is_overexposed = overexposed_probability > 0.40 && histogram_mean_luminance > 175.0;
    let center_pixel_count =
        (center_x_end - center_x_start).saturating_mul(center_y_end - center_y_start);
    let center_shadow_clipping_ratio = probability(center_shadow_count, center_pixel_count);
    let center_highlight_clipping_ratio = probability(center_highlight_count, center_pixel_count);

    let analysis = ToneAnalysis {
        width,
        height,
        aspect_ratio: if height == 0 {
            0.0
        } else {
            width as f64 / height as f64
        },
        megapixels: count / 1_000_000.0,
        orientation: orientation(width, height),
        pixel_count: pixel_count as u64,
        red_histogram,
        green_histogram,
        blue_histogram,
        luminance_histogram,
        percentile_1,
        percentile_99,
        median_luminance,
        mean_luminance,
        exposure_mean_luminance: histogram_mean_luminance,
        luminance_stddev,
        luminance_entropy_bits,
        red_mean: mean_channel(&rgb, 0, pixel_count),
        green_mean: mean_channel(&rgb, 1, pixel_count),
        blue_mean: mean_channel(&rgb, 2, pixel_count),
        exposure_score: (1.0 - (histogram_mean_luminance - 118.0).abs() / 118.0).clamp(0.0, 1.0),
        underexposed_probability,
        overexposed_probability,
        is_underexposed,
        is_overexposed,
        is_balanced: !is_underexposed && !is_overexposed,
        shadow_clipping_ratio: probability(shadow_count, pixel_count),
        highlight_clipping_ratio: probability(highlight_count, pixel_count),
        center_shadow_clipping_ratio,
        center_highlight_clipping_ratio,
        red_shadow_clipping_ratio: probability(red_shadow_count, pixel_count),
        red_highlight_clipping_ratio: probability(red_highlight_count, pixel_count),
        green_shadow_clipping_ratio: probability(green_shadow_count, pixel_count),
        green_highlight_clipping_ratio: probability(green_highlight_count, pixel_count),
        blue_shadow_clipping_ratio: probability(blue_shadow_count, pixel_count),
        blue_highlight_clipping_ratio: probability(blue_highlight_count, pixel_count),
        has_severe_subject_clipping: center_highlight_clipping_ratio > 0.08
            || center_shadow_clipping_ratio > 0.12,
    };
    Ok(analysis)
}

impl ToneAnalysis {
    /// Return the stable numeric metric identifiers consumed by the shared
    /// `MetricResult` adapter. Ratios are fractions, not percentages.
    pub fn metric_values(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("dimensions.width", self.width as f64),
            ("dimensions.height", self.height as f64),
            ("dimensions.aspect_ratio", self.aspect_ratio),
            ("dimensions.megapixels", self.megapixels),
            ("histogram.percentile_1", self.percentile_1),
            ("histogram.percentile_99", self.percentile_99),
            ("histogram.median_luminance", self.median_luminance),
            ("histogram.mean_luminance", self.mean_luminance),
            ("histogram.luminance_stddev", self.luminance_stddev),
            ("histogram.luminance_entropy", self.luminance_entropy_bits),
            ("histogram.r_mean", self.red_mean),
            ("histogram.g_mean", self.green_mean),
            ("histogram.b_mean", self.blue_mean),
            ("exposure.score", self.exposure_score),
            ("exposure.mean_luminance", self.exposure_mean_luminance),
            (
                "exposure.underexposed_probability",
                self.underexposed_probability,
            ),
            (
                "exposure.overexposed_probability",
                self.overexposed_probability,
            ),
            ("clipping.shadow_ratio", self.shadow_clipping_ratio),
            ("clipping.highlight_ratio", self.highlight_clipping_ratio),
            (
                "clipping.center_shadow_ratio",
                self.center_shadow_clipping_ratio,
            ),
            (
                "clipping.center_highlight_ratio",
                self.center_highlight_clipping_ratio,
            ),
            ("clipping.r_shadow_ratio", self.red_shadow_clipping_ratio),
            (
                "clipping.r_highlight_ratio",
                self.red_highlight_clipping_ratio,
            ),
            ("clipping.g_shadow_ratio", self.green_shadow_clipping_ratio),
            (
                "clipping.g_highlight_ratio",
                self.green_highlight_clipping_ratio,
            ),
            ("clipping.b_shadow_ratio", self.blue_shadow_clipping_ratio),
            (
                "clipping.b_highlight_ratio",
                self.blue_highlight_clipping_ratio,
            ),
            ("exposure.is_underexposed", f64::from(self.is_underexposed)),
            ("exposure.is_overexposed", f64::from(self.is_overexposed)),
            ("exposure.is_balanced", f64::from(self.is_balanced)),
            (
                "clipping.has_severe_subject_clipping",
                f64::from(self.has_severe_subject_clipping),
            ),
        ]
    }
}

fn probability(count: u64, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        count as f64 / total as f64
    }
}

fn mean(values: &[f64]) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        values.iter().sum::<f64>() / values.len() as f64
    }
}

fn mean_channel(rgb: &[u8], channel: usize, pixel_count: usize) -> f64 {
    if pixel_count == 0 {
        0.0
    } else {
        rgb.chunks_exact(3)
            .map(|pixel| f64::from(pixel[channel]))
            .sum::<f64>()
            / pixel_count as f64
    }
}

fn population_stddev(values: &[f64], average: f64) -> f64 {
    if values.is_empty() {
        0.0
    } else {
        (values
            .iter()
            .map(|value| (value - average).powi(2))
            .sum::<f64>()
            / values.len() as f64)
            .sqrt()
    }
}

fn percentile(sorted: &[f64], fraction: f64) -> f64 {
    match sorted.len() {
        0 => 0.0,
        1 => sorted[0],
        length => {
            let position = (length - 1) as f64 * fraction;
            let lower = position.floor() as usize;
            let upper = position.ceil() as usize;
            let weight = position - lower as f64;
            sorted[lower] * (1.0 - weight) + sorted[upper] * weight
        }
    }
}

fn entropy(histogram: &[u64; 256], total: f64) -> f64 {
    if total == 0.0 {
        return 0.0;
    }
    histogram
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let probability = count as f64 / total;
            -probability * probability.log2()
        })
        .sum()
}

fn three_quarters(value: usize) -> usize {
    (value / 4) * 3 + ((value % 4) * 3) / 4
}

fn orientation(width: u32, height: u32) -> Orientation {
    let max_dimension = width.max(height);
    if max_dimension == 0 {
        return Orientation::Square;
    }
    let aspect_ratio = if height == 0 {
        0.0
    } else {
        width as f64 / height as f64
    };
    let inverse_aspect_ratio = if width == 0 {
        0.0
    } else {
        height as f64 / width as f64
    };
    if aspect_ratio >= 2.0 || inverse_aspect_ratio >= 2.0 {
        Orientation::Panorama
    } else if height > width {
        Orientation::Portrait
    } else if width > height {
        Orientation::Landscape
    } else {
        Orientation::Square
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixels(width: usize, height: usize, color: [u8; 3]) -> Vec<u8> {
        color.repeat(width * height)
    }

    fn assert_all_numeric_metrics_finite(result: &ToneAnalysis) {
        assert!(
            result
                .metric_values()
                .iter()
                .all(|metric| metric.1.is_finite())
        );
        assert!(result.percentile_1.is_finite());
        assert!(result.percentile_99.is_finite());
        assert!(result.luminance_entropy_bits.is_finite());
    }

    #[test]
    fn measures_black_pixels_and_underexposure() {
        let result = analyze(&pixels(4, 4, [0, 0, 0]), 4, 4).unwrap();

        assert_eq!(result.pixel_count, 16);
        assert_eq!(result.luminance_histogram[0], 16);
        assert_eq!(result.mean_luminance, 0.0);
        assert_eq!(result.shadow_clipping_ratio, 1.0);
        assert_eq!(result.underexposed_probability, 1.0);
        assert!(result.is_underexposed);
        assert!(result.has_severe_subject_clipping);
        assert_all_numeric_metrics_finite(&result);
    }

    #[test]
    fn measures_highlights_and_subject_clipping_as_fractions() {
        let mut rgb = pixels(4, 4, [128, 128, 128]);
        for index in [5, 6] {
            rgb[index * 3..index * 3 + 3].copy_from_slice(&[255, 255, 255]);
        }
        let result = analyze(&rgb, 4, 4).unwrap();

        assert_eq!(result.luminance_histogram[254], 2);
        assert_eq!(result.highlight_clipping_ratio, 2.0 / 16.0);
        assert_eq!(result.center_highlight_clipping_ratio, 2.0 / 4.0);
        assert_eq!(result.blue_highlight_clipping_ratio, 2.0 / 16.0);
        assert_all_numeric_metrics_finite(&result);
    }

    #[test]
    fn retains_stage_light_measurements_without_hiding_them() {
        let mut rgb = pixels(8, 8, [24, 24, 30]);
        // Four bright warm stage lights lie outside the central subject ROI.
        for index in [1, 6, 9, 14] {
            rgb[index * 3..index * 3 + 3].copy_from_slice(&[255, 255, 255]);
        }
        let result = analyze(&rgb, 8, 8).unwrap();

        assert!(result.highlight_clipping_ratio > 0.0);
        assert_eq!(result.center_highlight_clipping_ratio, 0.0);
        // Profile-specific concert tolerance is applied by scoring, while the
        // analyzer retains the same raw measurements for every profile.
        assert!(result.highlight_clipping_ratio < 0.08);
        assert_all_numeric_metrics_finite(&result);
    }

    #[test]
    fn empty_small_and_uniform_inputs_stay_finite() {
        let empty = analyze(&[], 0, 0).unwrap();
        let one_pixel = analyze(&[42, 42, 42], 1, 1).unwrap();
        let uniform = analyze(&pixels(12, 8, [128, 128, 128]), 12, 8).unwrap();

        assert_eq!(empty.pixel_count, 0);
        assert_eq!(empty.exposure_score, 0.0);
        assert_eq!(one_pixel.center_shadow_clipping_ratio, 0.0);
        assert_eq!(uniform.luminance_stddev, 0.0);
        for result in [&empty, &one_pixel, &uniform] {
            assert_all_numeric_metrics_finite(result);
        }
    }

    #[test]
    fn rejects_incomplete_decoded_buffers() {
        assert_eq!(
            analyze(&[1, 2, 3], 2, 1),
            Err(ToneInputError::BufferLengthMismatch {
                expected: 6,
                actual: 3,
            })
        );
    }

    #[test]
    fn preserves_dimensions_identifiers_and_units() {
        let result = analyze(&pixels(4, 3, [118, 118, 118]), 4, 3).unwrap();

        assert_eq!(result.orientation, Orientation::Landscape);
        assert!((result.aspect_ratio - (4.0 / 3.0)).abs() < f64::EPSILON);
        assert_eq!(result.megapixels, 12.0 / 1_000_000.0);
        let values = result.metric_values();
        assert!(
            values
                .iter()
                .any(|metric| metric.0 == "histogram.luminance_entropy")
        );
        assert!(
            values
                .iter()
                .any(|metric| metric.0 == "clipping.highlight_ratio")
        );
        assert!((0.0..=1.0).contains(&result.highlight_clipping_ratio));
    }
}
