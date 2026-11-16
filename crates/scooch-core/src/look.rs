use crate::area::{self, Area};
use crate::paint::{self, Materials, LAYERS};
use crate::shape::{self, smoothstep, Grid};
use crate::pack::fingerprint;
use crate::{blur, color, math, scenery, table, Analysis, Options, Placement, Scene, Scenery};
use rayon::prelude::*;
use std::collections::VecDeque;

pub use crate::surface::{live, relive, Live};

pub const P: usize = 16;
pub const HOLD: (f32, f32) = (-1.7, 1.15);
pub(crate) const FLOOR: f32 = -0.6;
pub(crate) const LABEL_TOP: f32 = 2.0 / 3.0;
pub(crate) const LABEL_FALL: f32 = 0.7;
pub const REACH: f32 = 1.12;
pub const CELL: f32 = 1.1;
pub const LIFT: f32 = 0.12;
pub const DIM: f32 = 0.05;
pub(crate) const BELONG_SALIENCY: (f32, f32) = (0.10, 0.35);
pub(crate) const BELONG_BUSY: (f32, f32) = (0.20, 0.45);
pub(crate) const BELONG_SPREAD: f32 = 0.35;
pub(crate) const SEAM: (f32, f32) = (0.6, 0.95);
pub(crate) const SKY_HUE: (f32, f32) = (200.0, 290.0);
pub(crate) const SKY_SEED: (f32, f32) = (215.0, 0.02);
pub(crate) const SKY_HORIZON: f32 = 0.08;
pub(crate) const LIGHT_STEPS: usize = 1 << 16;
pub(crate) const SKY_CHROMA: f32 = 0.03;
pub(crate) const WHITE_CHROMA: f32 = 0.04;
pub(crate) const WHITE_LIGHT: f32 = 0.6;
pub(crate) const SKY_BUSY: f32 = 0.45;
pub(crate) const SKY_JUMP: f32 = 0.03;
pub(crate) const SKY_SMOOTH: (f32, f32) = (0.004, 2.0);
pub(crate) const SKY_SPREAD: f32 = 0.15;
pub(crate) const SKY_EDGE: (f32, f32) = (0.4, 0.6);
pub(crate) const SKY_WINDOW: (f32, f32, f32) = (175.0, 295.0, 15.0);
pub(crate) const SKY_NEED: (f32, f32, f32) = (0.03, 0.018, 0.1);
pub(crate) const SKY_TURN: (f32, f32) = (215.0, 250.0);
pub(crate) const SKY_PALE: f32 = 0.66;
pub(crate) const SKY_LIGHT: (f32, f32, f32, f32) = (0.62, 0.48, 0.025, 0.05);
pub(crate) const SKY_UNDER: (f32, f32) = (0.08, 0.02);
pub(crate) const SKY_GROW: (f32, f32, f32) = (0.015, 0.3, 0.01);
pub(crate) const SKY_SIDE: f32 = 0.003;
pub(crate) const SKY_DRIFT: f32 = 0.3;
pub(crate) const HOME_MOST: f64 = 4.0;
pub(crate) const WATER_WINDOW: (f32, f32, f32) = (191.0, 235.0, 10.0);
pub(crate) const WATER_CHROMA: (f32, f32) = (0.04, 0.06);
pub(crate) const WATER_LIGHT: (f32, f32) = (0.22, 0.28);
pub(crate) const ALLOW_COLOUR: f32 = 0.2;
pub(crate) const ALLOW_OPEN: f32 = 1.35;
pub(crate) const ALLOW_SOFT: f32 = 0.45;
pub(crate) const SLACK: f32 = 1e-3;
pub(crate) const LOCAL_SPREAD: f32 = 0.3;
pub(crate) const SALIENT: f32 = 0.35;
pub(crate) const APART: f32 = 0.1;
pub(crate) const GUARD_CLEAR: (f32, f32) = (1.5, 0.2);
pub(crate) const GUARD_FADE: f32 = 0.5;
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
pub(crate) const CLOUD_K: f32 = 0.15;
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
pub(crate) const RIM: f32 = 1.5;
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
    pub columns: &'a [f64],
    pub rows: &'a [f64],
}

