use crate::{solver, GrayImage, Rect, Scene};
use rayon::prelude::*;
use std::collections::VecDeque;

pub const Q: usize = 4;
pub const FIELD_STEP: f32 = 1.0 / 64.0;
const GROW: f32 = 0.20;
const ROUND: f32 = 0.25;
const SMOOTH: f32 = 0.30;
const CUT: f32 = 1.8;
const FAR: f32 = 4.0;
const WOBBLE: [(f32, f32, u32); 2] = [(0.09, 1.4, 11), (0.04, 0.5, 23)];
const BOLD: (f32, f32, u32) = (0.45, 2.6, 71);
const ANCHOR: f32 = 1.0;
const BEYOND: f32 = 2.0;
const LAND_NOISE: (f32, f32, u32) = (0.03, 0.6, 37);
const PUFFS: [(f32, f32, u32); 2] = [(0.55, 1.2, 53), (0.30, 0.55, 59)];
const PUFF_LOW: f32 = 0.5;
const PUFF_JITTER: f32 = 0.35;
const PUFF_INSET: f32 = 0.10;
const PUFF_JOIN: f32 = 0.03;
const PUFF_REACH: f32 = 1.1;
const PUFF_SPAN: (f32, f32) = (0.8, 1.2);
const OPEN: f32 = 0.6;
const GAPS: f32 = 0.25;
const FOLDS: (f32, f32, u32) = (0.30, 0.45, 61);
const VOLUME: f32 = 0.35;
const REACH: f32 = 0.34;
const MARGIN: f32 = 1.5;
const CLOUD_BASE: f32 = 0.12;
const CLOUD_SOFT: f32 = 0.08;
pub const ISLAND_SHADOW: (f32, f32, f32) = (0.07, 0.10, 0.28);
pub const CLOUD_SHADOW: (f32, f32, f32) = (0.12, 0.16, 0.07);

pub struct Ground {
    pub rects: Vec<[f32; 4]>,
    pub icons: Vec<[f32; 4]>,
    pub u: f32,
}

pub fn ground(scene: &Scene) -> Option<Ground> {
    if scene.width == 0 || scene.height == 0 {
        return None;
    }
    let rects: Vec<[f32; 4]> = solver::screen_rects(scene).into_iter().map(|r| r.map(|v| v as f32)).collect();
    if rects.is_empty() {
        return None;
    }
    let mut heights: Vec<f32> = rects.iter().map(|r| r[3] - r[1]).collect();
    heights.sort_by(f32::total_cmp);
    let u = heights[heights.len() / 2];
    if u <= 0.5 {
        return None;
    }
    let (w, h) = (scene.width as f32, scene.height as f32);
    let (near, past) = (ANCHOR * u + GROW * u, BEYOND * u);
    let icons = rects.clone();
    let rects = rects
        .into_iter()
        .map(|r| {
            [
                if r[0] < near { -past } else { r[0] },
                if r[1] < near { -past } else { r[1] },
                if r[2] > w - near { w + past } else { r[2] },
                if r[3] > h - near { h + past } else { r[3] },
            ]
        })
        .collect();
    Some(Ground { rects, icons, u })
}

pub fn footprints(scene: &Scene) -> Vec<Rect> {
    let Some(g) = ground(scene) else { return Vec::new() };
    let grow = REACH * g.u;
    g.rects.iter().map(|r| Rect { x: r[0] - grow, y: r[1] - grow, w: r[2] - r[0] + 2.0 * grow, h: r[3] - r[1] + 2.0 * grow }).collect()
}

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343) ^ (y as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h as f32 / u32::MAX as f32) * 2.0 - 1.0
}

pub fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xf, yf) = (x.floor(), y.floor());
    let (xi, yi) = (xf as i32, yf as i32);
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let (tx, ty) = (fade(x - xf), fade(y - yf));
    let top = hash(xi, yi, seed) + (hash(xi + 1, yi, seed) - hash(xi, yi, seed)) * tx;
    let bottom = hash(xi, yi + 1, seed) + (hash(xi + 1, yi + 1, seed) - hash(xi, yi + 1, seed)) * tx;
    top + (bottom - top) * ty
}

fn wobble(px: f32, py: f32, u: f32) -> f32 {
    let small: f32 = WOBBLE.iter().map(|(size, scale, seed)| size * u * noise(px / (scale * u), py / (scale * u), *seed)).sum();
    small - BOLD.0 * u * noise(px / (BOLD.1 * u), py / (BOLD.1 * u), BOLD.2).max(0.0)
}

