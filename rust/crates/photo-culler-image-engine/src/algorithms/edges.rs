//! Edge density and laplacian variance statistics.

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

pub fn edge_statistics(luminance: &[f64], width: usize, height: usize) -> (f64, f64, f64, f64) {
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
