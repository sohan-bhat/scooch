use crate::table::Curve;
use crate::{analysis, blur, color, solver, Options, Placement, RgbImage, Scene};
use rayon::prelude::*;

const GROW: f64 = 0.10;
const ROUND: f64 = 0.18;
const FEATHER: f64 = 0.12;
const REACH: f64 = 3.0;
const GATE_FROM: f32 = 0.25;
const GATE_AT: f32 = 0.6;
const CALM: f64 = 0.004;
const PROTECT_FROM: f32 = 0.0;
const PROTECT_AT: f32 = 0.25;
const PROTECT_REACH: usize = 2;
const Q: usize = 4;

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

struct Grid {
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
}

impl Grid {
    fn sample(&self, values: &[f32], x: f32, y: f32) -> f32 {
        let fx = (x - self.x0 as f32).clamp(0.0, (self.width - 1) as f32);
        let fy = (y - self.y0 as f32).clamp(0.0, (self.height - 1) as f32);
        let (ix, iy) = ((fx as usize).min(self.width.saturating_sub(2)), (fy as usize).min(self.height.saturating_sub(2)));
        let (tx, ty) = (fx - ix as f32, fy - iy as f32);
        let at = |x: usize, y: usize| values[y.min(self.height - 1) * self.width + x.min(self.width - 1)];
        let top = at(ix, iy) + (at(ix + 1, iy) - at(ix, iy)) * tx;
        let bottom = at(ix, iy + 1) + (at(ix + 1, iy + 1) - at(ix, iy + 1)) * tx;
        top + (bottom - top) * ty
    }
}

fn rounded_box(px: f64, py: f64, rect: &[f64; 4], radius: f64) -> f64 {
    let (cx, cy) = ((rect[0] + rect[2]) / 2.0, (rect[1] + rect[3]) / 2.0);
    let (ex, ey) = ((rect[2] - rect[0]) / 2.0 - radius, (rect[3] - rect[1]) / 2.0 - radius);
    let (qx, qy) = ((px - cx).abs() - ex, (py - cy).abs() - ey);
    let outside = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt();
    outside + qx.max(qy).min(0.0) - radius
}

fn widen(values: &[f32], width: usize, height: usize, reach: usize) -> Vec<f32> {
    let across: Vec<f32> = (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            (x.saturating_sub(reach)..(x + reach + 1).min(width)).map(|c| values[y * width + c]).fold(0.0, f32::max)
        })
        .collect();
    (0..width * height)
        .map(|i| {
            let (x, y) = (i % width, i / width);
            (y.saturating_sub(reach)..(y + reach + 1).min(height)).map(|r| across[r * width + x]).fold(0.0, f32::max)
        })
        .collect()
}

