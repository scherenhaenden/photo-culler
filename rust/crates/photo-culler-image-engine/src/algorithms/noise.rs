//! Noise measurement algorithms.

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

pub fn noise_standard_deviations(luminance: &[f64], width: usize, height: usize) -> (f64, f64) {
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