pub struct Belonging {
    pub grid: Grid,
    pub size: [f64; 2],
    pub sky: Vec<f32>,
    pub belong: Vec<f32>,
    pub empty: bool,
    source: (u32, u32),
    stamp: u64,
}

const MAP_MAGIC: [u8; 8] = *b"SCOOCHBM";
const MAP_HEADER: usize = 56;
const MAP_LEAD: usize = 28;

fn fnv(parts: &[&[u8]]) -> u64 {
    parts.iter().flat_map(|p| p.iter()).fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x0100_0000_01b3))
}

impl Belonging {
    pub(crate) fn file(&self, picture: u64, source: (u32, u32)) -> Vec<u8> {
        let mut payload = Vec::with_capacity(MAP_LEAD + 8 * self.sky.len());
        for v in [self.grid.w, self.grid.h, self.grid.cell] {
            payload.extend_from_slice(&(v as u32).to_le_bytes());
        }
        for v in self.size {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        for v in self.sky.iter().chain(&self.belong) {
            payload.extend_from_slice(&v.to_le_bytes());
        }
        let mut out = Vec::with_capacity(MAP_HEADER + payload.len());
        out.extend_from_slice(&MAP_MAGIC);
        out.extend_from_slice(&crate::BELONGING_MAP_FORMAT.to_le_bytes());
        out.extend_from_slice(&crate::BELONGING_VERSION.to_le_bytes());
        out.extend_from_slice(&picture.to_le_bytes());
        out.extend_from_slice(&source.0.to_le_bytes());
        out.extend_from_slice(&source.1.to_le_bytes());
        out.extend_from_slice(&self.stamp.to_le_bytes());
        out.extend_from_slice(&(payload.len() as u64).to_le_bytes());
        let sum = fnv(&[&out, &payload]);
        out.extend_from_slice(&sum.to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    pub(crate) fn from_file(bytes: &[u8]) -> Option<(u64, (u32, u32), Belonging)> {
        let (head, payload) = (bytes.get(..MAP_HEADER)?, bytes.get(MAP_HEADER..)?);
        let half = |from: &[u8], at: usize| -> Option<u32> { Some(u32::from_le_bytes(from.get(at..at + 4)?.try_into().ok()?)) };
        let word = |from: &[u8], at: usize| -> Option<u64> { Some(u64::from_le_bytes(from.get(at..at + 8)?.try_into().ok()?)) };
        let trusted = head.get(..8)? == MAP_MAGIC.as_slice()
            && half(head, 8)? == crate::BELONGING_MAP_FORMAT
            && half(head, 12)? == crate::BELONGING_VERSION
            && word(head, 40)? == payload.len() as u64
            && word(head, 48)? == fnv(&[head.get(..48)?, payload]);
        if !trusted {
            return None;
        }
        let (picture, source, stamp) = (word(head, 16)?, (half(head, 24)?, half(head, 28)?), word(head, 32)?);
        let (w, h, cell) = (half(payload, 0)? as usize, half(payload, 4)? as usize, half(payload, 8)? as usize);
        let size = [f64::from_bits(word(payload, 12)?), f64::from_bits(word(payload, 20)?)];
        let cells = w.checked_mul(h)?;
        if w == 0 || h == 0 || cell == 0 || size.iter().any(|s| !s.is_finite() || *s <= 0.0) || cells.checked_mul(8)?.checked_add(MAP_LEAD)? != payload.len() {
            return None;
        }
        let values: Vec<f32> = payload.get(MAP_LEAD..)?.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
        if values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        let (sky, belong) = values.split_at(cells);
        let empty = belong.iter().all(|b| *b <= 0.0);
        Some((picture, source, Belonging { grid: Grid { w, h, cell }, size, sky: sky.to_vec(), belong: belong.to_vec(), empty, source, stamp }))
    }

    pub fn bytes(&self) -> usize {
        (self.sky.len() + self.belong.len()) * 4
    }

    pub(crate) fn stamp(&self) -> u64 {
        self.stamp
    }

    pub fn fits(&self, source: (u32, u32), stamp: u64) -> bool {
        self.source == source && self.stamp == stamp
    }

    pub fn at(&self, values: &[f32], fx: f64, fy: f64) -> f32 {
        let cell = self.grid.cell as f64;
        self.grid.linear(values, (fx * self.size[0] / cell - 0.5) as f32, (fy * self.size[1] / cell - 0.5) as f32)
    }
}

pub fn home_grid(source: (u32, u32), screen: (u32, u32)) -> (Grid, [f64; 2]) {
    let (sw, sh) = (source.0.max(1) as f64, source.1.max(1) as f64);
    let scale = (screen.0.max(1) as f64 / sw).max(screen.1.max(1) as f64 / sh);
    let size = [sw * scale, sh * scale];
    let screen_cells = (screen.0.max(1) as f64 / shape::Q as f64).ceil() * (screen.1.max(1) as f64 / shape::Q as f64).ceil();
    let home_cells = (size[0] / shape::Q as f64).ceil() * (size[1] / shape::Q as f64).ceil();
    let coarse = (home_cells / (HOME_MOST * screen_cells)).sqrt().ceil().max(1.0) as usize;
    (Grid::of(size[0].ceil() as usize, size[1].ceil() as usize, shape::Q * coarse), size)
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
    let (l, chroma, hue) = colour_of(lab);
    (chroma >= SKY_CHROMA && hue >= SKY_HUE.0 && hue <= SKY_HUE.1) || (chroma < WHITE_CHROMA && l > WHITE_LIGHT)
}

fn colour_of([l, a, b]: [f32; 3]) -> (f32, f32, f32) {
    (l, (a * a + b * b).sqrt(), math::atan2(b, a).to_degrees().rem_euclid(360.0))
}

fn window(hue: f32, (lo, hi, soft): (f32, f32, f32)) -> f32 {
    smoothstep(lo - soft, lo, hue) * (1.0 - smoothstep(hi, hi + soft, hue))
}

pub(crate) fn sky_colour(c: [f32; 3]) -> f32 {
    let (l, chroma, hue) = colour_of(c);
    let need = (SKY_NEED.0 + (SKY_NEED.1 - SKY_NEED.0) * smoothstep(SKY_TURN.0, SKY_TURN.1, hue)).max(SKY_NEED.2 * (l - SKY_PALE));
    let least = SKY_LIGHT.0 + (SKY_LIGHT.1 - SKY_LIGHT.0) * smoothstep(SKY_LIGHT.2, SKY_LIGHT.3, chroma);
    window(hue, SKY_WINDOW) * smoothstep(0.85 * need, 1.15 * need, chroma) * smoothstep(least - 0.05, least + 0.05, l) * under_cloud(l)
}

fn under_cloud(l: f32) -> f32 {
    let top = paint::SHADED.0 - SKY_UNDER.0;
    1.0 - smoothstep(top - SKY_UNDER.1, top + SKY_UNDER.1, l)
}

fn apart(a: [f32; 3], b: [f32; 3]) -> f32 {
    (0..3).map(|k| (a[k] - b[k]).powi(2)).sum::<f32>().sqrt()
}

const NONE: usize = usize::MAX;

fn flood(grid: &Grid, mut origin: Vec<usize>, down: bool, accept: &dyn Fn(usize, usize, usize) -> bool) -> Vec<usize> {
    let mut queue: VecDeque<usize> = (0..origin.len()).filter(|i| origin[*i] != NONE).collect();
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % grid.w, i / grid.w);
        let next = [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (down && y + 1 < grid.h).then(|| i + grid.w)];
        for n in next.into_iter().flatten() {
            if origin[n] == NONE && accept(i, n, origin[i]) {
                origin[n] = origin[i];
                queue.push_back(n);
            }
        }
    }
    origin
}

fn grown(seed: &[f32], colour: &[[f32; 3]], grid: &Grid) -> Vec<f32> {
    let open: Vec<bool> = colour
        .iter()
        .map(|c| {
            let (l, chroma, hue) = colour_of(*c);
            window(hue, SKY_WINDOW) >= 0.5 && chroma >= SKY_GROW.0 && l >= SKY_GROW.1 && under_cloud(l) >= 0.5
        })
        .collect();
    let seeds = (0..seed.len()).map(|i| if seed[i] >= 0.5 { i } else { NONE }).collect();
    let origin = flood(grid, seeds, false, &|i, n, o| {
        let step = if n + grid.w == i { SKY_GROW.2 } else { SKY_SIDE };
        open[n] && apart(colour[n], colour[i]) < step && apart(colour[n], colour[o]) <= SKY_DRIFT
    });
    horizon(grid, &origin, colour)
}

fn horizon(grid: &Grid, reached: &[usize], colour: &[[f32; 3]]) -> Vec<f32> {
    let step = |l: f32| (l.clamp(0.0, 1.0) * LIGHT_STEPS as f32).round() as u32;
    let none = u32::MAX;
    let light: Vec<u32> = colour.par_iter().zip(reached.par_iter()).map(|(c, r)| if *r == NONE { none } else { step(c[0]) }).collect();
    let fall = step(SKY_HORIZON);
    let mut peak = vec![none; light.len()];
    let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); LIGHT_STEPS + 1];
    for i in (0..grid.w.min(light.len())).filter(|i| light[*i] != none) {
        peak[i] = light[i];
        buckets[light[i] as usize].push(i as u32);
    }
    for top in 0..=LIGHT_STEPS as u32 {
        while let Some(i) = buckets[top as usize].pop() {
            let i = i as usize;
            if peak[i] != top {
                continue;
            }
            let (x, y) = (i % grid.w, i / grid.w);
            let next = [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (y + 1 < grid.h).then(|| i + grid.w)];
            for n in next.into_iter().flatten() {
                let l = light[n];
                if l == none || l.saturating_add(fall) < top {
                    continue;
                }
                let through = top.max(l);
                if through < peak[n] {
                    peak[n] = through;
                    buckets[through as usize].push(n as u32);
                }
            }
        }
    }
    peak.iter().map(|p| if *p != none { 1.0 } else { 0.0 }).collect()
}

