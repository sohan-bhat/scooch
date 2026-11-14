use rayon::prelude::*;

const EXACT_LIMIT: f32 = 3.0;

pub fn gaussian(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if width == 0 || height == 0 || values.len() != width * height || sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        return values.to_vec();
    }
    if sigma <= EXACT_LIMIT {
        exact(values, width, height, sigma)
    } else {
        boxed(values, width, height, sigma)
    }
}

fn reflection(i: isize, n: usize) -> Vec<(usize, f32)> {
    let last = n as isize - 1;
    if last <= 0 {
        return vec![(0, 1.0)];
    }
    if i < 0 {
        let mut out: Vec<(usize, f32)> = reflection(-i, n).into_iter().map(|(k, c)| (k, -c)).collect();
        out.push((0, 2.0));
        out
    } else if i > last {
        let mut out: Vec<(usize, f32)> = reflection(2 * last - i, n).into_iter().map(|(k, c)| (k, -c)).collect();
        out.push((last as usize, 2.0));
        out
    } else {
        vec![(i as usize, 1.0)]
    }
}

pub fn gaussian_extended(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if width == 0 || height == 0 || values.len() != width * height || sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        return values.to_vec();
    }
    let pad = (3.0 * sigma).ceil() as usize + 1;
    let (wide, tall) = (width + 2 * pad, height + 2 * pad);
    let across: Vec<Vec<(usize, f32)>> = (0..wide).map(|x| reflection(x as isize - pad as isize, width)).collect();
    let mut padded = vec![0.0f32; wide * tall];
    for (line, target) in values.chunks_exact(width).zip(padded[pad * wide..].chunks_exact_mut(wide)) {
        target[pad..pad + width].copy_from_slice(line);
        for x in (0..pad).chain(pad + width..wide) {
            target[x] = across[x].iter().map(|(k, c)| c * line[*k]).sum();
        }
    }
    for y in (0..pad).chain(pad + height..tall) {
        let mix = reflection(y as isize - pad as isize, height);
        let mut row = vec![0.0f32; wide];
        for (k, c) in mix {
            for (r, v) in row.iter_mut().zip(&padded[(k + pad) * wide..(k + pad + 1) * wide]) {
                *r += c * v;
            }
        }
        padded[y * wide..(y + 1) * wide].copy_from_slice(&row);
    }
    let blurred = gaussian(&padded, wide, tall, sigma);
    let mut out = Vec::with_capacity(width * height);
    for y in 0..height {
        out.extend_from_slice(&blurred[(y + pad) * wide + pad..(y + pad) * wide + pad + width]);
    }
    out
}

pub fn gaussian_boxes(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if width == 0 || height == 0 || values.len() != width * height || sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        return values.to_vec();
    }
    boxed(values, width, height, sigma)
}

pub fn gaussian_parallel(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if width == 0 || height == 0 || values.len() != width * height || sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        return values.to_vec();
    }
    if sigma <= EXACT_LIMIT {
        exact_parallel(values, width, height, sigma)
    } else {
        boxed_parallel(values, width, height, sigma)
    }
}

#[cfg(test)]
pub fn boxes_parallel(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    if width == 0 || height == 0 || values.len() != width * height || sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        return values.to_vec();
    }
    boxed_parallel(values, width, height, sigma)
}

fn exact_parallel(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let weights = kernel(sigma);
    let radius = weights.len() / 2;
    let mut across = vec![0.0f32; values.len()];
    across.par_chunks_mut(width).zip(values.par_chunks(width)).for_each(|(out, row)| {
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
    });
    let mut out = vec![0.0f32; values.len()];
    out.par_chunks_mut(width).enumerate().for_each(|(y, target)| {
        for (i, w) in weights.iter().enumerate() {
            let source = (y + i).saturating_sub(radius).min(height - 1);
            for (t, v) in target.iter_mut().zip(&across[source * width..(source + 1) * width]) {
                *t += w * v;
            }
        }
    });
    out
}

fn turn(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    let mut out = vec![0.0f32; values.len()];
    out.par_chunks_mut(height).enumerate().for_each(|(x, column)| {
        for (y, v) in column.iter_mut().enumerate() {
            *v = values[y * width + x];
        }
    });
    out
}

fn boxed_parallel(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let mut a = values.to_vec();
    let mut b = vec![0.0f32; a.len()];
    for radius in radii(sigma) {
        if radius == 0 {
            continue;
        }
        b.par_chunks_mut(width).zip(a.par_chunks(width)).for_each(|(out, row)| pass_rows(row, out, width, radius));
        let turned = turn(&b, width, height);
        let mut back = vec![0.0f32; turned.len()];
        back.par_chunks_mut(height).zip(turned.par_chunks(height)).for_each(|(out, column)| pass_rows(column, out, height, radius));
        a = turn(&back, height, width);
    }
    a
}

