use crate::area::{self, Area};
use crate::paint::{self, Materials, LAYERS};
use crate::shape::{self, smoothstep, Grid};
use crate::{blur, color, math, scenery, table, Analysis, Options, Placement, Scene, Scenery};
use rayon::prelude::*;
use std::collections::VecDeque;

pub use crate::surface::{live, relive, Live};

pub const P: usize = 16;
pub const HOLD: (f32, f32) = (-0.6, 1.15);
pub const REACH: f32 = 1.12;
pub const CELL: f32 = 1.1;
pub const LIFT: f32 = 0.12;
pub const DIM: f32 = 0.05;
pub(crate) const BELONG_SALIENCY: (f32, f32) = (0.10, 0.35);
pub(crate) const BELONG_BUSY: (f32, f32) = (0.20, 0.45);
pub(crate) const BELONG_SPREAD: f32 = 0.35;
pub(crate) const SEAM: (f32, f32) = (0.6, 0.95);
pub(crate) const SKY_HUE: (f32, f32) = (200.0, 290.0);
pub(crate) const SKY_CHROMA: f32 = 0.03;
pub(crate) const WHITE_CHROMA: f32 = 0.04;
pub(crate) const WHITE_LIGHT: f32 = 0.6;
pub(crate) const SKY_BUSY: f32 = 0.45;
pub(crate) const SKY_JUMP: f32 = 0.03;
pub(crate) const SKY_SMOOTH: (f32, f32) = (0.004, 2.0);
pub(crate) const SKY_SPREAD: f32 = 0.15;
pub(crate) const SKY_EDGE: (f32, f32) = (0.4, 0.6);
pub(crate) const SLACK: f32 = 1e-3;
pub(crate) const LOCAL_SPREAD: f32 = 0.3;
pub(crate) const SALIENT: f32 = 0.35;
pub(crate) const APART: f32 = 0.1;
pub(crate) const BACKDROP: f32 = 2.0;
pub(crate) const FALLBACK: [f32; 2] = [-0.6, -0.8];
pub(crate) const LEAN: f32 = 0.4;
pub(crate) const GLOW: (f32, f32) = (0.01, 0.06);
pub(crate) const GLOW_SPREAD: f32 = 0.15;
pub(crate) const GLOW_WEIGHT: (f32, f32) = (0.15, 0.6);
pub(crate) const MOTTLE: (f32, f32, u32) = (0.04, 2.0, 22);
pub(crate) const SPACING: f32 = 1.0 / 3.0;
pub(crate) const JITTER: f32 = 0.8;
pub(crate) const BIG: f32 = 0.45;
pub(crate) const SCALE: (f32, f32) = (0.8, 0.7);
pub(crate) const SPREAD: (f32, f32) = (0.4, 0.22);
pub(crate) const STRETCH: (f32, f32, f32, u32) = (0.4, 1.0, 3.0, 33);
pub(crate) const FLARE: (f32, f32) = (0.8, 0.25);
pub(crate) const BELOW: f32 = 0.6;
pub(crate) const SUPPORT: f32 = 3.0;
pub(crate) const SWELL: (f32, f32, u32) = (0.6, 2.5, 31);
pub(crate) const TILT: f32 = 0.35;
pub(crate) const PUFF: (f32, f32) = (0.48, 0.34);
pub(crate) const RISE: f32 = 0.35;
pub(crate) const WIDE: f32 = 0.8;
pub(crate) const MOST: f32 = 0.95;
pub(crate) const FADE: (f32, f32) = (0.2, 0.3);
pub(crate) const CLEAR: f32 = 0.45;
pub(crate) const ROOM: (f32, f32) = (0.5, 0.7);
pub(crate) const SMALL: (f32, f32) = (0.6, 0.4);
pub(crate) const INSIDE: (f32, f32) = (0.33, 0.5);
pub(crate) const FIRM: (f32, f32) = (0.5, 0.9);
pub(crate) const HOLE: f32 = 0.15;
pub(crate) const SOFT: f32 = 0.1;
pub(crate) const CLOUD_K: f32 = 0.1;
pub(crate) const LAND_K: f32 = 0.3;
pub(crate) const ISLE: (f32, f32) = (0.6, 0.35);
pub(crate) const ISLE_COVER: (f32, f32) = (0.35, 0.75);
pub(crate) const ISLE_FADE: (f32, f32) = (0.3, 0.45);
pub(crate) const PUSH: f32 = 0.6;
pub(crate) const CORE: f32 = 0.5;
pub(crate) const CORE_GATE: (f32, f32) = (0.03, 0.15);
pub(crate) const FIT: (f32, f32) = (0.15, 0.3);
pub(crate) const BARE: (f32, f32) = (1.0, 2.0);
pub(crate) const COAST: (f32, f32, u32) = (0.14, 1.5, 21);
pub(crate) const SHALLOW: (f32, f32, f32, u32) = (0.7, 0.8, 2.0, 25);
pub(crate) const GUARD: (f32, f32) = (0.1, 0.6);
pub(crate) const PUSHED: f32 = 1.2;
pub(crate) const FOAM_ALONG: (f32, f32, f32, u32) = (0.3, 0.4, 0.7, 24);
pub(crate) const FOAM_STRENGTH: (f32, f32, f32, u32) = (0.8, 0.35, 0.5, 28);
pub(crate) const CLOSE: f32 = 0.08;
pub(crate) const DEPTH: f32 = 1.5;
pub(crate) const DEPTH_BLUR: f32 = 2.0;
pub(crate) const TOWARD: f32 = 1.4;
pub(crate) const TOWARD_BLUR: f32 = 2.5;
pub(crate) const BROAD: (f32, f32) = (0.1, 1.3);
pub(crate) const FACING: f32 = 0.8;
pub(crate) const PILE_CELL: f32 = 1.1;
pub(crate) const PILE_SKIP: f32 = 0.25;
pub(crate) const ARC: (f32, f32) = (-0.3, -0.03);
pub(crate) const ARC_POWER: f32 = 1.5;
pub(crate) const TUCK: f32 = 0.25;
pub(crate) const AWAY: (f32, f32) = (0.6, 0.95);
pub(crate) const CALM: f32 = 0.85;
pub(crate) const MIDDLE_LIFT: f32 = 0.7;
pub(crate) const KIND: f32 = 0.05;
pub(crate) const ISLE_MOST: f32 = 1.2;
pub(crate) const HALF_ROOT_THREE: f32 = 0.866_025_4;
pub(crate) const SPOTS: [(f32, f32); 13] = [
    (0.0, 0.0),
    (0.5, 0.0),
    (0.25, 0.5 * HALF_ROOT_THREE),
    (-0.25, 0.5 * HALF_ROOT_THREE),
    (-0.5, 0.0),
    (-0.25, -0.5 * HALF_ROOT_THREE),
    (0.25, -0.5 * HALF_ROOT_THREE),
    (0.85 * HALF_ROOT_THREE, 0.425),
    (0.0, 0.85),
    (-0.85 * HALF_ROOT_THREE, 0.425),
    (-0.85 * HALF_ROOT_THREE, -0.425),
    (0.0, -0.85),
    (0.85 * HALF_ROOT_THREE, -0.425),
];