fn root(parent: &mut [usize], mut i: usize) -> usize {
    while parent[i] != i {
        parent[i] = parent[parent[i]];
        i = parent[i];
    }
    i
}

pub(crate) fn water_colour(c: [f32; 3], sand: f32) -> f32 {
    let (l, chroma, hue) = colour_of(c);
    let top = sand - paint::UNDER_SAND.0;
    window(hue, WATER_WINDOW) * smoothstep(WATER_CHROMA.0, WATER_CHROMA.1, chroma) * smoothstep(WATER_LIGHT.0, WATER_LIGHT.1, l) * (1.0 - smoothstep(top - paint::UNDER_SAND.1, top + paint::UNDER_SAND.1, l))
}

fn opened(mask: &[bool], grid: &Grid, r: f32) -> Vec<f32> {
    let whole = shape::whole(grid);
    let cap = r + grid.cell as f32;
    let room = chamfer(grid, whole, cap, &|i| !mask[i]);
    let core = chamfer(grid, whole, cap, &|i| room[i] >= r);
    core.iter().map(|d| if *d <= r { 1.0 } else { 0.0 }).collect()
}

fn allowances(lab: &[[f32; 3]], grid: &Grid, u: f32, sand: f32) -> (Vec<f32>, Vec<f32>) {
    let spread = |k: f32| k * u / grid.cell as f32;
    let soft: Vec<Vec<f32>> = (0..3).map(|k| blurred(&lab.iter().map(|c| c[k]).collect::<Vec<f32>>(), grid, spread(ALLOW_COLOUR))).collect();
    let colour: Vec<[f32; 3]> = (0..lab.len()).map(|i| [soft[0][i], soft[1][i], soft[2][i]]).collect();
    let marks: Vec<[bool; 2]> = colour.iter().map(|c| [sky_colour(*c) >= 0.5, water_colour(*c, sand) >= 0.5]).collect();
    let seed = |k: usize| opened(&marks.iter().map(|m| m[k]).collect::<Vec<bool>>(), grid, ALLOW_OPEN * u);
    (blurred(&grown(&seed(0), &colour, grid), grid, spread(ALLOW_SOFT)), blurred(&seed(1), grid, spread(ALLOW_SOFT)))
}

