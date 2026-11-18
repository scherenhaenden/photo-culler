//! Sharpness, noise, and directional-motion measurements over the engine's
//! normalized RGB8 analysis frame (longest side bounded to 1920 pixels).
//!
//! This module is intentionally isolated until the image-engine root registers
//! `analyzers::detail`. The frame is row-major RGB8; luminance uses the same
//! Rec.709 coefficients and 0..255 scale as `metrics::PixelFeatures`.

#[path = "../../algorithms/edges.rs"]
mod edge_engine;
#[path = "../../algorithms/noise.rs"]
mod noise_engine;

/// Bump when detail metric behavior or units change; adapters and caches use
/// this value to distinguish incompatible results.
pub const IMPLEMENTATION_VERSION: &str = "rust-detail-0.1";

/// Measurements calculated on the normalized frame. Variances and gradient
/// energy use luminance-levels squared (0..255 input); standard deviations use
/// luminance/chroma levels (0..255); densities, scores, anisotropy, confidence,
/// and frequency ratios are fractions in 0..1. Motion direction is degrees in
/// [0, 180), and streak length is in normalized-frame pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetailAnalysis {
    pub valid_input: bool,
    pub global_sharpness: f64,
    pub laplacian_variance: f64,
    pub center_laplacian_variance: f64,
    pub effective_focus_variance: f64,
    pub regional_laplacian_variance: [f64; 9],
    pub gradient_energy: f64,
    pub edge_density: f64,
    pub fft_high_freq_ratio: f64,
    pub luminance_noise_std: f64,
    pub chroma_noise_std: f64,
    pub shadow_noise_std: f64,
    pub estimated_noise_level: f64,
    pub is_tack_sharp: bool,
    pub is_soft: bool,
    pub is_noisy: bool,
    pub is_clean: bool,
    pub blur_direction_deg: f64,
    pub blur_length_px: f64,
    pub directional_anisotropy: f64,
    pub motion_blur_confidence: f64,
    pub is_directional_motion_blur: bool,
}

impl DetailAnalysis {
    /// Stable scalar metric identifiers used by the current Rust engine
    /// adapter. The nine regional values are available separately in
    /// `regional_laplacian_variance` for a later result-schema extension.
    pub fn metric_values(&self) -> Vec<(&'static str, f64)> {
        vec![
            ("sharpness.gradient_energy", self.gradient_energy),
            ("sharpness.edge_density", self.edge_density),
            ("sharpness.laplacian_variance", self.laplacian_variance),
            (
                "sharpness.center_laplacian_variance",
                self.center_laplacian_variance,
            ),
            (
                "sharpness.effective_focus_variance",
                self.effective_focus_variance,
            ),
            ("sharpness.global_score", self.global_sharpness),
            (
                "sharpness.fft_high_frequency_ratio",
                self.fft_high_freq_ratio,
            ),
            ("noise.luminance_stddev", self.luminance_noise_std),
            ("noise.chroma_stddev", self.chroma_noise_std),
            ("noise.shadow_stddev", self.shadow_noise_std),
            ("noise.estimated_level", self.estimated_noise_level),
            ("motion_blur.blur_direction_deg", self.blur_direction_deg),
            ("motion_blur.blur_length_px", self.blur_length_px),
            (
                "motion_blur.directional_anisotropy",
                self.directional_anisotropy,
            ),
            (
                "motion_blur.motion_blur_confidence",
                self.motion_blur_confidence,
            ),
        ]
    }

    /// True when every numeric output is finite. Degenerate frames produce
    /// finite zero metrics and a false `valid_input` flag.
    pub fn numeric_values_are_finite(&self) -> bool {
        [
            self.global_sharpness,
            self.laplacian_variance,
            self.center_laplacian_variance,
            self.effective_focus_variance,
            self.gradient_energy,
            self.edge_density,
            self.fft_high_freq_ratio,
            self.luminance_noise_std,
            self.chroma_noise_std,
            self.shadow_noise_std,
            self.estimated_noise_level,
            self.blur_direction_deg,
            self.blur_length_px,
            self.directional_anisotropy,
            self.motion_blur_confidence,
        ]
        .into_iter()
        .chain(self.regional_laplacian_variance)
        .all(f64::is_finite)
    }
}

