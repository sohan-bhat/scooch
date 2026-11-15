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
const PUFF_SIZE: f32 = 0.55;
const PAINT_REACH: f32 = 0.45;

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

pub(crate) fn hash(x: i32, y: i32, seed: u32) -> f32 {
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

fn field_into(g: &Ground, grid: &Grid, [x0, y0, x1, y1]: Area, d: &mut [f32], meanders: &mut [f32]) {
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
                    let e = crate::math::exp(-sdf / k);
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
                row[i] = (-k * crate::math::ln(sum[0]) + shift).min(far);
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

pub fn coverage(scene: &Scene, cell: usize) -> Option<(Grid, Vec<bool>)> {
    let g = ground(scene)?;
    let grid = Grid::of(scene.width as usize, scene.height as usize, cell);
    let (d, _) = field(&g, &grid);
    let reach = (PUFF_SIZE + PAINT_REACH) * g.u + cell as f32 * 0.75;
    Some((grid, d.iter().map(|v| *v < reach).collect()))
}

pub fn field_image(scene: &Scene, width: u32, height: u32) -> GrayImage {
    let (w, h) = (width as usize, height as usize);
    let Some(g) = ground(scene).filter(|_| w > 0 && h > 0) else { return GrayImage::from_pixel(width, height, image::Luma([0])) };
    let grid = Grid::of(w, h, Q);
    let (d, _) = field(&g, &grid);
    let step = FIELD_STEP * g.u;
    let mut out = vec![0u8; w * h];
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let gy = grid.at(y as f32 + 0.5);
        for (x, cell) in row.iter_mut().enumerate() {
            let d = grid.linear(&d, grid.at(x as f32 + 0.5), gy);
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
