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

fn tone(hue: [f32; 3], lightness: f32, chroma: f32) -> [f32; 3] {
    let c = (hue[1] * hue[1] + hue[2] * hue[2]).sqrt().max(1e-6);
    let k = chroma.min(c) / c;
    to_linear([lightness, hue[1] * k, hue[2] * k])
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

    fn field(&self, x: f32, y: f32, k: f32, far: f32) -> (f32, f32) {
        let (mut d, mut arcs) = (far, 0.0f32);
        for p in self.near(x, y) {
            let e = ((x - p.x).powi(2) + (y - p.y).powi(2)).sqrt() - p.r;
            d = smin(d, e, k);
            if e < 0.0 {
                arcs = arcs.max(smoothstep(-0.22 * p.r, -0.02 * p.r, e));
            }
        }
        (d, arcs)
    }
}

struct Maps {
    width: u32,
    height: u32,
    gw: usize,
    gh: usize,
    sky: Vec<f32>,
    belong: Vec<f32>,
    cover: Vec<f32>,
    step: f32,
    cw: usize,
    ch: usize,
}

impl Maps {
    fn sample(values: &[f32], w: usize, h: usize, gx: f32, gy: f32) -> f32 {
        let (gx, gy) = (gx.clamp(0.0, (w - 1) as f32), gy.clamp(0.0, (h - 1) as f32));
        let (x0, y0) = (gx as usize, gy as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (tx, ty) = (gx - x0 as f32, gy - y0 as f32);
        let a = values[y0 * w + x0] + (values[y0 * w + x1] - values[y0 * w + x0]) * tx;
        let b = values[y1 * w + x0] + (values[y1 * w + x1] - values[y1 * w + x0]) * tx;
        a + (b - a) * ty
    }

    fn at(&self, values: &[f32], x: f32, y: f32) -> f32 {
        let x = x.clamp(0.0, self.width as f32 - 1.0);
        let y = y.clamp(0.0, self.height as f32 - 1.0);
        Self::sample(values, self.gw, self.gh, x / 4.0 - 0.5, y / 4.0 - 0.5)
    }

    fn cover(&self, x: f32, y: f32) -> f32 {
        Self::sample(&self.cover, self.cw, self.ch, x / self.step - 0.5, y / self.step - 0.5)
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
    let step = unit / 8.0;
    let (cw, ch) = ((w as f32 / step).ceil() as usize + 1, (h as f32 / step).ceil() as usize + 1);
    let mut cover = vec![0.0f32; cw * ch];
    for r in &scene.icons {
        for j in 0..ch {
            for i in 0..cw {
                let (x, y) = ((i as f32 + 0.5) * step, (j as f32 + 0.5) * step);
                if x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h {
                    cover[j * cw + i] = 1.0;
                }
            }
        }
    }
    blur(&mut cover, cw, ch, 0.7 * unit / step, 0.45 * unit / step);
    let look = bundle.look.as_chunks::<4>().0;
    let maps = Maps {
        width: w,
        height: h,
        gw: bundle.grid_width as usize,
        gh: bundle.grid_height as usize,
        sky: look.iter().map(|p| p[0] as f32 / 255.0).collect(),
        belong: look.iter().map(|p| p[1] as f32 / 255.0).collect(),
        cover,
        step,
        cw,
        ch,
    };
    let picture: Vec<[f32; 3]> = plain.pixels().map(|p| [decode(p[0]), decode(p[1]), decode(p[2])]).collect();
    let mean = |x0: u32, x1: u32, y0: u32, y1: u32| {
        let mut s = 0.0;
        for y in (y0..y1).step_by(4) {
            for x in (x0..x1).step_by(4) {
                s += luminance(picture[(y * w + x) as usize]);
            }
        }
        s
    };
    let light = {
        let (lx, ly) = (mean(w / 2, w, 0, h) - mean(0, w / 2, 0, h), mean(0, w, h / 2, h) - mean(0, w, 0, h / 2));
        let n = (lx * lx + ly * ly).sqrt();
        let fallback = [-0.6f32, -0.8];
        let dir = if n > 1e-3 { [lx / n, ly / n] } else { fallback };
        let blended = [dir[0] * 0.5 + fallback[0] * 0.5, dir[1] * 0.5 + fallback[1] * 0.5];
        let m = (blended[0].powi(2) + blended[1].powi(2)).sqrt().max(1e-6);
        [blended[0] / m, blended[1] / m]
    };
    let mut clouds = Vec::new();
    let mut small = Vec::new();
    let mut islands = Vec::new();
    let spacing = unit / 3.0;
    let (nx, ny) = ((w as f32 / spacing).ceil() as i32 + 6, (h as f32 / spacing).ceil() as i32 + 6);
    for j in -3..ny {
        for i in -3..nx {
            let x = (i as f32 + 0.5 + 0.8 * (hash(i, j, 11) - 0.5)) * spacing;
            let y = (j as f32 + 0.5 + 0.8 * (hash(i, j, 12) - 0.5)) * spacing;
            let cover = maps.cover(x, y);
            if cover <= 0.01 {
                continue;
            }
            let (sky, belong) = (maps.at(&maps.sky, x, y), maps.at(&maps.belong, x, y));
            let gate = smoothstep(0.05, 0.4, belong);
            let swell = 1.0 + 0.45 * noise(x / (2.2 * unit), y / (2.2 * unit), 31);
            let big = hash(i, j, 13) < 0.34;
            if big {
                let r = (0.5 + 0.3 * hash(i, j, 14)) * unit * smoothstep(0.03, 0.3, cover) * smoothstep(0.15, 0.6, sky) * gate * swell;
                if r > 0.22 * unit {
                    clouds.push(Puff { x, y, r });
                }
            } else {
                let r = (0.15 + 0.2 * hash(i, j, 14)) * unit * smoothstep(0.1, 0.3, cover) * smoothstep(0.15, 0.6, sky) * gate;
                if r > 0.05 * unit {
                    small.push(Puff { x, y, r });
                }
            }
            let r = (0.55 + 0.3 * hash(i, j, 15)) * unit * smoothstep(0.03, 0.3, cover) * smoothstep(0.4, 0.15, sky) * gate * swell;
            if r > 0.25 * unit && hash(i, j, 16) < 0.6 {
                islands.push(Puff { x, y, r });
            }
        }
    }
    let body = Buckets::new(clouds, unit, w, h);
    let mut clouds: Vec<Puff> = small.into_iter().filter(|p| body.field(p.x, p.y, 0.05 * unit, 4.0 * unit).0 < -0.02 * unit).collect();
    clouds.extend(body.puffs);
    let clouds = Buckets::new(clouds, unit, w, h);
    let islands = Buckets::new(islands, unit, w, h);
    let palette: Vec<[f32; 3]> = analysis.palette.iter().filter(|c| c[3] >= 0.02).map(|c| [c[0], c[1], c[2]]).collect();
    let sky_hue = palette.iter().copied().max_by(|a, b| (-a[2] - a[1] * 0.2).total_cmp(&(-b[2] - b[1] * 0.2))).unwrap_or([0.7, -0.01, -0.04]);
    let sand_hue = palette.iter().copied().max_by(|a, b| (a[2] + 0.3 * a[1] + 0.2 * a[0]).total_cmp(&(b[2] + 0.3 * b[1] + 0.2 * b[0]))).filter(|c| c[2] > 0.0).unwrap_or([0.75, 0.01, 0.05]);
    let cloud_body = tone(sky_hue, 0.765, 0.02);
    let cloud_rim = tone(sky_hue, 0.98, 0.01);
    let cloud_under = tone(sky_hue, 0.64, 0.03);
    let sand_hue = [sand_hue[0], 0.5 * sand_hue[1] + 0.5 * 0.012, 0.5 * sand_hue[2] + 0.5 * 0.055];
    let sand = tone(sand_hue, 0.75, 0.055);
    let wet = tone(sand_hue, 0.6, 0.05);
    let foam = tone(sand_hue, 0.97, 0.01);
    let (k_cloud, k_land, far) = (0.05 * unit, 0.3 * unit, 4.0 * unit);
    let mut pixels = vec![[0u8; 3]; (w * h) as usize];
    let mut layer = vec![false; (w * h) as usize];
    pixels.par_chunks_mut(w as usize).zip(layer.par_chunks_mut(w as usize)).enumerate().for_each(|(y, (row, marks))| {
        for (x, px) in row.iter_mut().enumerate() {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let mut c = picture[y * w as usize + x];
            let ground = c;
            let (land, _) = islands.field(fx, fy, k_land, far);
            let land = land + 0.14 * unit * noise(fx / (1.5 * unit), fy / (1.5 * unit), 21);
            if land < 0.9 * unit {
                marks[x] = true;
                let shallow = (1.0 - (land / (0.8 * unit)).clamp(0.0, 1.0)).powi(2);
                let lab = to_lab(ground);
                let bright = to_linear([(lab[0] + 0.18).min(0.95), lab[1] * 1.4, lab[2] * 1.4]);
                c = mix(c, bright, 0.85 * shallow);
                let inside = smoothstep(0.75, -0.75, land);
                let depth = -land / unit;
                let grain = 1.0 + 0.04 * noise(fx / (3.0 * unit), fy / (3.0 * unit), 22) + 0.025 * (hash(x as i32, y as i32, 23) - 0.5);
                let mut body = sand.map(|v| v * grain);
                body = mix(body, wet, smoothstep(0.16, 0.03, depth) * (1.0 - smoothstep(0.0, 0.025, -depth)));
                c = mix(c, body, inside);
                let broken = smoothstep(-0.45, 0.0, noise(fx / (0.45 * unit), fy / (0.45 * unit), 24));
                let line = (1.0 - ((land + 0.02 * unit) / (0.035 * unit)).abs()).clamp(0.0, 1.0);
                c = mix(c, foam, 0.85 * line * broken);
            }
            let shadow_at = (fx - light[0] * 0.28 * unit, fy - light[1] * 0.28 * unit);
            let (cast, _) = clouds.field(shadow_at.0, shadow_at.1, k_cloud, far);
            if cast < 0.2 * unit {
                c = c.map(|v| v * (1.0 - 0.22 * smoothstep(0.15 * unit, -0.1 * unit, cast)));
            }
            let (d, arcs) = clouds.field(fx, fy, k_cloud, far);
            if d < 1.0 {
                marks[x] = true;
                let alpha = (0.5 - d / 1.2).clamp(0.0, 1.0);
                let e = 1.5;
                let gx = clouds.field(fx + e, fy, k_cloud, far).0 - clouds.field(fx - e, fy, k_cloud, far).0;
                let gy = clouds.field(fx, fy + e, k_cloud, far).0 - clouds.field(fx, fy - e, k_cloud, far).0;
                let n = (gx * gx + gy * gy).sqrt().max(1e-6);
                let facing = -(gx * light[0] + gy * light[1]) / n;
                let depth = -d / unit;
                let band = smoothstep(0.15, 0.0, depth);
                let rim = band * (0.55 + 0.45 * facing.max(0.0));
                let under = band * (-facing).max(0.0) * 0.6;
                let mut body = mix(cloud_body, cloud_rim, 0.03 * arcs);
                body = mix(body, cloud_under, under);
                body = mix(body, cloud_rim, rim);
                c = mix(c, body, alpha);
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
    println!("{name}: {} cloud puffs, {} island blobs, {k} crops", clouds.puffs.len(), islands.puffs.len());
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
