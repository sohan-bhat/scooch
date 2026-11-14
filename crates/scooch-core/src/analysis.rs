use crate::{blur, color, Analysis, Rect};
use image::{GrayImage, RgbImage};
use rayon::prelude::*;

const LONG_SIDE: usize = 320;
const SHORT_SIDE: usize = 8;
const TONE_SIGMA: f32 = 1.0;
const PAIRS: [(f32, f32); 3] = [(2.0, 8.0), (4.0, 16.0), (8.0, 32.0)];
const SCALES: [f32; 5] = [2.0, 4.0, 8.0, 16.0, 32.0];
const TONE_FLOOR: f32 = 0.08;
const CONTRAST_FLOOR: f32 = 0.04;
const STRUCTURE_FLOOR: f32 = 0.02;
const SMOOTH_SIGMA: f32 = 2.0;
const SALIENCY_FLOOR: f32 = 0.5;
const CALM_LEVEL: f32 = 0.25;
const FILL_REACH: f32 = 0.06;
const FILL_FROM: f32 = 0.30;
const FILL_AT: f32 = 0.65;
const FILL_LEVEL: f32 = 0.8;
const SALIENCY_POWER: f32 = 1.5;
const TERMS: usize = 10;
const BACKGROUND_ROUNDS: usize = 5;
const BACKGROUND_STEP: usize = 3;
const BACKGROUND_SPREAD: f64 = 0.03;
const BUSY_SIGMA: f32 = 3.0;
const BUSY_SCALE: f32 = 0.06;
const SUBJECT_REACH: f64 = 1.5;
const FLAT_SHARE: f64 = 1e-4;
const PALETTE: usize = 5;
const PALETTE_ROUNDS: usize = 8;
const CENTER: Rect = Rect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 };

pub fn grid_size(source_width: u32, source_height: u32) -> (usize, usize) {
    if source_width == 0 || source_height == 0 {
        return (0, 0);
    }
    let scale = LONG_SIDE as f64 / source_width.max(source_height) as f64;
    let side = |v: u32| ((v as f64 * scale).round() as usize).clamp(SHORT_SIDE, LONG_SIDE);
    (side(source_width), side(source_height))
}

struct Share {
    source: usize,
    cell: usize,
    weight: f32,
}

fn shares(source: usize, target: usize) -> Vec<Share> {
    let step = source as f64 / target as f64;
    let mut out = Vec::with_capacity(source + target);
    for cell in 0..target {
        let (lo, hi) = (cell as f64 * step, (cell + 1) as f64 * step);
        let first = (lo.floor() as usize).min(source - 1);
        let last = (hi.ceil() as usize).clamp(first + 1, source);
        for s in first..last {
            let weight = (hi.min((s + 1) as f64) - lo.max(s as f64)).max(0.0);
            if weight > 0.0 {
                out.push(Share { source: s, cell, weight: weight as f32 });
            }
        }
    }
    out
}

fn lab_planes(image: &RgbImage, width: usize, height: usize) -> [Vec<f32>; 3] {
    let (sw, sh) = (image.width() as usize, image.height() as usize);
    let raw = image.as_raw();
    let decode = color::decode_table();
    let columns = shares(sw, width);
    let rows = shares(sh, height);
    let mut by_row: Vec<Vec<(usize, f32)>> = vec![Vec::new(); height];
    for share in &rows {
        by_row[share.cell].push((share.source, share.weight));
    }
    let area = ((sw as f64 / width as f64) * (sh as f64 / height as f64)) as f32;
    let lines: Vec<Vec<[f32; 3]>> = by_row
        .par_iter()
        .map(|sources| {
            let mut sums = vec![[0.0f32; 3]; width];
            let mut line_sums = vec![[0.0f32; 3]; width];
            for &(y, wy) in sources {
                line_sums.iter_mut().for_each(|s| *s = [0.0; 3]);
                let line = &raw[y * sw * 3..(y + 1) * sw * 3];
                for share in &columns {
                    let p = &line[share.source * 3..share.source * 3 + 3];
                    let target = &mut line_sums[share.cell];
                    target[0] += share.weight * decode[p[0] as usize];
                    target[1] += share.weight * decode[p[1] as usize];
                    target[2] += share.weight * decode[p[2] as usize];
                }
                for (sum, part) in sums.iter_mut().zip(&line_sums) {
                    sum[0] += wy * part[0];
                    sum[1] += wy * part[1];
                    sum[2] += wy * part[2];
                }
            }
            sums.iter().map(|s| color::oklab(s[0] / area, s[1] / area, s[2] / area)).collect()
        })
        .collect();
    let plane = |c: usize| lines.iter().flat_map(|line| line.iter().map(move |lab| lab[c])).collect::<Vec<f32>>();
    [plane(0), plane(1), plane(2)]
}

