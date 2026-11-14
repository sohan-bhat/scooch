#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{Rgb, RgbImage};
use rayon::prelude::*;
use scooch_core::{Analysis, Options, Rect, Scene, Scenery};
use std::path::{Path, PathBuf};

fn hash(i: i32, j: i32, seed: u32) -> f32 {
    let mut h = (i as u32).wrapping_mul(0x8da6_b343) ^ (j as u32).wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    h as f32 / u32::MAX as f32
}

fn noise(x: f32, y: f32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - x.floor(), y - y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let a = hash(xi, yi, seed) + (hash(xi + 1, yi, seed) - hash(xi, yi, seed)) * sx;
    let b = hash(xi, yi + 1, seed) + (hash(xi + 1, yi + 1, seed) - hash(xi, yi + 1, seed)) * sx;
    (a + (b - a) * sy) * 2.0 - 1.0
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn smin(a: f32, b: f32, k: f32) -> f32 {
    let h = (0.5 + 0.5 * (b - a) / k).clamp(0.0, 1.0);
    b + (a - b) * h - k * h * (1.0 - h)
}

fn decode(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
}

fn encode(c: f32) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let v = if c <= 0.0031308 { c * 12.92 } else { 1.055 * c.powf(1.0 / 2.4) - 0.055 };
    (v * 255.0).round() as u8
}

fn to_lab(c: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * c[0] + 0.536_332_55 * c[1] + 0.051_445_99 * c[2]).cbrt();
    let m = (0.211_903_5 * c[0] + 0.680_699_5 * c[1] + 0.107_396_96 * c[2]).cbrt();
    let s = (0.088_302_46 * c[0] + 0.281_718_85 * c[1] + 0.629_978_7 * c[2]).cbrt();
    [0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s, 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s, 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s]
}

fn to_linear(lab: [f32; 3]) -> [f32; 3] {
    let l = (lab[0] + 0.396_337_78 * lab[1] + 0.215_803_76 * lab[2]).powi(3);
    let m = (lab[0] - 0.105_561_346 * lab[1] - 0.063_854_17 * lab[2]).powi(3);
    let s = (lab[0] - 0.089_484_18 * lab[1] - 1.291_485_5 * lab[2]).powi(3);
    [(4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s).clamp(0.0, 1.0), (-1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s).clamp(0.0, 1.0), (-0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s).clamp(0.0, 1.0)]
}