fn rounded_box(px: f32, py: f32, r: &[f32; 4], grow: f32) -> f32 {
    let radius = (ROUND * (r[2] - r[0]).min(r[3] - r[1])).min(ROUND * 4.0 * grow / GROW) + grow;
    let (cx, cy) = ((r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0);
    let (ex, ey) = ((r[2] - r[0]) / 2.0 + grow - radius, (r[3] - r[1]) / 2.0 + grow - radius);
    let (qx, qy) = ((px - cx).abs() - ex, (py - cy).abs() - ey);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius
}

pub fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smooth_min(a: f32, b: f32, k: f32) -> f32 {
    -smooth_max(-a, -b, k)
}

fn smooth_max(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (a - b) / k).clamp(0.0, 1.0);
    b + (a - b) * h + k * h * (1.0 - h)
}

#[derive(Clone, Copy)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
}

impl Grid {
    pub fn of(width: usize, height: usize) -> Self {
        Self { w: width.div_ceil(Q), h: height.div_ceil(Q) }
    }

    pub fn center(&self, i: usize) -> (f32, f32) {
        (((i % self.w) * Q + Q / 2) as f32, ((i / self.w) * Q + Q / 2) as f32)
    }

    pub fn cubic(&self, values: &[f32], gx: f32, gy: f32) -> f32 {
        let (bx, by) = (gx.floor(), gy.floor());
        let (tx, ty) = (gx - bx, gy - by);
        let weights = |t: f32| {
            let (t2, t3) = (t * t, t * t * t);
            [(-t3 + 2.0 * t2 - t) * 0.5, (3.0 * t3 - 5.0 * t2 + 2.0) * 0.5, (-3.0 * t3 + 4.0 * t2 + t) * 0.5, (t3 - t2) * 0.5]
        };
        let (wx, wy) = (weights(tx), weights(ty));
        let mut sum = 0.0;
        for (j, wj) in wy.iter().enumerate() {
            let y = (by as i64 - 1 + j as i64).clamp(0, self.h as i64 - 1) as usize;
            let mut row = 0.0;
            for (i, wi) in wx.iter().enumerate() {
                let x = (bx as i64 - 1 + i as i64).clamp(0, self.w as i64 - 1) as usize;
                row += wi * values[y * self.w + x];
            }
            sum += wj * row;
        }
        sum
    }

    pub fn probe(&self, gx: f32, gy: f32) -> Probe {
        let gx = gx.clamp(0.0, (self.w - 1) as f32);
        let gy = gy.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        Probe { top: y0 * self.w, bottom: y1 * self.w, left: x0, right: x1, tx: gx - x0 as f32, ty: gy - y0 as f32 }
    }

    pub fn linear(&self, values: &[f32], gx: f32, gy: f32) -> f32 {
        let gx = gx.clamp(0.0, (self.w - 1) as f32);
        let gy = gy.clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let top = values[y0 * self.w + x0] + (values[y0 * self.w + x1] - values[y0 * self.w + x0]) * tx;
        let bottom = values[y1 * self.w + x0] + (values[y1 * self.w + x1] - values[y1 * self.w + x0]) * tx;
        top + (bottom - top) * ty
    }
}

#[derive(Clone, Copy)]
pub struct Probe {
    top: usize,
    bottom: usize,
    left: usize,
    right: usize,
    tx: f32,
    ty: f32,
}

impl Probe {
    #[inline(always)]
    pub fn at(&self, values: &[f32]) -> f32 {
        let upper = values[self.top + self.left] + (values[self.top + self.right] - values[self.top + self.left]) * self.tx;
        let lower = values[self.bottom + self.left] + (values[self.bottom + self.right] - values[self.bottom + self.left]) * self.tx;
        upper + (lower - upper) * self.ty
    }
}

