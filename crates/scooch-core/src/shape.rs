use crate::blur::Area;
use crate::{solver, GrayImage, Scene};
use rayon::prelude::*;
#[cfg(test)]
use std::collections::VecDeque;

pub const Q: usize = 4;
pub const FIELD_STEP: f32 = 1.0 / 64.0;
pub const FAR: f32 = 4.0;
const GROW: f32 = 0.20;
const ROUND: f32 = 0.25;
const SMOOTH: f32 = 0.30;
const CUT: f32 = 2.2;
const WOBBLE: [(f32, f32, u32); 2] = [(0.09, 1.4, 11), (0.04, 0.5, 23)];
const BOLD: (f32, f32, u32) = (0.45, 2.6, 71);
const MEANDER: (f32, f32, u32) = (1.4, 3.2, 73);
const MEANDER_RISE: f32 = 0.6;
const ANCHOR: f32 = 1.0;
const BEYOND: f32 = 2.0;
const PUFFS: [(f32, f32, u32); 2] = [(0.55, 1.2, 53), (0.30, 0.55, 59)];
const PUFF_LOW: f32 = 0.25;
const PUFF_FACE: (f32, f32) = (-0.4, 0.5);
const PUFF_JITTER: f32 = 0.35;
const PUFF_INSET: f32 = 0.10;
const PUFF_JOIN: f32 = 0.03;
const PUFF_REACH: f32 = 1.1;
const PUFF_SPAN: (f32, f32) = (0.8, 1.2);
const OPEN: f32 = 1.0;
const GAPS: f32 = 0.25;
const FOLDS: (f32, f32, u32) = (0.12, 0.6, 61);
const BILLOWS: (f32, f32, u32) = (0.75, 1.25, 67);
const MARGIN: f32 = 2.6;
const PAINT_REACH: f32 = 0.45;
const CLOUD_MEANDER: f32 = 0.6;

pub struct Ground {
    pub rects: Vec<[f32; 4]>,
    pub icons: Vec<[f32; 4]>,
    pub anchored: Vec<bool>,
    pub u: f32,
}

pub fn anchor(r: &[f32; 4], u: f32, scene: &Scene) -> [f32; 4] {
    let (w, h) = (scene.width as f32, scene.height as f32);
    let (near, past) = (ANCHOR * u + GROW * u, BEYOND * u);
    [
        if r[0] < near { -past } else { r[0] },
        if r[1] < near { -past } else { r[1] },
        if r[2] > w - near { w + past } else { r[2] },
        if r[3] > h - near { h + past } else { r[3] },
    ]
}