pub struct Sampled<'a> {
    pub grid: Grid,
    pub small: &'a [[f32; 3]],
    pub saliency: &'a [f32],
    pub busy: &'a [f32],
}

pub struct Scenic {
    pub field: Vec<f32>,
    pub look: Vec<u8>,
    pub paints: Vec<u8>,
    pub paint_grid: Grid,
}

pub fn belonging(saliency: f32, busy: f32) -> f32 {
    (1.0 - smoothstep(BELONG_SALIENCY.0, BELONG_SALIENCY.1, saliency)) * (1.0 - smoothstep(BELONG_BUSY.0, BELONG_BUSY.1, busy))
}

pub fn active(found: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> bool {
    if !scenery::active(scene, options) {
        return false;
    }
    let (w, h) = (found.width as usize, found.height as usize);
    if w == 0 || h == 0 || found.saliency.len() != w * h || found.busy.len() != w * h {
        return true;
    }
    let span = |table: &[f32], size: usize| {
        if !table::usable(table) {
            return (0, size - 1);
        }
        let (lo, hi) = (table[0].min(table[table.len() - 1]), table[0].max(table[table.len() - 1]));
        let lo = ((lo * size as f32 - 0.5).floor() as i64 - 1).clamp(0, size as i64 - 1) as usize;
        let hi = ((hi * size as f32 - 0.5).ceil() as i64 + 1).clamp(0, size as i64 - 1) as usize;
        (lo, hi)
    };
    let ((x0, x1), (y0, y1)) = (span(&placement.fx, w), span(&placement.fy, h));
    let clean = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let low = |plane: &[f32], x: usize, y: usize| {
        let (x2, y2) = ((x + 1).min(x1), (y + 1).min(y1));
        clean(plane[y * w + x]).min(clean(plane[y * w + x2])).min(clean(plane[y2 * w + x])).min(clean(plane[y2 * w + x2])) - SLACK
    };
    (y0..=y1).any(|y| (x0..=x1).any(|x| belonging(if found.flat { 0.0 } else { low(&found.saliency, x, y) }, low(&found.busy, x, y)) > 0.0))
}

fn skyish(lab: [f32; 3]) -> bool {
    let [l, a, b] = lab;
    let chroma = (a * a + b * b).sqrt();
    let hue = math::atan2(b, a).to_degrees().rem_euclid(360.0);
    (chroma >= SKY_CHROMA && hue >= SKY_HUE.0 && hue <= SKY_HUE.1) || (chroma < WHITE_CHROMA && l > WHITE_LIGHT)
}

fn blurred(values: &[f32], grid: &Grid, spread: f32) -> Vec<f32> {
    blur::gaussian_parallel(values, grid.w, grid.h, spread)
}

fn sky_region(lab: &[[f32; 3]], busy: &[f32], grid: &Grid) -> Vec<f32> {
    let open: Vec<bool> = lab.iter().zip(busy).map(|(c, b)| skyish(*c) && *b < SKY_BUSY).collect();
    let calm: Vec<bool> = busy.iter().map(|b| *b < SKY_BUSY).collect();
    let soft: Vec<Vec<f32>> = (0..3).map(|k| blurred(&lab.iter().map(|c| c[k]).collect::<Vec<f32>>(), grid, SKY_SMOOTH.1)).collect();
    let mut reached = vec![false; lab.len()];
    let mut queue: VecDeque<usize> = (0..grid.w).filter(|i| open[*i]).collect();
    for i in &queue {
        reached[*i] = true;
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % grid.w, i / grid.w);
        let next = [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (y + 1 < grid.h).then(|| i + grid.w)];
        for n in next.into_iter().flatten() {
            let jump = (0..3).map(|k| (lab[n][k] - lab[i][k]).powi(2)).sum::<f32>().sqrt();
            let smooth = || (0..3).map(|k| (soft[k][n] - soft[k][i]).powi(2)).sum::<f32>().sqrt() < SKY_SMOOTH.0;
            if !reached[n] && ((open[n] && jump < SKY_JUMP) || (calm[n] && smooth())) {
                reached[n] = true;
                queue.push_back(n);
            }
        }
    }
    reached.into_iter().map(|r| if r { 1.0 } else { 0.0 }).collect()
}

