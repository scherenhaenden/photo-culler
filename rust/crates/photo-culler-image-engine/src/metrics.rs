//! Native pixel features calculation structure.

use image::{DynamicImage, GenericImageView};
use serde::Serialize;

use crate::normalize;
use crate::algorithms::noise::noise_standard_deviations;
use crate::algorithms::edges::edge_statistics;
use crate::algorithms::fft::fft_high_frequency_ratio;

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

fn standard_deviation_from_moments(sum: f64, squared_sum: f64, count: f64) -> f64 {
    if count == 0.0 {
        return 0.0;
    }
    (squared_sum / count - (sum / count).powi(2))
        .max(0.0)
        .sqrt()
}