pub fn boxes_parallel4(values: &[[f32; 4]], width: usize, height: usize, sigma: f32) -> Vec<[f32; 4]> {
    if width == 0 || height == 0 || values.len() != width * height || sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        return values.to_vec();
    }
    let mut a = values.to_vec();
    let mut b = vec![[0.0f32; 4]; a.len()];
    for radius in radii(sigma) {
        if radius == 0 {
            continue;
        }
        b.par_chunks_mut(width).zip(a.par_chunks(width)).for_each(|(out, row)| rows4(row, out, radius));
        let turned = turn4(&b, width, height);
        let mut back = vec![[0.0f32; 4]; turned.len()];
        back.par_chunks_mut(height).zip(turned.par_chunks(height)).for_each(|(out, column)| rows4(column, out, radius));
        a = turn4(&back, height, width);
    }
    a
}

fn rows4(row: &[[f32; 4]], out: &mut [[f32; 4]], radius: usize) {
    let scale = 1.0 / (2 * radius + 1) as f32;
    let last = row.len() - 1;
    let mut sum = row[0].map(|v| v * (radius + 1) as f32);
    for k in 1..=radius {
        for c in 0..4 {
            sum[c] += row[k.min(last)][c];
        }
    }
    for (x, o) in out.iter_mut().enumerate() {
        *o = sum.map(|v| v * scale);
        let (enter, leave) = (row[(x + radius + 1).min(last)], row[x.saturating_sub(radius)]);
        for c in 0..4 {
            sum[c] += enter[c] - leave[c];
        }
    }
}

fn turn4(values: &[[f32; 4]], width: usize, height: usize) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0f32; 4]; values.len()];
    out.par_chunks_mut(height).enumerate().for_each(|(x, column)| {
        for (y, v) in column.iter_mut().enumerate() {
            *v = values[y * width + x];
        }
    });
    out
}

fn kernel(sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil().max(1.0) as usize;
    let mut weights: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let d = i as f32 - radius as f32;
            crate::math::exp(-d * d / (2.0 * sigma * sigma))
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

const ONE: f64 = (1u64 << 24) as f64;

pub type Area = [usize; 4];

pub fn reach(sigma: f32) -> usize {
    if sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        0
    } else if sigma <= EXACT_LIMIT {
        kernel(sigma).len() / 2
    } else {
        radii(sigma).iter().sum()
    }
}

pub fn box_reach(sigma: f32) -> usize {
    if sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        0
    } else {
        radii(sigma).iter().sum()
    }
}

pub fn boxes_fixed_into(values: &[f32], out: &mut [f32], width: usize, height: usize, sigma: f32, area: Area) {
    let [x0, y0, x1, y1] = [area[0], area[1], area[2].min(width), area[3].min(height)];
    if x0 >= x1 || y0 >= y1 || values.len() != width * height || out.len() != width * height {
        return;
    }
    if sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        for y in y0..y1 {
            out[y * width + x0..y * width + x1].copy_from_slice(&values[y * width + x0..y * width + x1]);
        }
    } else {
        boxes_into(values, out, width, height, sigma, [x0, y0, x1, y1]);
    }
}

#[cfg(test)]
pub fn fixed(values: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let mut out = vec![0.0f32; values.len()];
    fixed_into(values, &mut out, width, height, sigma, [0, 0, width, height]);
    out
}

pub fn fixed_into(values: &[f32], out: &mut [f32], width: usize, height: usize, sigma: f32, area: Area) {
    let [x0, y0, x1, y1] = [area[0], area[1], area[2].min(width), area[3].min(height)];
    if x0 >= x1 || y0 >= y1 || values.len() != width * height || out.len() != width * height {
        return;
    }
    if sigma.is_nan() || sigma <= 0.0 || sigma.is_infinite() {
        for y in y0..y1 {
            out[y * width + x0..y * width + x1].copy_from_slice(&values[y * width + x0..y * width + x1]);
        }
    } else if sigma <= EXACT_LIMIT {
        taps_into(values, out, width, height, sigma, [x0, y0, x1, y1]);
    } else {
        boxes_into(values, out, width, height, sigma, [x0, y0, x1, y1]);
    }
}