fn field(g: &Ground, grid: &Grid) -> Vec<f32> {
    let k = SMOOTH * g.u;
    let cut = CUT * g.u;
    let grow = GROW * g.u;
    let far = FAR * g.u;
    let reach = grow + cut;
    let mut d = vec![far; grid.w * grid.h];
    d.par_chunks_mut(grid.w).enumerate().for_each(|(j, row)| {
        let py = (j * Q + Q / 2) as f32;
        let mut sums = vec![0.0f32; grid.w];
        for r in g.rects.iter().filter(|r| py >= r[1] - reach && py <= r[3] + reach) {
            let i0 = (((r[0] - reach) / Q as f32).floor().max(0.0) as usize).min(grid.w);
            let i1 = (((r[2] + reach) / Q as f32).ceil().max(0.0) as usize).min(grid.w);
            for (i, sum) in sums.iter_mut().enumerate().take(i1).skip(i0) {
                let sdf = rounded_box((i * Q + Q / 2) as f32, py, r, grow);
                if sdf < cut {
                    *sum += (-sdf / k).exp();
                }
            }
        }
        for (i, (cell, sum)) in row.iter_mut().zip(&sums).enumerate() {
            if *sum > 0.0 {
                *cell = (-k * sum.ln() + wobble((i * Q + Q / 2) as f32, py, g.u)).min(far);
            }
        }
    });
    d
}

fn neighbours(i: usize, grid: &Grid) -> [Option<usize>; 4] {
    let (x, y) = (i % grid.w, i / grid.w);
    [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (y + 1 < grid.h).then(|| i + grid.w)]
}

fn label(d: &[f32], grid: &Grid) -> (Vec<i32>, Vec<Vec<usize>>) {
    let mut labels = vec![-1i32; d.len()];
    let mut groups = Vec::new();
    let mut queue = VecDeque::new();
    for start in 0..d.len() {
        if d[start] >= 0.0 || labels[start] >= 0 {
            continue;
        }
        let id = groups.len() as i32;
        let mut cells = Vec::new();
        labels[start] = id;
        queue.push_back(start);
        while let Some(i) = queue.pop_front() {
            cells.push(i);
            for n in neighbours(i, grid).into_iter().flatten() {
                if labels[n] < 0 && d[n] < 0.0 {
                    labels[n] = id;
                    queue.push_back(n);
                }
            }
        }
        groups.push(cells);
    }
    (labels, groups)
}

fn owners(labels: &[i32], grid: &Grid, reach: usize) -> Vec<i32> {
    let mut owner = labels.to_vec();
    let mut distance = vec![usize::MAX; labels.len()];
    let mut queue = VecDeque::new();
    for (i, l) in labels.iter().enumerate() {
        if *l >= 0 {
            distance[i] = 0;
            queue.push_back(i);
        }
    }
    while let Some(i) = queue.pop_front() {
        if distance[i] >= reach {
            continue;
        }
        for n in neighbours(i, grid).into_iter().flatten() {
            if distance[n] == usize::MAX {
                distance[n] = distance[i] + 1;
                owner[n] = owner[i];
                queue.push_back(n);
            }
        }
    }
    owner
}

pub struct Base {
    pub grid: Grid,
    pub u: f32,
    pub d: Vec<f32>,
    pub labels: Vec<i32>,
    pub groups: Vec<Vec<usize>>,
    pub bottoms: Vec<f32>,
    pub icons: Vec<[f32; 4]>,
}

pub fn base(scene: &Scene, width: usize, height: usize) -> Option<Base> {
    let g = ground(scene)?;
    if width == 0 || height == 0 {
        return None;
    }
    let grid = Grid::of(width, height);
    let d = field(&g, &grid);
    let (labels, groups) = label(&d, &grid);
    let mut bottoms = vec![0.0f32; groups.len()];
    for r in &g.rects {
        let (cx, cy) = (((r[0] + r[2]) / 2.0) as usize / Q, ((r[1] + r[3]) / 2.0) as usize / Q);
        let at = cy.min(grid.h - 1) * grid.w + cx.min(grid.w - 1);
        if let Some(b) = usize::try_from(labels[at]).ok().and_then(|id| bottoms.get_mut(id)) {
            *b = b.max(r[3]);
        }
    }
    for (b, cells) in bottoms.iter_mut().zip(&groups) {
        if *b <= 0.0 {
            *b = cells.iter().map(|i| grid.center(*i).1).fold(0.0, f32::max) - GROW * g.u;
        }
    }
    Some(Base { grid, u: g.u, d, labels, groups, bottoms, icons: g.icons })
}

pub struct Shapes {
    pub base: Base,
    pub owner: Vec<i32>,
    pub land: Vec<f32>,
    pub cloud: Vec<f32>,
    pub icons: Vec<f32>,
    pub light: Vec<f32>,
}