fn luminance(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

struct Puff {
    x: f32,
    y: f32,
    r: f32,
}

struct Buckets {
    size: f32,
    w: i32,
    h: i32,
    cells: Vec<Vec<usize>>,
    puffs: Vec<Puff>,
}

impl Buckets {
    fn new(puffs: Vec<Puff>, size: f32, width: u32, height: u32) -> Self {
        let (w, h) = ((width as f32 / size).ceil() as i32 + 4, (height as f32 / size).ceil() as i32 + 4);
        let mut cells = vec![Vec::new(); (w * h) as usize];
        for (i, p) in puffs.iter().enumerate() {
            let (bx, by) = (((p.x / size).floor() as i32 + 2).clamp(0, w - 1), ((p.y / size).floor() as i32 + 2).clamp(0, h - 1));
            cells[(by * w + bx) as usize].push(i);
        }
        Self { size, w, h, cells, puffs }
    }

    fn near(&self, x: f32, y: f32) -> impl Iterator<Item = &Puff> {
        let (bx, by) = ((x / self.size).floor() as i32 + 2, (y / self.size).floor() as i32 + 2);
        (by - 1..=by + 1).flat_map(move |j| (bx - 1..=bx + 1).map(move |i| (i, j))).filter(|(i, j)| *i >= 0 && *j >= 0 && *i < self.w && *j < self.h).flat_map(move |(i, j)| self.cells[(j * self.w + i) as usize].iter().map(move |k| &self.puffs[*k]))
    }

    fn field(&self, x: f32, y: f32, k: f32, far: f32, light: [f32; 2]) -> (f32, f32) {
        let (mut d, mut arcs) = (far, 0.0f32);
        for p in self.near(x, y) {
            let length = ((x - p.x).powi(2) + (y - p.y).powi(2)).sqrt();
            let e = length - p.r;
            d = smin(d, e, k);
            if e < 0.0 && length > 1e-3 {
                let facing = ((x - p.x) * light[0] + (y - p.y) * light[1]) / length;
                arcs = arcs.max(smoothstep(-0.3 * p.r, -0.03 * p.r, e) * facing.max(0.0));
            }
        }
        (d, arcs)
    }
}

struct Grid {
    w: usize,
    h: usize,
    step: f32,
}

impl Grid {
    fn at(&self, values: &[f32], x: f32, y: f32) -> f32 {
        let gx = (x / self.step - 0.5).clamp(0.0, (self.w - 1) as f32);
        let gy = (y / self.step - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let a = values[y0 * self.w + x0] + (values[y0 * self.w + x1] - values[y0 * self.w + x0]) * tx;
        let b = values[y1 * self.w + x0] + (values[y1 * self.w + x1] - values[y1 * self.w + x0]) * tx;
        a + (b - a) * ty
    }
}

fn table_at(table: &[f32], u: f32) -> f32 {
    let f = u.clamp(0.0, 1.0) * (table.len() - 1) as f32;
    let k = (f as usize).min(table.len() - 2);
    table[k] + (table[k + 1] - table[k]) * (f - k as f32)
}

fn keep_touching(puffs: Vec<Puff>, anchors: &[(f32, f32, f32, f32)], edge: f32) -> Vec<Puff> {
    let n = puffs.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn find(parent: &mut [usize], i: usize) -> usize {
        let mut r = i;
        while parent[r] != r {
            r = parent[r];
        }
        let mut i = i;
        while parent[i] != r {
            let next = parent[i];
            parent[i] = r;
            i = next;
        }
        r
    }
    for i in 0..n {
        for j in i + 1..n {
            let (a, b) = (&puffs[i], &puffs[j]);
            if (a.x - b.x).powi(2) + (a.y - b.y).powi(2) < (a.r + b.r).powi(2) {
                let (ra, rb) = (find(&mut parent, i), find(&mut parent, j));
                parent[ra] = rb;
            }
        }
    }
    let mut good = vec![false; n];
    for (i, p) in puffs.iter().enumerate() {
        if anchors.iter().any(|(x0, y0, x1, y1)| {
            let (cx, cy) = (0.5 * (x0 + x1), 0.5 * (y0 + y1));
            let (hx, hy) = (0.5 * edge * (x1 - x0), 0.5 * edge * (y1 - y0));
            (p.x - p.x.clamp(cx - hx, cx + hx)).powi(2) + (p.y - p.y.clamp(cy - hy, cy + hy)).powi(2) < p.r.powi(2)
        }) {
            let r = find(&mut parent, i);
            good[r] = true;
        }
    }
    let roots: Vec<usize> = (0..n).map(|i| find(&mut parent, i)).collect();
    puffs.into_iter().enumerate().filter(|(i, _)| good[roots[*i]]).map(|(_, p)| p).collect()
}

fn grid_color(values: &[[f32; 3]], grid: &Grid, x: f32, y: f32) -> [f32; 3] {
    let gx = (x / grid.step - 0.5).clamp(0.0, (grid.w - 1) as f32);
    let gy = (y / grid.step - 0.5).clamp(0.0, (grid.h - 1) as f32);
    let (x0, y0) = (gx as usize, gy as usize);
    let (x1, y1) = ((x0 + 1).min(grid.w - 1), (y0 + 1).min(grid.h - 1));
    let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
    let (a, b, c, d) = (values[y0 * grid.w + x0], values[y0 * grid.w + x1], values[y1 * grid.w + x0], values[y1 * grid.w + x1]);
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bottom = c[k] + (d[k] - c[k]) * tx;
        top + (bottom - top) * ty
    })
}