fn taps_into(values: &[f32], out: &mut [f32], width: usize, height: usize, sigma: f32, [x0, y0, x1, y1]: Area) {
    let weights = kernel(sigma);
    let radius = weights.len() / 2;
    let (r0, r1) = (y0.saturating_sub(radius), (y1 + radius).min(height));
    let span = x1 - x0;
    let mut across = vec![0.0f32; span * (r1 - r0)];
    across.par_chunks_mut(span).enumerate().for_each(|(k, line)| {
        let row = &values[(r0 + k) * width..(r0 + k + 1) * width];
        for (i, target) in line.iter_mut().enumerate() {
            let x = x0 + i;
            let mut acc = 0.0f32;
            for (t, w) in weights.iter().enumerate() {
                acc += w * row[(x + t).saturating_sub(radius).min(width - 1)];
            }
            *target = acc;
        }
    });
    out.par_chunks_mut(width).enumerate().skip(y0).take(y1 - y0).for_each(|(y, line)| {
        for (i, target) in line[x0..x1].iter_mut().enumerate() {
            let mut acc = 0.0f32;
            for (t, w) in weights.iter().enumerate() {
                let source = (y + t).saturating_sub(radius).min(height - 1);
                acc += w * across[(source - r0) * span + i];
            }
            *target = acc;
        }
    });
}

fn share(sum: i64, count: i64) -> i64 {
    (2 * sum + count).div_euclid(2 * count)
}