pub fn quantile(values: &[f32], q: f64) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    let index = (((sorted.len() - 1) as f64) * q).round() as usize;
    *sorted.select_nth_unstable_by(index, f32::total_cmp).1
}

fn distance(a: &[Vec<f32>], b: &[Vec<f32>]) -> Vec<f32> {
    (0..a[0].len())
        .map(|i| {
            let (dl, da, db) = (a[0][i] - b[0][i], a[1][i] - b[1][i], a[2][i] - b[2][i]);
            (dl * dl + da * da + db * db).sqrt()
        })
        .collect()
}

fn sobel(values: &[f32], width: usize, height: usize) -> Vec<f32> {
    let at = |x: isize, y: isize| values[(y.clamp(0, height as isize - 1) as usize) * width + x.clamp(0, width as isize - 1) as usize];
    let mut out = vec![0.0f32; width * height];
    for y in 0..height as isize {
        for x in 0..width as isize {
            let gx = (at(x + 1, y - 1) + 2.0 * at(x + 1, y) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2.0 * at(x - 1, y) + at(x - 1, y + 1));
            let gy = (at(x - 1, y + 1) + 2.0 * at(x, y + 1) + at(x + 1, y + 1)) - (at(x - 1, y - 1) + 2.0 * at(x, y - 1) + at(x + 1, y - 1));
            out[y as usize * width + x as usize] = (gx * gx + gy * gy).sqrt() / 8.0;
        }
    }
    out
}

fn busy_from_lightness(lightness: &[f32], width: usize, height: usize) -> Vec<f32> {
    let edges = sobel(lightness, width, height);
    blur::gaussian(&edges, width, height, BUSY_SIGMA).into_iter().map(|v| (v / BUSY_SCALE).clamp(0.0, 1.0)).collect()
}

fn mean(plane: &[f32]) -> f32 {
    (plane.iter().map(|v| *v as f64).sum::<f64>() / plane.len().max(1) as f64) as f32
}

fn blurs(planes: &[Vec<f32>], width: usize, height: usize, sigmas: &[f32]) -> Vec<Vec<f32>> {
    let count = planes.len();
    (0..sigmas.len() * count).into_par_iter().map(|i| blur::gaussian_extended(&planes[i % count], width, height, sigmas[i / count])).collect()
}

fn solve_square(mut m: [[f64; TERMS + 1]; TERMS]) -> Option<[f64; TERMS]> {
    for c in 0..TERMS {
        let pivot = (c..TERMS).max_by(|a, b| m[*a][c].abs().total_cmp(&m[*b][c].abs()))?;
        if m[pivot][c].abs() < 1e-12 {
            return None;
        }
        m.swap(c, pivot);
        let lead = m[c];
        for (r, row) in m.iter_mut().enumerate() {
            if r != c {
                let f = row[c] / lead[c];
                for (cell, pivot_cell) in row[c..].iter_mut().zip(&lead[c..]) {
                    *cell -= f * pivot_cell;
                }
            }
        }
    }
    Some(std::array::from_fn(|r| m[r][TERMS] / m[r][r]))
}

fn terms(u: f64, v: f64) -> [f64; TERMS] {
    [1.0, u, v, u * u, u * v, v * v, u * u * u, u * u * v, u * v * v, v * v * v]
}