fn chamfer(values: &mut [f32], grid: &Grid) {
    let diag = grid.step * std::f32::consts::SQRT_2;
    let cells = grid.w * grid.h;
    for pass in 0..2 {
        let order: Vec<usize> = if pass == 0 { (0..cells).collect() } else { (0..cells).rev().collect() };
        for i in order {
            let (x, y) = ((i % grid.w) as i32, (i / grid.w) as i32);
            let sign = if pass == 0 { -1 } else { 1 };
            for (dx, dy, cost) in [(sign, 0, grid.step), (0, sign, grid.step), (sign, sign, diag), (-sign, sign, diag)] {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && (nx as usize) < grid.w && (ny as usize) < grid.h {
                    let v = values[ny as usize * grid.w + nx as usize] + cost;
                    if v < values[i] {
                        values[i] = v;
                    }
                }
            }
        }
    }
}

fn kernel(sigma: f32) -> (i32, Vec<f32>) {
    let r = (3.0 * sigma).ceil() as i32;
    let weights: Vec<f32> = (-r..=r).map(|d| (-(d * d) as f32 / (2.0 * sigma * sigma)).exp()).collect();
    let total: f32 = weights.iter().sum();
    (r, weights.iter().map(|v| v / total).collect())
}

fn blur(values: &mut [f32], w: usize, h: usize, sx: f32, sy: f32) {
    let ((rx, wx), (ry, wy)) = (kernel(sx), kernel(sy));
    let mut tmp = vec![0.0f32; values.len()];
    for y in 0..h {
        for x in 0..w {
            tmp[y * w + x] = (-rx..=rx).map(|d| wx[(d + rx) as usize] * values[y * w + (x as i32 + d).clamp(0, w as i32 - 1) as usize]).sum::<f32>();
        }
    }
    for y in 0..h {
        for x in 0..w {
            values[y * w + x] = (-ry..=ry).map(|d| wy[(d + ry) as usize] * tmp[(y as i32 + d).clamp(0, h as i32 - 1) as usize * w + x]).sum::<f32>();
        }
    }
}