fn blurred(values: &[f32], grid: &Grid, spread: f32) -> Vec<f32> {
    blur::gaussian_parallel(values, grid.w, grid.h, spread)
}

fn sky_region(lab: &[[f32; 3]], busy: &[f32], grid: &Grid) -> Vec<f32> {
    let open: Vec<bool> = lab.par_iter().zip(busy.par_iter()).map(|(c, b)| skyish(*c) && *b < SKY_BUSY).collect();
    let soft: Vec<Vec<f32>> = (0..3).map(|k| blurred(&lab.par_iter().map(|c| c[k]).collect::<Vec<f32>>(), grid, SKY_SMOOTH.1)).collect();
    let accept = |i: usize, n: usize| (open[n] && apart(lab[n], lab[i]) < SKY_JUMP) || (busy[n] < SKY_BUSY && (0..3).map(|k| (soft[k][n] - soft[k][i]).powi(2)).sum::<f32>().sqrt() < SKY_SMOOTH.0);
    let (w, h) = (grid.w, grid.h);
    let ways: Vec<u8> = (0..lab.len())
        .into_par_iter()
        .map(|i| {
            let (x, y) = (i % w, i / w);
            let next = [(x > 0).then(|| i - 1), (x + 1 < w).then(|| i + 1), (y > 0).then(|| i - w), (y + 1 < h).then(|| i + w)];
            next.iter().enumerate().fold(0u8, |m, (k, n)| if n.is_some_and(|n| accept(i, n)) { m | 1 << k } else { m })
        })
        .collect();
    let top = w.min(lab.len());
    let mut origin: Vec<u32> = (0..lab.len()).map(|i| if i < top && open[i] { i as u32 } else { u32::MAX }).collect();
    let mut parent: Vec<usize> = (0..top).collect();
    let mut queue: VecDeque<u32> = (0..top as u32).filter(|i| open[*i as usize]).collect();
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        let next = [i.wrapping_sub(1), i + 1, i.wrapping_sub(w), i + w];
        for (k, n) in next.into_iter().enumerate() {
            if ways[i] & 1 << k == 0 {
                continue;
            }
            if origin[n] == u32::MAX {
                origin[n] = origin[i];
                queue.push_back(n as u32);
            } else if origin[n] != origin[i] {
                let (a, b) = (root(&mut parent, origin[i] as usize), root(&mut parent, origin[n] as usize));
                parent[a.max(b)] = a.min(b);
            }
        }
    }
    let mut votes = vec![(0usize, 0usize); top];
    for i in (0..top).filter(|i| open[*i]) {
        let (l, chroma, hue) = colour_of([soft[0][i], soft[1][i], soft[2][i]]);
        let vote = &mut votes[root(&mut parent, i)];
        vote.0 += usize::from(if chroma < SKY_SEED.1 { l > WHITE_LIGHT } else { (SKY_SEED.0..=SKY_WINDOW.1).contains(&hue) });
        vote.1 += 1;
    }
    let sky: Vec<bool> = votes.iter().map(|(yes, all)| 2 * yes > *all).collect();
    let roots: Vec<usize> = (0..top).map(|i| root(&mut parent, i)).collect();
    let under = undecided(&lab[..top], busy, &ways, w);
    origin.par_iter().enumerate().map(|(i, o)| if *o != u32::MAX { if sky[roots[*o as usize]] { 1.0 } else { 0.0 } } else if under.get(i).copied().unwrap_or(false) { 0.5 } else { 0.0 }).collect()
}

