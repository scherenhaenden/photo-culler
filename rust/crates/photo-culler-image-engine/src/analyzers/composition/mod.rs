//! Conservative composition measurements. These functions return suggestions only;
//! they never rotate or crop the source image.

use image::DynamicImage;

pub const ANALYZER_VERSION: &str = "composition-0.1.0";
const MIN_CONFIDENCE: f64 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    /// Horizontal coordinate in [0, 1], from the left edge.
    pub x: f64,
    /// Vertical coordinate in [0, 1], from the top edge.
    pub y: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HorizonEstimate {
    pub analyzer_version: &'static str,
    pub available: bool,
    pub angle_degrees: Option<f64>,
    pub suggested_rotation_degrees: Option<f64>,
    pub confidence: f64,
    /// Endpoints of the estimated line in normalized image coordinates.
    pub endpoints: Option<[Point; 2]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VisualBalance {
    pub analyzer_version: &'static str,
    pub available: bool,
    pub visual_weight_center: Option<Point>,
    pub offset_from_center: Option<f64>,
    pub confidence: f64,
    /// A small review crop centered on visual weight, normalized to the image.
    pub suggested_crop: Option<NormalizedCrop>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NormalizedCrop {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Estimate a near-horizontal edge and abstain when the evidence is weak or curved.
pub fn estimate_horizon(image: &DynamicImage) -> HorizonEstimate {
    let gray = grayscale(image);
    let (width, height) = (gray.0, gray.1);
    let pixels = gray.2;
    if width < 8 || height < 8 {
        return unavailable_horizon();
    }
    let mut edges = Vec::new();
    let mut magnitudes = Vec::new();
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let gx = pixels[(y * width + x + 1) as usize] as f64
                - pixels[(y * width + x - 1) as usize] as f64;
            let gy = pixels[((y + 1) * width + x) as usize] as f64
                - pixels[((y - 1) * width + x) as usize] as f64;
            let magnitude = gx.hypot(gy);
            if magnitude > 0.0 && gy.abs() >= gx.abs() * 1.8 {
                edges.push((x as f64, y as f64, magnitude));
                magnitudes.push(magnitude);
            }
        }
    }
    if edges.len() < (width as usize / 12).max(12) {
        return unavailable_horizon();
    }
    magnitudes.sort_by(f64::total_cmp);
    let threshold = magnitudes[(magnitudes.len() * 55 / 100).min(magnitudes.len() - 1)];
    edges.retain(|e| e.2 >= threshold);
    if edges.len() < (width as usize / 12).max(12) {
        return unavailable_horizon();
    }
    let total_weight: f64 = edges.iter().map(|e| e.2).sum();
    let mean_x = edges.iter().map(|e| e.0 * e.2).sum::<f64>() / total_weight;
    let mean_y = edges.iter().map(|e| e.1 * e.2).sum::<f64>() / total_weight;
    let cov_xx = edges
        .iter()
        .map(|e| e.2 * (e.0 - mean_x).powi(2))
        .sum::<f64>();
    let cov_xy = edges
        .iter()
        .map(|e| e.2 * (e.0 - mean_x) * (e.1 - mean_y))
        .sum::<f64>();
    let slope = cov_xy / cov_xx.max(1e-9);
    let intercept = mean_y - slope * mean_x;
    let residual = edges
        .iter()
        .map(|e| e.2 * (e.1 - slope * e.0 - intercept).powi(2))
        .sum::<f64>()
        / total_weight;
    let spread = edges
        .iter()
        .map(|e| e.2 * (e.1 - mean_y).powi(2))
        .sum::<f64>()
        / total_weight;
    let fit = (1.0 - residual / (spread + 1e-9)).clamp(0.0, 1.0);
    // A single straight line should explain the edge across the full frame.
    // The robust bin check prevents a curved fisheye horizon from looking like
    // a confident straight line when only its locally strongest segments win.
    let bins = 8usize;
    let mut bin_sums = vec![(0.0f64, 0.0f64); bins];
    for &(x, y, weight) in &edges {
        let index = ((x / width as f64) * bins as f64).floor() as usize;
        let index = index.min(bins - 1);
        bin_sums[index].0 += y * weight;
        bin_sums[index].1 += weight;
    }
    let mut curve_error = 0.0;
    let mut curve_bins = 0usize;
    for (index, (sum_y, sum_weight)) in bin_sums.iter().enumerate() {
        if *sum_weight > 0.0 {
            let x_mid = (index as f64 + 0.5) * width as f64 / bins as f64;
            let y_mean = sum_y / sum_weight;
            curve_error += (y_mean - (slope * x_mid + intercept)).powi(2);
            curve_bins += 1;
        }
    }
    let curved =
        curve_bins >= 4 && (curve_error / curve_bins as f64).sqrt() / height as f64 > 0.025;
    let coverage = (edges.len() as f64 / (width as f64 * 0.35)).min(1.0);
    // Curved horizons (common with fisheye lenses) have a poor weighted linear fit.
    let confidence = if curved {
        0.0
    } else {
        (fit * coverage).clamp(0.0, 1.0)
    };
    let correction = (-slope.atan().to_degrees()).clamp(-15.0, 15.0);
    if confidence < MIN_CONFIDENCE || correction.abs() < 0.25 {
        return HorizonEstimate {
            analyzer_version: ANALYZER_VERSION,
            available: false,
            angle_degrees: None,
            suggested_rotation_degrees: None,
            confidence,
            endpoints: None,
        };
    }
    let y0 = intercept;
    let y1 = slope * (width - 1) as f64 + intercept;
    if !(0.0..=height as f64).contains(&y0) || !(0.0..=height as f64).contains(&y1) {
        return unavailable_horizon();
    }
    HorizonEstimate {
        analyzer_version: ANALYZER_VERSION,
        available: true,
        angle_degrees: Some(slope.atan().to_degrees()),
        suggested_rotation_degrees: Some(correction),
        confidence,
        endpoints: Some([
            Point {
                x: 0.0,
                y: (y0 / (height - 1) as f64).clamp(0.0, 1.0),
            },
            Point {
                x: 1.0,
                y: (y1 / (height - 1) as f64).clamp(0.0, 1.0),
            },
        ]),
    }
}

/// Estimate the centroid of strong image detail, without semantic subject detection.
pub fn estimate_visual_balance(image: &DynamicImage) -> VisualBalance {
    let (width, height, pixels) = grayscale(image);
    if width < 3 || height < 3 {
        return unavailable_balance();
    }
    let mut energy = Vec::with_capacity(((width - 2) * (height - 2)) as usize);
    for y in 1..height - 1 {
        for x in 1..width - 1 {
            let gx = pixels[(y * width + x + 1) as usize] as f64
                - pixels[(y * width + x - 1) as usize] as f64;
            let gy = pixels[((y + 1) * width + x) as usize] as f64
                - pixels[((y - 1) * width + x) as usize] as f64;
            energy.push((x, y, gx.hypot(gy)));
        }
    }
    let mut sorted: Vec<f64> = energy.iter().map(|e| e.2).collect();
    sorted.sort_by(f64::total_cmp);
    let cutoff = sorted[sorted.len() * 85 / 100];
    let weighted: Vec<_> = energy
        .into_iter()
        .filter(|e| e.2 >= cutoff && e.2 > 1e-9)
        .collect();
    let total: f64 = weighted.iter().map(|e| e.2).sum();
    if total <= 1e-9 {
        return unavailable_balance();
    }
    let cx = weighted.iter().map(|e| e.0 as f64 * e.2).sum::<f64>() / total / (width - 1) as f64;
    let cy = weighted.iter().map(|e| e.1 as f64 * e.2).sum::<f64>() / total / (height - 1) as f64;
    let offset = (cx - 0.5).hypot(cy - 0.5);
    let confidence = (offset / 0.35).clamp(0.0, 1.0);
    let crop_width = 0.85;
    let crop_height = 0.85;
    let crop = NormalizedCrop {
        x: (cx - crop_width / 2.0).clamp(0.0, 1.0 - crop_width),
        y: (cy - crop_height / 2.0).clamp(0.0, 1.0 - crop_height),
        width: crop_width,
        height: crop_height,
    };
    VisualBalance {
        analyzer_version: ANALYZER_VERSION,
        available: true,
        visual_weight_center: Some(Point { x: cx, y: cy }),
        offset_from_center: Some(offset),
        confidence,
        suggested_crop: Some(crop),
    }
}

fn grayscale(image: &DynamicImage) -> (u32, u32, Vec<u8>) {
    let gray = image.to_luma8();
    let (width, height) = gray.dimensions();
    (width, height, gray.into_raw())
}
fn unavailable_horizon() -> HorizonEstimate {
    HorizonEstimate {
        analyzer_version: ANALYZER_VERSION,
        available: false,
        angle_degrees: None,
        suggested_rotation_degrees: None,
        confidence: 0.0,
        endpoints: None,
    }
}
fn unavailable_balance() -> VisualBalance {
    VisualBalance {
        analyzer_version: ANALYZER_VERSION,
        available: false,
        visual_weight_center: None,
        offset_from_center: None,
        confidence: 0.0,
        suggested_crop: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{GrayImage, Luma};

    fn tilted_edge(width: u32, height: u32, curve: bool) -> DynamicImage {
        let mut img = GrayImage::from_pixel(width, height, Luma([20]));
        for x in 0..width {
            let nx = x as f64 / (width - 1) as f64;
            let y = if curve {
                height as f64 * (0.5 + 0.18 * (nx * std::f64::consts::TAU).sin())
            } else {
                height as f64 * (0.35 + 0.22 * nx)
            } as u32;
            for yy in y.saturating_sub(1)..=(y + 1).min(height - 1) {
                img.put_pixel(x, yy, Luma([240]));
            }
        }
        DynamicImage::ImageLuma8(img)
    }

    #[test]
    fn detects_tilt_direction_and_normalized_endpoints() {
        let estimate = estimate_horizon(&tilted_edge(320, 200, false));
        assert!(estimate.available, "{estimate:?}");
        assert!(estimate.suggested_rotation_degrees.unwrap() < 0.0);
        let [a, b] = estimate.endpoints.unwrap();
        assert_eq!(a.x, 0.0);
        assert_eq!(b.x, 1.0);
        assert!((0.0..=1.0).contains(&a.y));
        assert!((0.0..=1.0).contains(&b.y));
    }

    #[test]
    fn abstains_without_horizon() {
        let img = DynamicImage::ImageLuma8(GrayImage::from_pixel(120, 80, Luma([127])));
        let estimate = estimate_horizon(&img);
        assert!(!estimate.available);
        assert_eq!(estimate.suggested_rotation_degrees, None);
    }

    #[test]
    fn abstains_on_fisheye_curvature() {
        let estimate = estimate_horizon(&tilted_edge(360, 240, true));
        assert!(!estimate.available, "{estimate:?}");
    }

    #[test]
    fn balance_coordinates_and_crop_are_normalized() {
        let mut img = GrayImage::from_pixel(100, 80, Luma([100]));
        for y in 8..24 {
            for x in 70..90 {
                img.put_pixel(x, y, Luma(if (x + y) % 2 == 0 { [0] } else { [255] }));
            }
        }
        let balance = estimate_visual_balance(&DynamicImage::ImageLuma8(img));
        assert!(balance.available);
        let center = balance.visual_weight_center.unwrap();
        assert!((0.0..=1.0).contains(&center.x) && (0.0..=1.0).contains(&center.y));
        let crop = balance.suggested_crop.unwrap();
        assert!(
            crop.x >= 0.0
                && crop.y >= 0.0
                && crop.x + crop.width <= 1.0
                && crop.y + crop.height <= 1.0
        );
    }
}