/// Analyze a normalized row-major RGB8 image.
///
/// The FFT high-frequency ratio is computed by the existing image-engine FFT
/// primitive and supplied by the caller, avoiding a second FFT implementation.
pub fn analyze(
    rgb: &[u8],
    width: u32,
    height: u32,
    fft_high_frequency_ratio: f64,
) -> DetailAnalysis {
    let (Ok(width), Ok(height)) = (usize::try_from(width), usize::try_from(height)) else {
        return DetailAnalysis::default();
    };
    let Some(expected_len) = width
        .checked_mul(height)
        .and_then(|pixels| pixels.checked_mul(3))
    else {
        return DetailAnalysis::default();
    };
    if width == 0 || height == 0 || expected_len != rgb.len() {
        return DetailAnalysis::default();
    }

    let luminance = rgb
        .chunks_exact(3)
        .map(|pixel| {
            0.2126 * f64::from(pixel[0])
                + 0.7152 * f64::from(pixel[1])
                + 0.0722 * f64::from(pixel[2])
        })
        .collect::<Vec<_>>();
    let (gradient_energy, edge_density, laplacian_variance, center_laplacian_variance) =
        edge_engine::edge_statistics(&luminance, width, height);
    let effective_focus_variance = center_laplacian_variance * 0.6 + laplacian_variance * 0.4;
    let global_sharpness = (effective_focus_variance.max(1.0).log10() / 3.5).clamp(0.0, 1.0);
    let regional_laplacian_variance = regional_laplacian_variances(&luminance, width, height);
    let (luminance_noise_std, shadow_noise_std) =
        noise_engine::noise_standard_deviations(&luminance, width, height);
    let chroma_noise_std = chroma_noise_standard_deviation(rgb);
    let estimated_noise_level =
        ((luminance_noise_std + chroma_noise_std * 0.5) / 30.0).clamp(0.0, 1.0);
    let (blur_direction_deg, blur_length_px, directional_anisotropy) =
        motion_features(&luminance, width, height);
    let motion_blur_confidence = (directional_anisotropy * 1.5).clamp(0.0, 1.0);

    DetailAnalysis {
        valid_input: true,
        global_sharpness,
        laplacian_variance,
        center_laplacian_variance,
        effective_focus_variance,
        regional_laplacian_variance,
        gradient_energy,
        edge_density,
        fft_high_freq_ratio: finite_fraction(fft_high_frequency_ratio),
        luminance_noise_std,
        chroma_noise_std,
        shadow_noise_std,
        estimated_noise_level,
        is_tack_sharp: effective_focus_variance > 250.0,
        is_soft: effective_focus_variance < 45.0,
        is_noisy: estimated_noise_level > 0.35,
        is_clean: estimated_noise_level < 0.12,
        blur_direction_deg,
        blur_length_px,
        directional_anisotropy,
        motion_blur_confidence,
        is_directional_motion_blur: directional_anisotropy > 0.45 && blur_length_px > 3.0,
    }
}

fn finite_fraction(value: f64) -> f64 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn chroma_noise_standard_deviation(rgb: &[u8]) -> f64 {
    if rgb.is_empty() {
        return 0.0;
    }
    let red_green = rgb
        .chunks_exact(3)
        .map(|pixel| f64::from(pixel[0]) - f64::from(pixel[1]))
        .collect::<Vec<_>>();
    let blue_green = rgb
        .chunks_exact(3)
        .map(|pixel| f64::from(pixel[2]) - f64::from(pixel[1]))
        .collect::<Vec<_>>();
    (standard_deviation(&red_green) + standard_deviation(&blue_green)) / 2.0
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
    variance.max(0.0).sqrt()
}

/// Divide the normalized frame into a 3x3 grid and reuse the engine's edge
/// statistic for each region. Cells smaller than 2x2 abstain with zero.
fn regional_laplacian_variances(luminance: &[f64], width: usize, height: usize) -> [f64; 9] {
    let mut variances = [0.0; 9];
    if width < 6 || height < 6 {
        return variances;
    }
    for row in 0..3 {
        let y0 = row * height / 3;
        let y1 = (row + 1) * height / 3;
        for column in 0..3 {
            let x0 = column * width / 3;
            let x1 = (column + 1) * width / 3;
            let cell_width = x1 - x0;
            let cell_height = y1 - y0;
            let mut cell = Vec::with_capacity(cell_width * cell_height);
            for y in y0..y1 {
                cell.extend_from_slice(&luminance[y * width + x0..y * width + x1]);
            }
            let index = row * 3 + column;
            variances[index] = edge_engine::edge_statistics(&cell, cell_width, cell_height).2;
        }
    }
    variances
}

