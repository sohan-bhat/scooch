use crate::table::{self, Curve};
use crate::shape::Grid;
use crate::{blur, color, scenery, solver, Analysis, Options, Placement, RgbImage, Scene};
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

pub fn amount(options: &Options) -> f32 {
    if options.pads.is_finite() {
        options.pads.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

pub fn wanted(scene: &Scene, options: &Options) -> bool {
    amount(options) > 0.0 && scene.width > 0 && scene.height > 0 && !solver::screen_rects(scene).is_empty()
}

#[derive(Clone, Copy)]
struct Area {
    x0: usize,
    y0: usize,
    width: usize,
    height: usize,
}

#[derive(Clone, Copy)]
struct Lerp {
    a: usize,
    b: usize,
    t: f32,
}

fn lerp_at(curve: &Curve, q: usize, out: usize, cells: usize) -> Lerp {
    let p = ((curve.at((q * Q + Q / 2) as f64 / out as f64) * cells as f64 - 0.5) as f32).clamp(0.0, (cells - 1) as f32);
    let a = p as usize;
    Lerp { a, b: (a + 1).min(cells - 1), t: p - a as f32 }
}

fn lerps(curve: &Curve, first: usize, count: usize, out: usize, cells: usize) -> Vec<Lerp> {
    (first..first + count).map(|q| lerp_at(curve, q, out, cells)).collect()
}

struct Gate {
    cells: usize,
    rows: usize,
    ground: Vec<f32>,
    free: Vec<f32>,
    xc: Curve,
    yc: Curve,
    width: usize,
    height: usize,
}

impl Gate {
    fn new(found: &Analysis, placement: &Placement, width: usize, height: usize) -> Option<Self> {
        let (bw, bh) = (found.width as usize, found.height as usize);
        if bw == 0 || bh == 0 || found.busy.len() != bw * bh || found.saliency.len() != bw * bh || width == 0 || height == 0 {
            return None;
        }
        let (ground, free) = gates(found)?;
        Some(Self { cells: bw, rows: bh, ground, free, xc: Curve::new(&placement.fx), yc: Curve::new(&placement.fy), width, height })
    }

    fn pick(&self, values: &[f32], c: &Lerp, r: &Lerp) -> f32 {
        let w = self.cells;
        let top = values[r.a * w + c.a] + (values[r.a * w + c.b] - values[r.a * w + c.a]) * c.t;
        let bottom = values[r.b * w + c.a] + (values[r.b * w + c.b] - values[r.b * w + c.a]) * c.t;
        top + (bottom - top) * r.t
    }

    fn columns(&self, first: usize, count: usize) -> Vec<Lerp> {
        lerps(&self.xc, first, count, self.width, self.cells)
    }

    fn lines(&self, first: usize, count: usize) -> Vec<Lerp> {
        lerps(&self.yc, first, count, self.height, self.rows)
    }

    fn under(&self, rects: &[[f64; 4]]) -> bool {
        let (qw, qh) = (self.width.div_ceil(Q), self.height.div_ceil(Q));
        rects.iter().any(|r| {
            let (qx0, qx1) = quarter_span(r[0], r[2], qw);
            let (qy0, qy1) = quarter_span(r[1], r[3], qh);
            let (qx1, qy1) = (qx1.max(qx0 + 1).min(qw), qy1.max(qy0 + 1).min(qh));
            (qy0..qy1).any(|qy| {
                let row = lerp_at(&self.yc, qy, self.height, self.rows);
                (qx0..qx1).any(|qx| {
                    let column = lerp_at(&self.xc, qx, self.width, self.cells);
                    self.pick(&self.ground, &column, &row) * self.pick(&self.free, &column, &row) > 0.0
                })
            })
        })
    }
}

pub fn active(found: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> bool {
    if scenery::active(scene, options) {
        return true;
    }
    let rects = solver::screen_rects(scene);
    if !wanted(scene, options) || rects.is_empty() {
        return false;
    }
    let fallback;
    let placement = if table::usable(&placement.fx) && table::usable(&placement.fy) {
        placement
    } else {
        fallback = table::fill(found.source_width, found.source_height, scene.width, scene.height);
        &fallback
    };
    Gate::new(found, placement, scene.width as usize, scene.height as usize).is_some_and(|gate| gate.under(&rects))
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

fn quarter_span(lo: f64, hi: f64, limit: usize) -> (usize, usize) {
    let a = ((lo / Q as f64).floor().max(0.0) as usize).min(limit);
    let b = ((hi / Q as f64).ceil().max(0.0) as usize).min(limit);
    (a, b.max(a))
}

pub fn apply(base: RgbImage, found: Option<&Analysis>, placement: &Placement, scene: &Scene, options: &Options) -> RgbImage {
    let amount = amount(options);
    let rects = solver::screen_rects(scene);
    let (width, height) = (base.width() as usize, base.height() as usize);
    let Some(found) = found else { return base };
    if amount <= 0.0 || rects.is_empty() || width == 0 || height == 0 {
        return base;
    }
    let (qw, qh) = (width.div_ceil(Q), height.div_ceil(Q));
    let feather = FEATHER * median(&mut rects.iter().map(|r| r[3] - r[1]).collect::<Vec<_>>());
    let pad = REACH * feather;
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
    let x0 = padded.iter().map(|b| b[0] / Q).min().unwrap_or(0);
    let y0 = padded.iter().map(|b| b[1] / Q).min().unwrap_or(0);
    let region = Area { x0, y0, width: padded.iter().map(|b| b[2].div_ceil(Q)).max().unwrap_or(0).min(qw) - x0, height: padded.iter().map(|b| b[3].div_ceil(Q)).max().unwrap_or(0).min(qh) - y0 };
    let sigma = (CALM * width as f64 / Q as f64) as f32;
    let margin = (REACH * sigma as f64).ceil() as usize + 1;
    let cx0 = region.x0.saturating_sub(margin);
    let cy0 = region.y0.saturating_sub(margin);
    let calm_area = Area { x0: cx0, y0: cy0, width: (region.x0 + region.width + margin).min(qw) - cx0, height: (region.y0 + region.height + margin).min(qh) - cy0 };
    let (dx, dy) = (region.x0 - calm_area.x0, region.y0 - calm_area.y0);

    let Some(gates) = Gate::new(found, placement, width, height) else { return base };
    if !gates.under(&rects) {
        return base;
    }
    let columns = gates.columns(calm_area.x0, calm_area.width);
    let rows = gates.lines(calm_area.y0, calm_area.height);
    let mut open = vec![0.0f32; calm_area.width * calm_area.height];
    open.par_chunks_mut(calm_area.width).zip(&rows).for_each(|(line, r)| {
        for (cell, c) in line.iter_mut().zip(&columns) {
            *cell = gates.pick(&gates.free, c, r);
        }
    });
    let mut gate = vec![0.0f32; region.width * region.height];
    gate.par_chunks_mut(region.width).enumerate().for_each(|(j, line)| {
        let r = &rows[j + dy];
        for (i, cell) in line.iter_mut().enumerate() {
            *cell = gates.pick(&gates.ground, &columns[i + dx], r) * open[(j + dy) * calm_area.width + i + dx];
        }
    });

    let mut mask = vec![0.0f32; region.width * region.height];
    mask.par_chunks_mut(region.width).enumerate().for_each(|(j, line)| {
        let qy = region.y0 + j;
        let py = (qy * Q) as f64 + Q as f64 / 2.0;
        for g in &grown {
            if py < g[1] - Q as f64 || py > g[3] + Q as f64 {
                continue;
            }
            let radius = ROUND * (g[2] - g[0]).min(g[3] - g[1]);
            let (qx0, qx1) = quarter_span(g[0] - Q as f64, g[2] + Q as f64, region.x0 + region.width);
            for qx in qx0.max(region.x0)..qx1 {
                let d = rounded_box((qx * Q) as f64 + Q as f64 / 2.0, py, g, radius);
                let cover = (0.5 - d / Q as f64).clamp(0.0, 1.0) as f32;
                let cell = &mut line[qx - region.x0];
                *cell = cell.max(cover);
            }
        }
    });
    let mask = blur::gaussian_boxes(&mask, region.width, region.height, (feather / Q as f64) as f32);
    let mut weight = vec![0.0f32; region.width * region.height];
    weight.par_chunks_mut(region.width).enumerate().for_each(|(j, line)| {
        let qy = region.y0 + j;
        for b in padded.iter().filter(|b| b[1] / Q <= qy && qy < b[3].div_ceil(Q)) {
            for qx in b[0] / Q..b[2].div_ceil(Q).min(region.x0 + region.width) {
                let i = qx - region.x0;
                line[i] = mask[j * region.width + i] * gate[j * region.width + i] * amount;
            }
        }
    });
    if !weight.iter().any(|w| *w > 0.0) {
        return base;
    }
    let calm = calm_planes(&base, &calm_area, &open, sigma);
    blend(base, &padded, &region, &weight, &calm_area, &calm)
}

fn calm_planes(base: &RgbImage, area: &Area, open: &[f32], sigma: f32) -> [Vec<f32>; 3] {
    let decode = color::decode_table();
    let (width, height) = (base.width() as usize, base.height() as usize);
    let raw = base.as_raw();
    let mut planes = [vec![0.0f32; area.width * area.height], vec![0.0f32; area.width * area.height], vec![0.0f32; area.width * area.height]];
    let [p0, p1, p2] = &mut planes;
    p0.par_chunks_mut(area.width).zip(p1.par_chunks_mut(area.width)).zip(p2.par_chunks_mut(area.width)).enumerate().for_each(|(r, ((l0, l1), l2))| {
        let qy = area.y0 + r;
        let (y0, y1) = (qy * Q, ((qy + 1) * Q).min(height));
        for c in 0..area.width {
            let qx = area.x0 + c;
            let (x0, x1) = (qx * Q, ((qx + 1) * Q).min(width));
            let mut acc = [0.0f32; 3];
            for y in y0..y1 {
                for p in raw[(y * width + x0) * 3..(y * width + x1) * 3].as_chunks::<3>().0 {
                    acc[0] += decode[p[0] as usize];
                    acc[1] += decode[p[1] as usize];
                    acc[2] += decode[p[2] as usize];
                }
            }
            let scale = open[r * area.width + c] / ((x1 - x0) * (y1 - y0)).max(1) as f32;
            l0[c] = acc[0] * scale;
            l1[c] = acc[1] * scale;
            l2[c] = acc[2] * scale;
        }
    });
    let [p0, p1, p2] = planes;
    let sources = [p0, p1, p2, open.to_vec()];
    let blurred: Vec<Vec<f32>> = sources.par_iter().map(|p| blur::gaussian_boxes(p, area.width, area.height, sigma)).collect();
    let share = &blurred[3];
    let normal = |k: usize| blurred[k].iter().zip(share).map(|(v, w)| if *w > 1e-4 { v / w } else { 0.0 }).collect::<Vec<f32>>();
    [normal(0), normal(1), normal(2)]
}

fn row_at(values: &[f32], area: &Area, y: f32) -> Vec<f32> {
    let fy = (y - area.y0 as f32).clamp(0.0, (area.height - 1) as f32);
    let (a, b) = (fy as usize, (fy as usize + 1).min(area.height - 1));
    let t = fy - a as f32;
    let (top, bottom) = (&values[a * area.width..(a + 1) * area.width], &values[b * area.width..(b + 1) * area.width]);
    top.iter().zip(bottom).map(|(p, q)| p + (q - p) * t).collect()
}

fn along(row: &[f32], area: &Area, x: f32) -> f32 {
    let fx = (x - area.x0 as f32).clamp(0.0, (area.width - 1) as f32);
    let (a, b) = (fx as usize, (fx as usize + 1).min(area.width - 1));
    row[a] + (row[b] - row[a]) * (fx - a as f32)
}

fn blend(mut base: RgbImage, padded: &[[usize; 4]], region: &Area, weight: &[f32], calm_area: &Area, calm: &[Vec<f32>; 3]) -> RgbImage {
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
        let strength = row_at(weight, region, qy);
        if !strength.iter().any(|w| *w > 0.0) {
            return;
        }
        let soft = [row_at(&calm[0], calm_area, qy), row_at(&calm[1], calm_area, qy), row_at(&calm[2], calm_area, qy)];
        for (a, b) in merged {
            for x in a..b {
                let qx = (x as f32 + 0.5) / Q as f32 - 0.5;
                let f = along(&strength, region, qx);
                if f <= 0.0 {
                    continue;
                }
                let px = &mut line[x * 3..x * 3 + 3];
                for k in 0..3 {
                    let hard = decode[px[k] as usize];
                    px[k] = color::encode(encode, hard + (along(&soft[k], calm_area, qx) - hard) * f);
                }
            }
        }
    });
    base
}

pub struct Lookup {
    pub ax: Vec<f32>,
    pub ay: Vec<f32>,
    pub aw: usize,
    pub ah: usize,
}

impl Lookup {
    pub fn sample(&self, values: &[f32], i: usize, j: usize) -> f32 {
        let gx = self.ax[i].clamp(0.0, (self.aw - 1) as f32);
        let gy = self.ay[j].clamp(0.0, (self.ah - 1) as f32);
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(self.aw - 1), (y0 + 1).min(self.ah - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let top = values[y0 * self.aw + x0] + (values[y0 * self.aw + x1] - values[y0 * self.aw + x0]) * tx;
        let bottom = values[y1 * self.aw + x0] + (values[y1 * self.aw + x1] - values[y1 * self.aw + x0]) * tx;
        top + (bottom - top) * ty
    }

    pub fn plane(&self, values: &[f32], grid: &Grid) -> Vec<f32> {
        (0..grid.w * grid.h).into_par_iter().map(|c| self.sample(values, c % grid.w, c / grid.w)).collect()
    }
}

pub fn gates(found: &Analysis) -> Option<(Vec<f32>, Vec<f32>)> {
    let (bw, bh) = (found.width as usize, found.height as usize);
    if bw == 0 || bh == 0 || found.busy.len() != bw * bh || found.saliency.len() != bw * bh {
        return None;
    }
    let ground = found.busy.iter().map(|b| smoothstep(GATE_FROM, GATE_AT, *b)).collect();
    let keep: Vec<f32> = found.saliency.iter().map(|s| smoothstep(PROTECT_FROM, PROTECT_AT, *s)).collect();
    let free = widen(&keep, bw, bh, PROTECT_REACH).into_iter().map(|k| 1.0 - k).collect();
    Some((ground, free))
}

pub fn maps(small: &[[f32; 3]], grid: &Grid, found: &Analysis, lookup: &Lookup, scene: &Scene, options: &Options, belong: Option<&[f32]>) -> Option<Vec<u8>> {
    let amount = amount(options);
    let rects = solver::screen_rects(scene);
    let (width, height) = (scene.width as usize, scene.height as usize);
    if amount <= 0.0 || rects.is_empty() || width == 0 || height == 0 {
        return None;
    }
    let (ground, free) = gates(found)?;
    let feather = FEATHER * median(&mut rects.iter().map(|r| r[3] - r[1]).collect::<Vec<_>>());
    let pad = REACH * feather;
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
            [
                ((g[0] - pad).floor().max(0.0) as usize) / Q,
                ((g[1] - pad).floor().max(0.0) as usize) / Q,
                (((g[2] + pad).ceil().max(0.0) as usize).min(width)).div_ceil(Q),
                (((g[3] + pad).ceil().max(0.0) as usize).min(height)).div_ceil(Q),
            ]
        })
        .filter(|b| b[2] > b[0] && b[3] > b[1])
        .collect();
    let open = lookup.plane(&free, grid);
    let mut mask = vec![0.0f32; grid.w * grid.h];
    mask.par_chunks_mut(grid.w).enumerate().for_each(|(qy, line)| {
        let py = (qy * Q) as f64 + Q as f64 / 2.0;
        for g in &grown {
            if py < g[1] - Q as f64 || py > g[3] + Q as f64 {
                continue;
            }
            let radius = ROUND * (g[2] - g[0]).min(g[3] - g[1]);
            let (qx0, qx1) = quarter_span(g[0] - Q as f64, g[2] + Q as f64, grid.w);
            for (qx, cell) in line.iter_mut().enumerate().take(qx1).skip(qx0) {
                let d = rounded_box((qx * Q) as f64 + Q as f64 / 2.0, py, g, radius);
                *cell = cell.max((0.5 - d / Q as f64).clamp(0.0, 1.0) as f32);
            }
        }
    });
    let mask = blur::boxes_parallel(&mask, grid.w, grid.h, (feather / Q as f64) as f32);
    let mut weight = vec![0.0f32; grid.w * grid.h];
    for b in &padded {
        for qy in b[1]..b[3].min(grid.h) {
            for qx in b[0]..b[2].min(grid.w) {
                let c = qy * grid.w + qx;
                if weight[c] == 0.0 && mask[c] > 0.0 {
                    weight[c] = mask[c] * lookup.sample(&ground, qx, qy) * open[c] * amount * belong.map_or(1.0, |b| 1.0 - b[c]);
                }
            }
        }
    }
    if !weight.iter().any(|w| *w > 0.0) {
        return None;
    }
    let sigma = (CALM * width as f64 / Q as f64) as f32;
    let stacked: Vec<[f32; 4]> = small.iter().zip(&open).map(|(c, o)| [c[0] * o, c[1] * o, c[2] * o, *o]).collect();
    let planes = blur::boxes_parallel4(&stacked, grid.w, grid.h, sigma);
    let encode = color::encode_table();
    let mut out = vec![0u8; grid.w * grid.h * 4];
    out.par_chunks_mut(4).enumerate().for_each(|(c, px)| {
        let share = planes[c][3];
        let calm = |k: usize| if share > 1e-4 { planes[c][k] / share } else { small[c][k] };
        px.copy_from_slice(&[color::encode(encode, calm(0)), color::encode(encode, calm(1)), color::encode(encode, calm(2)), (weight[c].clamp(0.0, 1.0) * 255.0).round() as u8]);
    });
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analysis, table, Rect};

    fn stripes(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| if (x / 6 + y / 6) % 2 == 0 { image::Rgb([240, 240, 240]) } else { image::Rgb([20, 20, 20]) })
    }

    fn run(image: &RgbImage, scene: &Scene, options: &Options) -> RgbImage {
        let found = analysis::analyze(image);
        apply(image.clone(), Some(&found), &table::fill(image.width(), image.height(), scene.width, scene.height), scene, options)
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
        let out = run(&image, &scene, &Options::default());
        let changed = |x: u32, y: u32| out.get_pixel(x, y) != image.get_pixel(x, y);
        assert!((300..360).any(|x| (40..110).any(|y| changed(x, y))));
        assert!((0..150).all(|x| (0..250).all(|y| !changed(x, y))));
        assert!((0..400).all(|x| (200..250).all(|y| !changed(x, y))));
    }

    #[test]
    fn calm_ground_is_left_bit_identical() {
        let image = RgbImage::from_fn(400, 250, |x, _| image::Rgb([(x / 2) as u8, 120, 200]));
        let scene = Scene { width: 400, height: 250, icons: vec![Rect { x: 300.0, y: 40.0, w: 60.0, h: 70.0 }] };
        let out = run(&image, &scene, &Options::default());
        assert_eq!(out.as_raw(), image.as_raw());
    }

    #[test]
    fn zero_strength_pads_do_nothing() {
        let image = stripes(200, 120);
        let scene = Scene { width: 200, height: 120, icons: vec![Rect { x: 20.0, y: 20.0, w: 40.0, h: 40.0 }] };
        let out = run(&image, &scene, &Options { pads: 0.0, ..Options::default() });
        assert_eq!(out.as_raw(), image.as_raw());
        assert!(!wanted(&scene, &Options { pads: 0.0, ..Options::default() }));
        assert!(!wanted(&Scene { icons: Vec::new(), ..scene.clone() }, &Options::default()));
        assert!(wanted(&scene, &Options::default()));
    }
}
