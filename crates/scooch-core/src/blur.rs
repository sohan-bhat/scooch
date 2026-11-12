const EXACT_LIMIT: f32 = 4.0;

pub fn gaussian(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if width == 0 || height == 0 || values.len() != width * height || !(sigma > 0.0) || !sigma.is_finite() {
        return values.to_vec();
    }
    if sigma <= EXACT_LIMIT {
        exact(values, width, height, sigma)
    } else {
        boxed(values, width, height, sigma)
    }
}

fn kernel(sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil().max(1.0) as usize;
    let mut weights: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let d = i as f32 - radius as f32;
            (-d * d / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f32 = weights.iter().sum();
    for w in weights.iter_mut() {
        *w /= sum;
    }
    weights
}

fn exact(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let weights = kernel(sigma);
    let radius = weights.len() / 2;
    let mut across = vec![0.0f32; values.len()];
    for (row, out) in values.chunks_exact(width).zip(across.chunks_exact_mut(width)) {
        for (x, target) in out.iter_mut().enumerate() {
            let mut acc = 0.0f32;
            if x >= radius && x + radius < width {
                for (w, v) in weights.iter().zip(&row[x - radius..=x + radius]) {
                    acc += w * v;
                }
            } else {
                for (i, w) in weights.iter().enumerate() {
                    acc += w * row[(x + i).saturating_sub(radius).min(width - 1)];
                }
            }
            *target = acc;
        }
    }
    let mut out = vec![0.0f32; values.len()];
    for (y, target) in out.chunks_exact_mut(width).enumerate() {
        for (i, w) in weights.iter().enumerate() {
            let source = (y + i).saturating_sub(radius).min(height - 1);
            for (t, v) in target.iter_mut().zip(&across[source * width..(source + 1) * width]) {
                *t += w * v;
            }
        }
    }
    out
}

fn radii(sigma: f32) -> [usize; 3] {
    let ideal = (12.0 * sigma * sigma / 3.0 + 1.0).sqrt();
    let mut lower = ideal.floor() as i64;
    if lower % 2 == 0 {
        lower -= 1;
    }
    let lower = lower.max(1);
    let upper = lower + 2;
    let split = (12.0 * sigma * sigma - (3 * lower * lower + 12 * lower + 9) as f32) / (-4 * lower - 4) as f32;
    let count = split.round().clamp(0.0, 3.0) as usize;
    let mut out = [0usize; 3];
    for (i, radius) in out.iter_mut().enumerate() {
        *radius = ((if i < count { lower } else { upper }) as usize - 1) / 2;
    }
    out
}

fn pass_rows(source: &[f32], target: &mut [f32], width: usize, radius: usize) {
    let scale = 1.0 / (2 * radius + 1) as f32;
    let last = width - 1;
    for (row, out) in source.chunks_exact(width).zip(target.chunks_exact_mut(width)) {
        let mut sum = row[0] * (radius + 1) as f32;
        for k in 1..=radius {
            sum += row[k.min(last)];
        }
        for (x, o) in out.iter_mut().enumerate() {
            *o = sum * scale;
            sum += row[(x + radius + 1).min(last)] - row[x.saturating_sub(radius)];
        }
    }
}

fn pass_columns(source: &[f32], target: &mut [f32], width: usize, height: usize, radius: usize) {
    let scale = 1.0 / (2 * radius + 1) as f32;
    let last = height - 1;
    let row = |y: usize| &source[y * width..(y + 1) * width];
    let mut sum: Vec<f32> = row(0).iter().map(|v| v * (radius + 1) as f32).collect();
    for k in 1..=radius {
        for (s, v) in sum.iter_mut().zip(row(k.min(last))) {
            *s += v;
        }
    }
    for y in 0..height {
        let enter = row((y + radius + 1).min(last));
        let leave = row(y.saturating_sub(radius));
        let out = &mut target[y * width..(y + 1) * width];
        for x in 0..width {
            out[x] = sum[x] * scale;
            sum[x] += enter[x] - leave[x];
        }
    }
}

fn boxed(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let mut a = values.to_vec();
    let mut b = vec![0.0f32; a.len()];
    for radius in radii(sigma) {
        if radius == 0 {
            continue;
        }
        pass_rows(&a, &mut b, width, radius);
        pass_columns(&b, &mut a, width, height, radius);
    }
    a
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_input_stays_flat() {
        for sigma in [1.0, 3.0, 6.0, 32.0] {
            let out = gaussian(&vec![0.5; 40 * 7], 40, 7, sigma);
            assert!(out.iter().all(|v| (v - 0.5).abs() < 1e-4));
        }
    }

    #[test]
    fn blur_keeps_the_mass_of_an_impulse_away_from_edges() {
        let (w, h) = (81, 81);
        let mut values = vec![0.0f32; w * h];
        values[40 * w + 40] = 1.0;
        for sigma in [1.0, 3.0, 8.0] {
            let out = gaussian(&values, w, h, sigma);
            let sum: f32 = out.iter().sum();
            assert!((sum - 1.0).abs() < 1e-3, "{sum}");
            assert!(out[40 * w + 40] > out[40 * w + 44]);
        }
    }

    #[test]
    fn spread_matches_sigma() {
        let n = 401;
        for sigma in [1.0f32, 2.0, 4.0, 8.0, 16.0] {
            let mut values = vec![0.0f32; n];
            values[n / 2] = 1.0;
            let out = gaussian(&values, n, 1, sigma);
            let variance: f32 = out.iter().enumerate().map(|(i, v)| v * (i as f32 - (n / 2) as f32).powi(2)).sum();
            assert!((variance.sqrt() - sigma).abs() < 0.15 * sigma, "{sigma} {}", variance.sqrt());
        }
    }

    #[test]
    fn single_pixel_maps_are_safe() {
        assert!((gaussian(&[0.25], 1, 1, 12.0)[0] - 0.25).abs() < 1e-6);
        assert!((gaussian(&[0.25], 1, 1, 1.0)[0] - 0.25).abs() < 1e-6);
        assert_eq!(gaussian(&[0.25, 0.75], 2, 1, 12.0).len(), 2);
    }
}