fn nearest(base: &Base, keep: impl Fn(usize) -> bool + Sync) -> Vec<f32> {
    let (grid, reach) = (base.grid, OPEN * base.u);
    let mut out = vec![reach; grid.w * grid.h];
    out.par_chunks_mut(grid.w).enumerate().for_each(|(j, row)| {
        let py = (j * Q + Q / 2) as f32;
        let mut touched = false;
        for r in base.icons.iter().filter(|r| py >= r[1] - reach && py <= r[3] + reach) {
            let i0 = (((r[0] - reach) / Q as f32).floor().max(0.0) as usize).min(grid.w);
            let i1 = (((r[2] + reach) / Q as f32).ceil().max(0.0) as usize).min(grid.w);
            for (i, cell) in row.iter_mut().enumerate().take(i1).skip(i0) {
                if keep(j * grid.w + i) {
                    *cell = smooth_min(*cell, rounded_box((i * Q + Q / 2) as f32, py, r, 0.0), GAPS * base.u);
                    touched = true;
                }
            }
        }
        if touched {
            for (i, cell) in row.iter_mut().enumerate() {
                if *cell < reach {
                    let px = (i * Q + Q / 2) as f32;
                    *cell -= FOLDS.0 * base.u * noise(px / (FOLDS.1 * base.u), py / (FOLDS.1 * base.u), FOLDS.2).abs();
                }
            }
        }
    });
    out
}

struct Lattice {
    spacing: f32,
    cols: i32,
    rows: i32,
    points: Vec<[f32; 4]>,
}

impl Lattice {
    fn new(d: &[f32], grid: &Grid, u: f32, (size, spacing, seed): (f32, f32, u32)) -> Self {
        let spacing = spacing * u;
        let cols = ((grid.w * Q) as f32 / spacing).ceil() as i32 + 3;
        let rows = ((grid.h * Q) as f32 / spacing).ceil() as i32 + 3;
        let points = (0..rows * cols)
            .map(|k| {
                let (cx, cy) = (k % cols - 1, k / cols - 1);
                let x = (cx as f32 + 0.5 + PUFF_JITTER * hash(cx, cy, seed)) * spacing;
                let y = (cy as f32 + 0.5 + PUFF_JITTER * hash(cx, cy, seed + 1)) * spacing;
                [x, y, size * u * (0.6 + 0.4 * hash(cx, cy, seed + 2)), grid.linear(d, x / Q as f32 - 0.5, y / Q as f32 - 0.5)]
            })
            .collect();
        Self { spacing, cols, rows, points }
    }

    fn near(&self, px: f32, py: f32) -> impl Iterator<Item = &[f32; 4]> {
        let (ix, iy) = ((px / self.spacing).floor() as i32, (py / self.spacing).floor() as i32);
        (iy - 1..=iy + 1)
            .flat_map(move |cy| (ix - 1..=ix + 1).map(move |cx| (cx, cy)))
            .filter(|&(cx, cy)| cx >= -1 && cy >= -1 && cx + 1 < self.cols && cy + 1 < self.rows)
            .map(|(cx, cy)| &self.points[((cy + 1) * self.cols + cx + 1) as usize])
    }
}

pub fn shapes(base: Base, clouds: &[bool], centers: &[(f32, f32)], wide: &[bool]) -> Shapes {
    let (grid, u) = (base.grid, base.u);
    let owner = owners(&base.labels, &grid, (MARGIN * u / Q as f32).ceil() as usize);
    let d = &base.d;
    let owned = |i: usize| usize::try_from(owner[i]).ok();
    let land: Vec<f32> = (0..d.len())
        .into_par_iter()
        .map(|i| {
            if owner[i] < 0 {
                return d[i];
            }
            let (px, py) = grid.center(i);
            d[i] + LAND_NOISE.0 * u * noise(px / (LAND_NOISE.1 * u), py / (LAND_NOISE.1 * u), LAND_NOISE.2)
        })
        .collect();
    let lattices: Vec<Lattice> = PUFFS.iter().map(|p| Lattice::new(d, &grid, u, *p)).collect();
    let cloud: Vec<f32> = (0..d.len())
        .into_par_iter()
        .map(|i| {
            let Some(id) = owned(i).filter(|id| clouds[*id]) else { return d[i] };
            let (px, py) = grid.center(i);
            let mut puffed = d[i] + PUFF_INSET * u;
            if d[i] > -PUFF_SPAN.0 * u && d[i] < PUFF_SPAN.1 * u {
                let upper = |y: f32| PUFF_LOW + (1.0 - PUFF_LOW) * smoothstep(centers[id].1 + 0.5 * u, centers[id].1 - 0.5 * u, y);
                for lattice in &lattices {
                    for &[x, y, bulge, inside] in lattice.near(px, py) {
                        if inside < 0.0 {
                            let radius = (bulge * upper(y) - inside).min(PUFF_REACH * lattice.spacing);
                            puffed = smooth_min(puffed, ((px - x) * (px - x) + (py - y) * (py - y)).sqrt() - radius, PUFF_JOIN * u);
                        }
                    }
                }
            }
            if wide[id] {
                smooth_max(puffed, py - (base.bottoms[id] + CLOUD_BASE * u), CLOUD_SOFT * u)
            } else {
                puffed
            }
        })
        .collect();
    let cloudy = |i: usize| owned(i).is_some_and(|id| clouds[id]);
    let icons = nearest(&base, cloudy);
    let light: Vec<f32> = (0..d.len())
        .into_par_iter()
        .map(|i| {
            if !cloudy(i) {
                return 0.0;
            }
            let (x, y) = (i % grid.w, i / grid.w);
            let at = |x: usize, y: usize| cloud[y * grid.w + x];
            let dx = at((x + 1).min(grid.w - 1), y) - at(x.saturating_sub(1), y);
            let dy = at(x, (y + 1).min(grid.h - 1)) - at(x, y.saturating_sub(1));
            let length = dx.hypot(dy);
            if length < 1e-6 {
                return 0.0;
            }
            -(dx + dy) * std::f32::consts::FRAC_1_SQRT_2 / length * smoothstep(-VOLUME * u, 0.0, cloud[i])
        })
        .collect();
    Shapes { base, owner, land, cloud, icons, light }
}

