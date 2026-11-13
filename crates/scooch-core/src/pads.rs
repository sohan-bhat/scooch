use crate::table::{self, Curve};
use crate::shape::Grid;
use crate::{blur, color, scenery, solver, Analysis, Options, Placement, Scene};
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
    let rough = lookup.plane(&ground, grid);
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
    let mask = blur::gaussian_boxes(&mask, grid.w, grid.h, (feather / Q as f64) as f32);
    let mut weight = vec![0.0f32; grid.w * grid.h];
    for b in &padded {
        for qy in b[1]..b[3].min(grid.h) {
            for qx in b[0]..b[2].min(grid.w) {
                let c = qy * grid.w + qx;
                weight[c] = mask[c] * rough[c] * open[c] * amount * belong.map_or(1.0, |b| 1.0 - b[c]);
            }
        }
    }
    if !weight.iter().any(|w| *w > 0.0) {
        return None;
    }
    let sigma = (CALM * width as f64 / Q as f64) as f32;
    let planes: Vec<Vec<f32>> = (0..4)
        .into_par_iter()
        .map(|k| {
            let plane: Vec<f32> = if k == 3 { open.clone() } else { small.iter().zip(&open).map(|(c, o)| c[k] * o).collect() };
            blur::gaussian_boxes(&plane, grid.w, grid.h, sigma)
        })
        .collect();
    let encode = color::encode_table();
    Some(
        (0..grid.w * grid.h)
            .flat_map(|c| {
                let share = planes[3][c];
                let calm = |k: usize| if share > 1e-4 { planes[k][c] / share } else { small[c][k] };
                [color::encode(encode, calm(0)), color::encode(encode, calm(1)), color::encode(encode, calm(2)), (weight[c].clamp(0.0, 1.0) * 255.0).round() as u8]
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{analysis, render, table, Rect, RgbImage, Scenery};

    fn stripes(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| if (x / 6 + y / 6) % 2 == 0 { image::Rgb([240, 240, 240]) } else { image::Rgb([20, 20, 20]) })
    }

    fn run(image: &RgbImage, scene: &Scene, options: &Options) -> RgbImage {
        let found = analysis::analyze(image);
        render::render(image, Some(&found), &table::fill(image.width(), image.height(), scene.width, scene.height), scene, options)
    }

    fn plain() -> Options {
        Options { scenery: Scenery::Off, ..Options::default() }
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
        let out = run(&image, &scene, &plain());
        let changed = |x: u32, y: u32| out.get_pixel(x, y) != image.get_pixel(x, y);
        assert!((300..360).any(|x| (40..110).any(|y| changed(x, y))));
        assert!((0..150).all(|x| (0..250).all(|y| !changed(x, y))));
        assert!((0..400).all(|x| (200..250).all(|y| !changed(x, y))));
    }

    #[test]
    fn calm_ground_is_left_bit_identical() {
        let image = RgbImage::from_fn(400, 250, |x, _| image::Rgb([(x / 2) as u8, 120, 200]));
        let scene = Scene { width: 400, height: 250, icons: vec![Rect { x: 300.0, y: 40.0, w: 60.0, h: 70.0 }] };
        let out = run(&image, &scene, &plain());
        assert_eq!(out.as_raw(), image.as_raw());
    }

    #[test]
    fn zero_strength_pads_do_nothing() {
        let image = stripes(200, 120);
        let scene = Scene { width: 200, height: 120, icons: vec![Rect { x: 20.0, y: 20.0, w: 40.0, h: 40.0 }] };
        let out = run(&image, &scene, &Options { pads: 0.0, ..plain() });
        assert_eq!(out.as_raw(), image.as_raw());
        assert!(!wanted(&scene, &Options { pads: 0.0, ..plain() }));
        assert!(!wanted(&Scene { icons: Vec::new(), ..scene.clone() }, &plain()));
        assert!(wanted(&scene, &plain()));
    }
}