/// Structure-tensor features match the Python convention: the reported angle
/// is the dominant gradient orientation, wrapped to [0, 180) degrees.
fn motion_features(luminance: &[f64], width: usize, height: usize) -> (f64, f64, f64) {
    if width < 2 || height < 2 {
        return (0.0, 0.0, 0.0);
    }
    let mut gxx_sum = 0.0;
    let mut gyy_sum = 0.0;
    let mut gxy_sum = 0.0;
    let mut count = 0_u64;
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let gx = if x == 0 {
                luminance[index + 1] - luminance[index]
            } else if x + 1 == width {
                luminance[index] - luminance[index - 1]
            } else {
                (luminance[index + 1] - luminance[index - 1]) / 2.0
            };
            let gy = if y == 0 {
                luminance[index + width] - luminance[index]
            } else if y + 1 == height {
                luminance[index] - luminance[index - width]
            } else {
                (luminance[index + width] - luminance[index - width]) / 2.0
            };
            gxx_sum += gx * gx;
            gyy_sum += gy * gy;
            gxy_sum += gx * gy;
            count += 1;
        }
    }
    let divisor = count as f64;
    let gxx = gxx_sum / divisor;
    let gyy = gyy_sum / divisor;
    let gxy = gxy_sum / divisor;
    let trace = gxx + gyy;
    if trace <= f64::EPSILON {
        return (0.0, 0.0, 0.0);
    }
    let determinant = (gxx * gyy - gxy * gxy).max(0.0);
    let half_trace = trace / 2.0;
    let discriminant = (half_trace * half_trace - determinant).max(0.0).sqrt();
    let lambda1 = (half_trace + discriminant).max(0.0);
    let lambda2 = (half_trace - discriminant).max(0.0);
    let anisotropy = finite_fraction((lambda1 - lambda2) / (lambda1 + lambda2 + 1e-8));
    let angle = (0.5 * (2.0 * gxy).atan2(gxx - gyy).to_degrees()).rem_euclid(180.0);
    let gradient_magnitude_mean = trace.sqrt();
    let blur_length = anisotropy * 15.0 / (gradient_magnitude_mean + 1e-3);
    (
        finite_fraction(angle / 180.0) * 180.0,
        blur_length.max(0.0),
        anisotropy,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray_frame(width: usize, height: usize, pixel: impl Fn(usize, usize) -> u8) -> Vec<[u8; 3]> {
        let mut frame = Vec::with_capacity(width.saturating_mul(height));
        for y in 0..height {
            for x in 0..width {
                let value = pixel(x, y);
                frame.push([value, value, value]);
            }
        }
        frame
    }

    fn analyze_frame(
        rgb: &[[u8; 3]],
        width: usize,
        height: usize,
        fft_ratio: f64,
    ) -> DetailAnalysis {
        let packed = rgb.iter().flatten().copied().collect::<Vec<_>>();
        analyze(&packed, width as u32, height as u32, fft_ratio)
    }

    fn box_blur(frame: &[[u8; 3]], width: usize, height: usize) -> Vec<[u8; 3]> {
        let mut blurred = vec![[0; 3]; frame.len()];
        for y in 0..height {
            for x in 0..width {
                let mut sum = 0_u32;
                let mut count = 0_u32;
                for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                    for xx in x.saturating_sub(1)..=(x + 1).min(width - 1) {
                        sum += u32::from(frame[yy * width + xx][0]);
                        count += 1;
                    }
                }
                let value = (sum / count) as u8;
                blurred[y * width + x] = [value; 3];
            }
        }
        blurred
    }

    #[test]
    fn focused_texture_scores_above_its_blurred_version_and_has_region_metrics() {
        let width = 72;
        let height = 72;
        let focused = gray_frame(width, height, |x, y| {
            if (x / 2 + y / 2) % 2 == 0 {
                0
            } else {
                255
            }
        });
        let blurred = box_blur(&focused, width, height);
        let sharp = analyze_frame(&focused, width, height, 0.8);
        let soft = analyze_frame(&blurred, width, height, 0.2);

        assert!(sharp.effective_focus_variance > soft.effective_focus_variance);
        assert!(sharp.global_sharpness >= soft.global_sharpness);
        assert_eq!(sharp.regional_laplacian_variance.len(), 9);
        assert!(sharp.numeric_values_are_finite());
    }

    #[test]
    fn noisy_frame_scores_above_uniform_clean_frame() {
        let width = 48;
        let height = 48;
        let clean = gray_frame(width, height, |_, _| 96);
        let noisy = gray_frame(width, height, |x, y| {
            let seed = (x as u32)
                .wrapping_mul(1_103_515_245)
                .wrapping_add((y as u32).wrapping_mul(12_345))
                .wrapping_add(7_253);
            ((seed ^ (seed >> 9) ^ (seed >> 17)) & 0xff) as u8
        });
        let clean_metrics = analyze_frame(&clean, width, height, 0.0);
        let noisy_metrics = analyze_frame(&noisy, width, height, 0.0);

        assert!(noisy_metrics.luminance_noise_std > clean_metrics.luminance_noise_std);
        assert!(noisy_metrics.estimated_noise_level > clean_metrics.estimated_noise_level);
    }

    #[test]
    fn directional_stripes_are_more_anisotropic_than_crosshatched_defocus() {
        let width = 64;
        let height = 64;
        let directional = gray_frame(width, height, |x, _| if (x / 2) % 2 == 0 { 0 } else { 255 });
        let defocus = gray_frame(width, height, |x, y| {
            if (x / 2 + y / 2) % 2 == 0 {
                0
            } else {
                255
            }
        });
        let directional_metrics = analyze_frame(&directional, width, height, 0.0);
        let defocus_metrics = analyze_frame(&defocus, width, height, 0.0);

        assert!(
            directional_metrics.directional_anisotropy > defocus_metrics.directional_anisotropy
        );
        assert!((0.0..180.0).contains(&directional_metrics.blur_direction_deg));
        assert!(directional_metrics.numeric_values_are_finite());
    }

    #[test]
    fn empty_small_uniform_and_malformed_frames_remain_finite() {
        let empty = analyze(&[], 0, 0, f64::NAN);
        let one_pixel = analyze_frame(&[[42, 42, 42]], 1, 1, 0.4);
        let small = analyze_frame(
            &[[0, 0, 0], [255, 255, 255], [90, 90, 90], [5, 5, 5]],
            2,
            2,
            0.4,
        );
        let uniform = analyze_frame(&gray_frame(16, 16, |_, _| 100), 16, 16, 0.4);
        let malformed = analyze(&[0, 0, 0], 2, 2, 0.4);
        let overflowing_dimensions = analyze(&[], u32::MAX, u32::MAX, 0.4);

        for metrics in [
            &empty,
            &one_pixel,
            &small,
            &uniform,
            &malformed,
            &overflowing_dimensions,
        ] {
            assert!(metrics.numeric_values_are_finite());
        }
        assert!(!empty.valid_input);
        assert!(one_pixel.valid_input);
        assert!(small.valid_input);
        assert!(uniform.valid_input);
        assert!(!malformed.valid_input);
        assert!(!overflowing_dimensions.valid_input);
        assert_eq!(empty.fft_high_freq_ratio, 0.0);
        assert_eq!(uniform.directional_anisotropy, 0.0);
        assert_eq!(uniform.luminance_noise_std, 0.0);
    }

    #[test]
    fn luminance_noise_units_and_existing_score_thresholds_are_preserved() {
        let frame = gray_frame(16, 16, |x, _| ((x * x + x * 17) % 256) as u8);
        let metrics = analyze_frame(&frame, 16, 16, 1.5);

        assert!(metrics.luminance_noise_std > 0.0);
        assert_eq!(metrics.fft_high_freq_ratio, 1.0);
        assert!(metrics.estimated_noise_level <= 1.0);
        assert_eq!(metrics.is_noisy, metrics.estimated_noise_level > 0.35);
        assert_eq!(metrics.is_clean, metrics.estimated_noise_level < 0.12);
    }

    #[test]
    fn keeps_existing_rust_metric_ids_and_adds_motion_ids() {
        let frame = gray_frame(8, 8, |x, y| ((x * 31 + y * 17) % 256) as u8);
        let metrics = analyze_frame(&frame, 8, 8, 0.25);
        let names = metrics
            .metric_values()
            .into_iter()
            .map(|(name, _)| name)
            .collect::<Vec<_>>();

        for expected in [
            "sharpness.global_score",
            "sharpness.fft_high_frequency_ratio",
            "noise.luminance_stddev",
            "noise.estimated_level",
            "motion_blur.blur_direction_deg",
            "motion_blur.directional_anisotropy",
        ] {
            assert!(names.contains(&expected));
        }
    }

    #[test]
    fn exports_a_stable_implementation_version() {
        assert_eq!(IMPLEMENTATION_VERSION, "rust-detail-0.1");
        assert!(!IMPLEMENTATION_VERSION.is_empty());
    }
}