pub fn field_image(scene: &Scene, width: u32, height: u32) -> GrayImage {
    let (w, h) = (width as usize, height as usize);
    let Some(b) = base(scene, w, h) else { return GrayImage::from_pixel(width, height, image::Luma([0])) };
    let step = FIELD_STEP * b.u;
    let mut out = vec![0u8; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let gy = (y as f32 + 0.5) / Q as f32 - 0.5;
        for (x, cell) in row.iter_mut().enumerate() {
            let gx = (x as f32 + 0.5) / Q as f32 - 0.5;
            let d = b.grid.cubic(&b.d, gx, gy);
            *cell = (128.0 - d / step).round().clamp(0.0, 255.0) as u8;
        }
    });
    GrayImage::from_raw(width, height, out).unwrap_or_else(|| GrayImage::new(width, height))
}

#[cfg(test)]
pub fn cluster_count(scene: &Scene) -> usize {
    base(scene, scene.width as usize, scene.height as usize).map_or(0, |b| b.groups.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon(x: f32, y: f32) -> Rect {
        Rect { x, y, w: 76.0, h: 94.0 }
    }

    #[test]
    fn near_icons_merge_and_far_icons_split() {
        let u = 94.0;
        let near = Scene { width: 1200, height: 800, icons: vec![icon(300.0, 300.0), icon(376.0 + 0.3 * u, 300.0)] };
        let far = Scene { width: 1200, height: 800, icons: vec![icon(300.0, 300.0), icon(376.0 + 1.5 * u, 300.0)] };
        assert_eq!(cluster_count(&near), 1);
        assert_eq!(cluster_count(&far), 2);
    }

    #[test]
    fn noise_stays_in_range_and_is_fixed() {
        for i in 0..2000 {
            let (x, y) = (i as f32 * 0.37, i as f32 * 0.11);
            let n = noise(x, y, 7);
            assert!((-1.0..=1.0).contains(&n));
            assert_eq!(n, noise(x, y, 7));
        }
    }

    #[test]
    fn footprints_reach_the_outer_ripple() {
        let scene = Scene { width: 1200, height: 800, icons: vec![icon(300.0, 300.0)] };
        let f = footprints(&scene)[0];
        assert!((f.x - (300.0 - 0.34 * 94.0)).abs() < 1e-3 && (f.h - (94.0 + 0.68 * 94.0)).abs() < 1e-3);
        assert!(footprints(&Scene { width: 1200, height: 800, icons: Vec::new() }).is_empty());
    }

    #[test]
    fn the_field_image_is_bright_inside_and_mid_grey_at_the_edge() {
        let scene = Scene { width: 400, height: 300, icons: vec![icon(150.0, 100.0)] };
        let field = field_image(&scene, 400, 300);
        assert!(field.get_pixel(188, 147).0[0] > 140);
        assert!(field.get_pixel(5, 5).0[0] < 60);
        assert_eq!(field_image(&Scene { icons: Vec::new(), ..scene }, 40, 30).dimensions(), (40, 30));
    }
}
