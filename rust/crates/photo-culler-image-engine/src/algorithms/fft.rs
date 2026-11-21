//! Fast Fourier Transform frequency analysis.

use rustfft::{FftPlanner, num_complex::Complex};

pub fn fft_high_frequency_ratio(luminance: &[f64], width: usize, height: usize) -> f64 {
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