pub(crate) fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub(crate) fn unit_hash(x: i32, y: i32, seed: u32) -> f32 {
    0.5 + 0.5 * shape::hash(x, y, seed)
}

pub(crate) fn length(x: f32, y: f32) -> f32 {
    (x * x + y * y).sqrt()
}

pub(crate) fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    if b >= a + k {
        return a;
    }
    if a >= b + k {
        return b;
    }
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

pub fn none(grid: Grid, width: usize, height: usize) -> Scenic {
    let paint_grid = Grid::of(width, height, P);
    Scenic { field: vec![HOLD.1; grid.w * grid.h], look: vec![0; grid.w * grid.h * 4], paints: vec![0; paint_grid.w * paint_grid.h * 4 * LAYERS], paint_grid }
}

pub(crate) fn chamfer(grid: &Grid, area: Area, cap: f32, source: impl Fn(usize) -> bool) -> Vec<f32> {
    let step = grid.cell as f32;
    let margin = (cap / step).ceil() as usize + 1;
    let crop = area::grow(area, margin, grid.w, grid.h);
    let (cw, ch) = (crop[2] - crop[0], crop[3] - crop[1]);
    let far = cap + step;
    let mut values: Vec<f32> = (0..cw * ch).map(|k| if source((crop[1] + k / cw) * grid.w + crop[0] + k % cw) { 0.0 } else { far }).collect();
    let diag = step * std::f32::consts::SQRT_2;
    for y in 0..ch {
        for x in 0..cw {
            let mut v = values[y * cw + x];
            if v == 0.0 {
                continue;
            }
            if x > 0 {
                v = v.min(values[y * cw + x - 1] + step);
            }
            if y > 0 {
                v = v.min(values[(y - 1) * cw + x] + step);
                if x > 0 {
                    v = v.min(values[(y - 1) * cw + x - 1] + diag);
                }
                if x + 1 < cw {
                    v = v.min(values[(y - 1) * cw + x + 1] + diag);
                }
            }
            values[y * cw + x] = v.min(far);
        }
    }
    for y in (0..ch).rev() {
        for x in (0..cw).rev() {
            let mut v = values[y * cw + x];
            if v == 0.0 {
                continue;
            }
            if x + 1 < cw {
                v = v.min(values[y * cw + x + 1] + step);
            }
            if y + 1 < ch {
                v = v.min(values[(y + 1) * cw + x] + step);
                if x + 1 < cw {
                    v = v.min(values[(y + 1) * cw + x + 1] + diag);
                }
                if x > 0 {
                    v = v.min(values[(y + 1) * cw + x - 1] + diag);
                }
            }
            values[y * cw + x] = v.min(far);
        }
    }
    let (aw, ah) = (area[2] - area[0], area[3] - area[1]);
    (0..aw * ah).map(|k| values[(area[1] - crop[1] + k / aw) * cw + area[0] - crop[0] + k % aw].min(cap)).collect()
}