fn study(out: &Path, name: &str, photo: &str, scene: Scene) {
    let path = layouts::photos().into_iter().find(|p| p.file_stem().unwrap() == photo).expect("photo");
    let image = scooch_core::open(&path).expect("open");
    let analysis: Analysis = scooch_core::analyze(&image);
    let on = Options { scenery: Scenery::Auto, ..Options::default() };
    let placement = scooch_core::solve(&analysis, &scene, &on, None);
    let plain = scooch_core::render_with(&image, &analysis, &placement, &scene, &Options { pads: 0.0, scenery: Scenery::Off, ..Options::default() });
    let bundle = scooch_core::bundle(&image, &analysis, &placement, &scene, &on);
    let (w, h) = (scene.width, scene.height);
    let unit = scene.icons.iter().map(|r| r.w.max(r.h)).fold(0.0f32, f32::max) * 1.1;
    let grid = Grid { w: (w as f32 / (unit / 8.0)).ceil() as usize, h: (h as f32 / (unit / 8.0)).ceil() as usize, step: unit / 8.0 };
    let cells = grid.w * grid.h;
    let picture: Vec<[f32; 3]> = plain.pixels().map(|p| [decode(p[0]), decode(p[1]), decode(p[2])]).collect();
    let mut mean = vec![[0.0f32; 3]; cells];
    let mut count = vec![0.0f32; cells];
    for (i, c) in picture.iter().enumerate() {
        let (x, y) = ((i as u32 % w) as f32, (i as u32 / w) as f32);
        let g = ((y / grid.step) as usize).min(grid.h - 1) * grid.w + ((x / grid.step) as usize).min(grid.w - 1);
        for k in 0..3 {
            mean[g][k] += c[k];
        }
        count[g] += 1.0;
    }
    for (m, n) in mean.iter_mut().zip(&count) {
        *m = m.map(|v| v / n.max(1.0));
    }
    let lab: Vec<[f32; 3]> = mean.iter().map(|c| to_lab(*c)).collect();
    let look = bundle.look.as_chunks::<4>().0;
    let (lw, lh) = (bundle.grid_width as usize, bundle.grid_height as usize);
    let quarter = |values: &dyn Fn(&[u8; 4]) -> f32, x: f32, y: f32| {
        let (qx, qy) = (((x / 4.0) as usize).min(lw - 1), ((y / 4.0) as usize).min(lh - 1));
        values(&look[qy * lw + qx])
    };
    let center = |i: usize| (((i % grid.w) as f32 + 0.5) * grid.step, ((i / grid.w) as f32 + 0.5) * grid.step);
    let mut sky = vec![0.0f32; cells];
    let mut queue: Vec<usize> = (0..cells).filter(|i| { let (x, y) = center(*i); quarter(&|p| p[0] as f32 / 255.0, x, y) >= 0.5 }).collect();
    for i in &queue {
        sky[*i] = 1.0;
    }
    while let Some(i) = queue.pop() {
        let (x, y) = (i % grid.w, i / grid.w);
        let near = [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (y + 1 < grid.h).then(|| i + grid.w)];
        for n in near.into_iter().flatten() {
            let jump = (0..3).map(|k| (lab[n][k] - lab[i][k]).powi(2)).sum::<f32>().sqrt();
            if sky[n] == 0.0 && jump < 0.012 {
                sky[n] = 1.0;
                queue.push(n);
            }
        }
    }
    blur(&mut sky, grid.w, grid.h, 1.5, 1.5);
    let belong: Vec<f32> = (0..cells).map(|i| { let (x, y) = center(i); quarter(&|p| p[1] as f32 / 255.0, x, y) }).collect();
    let (aw, ah) = (analysis.width as usize, analysis.height as usize);
    let far_px = 1e6f32;
    let mut subject: Vec<f32> = (0..cells)
        .map(|i| {
            let (x, y) = center(i);
            let (sx, sy) = (table_at(&placement.fx, x / w as f32), table_at(&placement.fy, y / h as f32));
            let (ax, ay) = (((sx * aw as f32) as usize).min(aw - 1), ((sy * ah as f32) as usize).min(ah - 1));
            if analysis.saliency[ay * aw + ax] >= 0.35 { 0.0 } else { far_px }
        })
        .collect();
    chamfer(&mut subject, &grid);
    let icons: Vec<(f32, f32, f32, f32, f32, f32)> = scene
        .icons
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let s = 0.8 + 0.7 * hash(k as i32, 7, 41);
            (r.x, r.y, r.x + r.w, r.y + r.h, 0.4 * unit * s, 0.22 * unit * (0.85 + 0.3 * hash(k as i32, 8, 41)))
        })
        .collect();
    let spread = |x: f32, y: f32| smoothstep(0.2, 0.8, 0.5 + 0.5 * noise(x / (3.0 * unit), y / (3.0 * unit), 33));
    let cover = |x: f32, y: f32| {
        let mut keep = 1.0f32;
        let reach = 0.4 + spread(x, y);
        for (x0, y0, x1, y1, sx, sy) in &icons {
            let (dx, dy) = ((x0 - x).max(x - x1).max(0.0), (y0 - y).max(y - y1).max(0.0));
            keep *= 1.0 - (-0.5 * (dx / (sx * reach)).powi(2) - 0.5 * (dy / sy).powi(2)).exp();
        }
        1.0 - keep
    };
    let anchors: Vec<(f32, f32, f32, f32)> = scene.icons.iter().map(|r| (r.x, r.y, r.x + r.w, r.y + r.h)).collect();
    let hold = |x: f32, y: f32| if anchors.iter().any(|(x0, y0, x1, y1)| x > x0 - 0.12 * unit && x < x1 + 0.12 * unit && y > y0 - 0.12 * unit && y < y1 + 0.12 * unit) { 0.3 } else { 0.0 };
    let (mut big, mut small, mut land) = (Vec::new(), Vec::new(), Vec::new());
    let spacing = unit / 3.0;
    let (nx, ny) = ((w as f32 / spacing).ceil() as i32 + 6, (h as f32 / spacing).ceil() as i32 + 6);
    for j in -3..ny {
        for i in -3..nx {
            let x = (i as f32 + 0.5 + 0.8 * (hash(i, j, 11) - 0.5)) * spacing;
            let y = (j as f32 + 0.5 + 0.8 * (hash(i, j, 12) - 0.5)) * spacing;
            let c = cover(x, y).max(hold(x, y));
            if c <= 0.01 {
                continue;
            }
            let (s, b, clear) = (grid.at(&sky, x, y), grid.at(&belong, x, y), grid.at(&subject, x, y) - 0.45 * unit);
            let gate = smoothstep(0.05, 0.4, b);
            let swell = 1.0 + 0.6 * noise(x / (2.5 * unit), y / (2.5 * unit), 31);
            let vg = cover(x, y + 0.35 * unit) - cover(x, y - 0.35 * unit);
            let (top, base) = (smoothstep(0.0, 0.2, vg), smoothstep(0.0, 0.2, -vg));
            let airy = smoothstep(0.3, 0.7, s) * gate;
            if hash(i, j, 13) < if hold(x, y) > 0.0 { 0.6 } else { 0.34 } {
                let r = ((0.48 + 0.34 * hash(i, j, 14)) * (1.0 + 0.35 * top) * unit * smoothstep(0.06, 0.3, c) * swell * airy * (0.6 + 0.4 * spread(x, y))).min(0.95 * unit);
                if r > 0.25 * unit && clear > 0.6 * r {
                    let r = r.min(clear);
                    let wide = (r * (1.0 + 0.8 * base)).min(clear).min(0.95 * unit);
                    big.push(Puff { x, y: y - (wide - r), r: wide });
                }
            } else {
                let r = ((0.15 + 0.2 * hash(i, j, 14)) * (1.0 - 0.4 * base) * unit * smoothstep(0.1, 0.3, c) * airy).min(clear);
                if r > 0.06 * unit {
                    small.push(Puff { x, y, r });
                }
            }
            if (i + 2 * j).rem_euclid(3) == 0 {
                let r = ((0.6 + 0.35 * hash(i, j, 15)) * unit * smoothstep(0.35, 0.75, c) * (1.0 - s) * gate * swell).min(clear);
                if r > 0.4 * unit {
                    land.push(Puff { x, y, r });
                }
            }
        }
    }
    let body = Buckets::new(keep_touching(big, &anchors, 1.0), unit, w, h);
    let inner = Buckets::new(small.into_iter().filter(|p| body.field(p.x, p.y, 0.1 * unit, 4.0 * unit, [0.0, 0.0]).0 < -p.r - 0.05 * unit).collect(), unit, w, h);
    let islands = Buckets::new(keep_touching(land, &anchors, 0.0), 2.2 * unit, w, h);
    let total = |x0: u32, x1: u32, y0: u32, y1: u32| {
        let mut s = 0.0;
        for y in (y0..y1).step_by(4) {
            for x in (x0..x1).step_by(4) {
                s += luminance(picture[(y * w + x) as usize]);
            }
        }
        s
    };
    let light = {
        let (lx, ly) = (total(w / 2, w, 0, h) - total(0, w / 2, 0, h), total(0, w, h / 2, h) - total(0, w, 0, h / 2));
        let n = (lx * lx + ly * ly).sqrt();
        let fallback = [-0.6f32, -0.8];
        let dir = if n > 1e-3 { [lx / n, ly / n] } else { fallback };
        let blended = [dir[0] * 0.4 + fallback[0] * 0.6, dir[1] * 0.4 + fallback[1] * 0.6];
        let m = (blended[0].powi(2) + blended[1].powi(2)).sqrt().max(1e-6);
        [blended[0] / m, blended[1] / m]
    };
    let palette: Vec<[f32; 3]> = analysis.palette.iter().filter(|c| c[3] >= 0.02).map(|c| [c[0], c[1], c[2]]).collect();
    let warm = palette.iter().copied().max_by(|a, b| (a[2] + 0.3 * a[1]).total_cmp(&(b[2] + 0.3 * b[1]))).unwrap_or([0.8, 0.01, 0.05]);
    let sand_lab = [0.2 * warm[0] + 0.8 * 0.87, 0.15 * warm[1] + 0.85 * 0.016, 0.15 * warm[2] + 0.85 * 0.064];
    let (k_cloud, k_land, far) = (0.1 * unit, 0.3 * unit, 4.0 * unit);
    let fine = Grid { w: (w as f32 / (unit / 12.0)).ceil() as usize, h: (h as f32 / (unit / 12.0)).ceil() as usize, step: unit / 12.0 };
    let solid: Vec<bool> = (0..fine.w * fine.h)
        .into_par_iter()
        .map(|i| body.field(((i % fine.w) as f32 + 0.5) * fine.step, ((i / fine.w) as f32 + 0.5) * fine.step, k_cloud, far, light).0 < 0.0)
        .collect();
    let mut open = vec![false; solid.len()];
    let mut queue: Vec<usize> = (0..solid.len()).filter(|i| !solid[*i] && (i % fine.w == 0 || i % fine.w == fine.w - 1 || i / fine.w == 0 || i / fine.w == fine.h - 1)).collect();
    for i in &queue {
        open[*i] = true;
    }
    while let Some(i) = queue.pop() {
        let (x, y) = (i % fine.w, i / fine.w);
        let near = [(x > 0).then(|| i - 1), (x + 1 < fine.w).then(|| i + 1), (y > 0).then(|| i - fine.w), (y + 1 < fine.h).then(|| i + fine.w)];
        for n in near.into_iter().flatten() {
            if !open[n] && !solid[n] {
                open[n] = true;
                queue.push(n);
            }
        }
    }
    let mut depths: Vec<f32> = open.iter().map(|o| if *o { 0.0 } else { 1e6 }).collect();
    chamfer(&mut depths, &fine);
    blur(&mut depths, fine.w, fine.h, 1.0, 1.0);
    let mut pixels = vec![[0u8; 3]; (w * h) as usize];
    let mut layer = vec![false; (w * h) as usize];
    pixels.par_chunks_mut(w as usize).zip(layer.par_chunks_mut(w as usize)).enumerate().for_each(|(y, (row, marks))| {
        for (x, px) in row.iter_mut().enumerate() {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = picture[y * w as usize + x];
            let (land, _) = islands.field(fx, fy, k_land, far, light);
            let land = land + 0.14 * unit * noise(fx / (1.5 * unit), fy / (1.5 * unit), 21);
            let reach = (0.6 + 0.4 * (0.5 + 0.5 * noise(fx / (0.9 * unit), fy / (0.9 * unit), 25))) * unit;
            if land < reach {
                marks[x] = true;
                if land > 0.0 {
                    let t = (1.0 - land / reach).clamp(0.0, 1.0);
                    let lab = to_lab(c);
                    let shallow = to_linear([(lab[0] + 0.08 + 0.16 * t).min(0.96), lab[1] * (1.25 + 0.4 * t), lab[2] * (1.25 + 0.4 * t)]);
                    c = mix(c, shallow, smoothstep(0.0, 0.4, t));
                    let along = 0.5 + 0.5 * noise(fx / (0.3 * unit), fy / (0.3 * unit), 24);
                    let strength = 0.5 + 0.5 * noise(fx / (0.8 * unit), fy / (0.8 * unit), 28);
                    let line = smoothstep(0.05 * unit, 0.01 * unit, land) * smoothstep(0.4, 0.7, along) * (0.35 + 0.5 * strength);
                    c = mix(c, to_linear([0.96, sand_lab[1] * 0.2, sand_lab[2] * 0.2]), line);
                }
                let inside = smoothstep(0.75, -0.75, land);
                if inside > 0.0 {
                    let depth = (-land / unit).max(0.0);
                    let mottle = 0.04 * noise(fx / (2.0 * unit), fy / (2.0 * unit), 22) + 0.015 * noise(fx / (0.5 * unit), fy / (0.5 * unit), 26);
                    let grain = 0.05 * (hash(x as i32, y as i32, 23) - 0.5) + 0.03 * (hash(x as i32 / 2, y as i32 / 2, 27) - 0.5);
                    let wet = smoothstep(0.35, 0.0, depth).powf(1.5);
                    let sand = to_linear([sand_lab[0] - 0.09 * wet + mottle + grain, sand_lab[1], sand_lab[2] * (1.0 + 0.15 * wet)]);
                    c = mix(c, sand, inside);
                }
            }
            let (d, arcs) = body.field(fx, fy, k_cloud, far, light);
            let d = if fine.at(&depths, fx, fy) > 0.15 * unit + 0.5 * fine.step { d.min(-1.0) } else { d };
            if d < 1.0 {
                marks[x] = true;
                let alpha = (0.5 - d).clamp(0.0, 1.0);
                let e = 1.5;
                let arcs = arcs.max(inner.field(fx, fy, k_cloud, far, light).1);
                let gx = body.field(fx + e, fy, k_cloud, far, light).0 - body.field(fx - e, fy, k_cloud, far, light).0;
                let gy = body.field(fx, fy + e, k_cloud, far, light).0 - body.field(fx, fy - e, k_cloud, far, light).0;
                let n = (gx * gx + gy * gy).sqrt().max(1e-6);
                let near = (gx * light[0] + gy * light[1]) / n;
                let s = fine.step;
                let (hx, hy) = (fine.at(&depths, fx - s, fy) - fine.at(&depths, fx + s, fy), fine.at(&depths, fx, fy - s) - fine.at(&depths, fx, fy + s));
                let m = (hx * hx + hy * hy).sqrt().max(1e-6);
                let depth = (-d).max(fine.at(&depths, fx, fy) - 0.5 * s);
                let facing = near + ((hx * light[0] + hy * light[1]) / m - near) * smoothstep(0.03 * unit, 0.2 * unit, depth);
                let sky_here = to_lab(grid_color(&mean, &grid, fx, fy));
                let chroma = (sky_here[1].powi(2) + sky_here[2].powi(2)).sqrt().max(1e-6);
                let hue = |l: f32, k: f32| to_linear([l, sky_here[1] / chroma * (chroma * k).min(0.08), sky_here[2] / chroma * (chroma * k).min(0.08)]);
                let (shaded, lit, deep) = (hue(0.82, 0.5), hue(0.965, 0.14), hue(0.76, 0.55));
                let band = smoothstep(0.38 * unit, 0.0, depth) * facing.max(0.0).powf(0.8);
                let under = smoothstep(0.15 * unit, 0.0, depth) * (-facing).max(0.0) * 0.9;
                let mut tone = mix(shaded, lit, 0.055 * arcs);
                tone = mix(tone, deep, under);
                tone = mix(tone, lit, band);
                c = mix(c, tone, alpha);
            }
            *px = [encode(c[0]), encode(c[1]), encode(c[2])];
        }
    });
    let mut still = RgbImage::new(w, h);
    for (i, p) in pixels.iter().enumerate() {
        still.put_pixel(i as u32 % w, i as u32 / w, Rgb(*p));
    }
    for r in &scene.icons {
        let s = r.w.min(r.h);
        let (ix, iy, isz) = (r.x + 0.2 * s, r.y + 0.08 * s, 0.6 * s);
        for y in iy as u32..(iy + isz) as u32 {
            for x in ix as u32..(ix + isz) as u32 {
                if x < w && y < h {
                    let p = still.get_pixel_mut(x, y);
                    *p = Rgb([p[0] / 4 + 180, p[1] / 4 + 180, p[2] / 4 + 186]);
                }
            }
        }
        let (lx, ly, lw, lh) = (r.x + 0.12 * s, r.y + 0.76 * s, 0.76 * s, (0.08 * s).max(3.0));
        for (dx, dy, v) in [(2.0, 2.0, 0u8), (0.0, 0.0, 255u8)] {
            for y in (ly + dy) as u32..(ly + dy + lh) as u32 {
                for x in (lx + dx) as u32..(lx + dx + lw) as u32 {
                    if x < w && y < h {
                        let p = still.get_pixel_mut(x, y);
                        *p = if v == 0 { Rgb([p[0] / 2, p[1] / 2, p[2] / 2]) } else { Rgb([255, 255, 255]) };
                    }
                }
            }
        }
    }
    still.save(out.join(format!("{name}.png"))).expect("still");
    let cell = 32u32;
    let (gw, gh) = (w.div_ceil(cell), h.div_ceil(cell));
    let mut on = vec![false; (gw * gh) as usize];
    for (i, m) in layer.iter().enumerate() {
        if *m {
            on[((i as u32 / w) / cell * gw + (i as u32 % w) / cell) as usize] = true;
        }
    }
    let mut seen = vec![false; on.len()];
    let mut k = 0;
    for start in 0..on.len() {
        if !on[start] || seen[start] {
            continue;
        }
        let (mut stack, mut b) = (vec![start], [u32::MAX, u32::MAX, 0, 0]);
        seen[start] = true;
        while let Some(i) = stack.pop() {
            let (x, y) = (i as u32 % gw, i as u32 / gw);
            b = [b[0].min(x), b[1].min(y), b[2].max(x + 1), b[3].max(y + 1)];
            for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                if nx >= 0 && ny >= 0 && (nx as u32) < gw && (ny as u32) < gh && on[(ny as u32 * gw + nx as u32) as usize] && !seen[(ny as u32 * gw + nx as u32) as usize] {
                    seen[(ny as u32 * gw + nx as u32) as usize] = true;
                    stack.push((ny as u32 * gw + nx as u32) as usize);
                }
            }
        }
        let (x0, y0) = (b[0] * cell, b[1] * cell);
        let (x1, y1) = ((b[2] * cell).min(w), (b[3] * cell).min(h));
        let (x1, y1) = (x1.min(x0 + 1400), y1.min(y0 + 1000));
        image::imageops::crop_imm(&still, x0, y0, x1 - x0, y1 - y0).to_image().save(out.join(format!("{name}-crop{k}.png"))).expect("crop");
        k += 1;
    }
    println!("{name}: {} cloud puffs, {} island blobs, {k} crops", body.puffs.len() + inner.puffs.len(), islands.puffs.len());
}