fn median(values: &mut [f64]) -> f64 {
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

pub fn apply(base: RgbImage, source: &RgbImage, placement: &Placement, scene: &Scene, options: &Options) -> RgbImage {
    let amount = if options.pads.is_finite() { options.pads.clamp(0.0, 1.0) } else { 0.0 };
    let rects = solver::screen_rects(scene);
    let (width, height) = (base.width() as usize, base.height() as usize);
    if amount <= 0.0 || rects.is_empty() || width == 0 || height == 0 {
        return base;
    }
    let (qw, qh) = (width.div_ceil(Q), height.div_ceil(Q));
    let feather = FEATHER * median(&mut rects.iter().map(|r| r[3] - r[1]).collect::<Vec<_>>());
    let grown: Vec<[f64; 4]> = rects
        .iter()
        .map(|r| {
            let (gx, gy) = (GROW * (r[2] - r[0]), GROW * (r[3] - r[1]));
            [r[0] - gx, r[1] - gy, r[2] + gx, r[3] + gy]
        })
        .collect();
    let padded: Vec<[usize; 4]> = grown
        .iter()
        .map(|g| {
            let pad = REACH * feather;
            [
                (g[0] - pad).floor().max(0.0) as usize,
                (g[1] - pad).floor().max(0.0) as usize,
                ((g[2] + pad).ceil().max(0.0) as usize).min(width),
                ((g[3] + pad).ceil().max(0.0) as usize).min(height),
            ]
        })
        .filter(|b| b[2] > b[0] && b[3] > b[1])
        .collect();
    if padded.is_empty() {
        return base;
    }
    let region = Grid {
        x0: padded.iter().map(|b| b[0] / Q).min().unwrap_or(0),
        y0: padded.iter().map(|b| b[1] / Q).min().unwrap_or(0),
        width: 0,
        height: 0,
    };
    let region = Grid {
        width: padded.iter().map(|b| b[2].div_ceil(Q)).max().unwrap_or(0).min(qw) - region.x0,
        height: padded.iter().map(|b| b[3].div_ceil(Q)).max().unwrap_or(0).min(qh) - region.y0,
        ..region
    };
    let n = region.width * region.height;
    let mut inside = vec![false; n];
    for b in &padded {
        for qy in b[1] / Q..b[3].div_ceil(Q) {
            for qx in b[0] / Q..b[2].div_ceil(Q) {
                inside[(qy - region.y0) * region.width + qx - region.x0] = true;
            }
        }
    }
    let mut mask = vec![0.0f32; n];
    for g in &grown {
        let radius = ROUND * (g[2] - g[0]).min(g[3] - g[1]);
        let qx0 = ((g[0] / Q as f64).floor().max(region.x0 as f64) as usize).min(region.x0 + region.width);
        let qx1 = ((g[2] / Q as f64).ceil().max(0.0) as usize + 1).min(region.x0 + region.width);
        let qy0 = ((g[1] / Q as f64).floor().max(region.y0 as f64) as usize).min(region.y0 + region.height);
        let qy1 = ((g[3] / Q as f64).ceil().max(0.0) as usize + 1).min(region.y0 + region.height);
        for qy in qy0..qy1 {
            for qx in qx0..qx1 {
                let d = rounded_box((qx * Q) as f64 + 2.0, (qy * Q) as f64 + 2.0, g, radius);
                let cover = (0.5 - d / Q as f64).clamp(0.0, 1.0) as f32;
                let cell = &mut mask[(qy - region.y0) * region.width + qx - region.x0];
                *cell = cell.max(cover);
            }
        }
    }
    let mask = blur::gaussian(&mask, region.width, region.height, (feather / Q as f64) as f32);
    let found = analysis::analyze(source);
    let (bw, bh) = (found.width as usize, found.height as usize);
    if bw == 0 {
        return base;
    }
    let field = Grid { x0: 0, y0: 0, width: bw, height: bh };
    let ground: Vec<f32> = found.busy.iter().map(|b| smoothstep(GATE_FROM, GATE_AT, *b)).collect();
    let keep: Vec<f32> = found.saliency.iter().map(|s| smoothstep(PROTECT_FROM, PROTECT_AT, *s)).collect();
    let free: Vec<f32> = widen(&keep, bw, bh, PROTECT_REACH).into_iter().map(|k| 1.0 - k).collect();
    let sigma = (CALM * width as f64 / Q as f64) as f32;
    let margin = (REACH * sigma as f64).ceil() as usize + 1;
    let calm_grid = Grid {
        x0: region.x0.saturating_sub(margin),
        y0: region.y0.saturating_sub(margin),
        width: (region.x0 + region.width + margin).min(qw) - region.x0.saturating_sub(margin),
        height: (region.y0 + region.height + margin).min(qh) - region.y0.saturating_sub(margin),
    };
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let gx: Vec<f32> = (calm_grid.x0..calm_grid.x0 + calm_grid.width).map(|qx| (xc.at((qx * Q + 2) as f64 / width as f64) * bw as f64 - 0.5) as f32).collect();
    let gy: Vec<f32> = (calm_grid.y0..calm_grid.y0 + calm_grid.height).map(|qy| (yc.at((qy * Q + 2) as f64 / height as f64) * bh as f64 - 0.5) as f32).collect();
    let open: Vec<f32> = (0..calm_grid.width * calm_grid.height).map(|i| field.sample(&free, gx[i % calm_grid.width], gy[i / calm_grid.width])).collect();
    let (dx, dy) = (region.x0 - calm_grid.x0, region.y0 - calm_grid.y0);
    let gate: Vec<f32> = (0..n)
        .map(|i| {
            let (c, r) = (i % region.width + dx, i / region.width + dy);
            field.sample(&ground, gx[c], gy[r]) * open[r * calm_grid.width + c]
        })
        .collect();
    let under = rects.iter().any(|r| {
        let qx0 = (r[0] / Q as f64).floor() as usize;
        let qy0 = (r[1] / Q as f64).floor() as usize;
        let qx1 = ((r[2] / Q as f64).ceil() as usize).max(qx0 + 1).min(region.x0 + region.width);
        let qy1 = ((r[3] / Q as f64).ceil() as usize).max(qy0 + 1).min(region.y0 + region.height);
        (qy0.max(region.y0)..qy1).any(|qy| (qx0.max(region.x0)..qx1).any(|qx| gate[(qy - region.y0) * region.width + qx - region.x0] > 0.0))
    });
    if !under {
        return base;
    }
    let weight: Vec<f32> = (0..n).map(|i| if inside[i] { mask[i] * gate[i] * amount } else { 0.0 }).collect();
    if !weight.iter().any(|w| *w > 0.0) {
        return base;
    }
    let calm = calm_planes(&base, &calm_grid, &open, sigma);
    blend(base, &padded, &region, &weight, &calm_grid, &calm)
}

fn calm_planes(base: &RgbImage, grid: &Grid, open: &[f32], sigma: f32) -> [Vec<f32>; 3] {
    let decode = color::decode_table();
    let (width, height) = (base.width() as usize, base.height() as usize);
    let raw = base.as_raw();
    let rows: Vec<Vec<[f32; 3]>> = (0..grid.height)
        .into_par_iter()
        .map(|r| {
            let qy = grid.y0 + r;
            let (y0, y1) = (qy * Q, ((qy + 1) * Q).min(height));
            (0..grid.width)
                .map(|c| {
                    let qx = grid.x0 + c;
                    let (x0, x1) = (qx * Q, ((qx + 1) * Q).min(width));
                    let mut acc = [0.0f32; 3];
                    for y in y0..y1 {
                        for x in x0..x1 {
                            let p = &raw[(y * width + x) * 3..(y * width + x) * 3 + 3];
                            acc[0] += decode[p[0] as usize];
                            acc[1] += decode[p[1] as usize];
                            acc[2] += decode[p[2] as usize];
                        }
                    }
                    let scale = open[r * grid.width + c] / ((x1 - x0) * (y1 - y0)).max(1) as f32;
                    acc.map(|v| v * scale)
                })
                .collect()
        })
        .collect();
    let plane = |k: usize| rows.iter().flat_map(|row| row.iter().map(move |p| p[k])).collect::<Vec<f32>>();
    let planes = [plane(0), plane(1), plane(2), open.to_vec()];
    let blurred: Vec<Vec<f32>> = planes.par_iter().map(|p| blur::gaussian(p, grid.width, grid.height, sigma)).collect();
    let share = &blurred[3];
    let normal = |k: usize| blurred[k].iter().zip(share).map(|(v, w)| if *w > 1e-4 { v / w } else { 0.0 }).collect::<Vec<f32>>();
    [normal(0), normal(1), normal(2)]
}

fn blend(mut base: RgbImage, padded: &[[usize; 4]], region: &Grid, weight: &[f32], calm_grid: &Grid, calm: &[Vec<f32>; 3]) -> RgbImage {
    let decode = color::decode_table();
    let encode = color::encode_table();
    let width = base.width() as usize;
    let top = padded.iter().map(|b| b[1]).min().unwrap_or(0);
    let bottom = padded.iter().map(|b| b[3]).max().unwrap_or(0);
    let buffer: &mut [u8] = &mut base;
    buffer.par_chunks_mut(width * 3).enumerate().skip(top).take(bottom - top).for_each(|(y, line)| {
        let mut spans: Vec<(usize, usize)> = padded.iter().filter(|b| b[1] <= y && y < b[3]).map(|b| (b[0], b[2])).collect();
        if spans.is_empty() {
            return;
        }
        spans.sort_unstable();
        let mut merged: Vec<(usize, usize)> = Vec::with_capacity(spans.len());
        for (a, b) in spans {
            match merged.last_mut() {
                Some(last) if a <= last.1 => last.1 = last.1.max(b),
                _ => merged.push((a, b)),
            }
        }
        let qy = (y as f32 + 0.5) / Q as f32 - 0.5;
        for (a, b) in merged {
            for x in a..b {
                let qx = (x as f32 + 0.5) / Q as f32 - 0.5;
                let f = region.sample(weight, qx, qy);
                if f <= 0.0 {
                    continue;
                }
                let px = &mut line[x * 3..x * 3 + 3];
                for k in 0..3 {
                    let soft = calm_grid.sample(&calm[k], qx, qy);
                    let hard = decode[px[k] as usize];
                    px[k] = color::encode(encode, hard + (soft - hard) * f);
                }
            }
        }
    });
    base
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{table, Rect};

    fn stripes(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| if (x / 3 + y / 3) % 2 == 0 { image::Rgb([240, 240, 240]) } else { image::Rgb([20, 20, 20]) })
    }

    #[test]
    fn the_rounded_box_is_negative_inside() {
        let rect = [0.0, 0.0, 100.0, 50.0];
        assert!(rounded_box(50.0, 25.0, &rect, 9.0) < -20.0);
        assert!(rounded_box(0.5, 0.5, &rect, 9.0) > 0.0);
        assert!(rounded_box(150.0, 25.0, &rect, 9.0) > 49.0);
    }

    #[test]
    fn busy_ground_gets_a_pad_only_near_the_icon() {
        let image = stripes(400, 250);
        let scene = Scene { width: 400, height: 250, icons: vec![Rect { x: 300.0, y: 40.0, w: 60.0, h: 70.0 }] };
        let placement = table::fill(400, 250, 400, 250);
        let plain = image.clone();
        let out = apply(plain.clone(), &image, &placement, &scene, &Options::default());
        let changed = |x: u32, y: u32| out.get_pixel(x, y) != plain.get_pixel(x, y);
        assert!((300..360).any(|x| (40..110).any(|y| changed(x, y))));
        assert!((0..150).all(|x| (0..250).all(|y| !changed(x, y))));
        assert!((0..400).all(|x| (200..250).all(|y| !changed(x, y))));
    }

    #[test]
    fn calm_ground_is_left_bit_identical() {
        let image = RgbImage::from_fn(400, 250, |x, _| image::Rgb([(x / 2) as u8, 120, 200]));
        let scene = Scene { width: 400, height: 250, icons: vec![Rect { x: 300.0, y: 40.0, w: 60.0, h: 70.0 }] };
        let placement = table::fill(400, 250, 400, 250);
        let out = apply(image.clone(), &image, &placement, &scene, &Options::default());
        assert_eq!(out.as_raw(), image.as_raw());
    }

    #[test]
    fn zero_strength_pads_do_nothing() {
        let image = stripes(200, 120);
        let scene = Scene { width: 200, height: 120, icons: vec![Rect { x: 20.0, y: 20.0, w: 40.0, h: 40.0 }] };
        let options = Options { pads: 0.0, ..Options::default() };
        let out = apply(image.clone(), &image, &table::fill(200, 120, 200, 120), &scene, &options);
        assert_eq!(out.as_raw(), image.as_raw());
    }
}