fn background(lab: &[Vec<f32>; 3], width: usize, height: usize) -> [Vec<f32>; 3] {
    let n = width * height;
    let us: Vec<f64> = (0..width).map(|x| 2.0 * (x as f64 + 0.5) / width as f64 - 1.0).collect();
    let vs: Vec<f64> = (0..height).map(|y| 2.0 * (y as f64 + 0.5) / height as f64 - 1.0).collect();
    let samples: Vec<([f64; TERMS], [f64; 3])> = (0..height)
        .step_by(BACKGROUND_STEP)
        .flat_map(|y| (0..width).step_by(BACKGROUND_STEP).map(move |x| (x, y)))
        .map(|(x, y)| (terms(us[x], vs[y]), std::array::from_fn(|c| lab[c][y * width + x] as f64)))
        .collect();
    let mut weights = vec![1.0f64; samples.len()];
    let mut fit: Option<[[f64; TERMS]; 3]> = None;
    for _ in 0..=BACKGROUND_ROUNDS {
        let mut normal = [[0.0f64; TERMS + 1]; TERMS];
        let mut sides = [[0.0f64; TERMS]; 3];
        for ((t, value), w) in samples.iter().zip(&weights) {
            for r in 0..TERMS {
                let wr = w * t[r];
                for c in r..TERMS {
                    normal[r][c] += wr * t[c];
                }
                for k in 0..3 {
                    sides[k][r] += wr * value[k];
                }
            }
        }
        let upper = normal;
        for (r, row) in normal.iter_mut().enumerate() {
            for (c, cell) in row.iter_mut().enumerate().take(r) {
                *cell = upper[c][r];
            }
        }
        let mut next = [[0.0f64; TERMS]; 3];
        let mut solved = true;
        for k in 0..3 {
            let mut m = normal;
            for r in 0..TERMS {
                m[r][TERMS] = sides[k][r];
            }
            match solve_square(m) {
                Some(coefficients) => next[k] = coefficients,
                None => solved = false,
            }
        }
        if !solved {
            break;
        }
        for ((t, value), w) in samples.iter().zip(weights.iter_mut()) {
            let r2: f64 = (0..3).map(|k| (value[k] - t.iter().zip(&next[k]).map(|(a, b)| a * b).sum::<f64>()).powi(2)).sum();
            *w = 1.0 / (1.0 + r2 / (BACKGROUND_SPREAD * BACKGROUND_SPREAD));
        }
        fit = Some(next);
    }
    let Some(k) = fit else { return std::array::from_fn(|c| vec![mean(&lab[c]); n]) };
    std::array::from_fn(|c| {
        let k = &k[c];
        let mut out = Vec::with_capacity(n);
        for &v in &vs {
            let a = k[0] + k[2] * v + k[5] * v * v + k[9] * v * v * v;
            let b = k[1] + k[4] * v + k[8] * v * v;
            let q = k[3] + k[7] * v;
            out.extend(us.iter().map(|&u| (a + u * (b + u * (q + u * k[6]))) as f32));
        }
        out
    })
}

fn contrast(planes: &[Vec<f32>], width: usize, height: usize) -> Vec<f32> {
    let blurred = blurs(planes, width, height, &SCALES);
    let level = |sigma: f32| {
        let index = SCALES.iter().position(|s| *s == sigma).unwrap_or(0);
        &blurred[index * 3..index * 3 + 3]
    };
    let mut out = vec![0.0f32; width * height];
    for (a, b) in PAIRS {
        for (c, d) in out.iter_mut().zip(distance(level(a), level(b))) {
            *c += d / PAIRS.len() as f32;
        }
    }
    out
}