pub(crate) fn put(values: &mut [f32], w: usize, area: Area, fresh: &[f32]) {
    let span = area[2] - area[0];
    for y in area[1]..area[3] {
        values[y * w + area[0]..y * w + area[2]].copy_from_slice(&fresh[(y - area[1]) * span..(y - area[1] + 1) * span]);
    }
}

pub(crate) fn gradient(values: &[f32], grid: &Grid, i: usize) -> (f32, f32) {
    let (x, y) = (i % grid.w, i / grid.w);
    let at = |x: usize, y: usize| values[y * grid.w + x];
    let (l, r) = (x.saturating_sub(1), (x + 1).min(grid.w - 1));
    let (t, b) = (y.saturating_sub(1), (y + 1).min(grid.h - 1));
    let dx = (at(r, y) - at(l, y)) / ((r - l).max(1) as f32);
    let dy = (at(x, b) - at(x, t)) / ((b - t).max(1) as f32);
    (dx, dy)
}

pub struct Fixed {
    pub sky: Vec<f32>,
    pub belong: Vec<f32>,
    pub paints: Vec<u8>,
    pub paint_grid: Grid,
    pub(crate) grid: Grid,
    pub(crate) u: f32,
    subject: Vec<f32>,
    solid: Vec<f32>,
    light: Vec<[i16; 2]>,
    blend: Vec<u8>,
}

impl Fixed {
    pub fn bytes(&self) -> usize {
        (self.sky.len() + self.belong.len() + self.subject.len() + self.solid.len()) * 4 + self.light.len() * 4 + self.blend.len() + self.paints.len()
    }

    pub fn u(&self) -> f32 {
        self.u
    }

    pub(crate) fn at(&self, values: &[f32], x: f32, y: f32) -> f32 {
        self.grid.linear(values, self.grid.at(x), self.grid.at(y))
    }

    pub(crate) fn coarse(&self, values: &[f32], x: f32, y: f32) -> f32 {
        self.paint_grid.linear(values, self.paint_grid.at(x), self.paint_grid.at(y))
    }

    pub(crate) fn subject(&self, x: f32, y: f32) -> f32 {
        self.coarse(&self.subject, x, y)
    }

    pub(crate) fn solid(&self, x: f32, y: f32) -> f32 {
        self.coarse(&self.solid, x, y)
    }