fn undecided(top: &[[f32; 3]], busy: &[f32], ways: &[u8], w: usize) -> Vec<bool> {
    let mut queue: VecDeque<u32> = (0..top.len()).filter(|i| busy[*i] < SKY_BUSY && top[*i][0] <= WHITE_LIGHT && top[*i][1].hypot(top[*i][2]) < SKY_CHROMA).map(|i| i as u32).collect();
    if queue.is_empty() {
        return Vec::new();
    }
    let mut reached = vec![false; ways.len()];
    for i in &queue {
        reached[*i as usize] = true;
    }
    while let Some(i) = queue.pop_front() {
        let i = i as usize;
        for (k, n) in [i.wrapping_sub(1), i + 1, i.wrapping_sub(w), i + w].into_iter().enumerate() {
            if ways[i] & 1 << k != 0 && !reached[n] {
                reached[n] = true;
                queue.push_back(n as u32);
            }
        }
    }
    reached
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

pub(crate) type Key = [u32; 5];

pub(crate) fn chamfer(grid: &Grid, area: Area, cap: f32, source: &dyn Fn(usize) -> bool) -> Vec<f32> {
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

    pub(crate) fn solid_cells(&self) -> Vec<bool> {
        self.solid.iter().map(|d| *d <= 0.0).collect()
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

pub(crate) fn print(found: &Analysis) -> u64 {
    let mut bytes = Vec::with_capacity((found.saliency.len() + found.busy.len() + found.palette.len() * 4 + 8) * 4);
    for v in [found.width, found.height, found.source_width, found.source_height, found.flat as u32, found.saliency.len() as u32, found.busy.len() as u32, found.palette.len() as u32] {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    let s = found.subject;
    for v in [s.x, s.y, s.w, s.h].iter().chain(&found.saliency).chain(&found.busy).chain(found.palette.iter().flatten()) {
        bytes.extend_from_slice(&v.to_bits().to_le_bytes());
    }
    fingerprint(0, 0, &bytes)
}

pub(crate) fn same(a: &Analysis, b: &Analysis) -> bool {
    let bits = |x: &[f32], y: &[f32]| x.len() == y.len() && x.iter().zip(y).all(|(p, q)| p.to_bits() == q.to_bits());
    let rect = |r: &crate::Rect| [r.x, r.y, r.w, r.h].map(f32::to_bits);
    (a.width, a.height, a.source_width, a.source_height, a.flat) == (b.width, b.height, b.source_width, b.source_height, b.flat)
        && rect(&a.subject) == rect(&b.subject)
        && bits(&a.saliency, &b.saliency)
        && bits(&a.busy, &b.busy)
        && bits(a.palette.as_flattened(), b.palette.as_flattened())
}

pub(crate) fn stamp(print: u64, scene: &Scene, options: &Options) -> u64 {
    let u = scenery::active(scene, options).then(|| shape::ground(scene)).flatten().map_or(0.0, |g| g.u);
    let words = [print, u.to_bits() as u64, options.scenery as u64, crate::BELONGING_VERSION as u64];
    fingerprint(scene.width, scene.height, &words.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>())
}

pub fn belonging_map(input: &Sampled, source: (u32, u32), screen: (u32, u32), options: &Options, u: f32, sand: f32, stamp: u64) -> Belonging {
    let grid = input.grid;
    let cells = grid.w * grid.h;
    let size = home_grid(source, screen).1;
    let spread = |k: f32| k * u / grid.cell as f32;
    let raw: Vec<f32> = input.saliency.iter().zip(input.busy).map(|(s, b)| belonging(*s, *b)).collect();
    let calm = blurred(&raw, &grid, spread(BELONG_SPREAD));
    let lab = || -> Vec<[f32; 3]> { input.small.par_iter().map(|c| color::oklab(c[0], c[1], c[2])).collect() };
    let (sky, belong) = match options.scenery {
        Scenery::Clouds => (vec![1.0; cells], calm),
        Scenery::Islands | Scenery::Off => (vec![0.0; cells], calm),
        Scenery::Auto => {
            let lab = lab();
            let sky: Vec<f32> = blurred(&sky_region(&lab, input.busy, &grid), &grid, spread(SKY_SPREAD)).into_iter().map(|v| smoothstep(SKY_EDGE.0, SKY_EDGE.1, v)).collect();
            let (air, water) = allowances(&lab, &grid, u, sand);
            let belong = (0..cells).map(|i| calm[i] * smoothstep(SEAM.0, SEAM.1, (2.0 * sky[i] - 1.0).abs()) * (sky[i] * air[i] + (1.0 - sky[i]) * water[i])).collect();
            (sky, belong)
        }
    };
    let empty = belong.iter().all(|b| *b <= 0.0);
    Belonging { grid, size, sky, belong, empty, source, stamp }
}

pub fn fixed(input: &Sampled, belonging: &Belonging, materials: &Materials, u: f32, (width, height): (usize, usize)) -> Fixed {
    let grid = input.grid;
    let cells = grid.w * grid.h;
    let cell = CELL * u;
    let encode = color::encode_table();
    let lab: Vec<[f32; 3]> = input.small.par_iter().map(|c| color::oklab(c[0], c[1], c[2])).collect();
    let spread = |k: f32| k * u / grid.cell as f32;
    let mapped = |values: &[f32]| -> Vec<f32> { (0..cells).into_par_iter().map(|c| belonging.at(values, input.columns[c % grid.w], input.rows[c / grid.w])).collect() };
    let (sky, belong) = (mapped(&belonging.sky), mapped(&belonging.belong));
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
    let subject = chamfer(&paint_grid, coarse, far, &|p| inside(p).any(|i| input.saliency[i] >= SALIENT));
    let backdrop: Vec<Vec<f32>> = pooled.iter().map(|c| blurred(c, &paint_grid, BACKDROP * cell / P as f32)).collect();
    let solid = chamfer(&paint_grid, coarse, far, &|p| inside(p).any(|i| input.saliency[i] >= SALIENT && (0..3).map(|k| (lab[i][k] - backdrop[k][p]).powi(2)).sum::<f32>().sqrt() > APART));
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
        let clear = (GUARD_CLEAR.0 * P as f32).max(GUARD_CLEAR.1 * u);
        paints[paint::GUARD * plane + i * 4 + 3] = byte(1.0 - smoothstep(clear, clear + GUARD_FADE * u, solid[i]));
    }
    Fixed { sky, belong, paints, paint_grid, grid, u, subject, solid, light, blend }
}


#[cfg(test)]
mod timing {
    use super::*;

    #[test]
    #[ignore]
    fn sky_region_and_horizon_at_the_largest_grids() {
        for (label, w, h) in [("the largest home grid, four times the cells of a 7680 by 4320 screen", 5760usize, 1440usize), ("a 4 to 1 panorama on a 6016 by 3384 screen", 3384, 846), ("a picture filling a 2880 by 1800 screen", 720, 450), ("a 4 to 1 panorama on a 2880 by 1800 screen", 1800, 450)] {
            let grid = Grid { w, h, cell: shape::Q };
            let lab: Vec<[f32; 3]> = (0..w * h).map(|i| [0.58 + 0.15 * (i / w) as f32 / h as f32 + 0.002 * shape::hash((i % w) as i32, (i / w) as i32, 3), -0.03, -0.1]).collect();
            let busy = vec![0.0f32; w * h];
            let started = std::time::Instant::now();
            let sky = sky_region(&lab, &busy, &grid);
            let region = started.elapsed();
            let reached: Vec<usize> = (0..w * h).collect();
            let started = std::time::Instant::now();
            let kept = horizon(&grid, &reached, &lab);
            let flood = started.elapsed();
            let share = |v: &[f32]| v.iter().filter(|x| **x > 0.5).count() as f64 / v.len() as f64;
            println!("{label}: {} cells, sky_region {:.1} ms, horizon {:.1} ms, sky {:.3}, kept {:.3}", w * h, region.as_secs_f64() * 1e3, flood.as_secs_f64() * 1e3, share(&sky), share(&kept));
        }
    }
}