fn saliency(lab: &[Vec<f32>; 3], contrast: &[f32], width: usize, height: usize) -> Vec<f32> {
    let (fine, ground) = rayon::join(|| blurs(lab, width, height, &[TONE_SIGMA]), || background(lab, width, height));
    let tone = distance(&fine, &ground);
    let tone_top = quantile(&tone, 0.99).max(TONE_FLOOR);
    let contrast_top = quantile(contrast, 0.99).max(CONTRAST_FLOOR);
    let raw: Vec<f32> = tone.iter().zip(contrast).map(|(t, c)| 0.5 * t / tone_top + 0.5 * c / contrast_top).collect();
    let smooth = blur::gaussian(&raw, width, height, SMOOTH_SIGMA);
    let top = quantile(&smooth, 0.99).max(SALIENCY_FLOOR);
    let scaled: Vec<f32> = smooth.iter().map(|v| (v / top).clamp(0.0, 1.0)).collect();
    let floor = quantile(&scaled, 0.5).max(CALM_LEVEL / top);
    let span = (1.0 - floor).max(1e-3);
    scaled.iter().map(|v| crate::math::powf((v - floor).max(0.0) / span, SALIENCY_POWER)).collect()
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn fill(saliency: &[f32], width: usize, height: usize) -> Vec<f32> {
    let reach = FILL_REACH * width.max(height) as f32;
    let envelope = blur::gaussian(saliency, width, height, reach);
    let top = quantile(&envelope, 0.99).max(1e-3);
    saliency.iter().zip(&envelope).map(|(s, e)| s.max(FILL_LEVEL * smoothstep(FILL_FROM, FILL_AT, (e / top).clamp(0.0, 1.0)))).collect()
}

fn subject(weights: &[f32], width: usize, height: usize) -> Option<Rect> {
    let (mut total, mut mx, mut my) = (0.0f64, 0.0f64, 0.0f64);
    for y in 0..height {
        for x in 0..width {
            let w = (weights[y * width + x] as f64).powi(2);
            total += w;
            mx += w * (x as f64 + 0.5) / width as f64;
            my += w * (y as f64 + 0.5) / height as f64;
        }
    }
    if total < FLAT_SHARE * (width * height) as f64 {
        return None;
    }
    let (cx, cy) = (mx / total, my / total);
    let (mut vx, mut vy) = (0.0f64, 0.0f64);
    for y in 0..height {
        for x in 0..width {
            let w = (weights[y * width + x] as f64).powi(2);
            vx += w * ((x as f64 + 0.5) / width as f64 - cx).powi(2);
            vy += w * ((y as f64 + 0.5) / height as f64 - cy).powi(2);
        }
    }
    let (sx, sy) = ((vx / total).sqrt() * SUBJECT_REACH, (vy / total).sqrt() * SUBJECT_REACH);
    let (x0, x1) = ((cx - sx).clamp(0.0, 1.0), (cx + sx).clamp(0.0, 1.0));
    let (y0, y1) = ((cy - sy).clamp(0.0, 1.0), (cy + sy).clamp(0.0, 1.0));
    Some(Rect { x: x0 as f32, y: y0 as f32, w: (x1 - x0) as f32, h: (y1 - y0) as f32 })
}

fn palette(lab: &[Vec<f32>; 3]) -> Vec<[f32; 4]> {
    let n = lab[0].len();
    if n == 0 {
        return Vec::new();
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| lab[0][*a].total_cmp(&lab[0][*b]).then(a.cmp(b)));
    let mut centers: Vec<[f32; 3]> = (0..PALETTE)
        .map(|k| {
            let i = order[((2 * k + 1) * n / (2 * PALETTE)).min(n - 1)];
            [lab[0][i], lab[1][i], lab[2][i]]
        })
        .collect();
    let mut counts = vec![0usize; PALETTE];
    for _ in 0..PALETTE_ROUNDS {
        let mut sums = [[0.0f64; 3]; PALETTE];
        counts.iter_mut().for_each(|c| *c = 0);
        for p in (0..n).map(|i| [lab[0][i], lab[1][i], lab[2][i]]) {
            let nearest = (0..PALETTE)
                .min_by(|a, b| {
                    let da: f32 = (0..3).map(|k| (p[k] - centers[*a][k]).powi(2)).sum();
                    let db: f32 = (0..3).map(|k| (p[k] - centers[*b][k]).powi(2)).sum();
                    da.total_cmp(&db)
                })
                .unwrap_or(0);
            counts[nearest] += 1;
            for k in 0..3 {
                sums[nearest][k] += p[k] as f64;
            }
        }
        for (c, (sum, count)) in centers.iter_mut().zip(sums.iter().zip(&counts)) {
            if *count > 0 {
                *c = sum.map(|v| (v / *count as f64) as f32);
            }
        }
    }
    let mut out: Vec<[f32; 4]> = centers.iter().zip(&counts).map(|(c, count)| [c[0], c[1], c[2], *count as f32 / n as f32]).collect();
    out.sort_by(|a, b| a[0].total_cmp(&b[0]));
    out
}

pub fn analyze(image: &RgbImage) -> Analysis {
    let (sw, sh) = image.dimensions();
    let (width, height) = grid_size(sw, sh);
    if width == 0 {
        return Analysis { width: 0, height: 0, saliency: Vec::new(), busy: Vec::new(), subject: CENTER, flat: true, source_width: sw, source_height: sh, palette: Vec::new() };
    }
    let lab = lab_planes(image, width, height);
    let (contrast, busy) = rayon::join(|| contrast(&lab, width, height), || busy_from_lightness(&lab[0], width, height));
    let shaped = contrast.iter().any(|c| *c >= STRUCTURE_FLOOR);
    let raw = saliency(&lab, &contrast, width, height);
    let solid = shaped && subject(&raw, width, height).is_some();
    let saliency = if solid { fill(&raw, width, height) } else { vec![0.0; width * height] };
    let found = subject(&saliency, width, height).filter(|_| solid);
    Analysis {
        width: width as u32,
        height: height as u32,
        saliency,
        busy,
        subject: found.unwrap_or(CENTER),
        flat: found.is_none(),
        source_width: sw,
        source_height: sh,
        palette: palette(&lab),
    }
}

pub fn gray(width: u32, height: u32, values: &[f32]) -> GrayImage {
    if values.len() != (width as usize) * (height as usize) {
        return GrayImage::new(width, height);
    }
    GrayImage::from_fn(width, height, |x, y| image::Luma([(values[(y * width + x) as usize].clamp(0.0, 1.0) * 255.0 + 0.5) as u8]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_grid_keeps_the_long_side_at_320() {
        assert_eq!(grid_size(2560, 1707), (320, 213));
        assert_eq!(grid_size(1891, 2240), (270, 320));
        assert_eq!(grid_size(8, 8000), (8, 320));
        assert_eq!(grid_size(1, 1), (320, 320));
        assert_eq!(grid_size(0, 5), (0, 0));
    }

    #[test]
    fn shares_cover_every_cell_evenly() {
        for (source, target) in [(2560usize, 320usize), (7, 320), (1, 320), (333, 320)] {
            let mut sums = vec![0.0f64; target];
            for share in shares(source, target) {
                sums[share.cell] += share.weight as f64;
            }
            let step = source as f64 / target as f64;
            assert!(sums.iter().all(|s| (s - step).abs() < 1e-4), "{source} {target}");
        }
    }

    #[test]
    fn a_bright_spot_on_grey_is_the_subject() {
        let mut image = RgbImage::from_pixel(640, 400, image::Rgb([90, 90, 90]));
        for y in 60..140 {
            for x in 440..540 {
                image.put_pixel(x, y, image::Rgb([240, 200, 40]));
            }
        }
        let analysis = analyze(&image);
        assert!(!analysis.flat);
        let s = analysis.subject;
        let (cx, cy) = (s.x + s.w / 2.0, s.y + s.h / 2.0);
        assert!((cx - 490.0 / 640.0).abs() < 0.05 && (cy - 100.0 / 400.0).abs() < 0.06, "{s:?}");
        let corner = analysis.saliency[(analysis.height as usize - 5) * analysis.width as usize + 5];
        assert_eq!(corner, 0.0);
    }

    #[test]
    fn a_tilted_plane_has_no_structure() {
        let (w, h) = (60, 40);
        let ramp: Vec<f32> = (0..w * h).map(|i| 0.2 + 0.01 * (i % w) as f32 - 0.004 * (i / w) as f32).collect();
        let lab = [ramp.clone(), vec![0.01; w * h], ramp.iter().map(|v| v * 0.1).collect()];
        assert!(contrast(&lab, w, h).iter().all(|c| *c < 1e-3));
        let mut spot = lab.clone();
        for y in 15..25 {
            for x in 25..35 {
                spot[0][y * w + x] += 0.3;
            }
        }
        assert!(contrast(&spot, w, h).iter().any(|c| *c >= STRUCTURE_FLOOR));
    }

    #[test]
    fn a_smooth_background_is_fitted_and_a_spot_stands_out() {
        let (w, h) = (80, 50);
        let curve = |i: usize| {
            let (u, v) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            0.4 + 0.2 * v * v - 0.1 * u * v + 0.05 * u
        };
        let mut lab = [(0..w * h).map(curve).collect::<Vec<f32>>(), vec![0.0; w * h], vec![0.02; w * h]];
        for y in 10..14 {
            for x in 60..64 {
                lab[0][y * w + x] += 0.5;
            }
        }
        let ground = background(&lab, w, h);
        assert!((ground[0][40 * w + 10] - curve(40 * w + 10)).abs() < 0.01);
        assert!((ground[0][12 * w + 62] - curve(12 * w + 62)).abs() < 0.05);
    }

    #[test]
    fn a_plain_image_is_flat() {
        let analysis = analyze(&RgbImage::from_pixel(300, 200, image::Rgb([30, 120, 200])));
        assert!(analysis.flat);
        assert!(analysis.saliency.iter().all(|v| *v == 0.0));
        assert!(analysis.busy.iter().all(|v| *v < 0.01));
    }
}