fn boxes_into(values: &[f32], out: &mut [f32], width: usize, height: usize, sigma: f32, [x0, y0, x1, y1]: Area) {
    let passes: Vec<usize> = radii(sigma).into_iter().filter(|r| *r > 0).collect();
    let mut areas = vec![[x0, y0, x1, y1]];
    for radius in passes.iter().rev() {
        let [a, b, c, d] = areas[areas.len() - 1];
        let tall = [a, b.saturating_sub(*radius), c, (d + radius).min(height)];
        areas.push(tall);
        areas.push([a.saturating_sub(*radius), tall[1], (c + radius).min(width), tall[3]]);
    }
    let outer = areas[areas.len() - 1];
    let (ow, oh) = (outer[2] - outer[0], outer[3] - outer[1]);
    let at = |x: usize, y: usize| (y - outer[1]) * ow + (x - outer[0]);
    let mut a = vec![0i64; ow * oh];
    a.par_chunks_mut(ow).enumerate().for_each(|(k, line)| {
        let row = &values[(outer[1] + k) * width + outer[0]..(outer[1] + k) * width + outer[2]];
        for (q, v) in line.iter_mut().zip(row) {
            *q = (*v as f64 * ONE).round() as i64;
        }
    });
    let mut b = vec![0i64; ow * oh];
    for (step, radius) in passes.iter().enumerate() {
        let across = areas[areas.len() - 2 - 2 * step];
        let down = areas[areas.len() - 3 - 2 * step];
        let (count, r) = ((2 * radius + 1) as i64, *radius);
        let source = &a;
        b.par_chunks_mut(ow).enumerate().skip(across[1] - outer[1]).take(across[3] - across[1]).for_each(|(k, line)| {
            let y = outer[1] + k;
            let pick = |x: isize| source[at((x.max(0) as usize).min(width - 1), y)];
            let first = across[0] as isize;
            let mut sum: i64 = (-(r as isize)..=r as isize).map(|t| pick(first + t)).sum();
            for x in across[0]..across[2] {
                line[x - outer[0]] = share(sum, count);
                if x + 1 < across[2] {
                    sum += pick(x as isize + r as isize + 1) - pick(x as isize - r as isize);
                }
            }
        });
        let source = &b;
        let columns: Vec<usize> = (down[0]..down[2]).collect();
        let strips: Vec<Vec<i64>> = columns
            .par_chunks(32)
            .map(|chunk| {
                let pick = |x: usize, y: isize| source[at(x, (y.max(0) as usize).min(height - 1))];
                let first = down[1] as isize;
                let mut sums: Vec<i64> = chunk.iter().map(|&x| (-(r as isize)..=r as isize).map(|t| pick(x, first + t)).sum()).collect();
                let mut strip = vec![0i64; chunk.len() * (down[3] - down[1])];
                for y in down[1]..down[3] {
                    for (c, &x) in chunk.iter().enumerate() {
                        strip[(y - down[1]) * chunk.len() + c] = share(sums[c], count);
                        if y + 1 < down[3] {
                            sums[c] += pick(x, y as isize + r as isize + 1) - pick(x, y as isize - r as isize);
                        }
                    }
                }
                strip
            })
            .collect();
        for (chunk, strip) in columns.chunks(32).zip(&strips) {
            for y in down[1]..down[3] {
                for (c, &x) in chunk.iter().enumerate() {
                    a[at(x, y)] = strip[(y - down[1]) * chunk.len() + c];
                }
            }
        }
    }
    out.par_chunks_mut(width).enumerate().skip(y0).take(y1 - y0).for_each(|(y, line)| {
        for x in x0..x1 {
            line[x] = (a[at(x, y)] as f64 / ONE) as f32;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_parallel_blurs_match_the_serial_ones_bit_for_bit() {
        let (w, h) = (97, 61);
        let values: Vec<f32> = (0..w * h).map(|i| ((i * 7919) % 1013) as f32 / 1013.0).collect();
        for sigma in [0.6f32, 1.5, 2.9, 3.4, 7.0, 16.5] {
            assert_eq!(gaussian(&values, w, h, sigma), gaussian_parallel(&values, w, h, sigma), "{sigma}");
            assert_eq!(gaussian_boxes(&values, w, h, sigma), boxes_parallel(&values, w, h, sigma), "{sigma}");
            let stacked: Vec<[f32; 4]> = values.iter().map(|v| [*v, v * 0.5, 1.0 - v, *v]).collect();
            let four = boxes_parallel4(&stacked, w, h, sigma);
            for c in 0..4 {
                let plane: Vec<f32> = stacked.iter().map(|v| v[c]).collect();
                assert_eq!(gaussian_boxes(&plane, w, h, sigma), four.iter().map(|v| v[c]).collect::<Vec<f32>>(), "{sigma} {c}");
            }
        }
    }

    #[test]
    fn a_window_of_the_fixed_blur_matches_the_whole() {
        let (w, h) = (83, 57);
        let values: Vec<f32> = (0..w * h).map(|i| ((i * 7919) % 1013) as f32 / 101.3 - 3.0).collect();
        for sigma in [0.0f32, 0.7, 2.9, 3.4, 7.0, 12.5] {
            let whole = fixed(&values, w, h, sigma);
            if sigma > 0.0 && sigma <= EXACT_LIMIT {
                assert_eq!(whole, gaussian(&values, w, h, sigma), "{sigma}");
            } else if sigma > EXACT_LIMIT {
                let float = gaussian(&values, w, h, sigma);
                assert!(whole.iter().zip(&float).all(|(a, b)| (a - b).abs() < 1e-4), "{sigma}");
            }
            for area in [[0, 0, w, h], [5, 7, 30, 21], [0, 40, 12, h], [70, 0, w, 3], [41, 28, 42, 29]] {
                let mut out = vec![9.0f32; w * h];
                fixed_into(&values, &mut out, w, h, sigma, area);
                for y in 0..h {
                    for x in 0..w {
                        let inside = x >= area[0] && x < area[2] && y >= area[1] && y < area[3];
                        assert_eq!(out[y * w + x], if inside { whole[y * w + x] } else { 9.0 }, "{sigma} {area:?} {x} {y}");
                    }
                }
            }
            let mut moved = values.clone();
            moved[20 * w + 30] += 5.0;
            let after = fixed(&moved, w, h, sigma);
            let r = reach(sigma);
            for y in 0..h {
                for x in 0..w {
                    if x.abs_diff(30) > r || y.abs_diff(20) > r {
                        assert_eq!(after[y * w + x], whole[y * w + x], "{sigma} {x} {y}");
                    }
                }
            }
        }
    }

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
    fn extended_edges_keep_a_tilted_plane() {
        let (w, h) = (50, 30);
        let plane: Vec<f32> = (0..w * h).map(|i| 0.3 + 0.01 * (i % w) as f32 - 0.02 * (i / w) as f32).collect();
        for sigma in [2.0, 8.0, 32.0] {
            let out = gaussian_extended(&plane, w, h, sigma);
            assert!(out.iter().zip(&plane).all(|(a, b)| (a - b).abs() < 1e-3), "{sigma}");
        }
        assert!((gaussian_extended(&[0.25], 1, 1, 8.0)[0] - 0.25).abs() < 1e-6);
    }

    #[test]
    fn single_pixel_maps_are_safe() {
        assert!((gaussian(&[0.25], 1, 1, 12.0)[0] - 0.25).abs() < 1e-6);
        assert!((gaussian(&[0.25], 1, 1, 1.0)[0] - 0.25).abs() < 1e-6);
        assert_eq!(gaussian(&[0.25, 0.75], 2, 1, 12.0).len(), 2);
    }
}