pub fn ground(scene: &Scene) -> Option<Ground> {
    if scene.width == 0 || scene.height == 0 {
        return None;
    }
    let icons: Vec<[f32; 4]> = solver::screen_rects(scene).into_iter().map(|r| r.map(|v| v as f32)).collect();
    if icons.is_empty() {
        return None;
    }
    let mut heights: Vec<f32> = icons.iter().map(|r| r[3] - r[1]).collect();
    heights.sort_by(f32::total_cmp);
    let u = heights[heights.len() / 2];
    if u <= 0.5 {
        return None;
    }
    let rects: Vec<[f32; 4]> = icons.iter().map(|r| anchor(r, u, scene)).collect();
    let anchored = rects.iter().zip(&icons).map(|(r, i)| r != i).collect();
    Some(Ground { rects, icons, anchored, u })
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

fn wobble(px: f32, py: f32, u: f32, share: f32) -> (f32, f32) {
    let small: f32 = WOBBLE.iter().map(|(size, scale, seed)| size * u * noise(px / (scale * u), py / (scale * u), *seed)).sum();
    let bold = BOLD.0 * u * noise(px / (BOLD.1 * u), py / (BOLD.1 * u), BOLD.2).max(0.0);
    let meander = if share > 0.0 { MEANDER.0 * u * smoothstep(0.0, MEANDER_RISE, noise(px / (MEANDER.1 * u), py / (MEANDER.1 * u), MEANDER.2)) * share } else { 0.0 };
    (small - bold * (1.0 - share) - meander, meander)
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub w: usize,
    pub h: usize,
    pub cell: usize,
}

impl Grid {
    pub fn of(width: usize, height: usize, cell: usize) -> Self {
        Self { w: width.div_ceil(cell), h: height.div_ceil(cell), cell }
    }

    pub fn center(&self, i: usize) -> (f32, f32) {
        (((i % self.w) * self.cell) as f32 + self.cell as f32 / 2.0, ((i / self.w) * self.cell) as f32 + self.cell as f32 / 2.0)
    }

    pub fn at(&self, px: f32) -> f32 {
        px / self.cell as f32 - 0.5
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

pub fn whole(grid: &Grid) -> Area {
    [0, 0, grid.w, grid.h]
}

pub fn field_reach(u: f32) -> f32 {
    (GROW + CUT) * u
}

pub fn cells_near(grid: &Grid, r: &[f32; 4], reach: f32) -> Area {
    let c = grid.cell as f32;
    let span = |lo: f32, hi: f32, n: usize| ((((lo - reach) / c).floor() - 1.0).clamp(0.0, n as f32) as usize, (((hi + reach) / c).ceil() + 1.0).clamp(0.0, n as f32) as usize);
    let (x0, x1) = span(r[0], r[2], grid.w);
    let (y0, y1) = span(r[1], r[3], grid.h);
    [x0, y0, x1, y1]
}

fn field(g: &Ground, grid: &Grid) -> (Vec<f32>, Vec<f32>) {
    let mut d = vec![FAR * g.u; grid.w * grid.h];
    let mut meanders = vec![0.0f32; grid.w * grid.h];
    field_into(g, grid, whole(grid), &mut d, &mut meanders);
    (d, meanders)
}

pub fn field_into(g: &Ground, grid: &Grid, [x0, y0, x1, y1]: Area, d: &mut [f32], meanders: &mut [f32]) {
    let k = SMOOTH * g.u;
    let cut = CUT * g.u;
    let grow = GROW * g.u;
    let far = FAR * g.u;
    let reach = grow + cut;
    let (x1, y1) = (x1.min(grid.w), y1.min(grid.h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    d.par_chunks_mut(grid.w).zip(meanders.par_chunks_mut(grid.w)).enumerate().skip(y0).take(y1 - y0).for_each(|(j, (row, bends))| {
        let py = grid.center(j * grid.w).1;
        let mut sums = vec![[0.0f32; 2]; x1 - x0];
        for (r, anchored) in g.rects.iter().zip(&g.anchored).filter(|(r, _)| py >= r[1] - reach && py <= r[3] + reach) {
            let i0 = (((r[0] - reach) / grid.cell as f32).floor().max(0.0) as usize).min(grid.w).max(x0);
            let i1 = (((r[2] + reach) / grid.cell as f32).ceil().max(0.0) as usize).min(grid.w).min(x1);
            for i in i0..i1 {
                let sdf = rounded_box(grid.center(i).0, py, r, grow);
                if sdf < cut {
                    let e = (-sdf / k).exp();
                    let sum = &mut sums[i - x0];
                    sum[0] += e;
                    if *anchored {
                        sum[1] += e;
                    }
                }
            }
        }
        for (i, sum) in (x0..x1).zip(&sums) {
            if sum[0] > 0.0 {
                let (shift, meander) = wobble(grid.center(i).0, py, g.u, sum[1] / sum[0]);
                row[i] = (-k * sum[0].ln() + shift).min(far);
                bends[i] = meander;
            } else {
                row[i] = far;
                bends[i] = 0.0;
            }
        }
    });
}

#[cfg(test)]
fn neighbours(i: usize, grid: &Grid) -> [Option<usize>; 4] {
    let (x, y) = (i % grid.w, i / grid.w);
    [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (y + 1 < grid.h).then(|| i + grid.w)]
}

#[cfg(test)]
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

pub fn reached(d: &[f32], grid: &Grid, reach: usize) -> Vec<bool> {
    let cap = reach as u32 + 1;
    let mut across = vec![cap; d.len()];
    across.par_chunks_mut(grid.w).zip(d.par_chunks(grid.w)).for_each(|(out, row)| {
        let mut last = cap;
        for (o, v) in out.iter_mut().zip(row) {
            last = if *v < 0.0 { 0 } else { (last + 1).min(cap) };
            *o = last;
        }
        for i in (0..grid.w.saturating_sub(1)).rev() {
            out[i] = out[i].min(out[i + 1] + 1);
        }
    });
    let mut down = vec![cap; d.len()];
    down.par_chunks_mut(grid.h).enumerate().for_each(|(x, column)| {
        let mut last = cap;
        for (y, o) in column.iter_mut().enumerate() {
            last = across[y * grid.w + x].min(last + 1);
            *o = last;
        }
        for y in (0..grid.h.saturating_sub(1)).rev() {
            column[y] = column[y].min(column[y + 1] + 1);
        }
    });
    (0..d.len()).into_par_iter().map(|i| down[(i % grid.w) * grid.h + i / grid.w] as usize <= reach).collect()
}

pub fn reached_into(d: &[f32], grid: &Grid, reach: usize, a: Area, owned: &mut [bool]) {
    let crop = [a[0].saturating_sub(reach + 1), a[1].saturating_sub(reach + 1), (a[2] + reach + 1).min(grid.w), (a[3] + reach + 1).min(grid.h)];
    let small = Grid { w: crop[2] - crop[0], h: crop[3] - crop[1], cell: grid.cell };
    if small.w == 0 || small.h == 0 {
        return;
    }
    let mut part = Vec::with_capacity(small.w * small.h);
    for y in crop[1]..crop[3] {
        part.extend_from_slice(&d[y * grid.w + crop[0]..y * grid.w + crop[2]]);
    }
    let near = reached(&part, &small, reach);
    for y in a[1]..a[3] {
        for x in a[0]..a[2] {
            owned[y * grid.w + x] = near[(y - crop[1]) * small.w + x - crop[0]];
        }
    }
}

pub struct Base {
    pub grid: Grid,
    pub u: f32,
    pub d: Vec<f32>,
    pub owned: Vec<bool>,
    pub icons: Vec<[f32; 4]>,
    pub meanders: Vec<f32>,
}

pub fn margin(u: f32) -> usize {
    (MARGIN * u / Q as f32).ceil() as usize
}

pub fn base(scene: &Scene, width: usize, height: usize) -> Option<Base> {
    let g = ground(scene)?;
    if width == 0 || height == 0 {
        return None;
    }
    let grid = Grid::of(width, height, Q);
    let (d, meanders) = field(&g, &grid);
    let owned = reached(&d, &grid, margin(g.u));
    Some(Base { grid, u: g.u, d, owned, icons: g.icons, meanders })
}

#[derive(PartialEq)]
struct Lattice {
    spacing: f32,
    cols: i32,
    rows: i32,
    points: Vec<[f32; 4]>,
}

impl Lattice {
    fn new(d: Option<&[f32]>, grid: &Grid, u: f32, (size, spacing, seed): (f32, f32, u32)) -> Self {
        let spacing = spacing * u;
        let cols = ((grid.w * grid.cell) as f32 / spacing).ceil() as i32 + 3;
        let rows = ((grid.h * grid.cell) as f32 / spacing).ceil() as i32 + 3;
        let points = (0..rows * cols)
            .map(|k| {
                let (cx, cy) = (k % cols - 1, k / cols - 1);
                let x = (cx as f32 + 0.5 + PUFF_JITTER * hash(cx, cy, seed)) * spacing;
                let y = (cy as f32 + 0.5 + PUFF_JITTER * hash(cx, cy, seed + 1)) * spacing;
                let (inside, upper) = d.map_or((-1.0, 1.0), |d| {
                    let at = |x: f32, y: f32| grid.linear(d, grid.at(x), grid.at(y));
                    let step = grid.cell as f32;
                    let (dx, dy) = (at(x + step, y) - at(x - step, y), at(x, y + step) - at(x, y - step));
                    let rise = -dy / (dx * dx + dy * dy).sqrt().max(1e-6);
                    (at(x, y), PUFF_LOW + (1.0 - PUFF_LOW) * smoothstep(PUFF_FACE.0, PUFF_FACE.1, rise))
                });
                [x, y, size * u * (0.6 + 0.4 * hash(cx, cy, seed + 2)) * upper, inside]
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

#[derive(PartialEq)]
pub struct Puffs(Vec<Lattice>);

pub fn calmer(d: f32, meander: f32) -> f32 {
    d + meander * CLOUD_MEANDER
}

pub fn puffs(base: &Base, calmer: &[f32]) -> Puffs {
    Puffs(PUFFS.iter().map(|p| Lattice::new(Some(calmer), &base.grid, base.u, *p)).collect())
}

impl Lattice {
    fn bytes(&self) -> usize {
        self.points.len() * 16
    }
}

impl Billows {
    pub fn bytes(&self) -> usize {
        self.0.bytes()
    }
}

impl Base {
    pub fn bytes(&self) -> usize {
        (self.d.len() + self.meanders.len()) * 4 + self.owned.len() + self.icons.len() * 16
    }
}

impl Puffs {
    pub fn bytes(&self) -> usize {
        self.0.iter().map(|l| l.bytes()).sum()
    }

    pub fn moved(&self, other: &Puffs, grid: &Grid) -> Vec<Area> {
        let mut out = Vec::new();
        for (a, b) in self.0.iter().zip(&other.0) {
            if a.points.len() != b.points.len() {
                return vec![whole(grid)];
            }
            for (k, (p, q)) in a.points.iter().zip(&b.points).enumerate() {
                if p.iter().zip(q).any(|(x, y)| x.to_bits() != y.to_bits()) {
                    let (cx, cy) = ((k as i32 % a.cols - 1) as f32, (k as i32 / a.cols - 1) as f32);
                    let s = a.spacing;
                    out.push(cells_near(grid, &[(cx - 1.0) * s, (cy - 1.0) * s, (cx + 2.0) * s, (cy + 2.0) * s], 0.0));
                }
            }
        }
        out
    }
}

pub fn clouds_into(base: &Base, d: &[f32], lattices: &Puffs, wanted: impl Fn(usize) -> bool + Sync, [x0, y0, x1, y1]: Area, out: &mut [f32]) {
    let (grid, u) = (base.grid, base.u);
    let (x1, y1) = (x1.min(grid.w), y1.min(grid.h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    out.par_chunks_mut(grid.w).enumerate().skip(y0).take(y1 - y0).for_each(|(j, row)| {
        for (x, cell) in row.iter_mut().enumerate().take(x1).skip(x0) {
            let i = j * grid.w + x;
            if !base.owned[i] || !wanted(i) {
                *cell = d[i];
                continue;
            }
            let (px, py) = grid.center(i);
            let mut puffed = d[i] + PUFF_INSET * u;
            if d[i] > -PUFF_SPAN.0 * u && d[i] < PUFF_SPAN.1 * u {
                for lattice in &lattices.0 {
                    for &[x, y, bulge, inside] in lattice.near(px, py) {
                        if inside < 0.0 {
                            let radius = (bulge - inside).min(PUFF_REACH * lattice.spacing);
                            puffed = smooth_min(puffed, ((px - x) * (px - x) + (py - y) * (py - y)).sqrt() - radius, PUFF_JOIN * u);
                        }
                    }
                }
            }
            *cell = puffed;
        }
    });
}

pub fn shade_reach(u: f32) -> f32 {
    OPEN * u
}

pub fn shade_into(icons: &[[f32; 4]], grid: &Grid, u: f32, [x0, y0, x1, y1]: Area, out: &mut [f32]) {
    let reach = OPEN * u;
    let (x1, y1) = (x1.min(grid.w), y1.min(grid.h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    out.par_chunks_mut(grid.w).enumerate().skip(y0).take(y1 - y0).for_each(|(j, row)| {
        let py = grid.center(j * grid.w).1;
        row[x0..x1].fill(reach);
        for r in icons.iter().filter(|r| py >= r[1] - reach && py <= r[3] + reach) {
            let i0 = (((r[0] - reach) / grid.cell as f32).floor().max(0.0) as usize).min(grid.w).max(x0);
            let i1 = (((r[2] + reach) / grid.cell as f32).ceil().max(0.0) as usize).min(grid.w).min(x1);
            for (i, cell) in row.iter_mut().enumerate().take(i1).skip(i0) {
                *cell = smooth_min(*cell, rounded_box(grid.center(i).0, py, r, 0.0), GAPS * u);
            }
        }
        for (i, cell) in row.iter_mut().enumerate().take(x1).skip(x0) {
            if *cell < reach {
                let px = grid.center(i).0;
                *cell -= FOLDS.0 * u * 0.5 * (1.0 + noise(px / (FOLDS.1 * u), py / (FOLDS.1 * u), FOLDS.2));
            }
        }
    });
}

pub struct Billows(Lattice);

pub fn billow_lattice(grid: &Grid, u: f32) -> Billows {
    Billows(Lattice::new(None, grid, u, (BILLOWS.0, BILLOWS.1, BILLOWS.2)))
}

pub fn billows_into(lattice: &Billows, grid: &Grid, u: f32, wanted: impl Fn(usize) -> bool + Sync, [x0, y0, x1, y1]: Area, out: &mut [f32]) {
    let (x1, y1) = (x1.min(grid.w), y1.min(grid.h));
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    out.par_chunks_mut(grid.w).enumerate().skip(y0).take(y1 - y0).for_each(|(j, row)| {
        for (x, cell) in row.iter_mut().enumerate().take(x1).skip(x0) {
            let i = j * grid.w + x;
            *cell = if !wanted(i) {
                FAR * u
            } else {
                let (px, py) = grid.center(i);
                lattice.0.near(px, py).fold(FAR * u, |acc, &[x, y, r, _]| smooth_min(acc, ((px - x) * (px - x) + (py - y) * (py - y)).sqrt() - r, GAPS * u))
            };
        }
    });
}

pub fn coverage(scene: &Scene, cell: usize) -> Option<(Grid, Vec<bool>)> {
    let g = ground(scene)?;
    let grid = Grid::of(scene.width as usize, scene.height as usize, cell);
    let (d, _) = field(&g, &grid);
    let reach = (PUFFS[0].0 + PAINT_REACH) * g.u + cell as f32 * 0.75;
    Some((grid, d.iter().map(|v| *v < reach).collect()))
}

pub fn field_image(scene: &Scene, width: u32, height: u32) -> GrayImage {
    let (w, h) = (width as usize, height as usize);
    let Some(b) = base(scene, w, h) else { return GrayImage::from_pixel(width, height, image::Luma([0])) };
    let step = FIELD_STEP * b.u;
    let mut out = vec![0u8; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let gy = b.grid.at(y as f32 + 0.5);
        for (x, cell) in row.iter_mut().enumerate() {
            let d = b.grid.linear(&b.d, b.grid.at(x as f32 + 0.5), gy);
            *cell = (128.0 - d / step).round().clamp(0.0, 255.0) as u8;
        }
    });
    GrayImage::from_raw(width, height, out).unwrap_or_else(|| GrayImage::new(width, height))
}

#[cfg(test)]
pub fn cluster_count(scene: &Scene) -> usize {
    let Some(g) = ground(scene) else { return 0 };
    let grid = Grid::of(scene.width as usize, scene.height as usize, Q);
    label(&field(&g, &grid).0, &grid).1.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;

    fn icon(x: f32, y: f32) -> Rect {
        Rect { x, y, w: 76.0, h: 94.0 }
    }

    #[test]
    fn reach_matches_a_breadth_first_walk() {
        let grid = Grid::of(160, 120, 4);
        let d: Vec<f32> = (0..grid.w * grid.h).map(|i| if [37usize, 520, 999, 1100].contains(&i) { -1.0 } else { 1.0 }).collect();
        let near = reached(&d, &grid, 6);
        let mut distance = vec![usize::MAX; d.len()];
        let mut queue: VecDeque<usize> = (0..d.len()).filter(|i| d[*i] < 0.0).collect();
        queue.iter().for_each(|i| distance[*i] = 0);
        while let Some(i) = queue.pop_front() {
            for n in neighbours(i, &grid).into_iter().flatten() {
                if distance[n] == usize::MAX {
                    distance[n] = distance[i] + 1;
                    queue.push_back(n);
                }
            }
        }
        for i in 0..d.len() {
            assert_eq!(near[i], distance[i] <= 6, "{i}");
        }
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
    fn edge_icons_are_anchored_past_the_screen() {
        let scene = Scene { width: 1200, height: 800, icons: vec![icon(4.0, 300.0), icon(600.0, 300.0)] };
        let g = ground(&scene).unwrap();
        assert_eq!(g.anchored, vec![true, false]);
        assert!(g.rects[0][0] < -100.0 && g.rects[1] == g.icons[1]);
    }

    #[test]
    fn coverage_holds_every_icon_and_spares_far_ground() {
        let scene = Scene { width: 1600, height: 1000, icons: vec![icon(700.0, 400.0)] };
        let (grid, mask) = coverage(&scene, 16).unwrap();
        assert!(mask[(445 / 16) * grid.w + 738 / 16]);
        assert!(!mask[(40 / 16) * grid.w + 40 / 16]);
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