    pub(crate) fn light(&self, x: f32, y: f32) -> [f32; 2] {
        let g = &self.paint_grid;
        let (gx, gy) = (g.at(x).clamp(0.0, (g.w - 1) as f32), g.at(y).clamp(0.0, (g.h - 1) as f32));
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(g.w - 1), (y0 + 1).min(g.h - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let v = |x: usize, y: usize, k: usize| self.light[y * g.w + x][k] as f32;
        let mix = |k: usize| {
            let top = v(x0, y0, k) + (v(x1, y0, k) - v(x0, y0, k)) * tx;
            let bottom = v(x0, y1, k) + (v(x1, y1, k) - v(x0, y1, k)) * tx;
            top + (bottom - top) * ty
        };
        let (lx, ly) = (mix(0), mix(1));
        let n = length(lx, ly).max(1e-6);
        [lx / n, ly / n]
    }

    pub(crate) fn blend(&self, x: f32, y: f32) -> f32 {
        let g = &self.paint_grid;
        let (gx, gy) = (g.at(x).clamp(0.0, (g.w - 1) as f32), g.at(y).clamp(0.0, (g.h - 1) as f32));
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(g.w - 1), (y0 + 1).min(g.h - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let v = |x: usize, y: usize| self.blend[y * g.w + x] as f32 / 255.0;
        let top = v(x0, y0) + (v(x1, y0) - v(x0, y0)) * tx;
        let bottom = v(x0, y1) + (v(x1, y1) - v(x0, y1)) * tx;
        top + (bottom - top) * ty
    }
}

pub fn fixed(input: &Sampled, options: &Options, materials: &Materials, u: f32, width: usize, height: usize) -> Fixed {
    let grid = input.grid;
    let cells = grid.w * grid.h;
    let cell = CELL * u;
    let encode = color::encode_table();
    let lab: Vec<[f32; 3]> = input.small.par_iter().map(|c| color::oklab(c[0], c[1], c[2])).collect();
    let raw: Vec<f32> = input.saliency.iter().zip(input.busy).map(|(s, b)| belonging(*s, *b)).collect();
    let spread = |k: f32| k * u / grid.cell as f32;
    let calm = blurred(&raw, &grid, spread(BELONG_SPREAD));
    let sky: Vec<f32> = match options.scenery {
        Scenery::Clouds => vec![1.0; cells],
        Scenery::Islands => vec![0.0; cells],
        _ => blurred(&sky_region(&lab, input.busy, &grid), &grid, spread(SKY_SPREAD)).into_iter().map(|v| smoothstep(SKY_EDGE.0, SKY_EDGE.1, v)).collect(),
    };
    let belong: Vec<f32> = calm.iter().zip(&sky).map(|(c, s)| if options.scenery == Scenery::Auto { c * smoothstep(SEAM.0, SEAM.1, (2.0 * s - 1.0).abs()) } else { *c }).collect();
    let paint_grid = Grid::of(width, height, P);
    let step = P / grid.cell;
    let pool = |values: &dyn Fn(usize) -> f32| -> Vec<f32> {
        (0..paint_grid.w * paint_grid.h)
            .map(|i| {
                let (px, py) = ((i % paint_grid.w) * step, (i / paint_grid.w) * step);
                let (mut sum, mut count) = (0.0f32, 0.0f32);
                for y in py..(py + step).min(grid.h) {
                    for x in px..(px + step).min(grid.w) {
                        sum += values(y * grid.w + x);
                        count += 1.0;
                    }
                }
                sum / count.max(1.0)
            })
            .collect()
    };
    let inside = |p: usize| {
        let (px, py) = ((p % paint_grid.w) * step, (p / paint_grid.w) * step);
        (py..(py + step).min(grid.h)).flat_map(move |y| (px..(px + step).min(grid.w)).map(move |x| y * grid.w + x))
    };
    let pooled: Vec<Vec<f32>> = (0..3).map(|k| pool(&|i| lab[i][k])).collect();
    let coarse = shape::whole(&paint_grid);
    let far = (paint_grid.w + paint_grid.h) as f32 * P as f32;
    let subject = chamfer(&paint_grid, coarse, far, |p| inside(p).any(|i| input.saliency[i] >= SALIENT));
    let backdrop: Vec<Vec<f32>> = pooled.iter().map(|c| blurred(c, &paint_grid, BACKDROP * cell / P as f32)).collect();
    let solid = chamfer(&paint_grid, coarse, far, |p| inside(p).any(|i| input.saliency[i] >= SALIENT && (0..3).map(|k| (lab[i][k] - backdrop[k][p]).powi(2)).sum::<f32>().sqrt() > APART));
    let luminance = pool(&|i| 0.2126 * input.small[i][0] + 0.7152 * input.small[i][1] + 0.0722 * input.small[i][2]);
    let half = |x0: usize, x1: usize, y0: usize, y1: usize| (y0..y1).map(|y| (x0..x1).map(|x| luminance[y * paint_grid.w + x]).sum::<f32>()).sum::<f32>();
    let (mw, mh) = (paint_grid.w / 2, paint_grid.h / 2);
    let (lx, ly) = (half(mw, paint_grid.w, 0, paint_grid.h) - half(0, mw, 0, paint_grid.h), half(0, paint_grid.w, mh, paint_grid.h) - half(0, paint_grid.w, 0, mh));
    let n = length(lx, ly);
    let toward = if n > 1e-3 { [lx / n, ly / n] } else { FALLBACK };
    let leaned = [toward[0] * LEAN + FALLBACK[0] * (1.0 - LEAN), toward[1] * LEAN + FALLBACK[1] * (1.0 - LEAN)];
    let m = length(leaned[0], leaned[1]).max(1e-6);
    let base = [leaned[0] / m, leaned[1] / m];
    let skies = pool(&|i| sky[i]);
    let glow_raw: Vec<f32> = (0..paint_grid.w * paint_grid.h).map(|p| pooled[0][p] * smoothstep(GLOW.0, GLOW.1, pooled[2][p]) * skies[p]).collect();
    let glow = blurred(&glow_raw, &paint_grid, GLOW_SPREAD * paint_grid.w.max(paint_grid.h) as f32);
    let top = glow.iter().copied().fold(0.0f32, f32::max);
    let (light, blend): (Vec<[i16; 2]>, Vec<u8>) = (0..paint_grid.w * paint_grid.h)
        .map(|i| {
            let pack = |l: [f32; 2], weight: f32| (l.map(|v| (v * i16::MAX as f32).round() as i16), byte(weight));
            if top < 0.01 {
                return pack(base, 0.0);
            }
            let (x, y) = (i % paint_grid.w, i / paint_grid.w);
            let at = |x: usize, y: usize| glow[y * paint_grid.w + x];
            let (l, r) = (x.saturating_sub(1), (x + 1).min(paint_grid.w - 1));
            let (t, b) = (y.saturating_sub(1), (y + 1).min(paint_grid.h - 1));
            let (gx, gy) = (at(r, y) - at(l, y), at(x, b) - at(x, t));
            let g = length(gx, gy);
            let weight = smoothstep(GLOW_WEIGHT.0, GLOW_WEIGHT.1, glow[i] / top);
            if g < 1e-9 || weight <= 0.0 {
                return pack(base, 0.0);
            }
            let (px, py) = (base[0] + (gx / g - base[0]) * weight, base[1] + (gy / g - base[1]) * weight);
            let k = length(px, py).max(1e-6);
            pack([px / k, py / k], weight)
        })
        .unzip();
    let wide = spread(LOCAL_SPREAD * CELL) / step as f32;
    let channels: Vec<Vec<f32>> = pooled.par_iter().map(|c| blur::gaussian_parallel(c, paint_grid.w, paint_grid.h, wide)).collect();
    let sand = paint::sand(materials);
    let tones: Vec<paint::Tones> = (0..paint_grid.w * paint_grid.h)
        .into_par_iter()
        .map(|i| {
            let (px, py) = paint_grid.center(i);
            let mottle = MOTTLE.0 * shape::noise(px / (MOTTLE.1 * cell), py / (MOTTLE.1 * cell), MOTTLE.2);
            paint::tones([channels[0][i], channels[1][i], channels[2][i]], sand, mottle)
        })
        .collect();
    let mut paints = vec![0u8; paint_grid.w * paint_grid.h * 4 * LAYERS];
    let plane = paint_grid.w * paint_grid.h * 4;
    for (i, t) in tones.iter().enumerate() {
        for (layer, (rgb, alpha)) in t.colors.iter().zip(&t.alphas).enumerate() {
            let at = layer * plane + i * 4;
            for k in 0..3 {
                paints[at + k] = color::encode(encode, rgb[k]);
            }
            paints[at + 3] = byte(*alpha);
        }
    }
    Fixed { sky, belong, paints, paint_grid, grid, u, subject, solid, light, blend }
}