fn main() {
    let out = PathBuf::from(std::env::args().nth(1).expect("usage: study <out folder>"));
    std::fs::create_dir_all(&out).expect("out");
    let (w, h) = (2880u32, 1800u32);
    let column: Vec<Rect> = (0..16).map(|k| Rect { x: w as f32 - 120.0, y: 20.0 + 110.0 * k as f32, w: 100.0, h: 100.0 }).collect();
    let mut state = 2027u32;
    let mut next = || {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (state >> 8) as f32 / (1u32 << 24) as f32
    };
    let scattered: Vec<Rect> = (0..12).map(|_| Rect { x: 60.0 + next() * (w as f32 - 220.0), y: 60.0 + next() * (h as f32 - 220.0), w: 100.0, h: 100.0 }).collect();
    let site: Vec<Rect> = [(980.0, 90.0), (150.0, 710.0), (130.0, 540.0), (280.0, 600.0), (1030.0, 250.0), (1110.0, 140.0)].iter().map(|(x, y)| Rect { x: *x, y: *y, w: 150.0, h: 150.0 }).collect();
    let scenes = [
        ("balloon-scattered", "balloon-sky", w, h, &scattered),
        ("balloon-column", "balloon-sky", w, h, &column),
        ("moon-column", "moon-sky", w, h, &column),
        ("swans-column", "two-swans", w, h, &column),
        ("swans-scattered", "two-swans", w, h, &scattered),
        ("main-a", "balloon-sky", 1280, 960, &site),
    ];
    let only = std::env::args().nth(2);
    for (name, photo, sw, sh, icons) in scenes {
        if only.as_deref().is_some_and(|o| o != name) {
            continue;
        }
        study(&out, name, photo, Scene { width: sw, height: sh, icons: icons.clone() });
    }
}
