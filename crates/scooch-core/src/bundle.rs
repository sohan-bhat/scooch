use crate::look::{self, Sampled, P};
use crate::pads::{self, Lookup};
use crate::paint::{self, Materials, BELLY, CORE, FOAM, LAND, LAYERS, LIT, RIM, SHORE, TOP};
use crate::render;
use crate::shape::{self, smoothstep, Grid, Q};
use crate::table::{self, Curve};
use crate::{color, scenery, Analysis, Options, Placement, RgbImage, RgbaImage, Scene};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const RAMP: usize = 512;
const RAMP_RANGE: (f32, f32) = (-0.8, 0.8);
const CANVAS_MOST: u32 = 8192;
const EDGE: f32 = 1.2;
const RIPPLES: [(f32, f32); 2] = [(0.16, 0.55), (0.34, 0.30)];
const RIPPLE_WIDTH: f32 = 0.018;
const RIPPLE_WHITE: f32 = 0.22;
const FOAM_BAND: f32 = 0.03;
const LAND_BAND: f32 = 0.13;
const TOP_BAND: f32 = 0.42;
const CLOUD_FEATHER: f32 = 0.05;
const SHADOW_SPREAD: f32 = 1.7;
const ISLAND_SHADOW: (f32, f32) = (0.10, 0.28);
const CLOUD_SHADOW: (f32, f32) = (0.16, 0.07);
const SHADOW_AT: [f32; 2] = [0.0, -0.10];
const RIM_AT: [f32; 2] = [-0.10, -0.10];
const BELLY_AT: [f32; 2] = [0.0, 0.14];
const GRAIN_SCALE: f32 = 0.10;
const GRAIN_FLOOR: f32 = 0.03;
const CORE_LIFT: f32 = 0.08;
const CORE_DROP: f32 = 0.15;
const VOLUME_LIT: f32 = 0.6;
const VOLUME_DARK: f32 = 0.5;
const REACH: f32 = 0.38;
const BEND: f32 = 0.002;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Constants {
    pub unit: f32,
    pub ramp_low: f32,
    pub ramp_high: f32,
    pub reach: f32,
    pub shadow: [f32; 2],
    pub rim: [f32; 2],
    pub belly: [f32; 2],
    pub grain: f32,
    pub grain_scale: f32,
    pub grain_floor: f32,
    pub core_lift: f32,
    pub core_drop: f32,
    pub volume_lit: f32,
    pub volume_dark: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Bundle {
    pub width: u32,
    pub height: u32,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub canvas: Vec<u8>,
    pub xmap: Vec<f64>,
    pub ymap: Vec<f64>,
    pub ratio: f64,
    pub grid_width: u32,
    pub grid_height: u32,
    pub field: Vec<f32>,
    pub look: Vec<u8>,
    pub pad: Vec<u8>,
    pub paint_width: u32,
    pub paint_height: u32,
    pub paints: Vec<u8>,
    pub ramps: Vec<u8>,
    pub constants: Constants,
}

impl Bundle {
    pub fn mirrored(&self) -> bool {
        self.xmap.len() > 1 && self.xmap[self.xmap.len() - 1] < self.xmap[0]
    }

    pub fn table_bytes(&self) -> usize {
        self.xmap.len() * 4 + self.ymap.len() * 4 + self.field.len() * 2 + self.look.len() + self.pad.len() + self.paints.len() + self.ramps.len()
    }

    fn grid(&self) -> Grid {
        Grid { w: self.grid_width as usize, h: self.grid_height as usize, cell: Q }
    }

    fn paint_grid(&self) -> Grid {
        Grid { w: self.paint_width as usize, h: self.paint_height as usize, cell: P }
    }
}

pub fn half(v: f32) -> f32 {
    if !v.is_finite() {
        return if v.is_nan() { 0.0 } else { v.signum() * 65504.0 };
    }
    let clamped = v.clamp(-65504.0, 65504.0);
    let bits = clamped.to_bits();
    let exponent = ((bits >> 23) & 0xff) as i32 - 127;
    if exponent < -14 {
        let step = 2.0f32.powi(-24);
        return (clamped / step).round() * step;
    }
    let drop = 13u32;
    let half_ulp = 1u32 << (drop - 1);
    let mantissa = bits & ((1 << drop) - 1);
    let mut kept = bits & !((1 << drop) - 1);
    if mantissa > half_ulp || (mantissa == half_ulp && (kept >> drop) & 1 == 1) {
        kept += 1 << drop;
    }
    f32::from_bits(kept)
}

fn cover(x: f32) -> f32 {
    smoothstep(EDGE / 2.0, -EDGE / 2.0, x)
}

fn ramps(u: f32) -> Vec<u8> {
    let mut out = vec![0u8; RAMP * 8];
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    for k in 0..RAMP {
        let t = RAMP_RANGE.0 + (RAMP_RANGE.1 - RAMP_RANGE.0) * k as f32 / (RAMP - 1) as f32;
        let d = t * u;
        let ripple: f32 = RIPPLES.iter().map(|(at, strength)| cover((d - at * u).abs() - RIPPLE_WIDTH * u / 2.0) * strength * RIPPLE_WHITE).sum();
        let shadow = |(sigma, strength): (f32, f32)| strength * smoothstep(SHADOW_SPREAD * sigma * u, -SHADOW_SPREAD * sigma * u, d);
        let first = [ripple, cover(d), (cover(d) - cover(d + FOAM_BAND * u)).max(0.0), cover(d + LAND_BAND * u)];
        let second = [cover(d + TOP_BAND * u), smoothstep(CLOUD_FEATHER * u, -CLOUD_FEATHER * u, d), shadow(ISLAND_SHADOW), shadow(CLOUD_SHADOW)];
        for c in 0..4 {
            out[k * 4 + c] = byte(first[c]);
            out[(RAMP + k) * 4 + c] = byte(second[c]);
        }
    }
    out
}

pub fn canvas(image: &RgbImage, width: u32, height: u32) -> Cow<'_, RgbImage> {
    let cap = (2 * width.max(height)).clamp(1, CANVAS_MOST);
    let long = image.width().max(image.height());
    if long <= cap {
        Cow::Borrowed(image)
    } else {
        Cow::Owned(render::shrink(image, long.div_ceil(cap)))
    }
}

fn sample(canvas: &RgbImage, decode: &[f32; 256], x: f64, y: f64) -> [f32; 3] {
    let (cw, ch) = (canvas.width() as i64, canvas.height() as i64);
    let (fx, fy) = (x.floor(), y.floor());
    let (tx, ty) = ((x - fx) as f32, (y - fy) as f32);
    let raw = canvas.as_raw();
    let at = |a: i64, b: i64| {
        let i = (b.clamp(0, ch - 1) * cw + a.clamp(0, cw - 1)) as usize * 3;
        [decode[raw[i] as usize], decode[raw[i + 1] as usize], decode[raw[i + 2] as usize]]
    };
    let (x0, y0) = (fx as i64, fy as i64);
    let (p, q, r, s) = (at(x0, y0), at(x0 + 1, y0), at(x0, y0 + 1), at(x0 + 1, y0 + 1));
    std::array::from_fn(|k| {
        let top = p[k] + (q[k] - p[k]) * tx;
        let bottom = r[k] + (s[k] - r[k]) * tx;
        top + (bottom - top) * ty
    })
}

fn small_picture(canvas: &RgbImage, xmap: &[f64], ymap: &[f64], grid: &Grid) -> Vec<[f32; 3]> {
    let decode = color::decode_table();
    let (cw, ch) = (canvas.width() as f64, canvas.height() as f64);
    let at = |map: &[f64], cell: usize, size: f64| {
        let (a, b) = ((cell * Q + Q / 2 - 1).min(map.len() - 1), (cell * Q + Q / 2).min(map.len() - 1));
        (map[a] + map[b]) * 0.5 * size - 0.5
    };
    let columns: Vec<f64> = (0..grid.w).map(|i| at(xmap, i, cw)).collect();
    let rows: Vec<f64> = (0..grid.h).map(|j| at(ymap, j, ch)).collect();
    (0..grid.w * grid.h).into_par_iter().map(|c| sample(canvas, decode, columns[c % grid.w], rows[c / grid.w])).collect()
}

pub fn make(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> Bundle {
    let (mut out, canvas) = still(image, found, placement, scene, options, materials);
    out.canvas = canvas.into_owned().into_raw();
    out
}

pub fn still<'a>(image: &'a RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> (Bundle, Cow<'a, RgbImage>) {
    let (width, height) = (scene.width, scene.height);
    let (sw, sh) = image.dimensions();
    let placement: Cow<Placement> = if table::usable(&placement.fx) && table::usable(&placement.fy) { Cow::Borrowed(placement) } else { Cow::Owned(table::fill(sw.max(1), sh.max(1), width.max(1), height.max(1))) };
    let canvas = canvas(image, width, height);
    let (cw, ch) = canvas.dimensions();
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let xmap: Vec<f64> = (0..width).map(|i| xc.at((i as f64 + 0.5) / width as f64)).collect();
    let ymap: Vec<f64> = (0..height).map(|j| yc.at((j as f64 + 0.5) / height as f64)).collect();
    let ratio = render::worst_ratio(&placement.fx, cw, width).max(render::worst_ratio(&placement.fy, ch, height));
    let grid = Grid::of(width as usize, height as usize, Q);
    let mut out = Bundle {
        width,
        height,
        canvas_width: cw,
        canvas_height: ch,
        canvas: Vec::new(),
        xmap,
        ymap,
        ratio,
        grid_width: grid.w as u32,
        grid_height: grid.h as u32,
        field: Vec::new(),
        look: Vec::new(),
        pad: Vec::new(),
        paint_width: 0,
        paint_height: 0,
        paints: Vec::new(),
        ramps: Vec::new(),
        constants: Constants {
            unit: 1.0,
            ramp_low: RAMP_RANGE.0,
            ramp_high: RAMP_RANGE.1,
            reach: REACH,
            shadow: SHADOW_AT,
            rim: RIM_AT,
            belly: BELLY_AT,
            grain: materials.grain,
            grain_scale: GRAIN_SCALE,
            grain_floor: GRAIN_FLOOR,
            core_lift: CORE_LIFT,
            core_drop: CORE_DROP,
            volume_lit: VOLUME_LIT,
            volume_dark: VOLUME_DARK,
        },
    };
    if width == 0 || height == 0 || cw == 0 || ch == 0 {
        return (out, canvas);
    }
    let small = small_picture(&canvas, &out.xmap, &out.ymap, &grid);
    let (aw, ah) = (found.width as usize, found.height as usize);
    let usable = aw > 0 && ah > 0 && found.saliency.len() == aw * ah && found.busy.len() == aw * ah;
    let lookup = Lookup {
        ax: (0..grid.w).map(|i| (out.xmap[(i * Q + Q / 2).min(width as usize - 1)] * aw as f64 - 0.5) as f32).collect(),
        ay: (0..grid.h).map(|j| (out.ymap[(j * Q + Q / 2).min(height as usize - 1)] * ah as f64 - 0.5) as f32).collect(),
        aw: aw.max(1),
        ah: ah.max(1),
    };
    let clean = |v: &f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let (saliency, busy) = if usable {
        let s: Vec<f32> = if found.flat { vec![0.0; aw * ah] } else { found.saliency.iter().map(clean).collect() };
        let b: Vec<f32> = found.busy.iter().map(clean).collect();
        (lookup.plane(&s, &grid), lookup.plane(&b, &grid))
    } else {
        (vec![0.0; grid.w * grid.h], vec![0.0; grid.w * grid.h])
    };
    let base = scenery::active(scene, options).then(|| shape::base(scene, width as usize, height as usize)).flatten();
    let scenic = match &base {
        Some(base) => look::build(base, &Sampled { grid, small: &small, saliency: &saliency, busy: &busy }, options, materials, width as usize, height as usize),
        None => look::none(grid, width as usize, height as usize),
    };
    let unit = base.as_ref().map_or(1.0, |b| b.u);
    let padded = usable && (base.is_some() || pads::active(found, &placement, scene, options));
    let pad = if padded { pads::maps(&small, &grid, found, &lookup, scene, options, base.is_some().then_some(&scenic.belong[..])) } else { None };
    out.field = scenic.field.iter().map(|v| half(*v)).collect();
    out.look = scenic.look;
    out.pad = pad.unwrap_or_else(|| vec![0; grid.w * grid.h * 4]);
    out.paint_width = scenic.paint_grid.w as u32;
    out.paint_height = scenic.paint_grid.h as u32;
    out.paints = scenic.paints;
    out.ramps = ramps(unit);
    out.constants.unit = unit;
    (out, canvas)
}

pub fn compatible(a: &Bundle, b: &Bundle) -> bool {
    a.width == b.width
        && a.height == b.height
        && a.canvas_width == b.canvas_width
        && a.canvas_height == b.canvas_height
        && a.grid_width == b.grid_width
        && a.grid_height == b.grid_height
        && a.paint_width == b.paint_width
        && a.paint_height == b.paint_height
        && a.mirrored() == b.mirrored()
        && a.canvas == b.canvas
}

#[derive(Clone, Copy)]
struct Probe {
    a: usize,
    b: usize,
    t: f32,
}

fn probes(count: usize, cell: usize, cells: usize) -> Vec<Probe> {
    (0..count)
        .map(|x| {
            let g = ((x as f32 + 0.5) / cell as f32 - 0.5).clamp(0.0, (cells - 1) as f32);
            let a = g as usize;
            Probe { a, b: (a + 1).min(cells - 1), t: g - a as f32 }
        })
        .collect()
}

#[inline(always)]
fn bilinear4(values: &[[f32; 4]], w: usize, column: &Probe, row: &Probe) -> [f32; 4] {
    let (p, q, r, s) = (&values[row.a * w + column.a], &values[row.a * w + column.b], &values[row.b * w + column.a], &values[row.b * w + column.b]);
    std::array::from_fn(|k| {
        let upper = p[k] + (q[k] - p[k]) * column.t;
        let lower = r[k] + (s[k] - r[k]) * column.t;
        upper + (lower - upper) * row.t
    })
}

#[inline(always)]
fn lerp4(line: &[[f32; 4]], probe: &Probe) -> [f32; 4] {
    let (p, q) = (&line[probe.a], &line[probe.b]);
    std::array::from_fn(|k| p[k] + (q[k] - p[k]) * probe.t)
}

fn line4(values: &[[f32; 4]], w: usize, row: &Probe) -> Vec<[f32; 4]> {
    let (top, bottom) = (&values[row.a * w..(row.a + 1) * w], &values[row.b * w..(row.b + 1) * w]);
    top.iter().zip(bottom).map(|(p, q)| std::array::from_fn(|k| p[k] + (q[k] - p[k]) * row.t)).collect()
}

fn line1(values: &[f32], w: usize, row: &Probe) -> Vec<f32> {
    let (top, bottom) = (&values[row.a * w..(row.a + 1) * w], &values[row.b * w..(row.b + 1) * w]);
    top.iter().zip(bottom).map(|(p, q)| p + (q - p) * row.t).collect()
}

struct Plane {
    look: Vec<[f32; 4]>,
    pad: Vec<[f32; 4]>,
    paints: Vec<Vec<[f32; 4]>>,
    ramps: Vec<[f32; 8]>,
}

fn plane(x: &Bundle, decode: &[f32; 256]) -> Plane {
    let unit = |p: &[u8; 4]| p.map(|v| v as f32 / 255.0);
    let linear = |p: &[u8; 4]| [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize], p[3] as f32 / 255.0];
    let size = x.paint_grid().w * x.paint_grid().h * 4;
    Plane {
        look: x.look.as_chunks::<4>().0.par_iter().map(unit).collect(),
        pad: x.pad.as_chunks::<4>().0.par_iter().map(linear).collect(),
        paints: (0..LAYERS).map(|k| x.paints[k * size..(k + 1) * size].as_chunks::<4>().0.iter().map(linear).collect()).collect(),
        ramps: (0..RAMP).map(|k| std::array::from_fn(|c| x.ramps[((c / 4) * RAMP + k) * 4 + c % 4] as f32 / 255.0)).collect(),
    }
}

#[derive(Clone, Copy)]
struct Ramp<'a> {
    p: &'a [f32; 8],
    q: &'a [f32; 8],
    t: f32,
}

impl Ramp<'_> {
    #[inline(always)]
    fn at(&self, k: usize) -> f32 {
        self.p[k] + (self.q[k] - self.p[k]) * self.t
    }
}

#[inline(always)]
fn ramp<'a>(table: &'a [[f32; 8]], value: f32, c: &Constants) -> Ramp<'a> {
    let f = ((value - c.ramp_low) / (c.ramp_high - c.ramp_low) * (RAMP - 1) as f32).clamp(0.0, (RAMP - 1) as f32);
    let a = (f as usize).min(RAMP - 1);
    Ramp { p: &table[a], q: &table[(a + 1).min(RAMP - 1)], t: f - a as f32 }
}

#[derive(Clone, Copy, Default)]
struct Paint {
    rgb: [f32; 3],
    a: f32,
}

impl Paint {
    #[inline(always)]
    fn over(&mut self, color: [f32; 3], alpha: f32) {
        if alpha <= 0.0 {
            return;
        }
        let alpha = alpha.min(1.0);
        for (mine, theirs) in self.rgb.iter_mut().zip(color) {
            *mine = theirs * alpha + *mine * (1.0 - alpha);
        }
        self.a = alpha + self.a * (1.0 - alpha);
    }

    fn scaled(self, k: f32) -> Self {
        Self { rgb: self.rgb.map(|v| v * k), a: self.a * k }
    }

    fn plus(self, other: Self) -> Self {
        Self { rgb: std::array::from_fn(|i| self.rgb[i] + other.rgb[i]), a: self.a + other.a }
    }
}

fn mix3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    std::array::from_fn(|k| a[k] + (b[k] - a[k]) * t)
}

fn rgb(c: [f32; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

struct Reads<F: Fn([f32; 2]) -> f32> {
    d: f32,
    at: F,
    shadow: Option<f32>,
    rim: Option<f32>,
    belly: Option<f32>,
}

impl<F: Fn([f32; 2]) -> f32> Reads<F> {
    fn get(&mut self, which: usize, offset: [f32; 2]) -> f32 {
        let slot = match which {
            0 => &mut self.shadow,
            1 => &mut self.rim,
            _ => &mut self.belly,
        };
        *slot.get_or_insert_with(|| (self.at)(offset))
    }
}

#[inline(always)]
fn paint<F: Fn([f32; 2]) -> f32, G: Fn() -> f32, L: Fn(usize) -> [f32; 4]>(c: &Constants, ramps: &[[f32; 8]], reads: &mut Reads<F>, look: [f32; 4], layer: L, grain: &mut Grain<G>) -> Paint {
    let [sky, belong, open, light] = look;
    if belong <= 0.0 {
        return Paint::default();
    }
    let light = light * 2.0 - 1.0;
    let d = reads.d;
    let here = ramp(ramps, d, c);
    let outside = d > EDGE / c.unit;
    let (land_shadow, cloud_shadow) = if d > -EDGE / c.unit {
        let shade = ramp(ramps, reads.get(0, c.shadow), c);
        (shade.at(6), shade.at(7))
    } else {
        (0.0, 0.0)
    };
    let mut land = Paint::default();
    if sky < 1.0 && !(outside && here.at(0) <= 0.0 && land_shadow <= 0.0) {
        let fill = layer(LAND);
        if outside {
            land.over([1.0; 3], here.at(0) * layer(SHORE)[3]);
            land.over([0.0; 3], land_shadow);
        } else if here.at(4) >= 1.0 {
            land.over(grain.apply(layer(TOP)), 1.0);
        } else {
            let shore = layer(SHORE);
            land.over([1.0; 3], here.at(0) * shore[3]);
            land.over([0.0; 3], land_shadow);
            land.over(grain.apply(shore), here.at(1));
            if here.at(2) > 0.0 {
                land.over(grain.apply(layer(FOAM)), here.at(2));
            }
            if here.at(3) > 0.0 {
                land.over(grain.apply(fill), here.at(3));
            }
            if here.at(4) > 0.0 {
                land.over(grain.apply(layer(TOP)), here.at(4));
            }
        }
        land = land.scaled(fill[3]);
    }
    let mut cloud = Paint::default();
    if sky > 0.0 && !(here.at(5) <= 0.0 && cloud_shadow <= 0.0) {
        let core = layer(CORE);
        cloud.over([0.0; 3], cloud_shadow);
        if here.at(5) > 0.0 {
            let (rim, belly) = (rgb(layer(RIM)), rgb(layer(BELLY)));
            let mut tone = mix3(rgb(core), rgb(layer(LIT)), open);
            if light > 0.0 {
                tone = mix3(tone, rim, light * (c.core_lift + (c.volume_lit - c.core_lift) * open));
            } else {
                tone = mix3(tone, belly, -light * (c.core_drop + (c.volume_dark - c.core_drop) * open));
            }
            if d > -(c.rim[0].abs() + c.rim[1].abs()) - EDGE / c.unit {
                tone = mix3(tone, rim, 1.0 - ramp(ramps, reads.get(1, c.rim), c).at(1));
            }
            if d > -c.belly[1].abs() - EDGE / c.unit {
                tone = mix3(tone, belly, 1.0 - ramp(ramps, reads.get(2, c.belly), c).at(1));
            }
            cloud.over(grain.apply([tone[0], tone[1], tone[2], 1.0]), here.at(5));
        }
        cloud = cloud.scaled(core[3]);
    }
    land.scaled(1.0 - sky).plus(cloud.scaled(sky)).scaled(belong)
}

struct Grain<G: Fn() -> f32> {
    factor: Option<f32>,
    make: G,
}

impl<G: Fn() -> f32> Grain<G> {
    fn apply(&mut self, color: [f32; 4]) -> [f32; 3] {
        let k = *self.factor.get_or_insert_with(|| (self.make)());
        [(color[0] * k).min(1.0), (color[1] * k).min(1.0), (color[2] * k).min(1.0)]
    }
}

fn crossfade(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    let (first, second) = (frame(a, a, 0.0), frame(b, b, 0.0));
    if first.dimensions() != second.dimensions() {
        return if s < 0.5 { first } else { second };
    }
    let decode = color::decode_table();
    let encode = color::encode_table();
    let mut out = first;
    let below: &mut [u8] = &mut out;
    below.par_iter_mut().zip(second.as_raw().par_iter()).for_each(|(p, q)| {
        let (x, y) = (decode[*p as usize], decode[*q as usize]);
        *p = color::encode(encode, x + (y - x) * s);
    });
    out
}

pub fn frame(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    if !compatible(a, b) {
        return crossfade(a, b, s.clamp(0.0, 1.0));
    }
    let (mut picture, layer) = layers(a, b, s);
    paint::composite(&mut picture, &layer);
    picture
}

pub fn layers(a: &Bundle, b: &Bundle, s: f32) -> (RgbImage, RgbaImage) {
    draw(a, b, s, &a.canvas)
}

fn dilate(cells: &[bool], grid: &Grid) -> Vec<bool> {
    let mut across = vec![false; cells.len()];
    across.par_chunks_mut(grid.w).zip(cells.par_chunks(grid.w)).for_each(|(out, row)| {
        for (i, o) in out.iter_mut().enumerate() {
            *o = row[i.saturating_sub(1)] || row[i] || row[(i + 1).min(grid.w - 1)];
        }
    });
    let mut out = vec![false; cells.len()];
    out.par_chunks_mut(grid.w).enumerate().for_each(|(j, line)| {
        let (up, here, down) = (j.saturating_sub(1), j, (j + 1).min(grid.h - 1));
        for (i, o) in line.iter_mut().enumerate() {
            *o = across[up * grid.w + i] || across[here * grid.w + i] || across[down * grid.w + i];
        }
    });
    out
}

pub fn draw(a: &Bundle, b: &Bundle, s: f32, canvas: &[u8]) -> (RgbImage, RgbaImage) {
    let s = if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 };
    let (a, b, s) = if !compatible(a, b) {
        if s < 0.5 {
            (a, a, 0.0)
        } else {
            (b, b, 0.0)
        }
    } else if s <= 0.0 {
        (a, a, 0.0)
    } else if s >= 1.0 {
        (b, b, 0.0)
    } else {
        (a, b, s)
    };
    let (width, height) = (a.width, a.height);
    if width == 0 || height == 0 || a.canvas_width == 0 || a.canvas_height == 0 || canvas.len() != (a.canvas_width * a.canvas_height * 3) as usize {
        return (RgbImage::new(width, height), RgbaImage::new(width, height));
    }
    let same = std::ptr::eq(a, b) || s == 0.0;
    let (xmap, ymap): (Cow<[f64]>, Cow<[f64]>) = if same {
        (Cow::Borrowed(&a.xmap), Cow::Borrowed(&a.ymap))
    } else {
        let blend = |p: &[f64], q: &[f64]| p.iter().zip(q).map(|(x, y)| x + (y - x) * s as f64).collect::<Vec<f64>>();
        (Cow::Owned(blend(&a.xmap, &b.xmap)), Cow::Owned(blend(&a.ymap, &b.ymap)))
    };
    let ratio = if same { a.ratio } else { a.ratio.max(b.ratio) };
    let mut picture = render::picture((canvas, a.canvas_width, a.canvas_height), (&xmap, &ymap), ratio, (width, height));
    let mut layer = RgbaImage::new(width, height);
    let decode = color::decode_table();
    let encode = color::encode_table();
    let pad_on = |x: &Bundle| x.pad.as_chunks::<4>().0.iter().any(|p| p[3] > 0);
    let look_on = |x: &Bundle| x.look.as_chunks::<4>().0.iter().any(|p| p[1] > 0);
    let wanted = |x: &Bundle| pad_on(x) || look_on(x);
    if !(wanted(a) || (!same && wanted(b))) {
        return (picture, layer);
    }
    let grid = a.grid();
    let (w, h) = (width as usize, height as usize);
    let columns = probes(w, Q, grid.w);
    let rows = probes(h, Q, grid.h);
    let planes: Vec<Plane> = if same { vec![plane(a, decode)] } else { vec![plane(a, decode), plane(b, decode)] };
    let shares: Vec<(f32, &Bundle)> = if same { vec![(1.0, a)] } else { vec![(1.0 - s, a), (s, b)] };
    let padded = dilate(&(0..grid.w * grid.h).map(|c| planes.iter().any(|p| p.pad[c][3] > 0.0)).collect::<Vec<bool>>(), &grid);
    if padded.iter().any(|p| *p) {
        let buffer: &mut [u8] = &mut picture;
        buffer.par_chunks_mut(w * 3).enumerate().for_each(|(y, line)| {
            let row = &rows[y];
            if !padded[row.a * grid.w..(row.a + 1) * grid.w].iter().any(|p| *p) {
                return;
            }
            for (x, px) in line.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let column = &columns[x];
                if !padded[row.a * grid.w + column.a] {
                    continue;
                }
                let (mut weight, mut calm) = (0.0f32, [0.0f32; 3]);
                for (plane, (share, _)) in planes.iter().zip(&shares) {
                    let here = bilinear4(&plane.pad, grid.w, column, row);
                    let k = here[3] * share;
                    weight += k;
                    for c in 0..3 {
                        calm[c] += here[c] * k;
                    }
                }
                if weight <= 0.0 {
                    continue;
                }
                for k in 0..3 {
                    let hard = decode[px[k] as usize];
                    px[k] = color::encode(encode, hard + (calm[k] / weight - hard) * weight);
                }
            }
        });
    }
    let c = a.constants;
    let fields: Vec<&[f32]> = shares.iter().map(|(_, x)| &x.field[..]).collect();
    let active = dilate(&(0..grid.w * grid.h).map(|cell| planes.iter().zip(&fields).any(|(p, f)| p.look[cell][1] > 0.0 && f[cell] < c.reach)).collect::<Vec<bool>>(), &grid);
    if !active.iter().any(|a| *a) {
        return (picture, layer);
    }
    let raw = picture.as_raw();
    let block: Vec<[f32; 2]> = (0..grid.w * grid.h)
        .into_par_iter()
        .map(|cell| {
            if !active[cell] {
                return [0.0, 1.0];
            }
            let (i, j) = (cell % grid.w, cell / grid.w);
            let (mut sum, mut count) = (0.0f32, 0.0f32);
            for y in j * Q..((j + 1) * Q).min(h) {
                for p in raw[(y * w + i * Q) * 3..(y * w + ((i + 1) * Q).min(w)) * 3].as_chunks::<3>().0 {
                    sum += 0.2126 * decode[p[0] as usize] + 0.7152 * decode[p[1] as usize] + 0.0722 * decode[p[2] as usize];
                    count += 1.0;
                }
            }
            let mean = sum / count.max(1.0);
            [mean, 1.0 / (3.0 * mean.cbrt().powi(2)).max(c.grain_floor)]
        })
        .collect();
    let means: Vec<f32> = block.iter().map(|b| b[0]).collect();
    let paint_grid = a.paint_grid();
    let tint_columns = probes(w, P, paint_grid.w);
    let tint_rows = probes(h, P, paint_grid.h);
    let u = c.unit;
    let deep_land = -(TOP_BAND + 2.0 * EDGE / u + 0.01);
    let deep_cloud = -((c.rim[0].abs() + c.rim[1].abs()).max(c.belly[1].abs()) + 2.0 * EDGE / u + 0.01);
    let land_band = (-(TOP_BAND - 2.0 * EDGE / u - 0.01), -(LAND_BAND + 2.0 * EDGE / u + 0.01));
    let core: Vec<u8> = if same {
        (0..grid.w * grid.h)
            .into_par_iter()
            .map(|cell| match (planes[0].look[cell], fields[0][cell]) {
                (look, f) if look[0] <= 0.0 && f <= deep_land => 1,
                (look, f) if look[0] <= 0.0 && f >= land_band.0 && f <= land_band.1 => 4,
                (look, f) if look[0] >= 1.0 && f <= deep_cloud => 2 + (look[3] < 0.5) as u8,
                _ => 0,
            })
            .collect()
    } else {
        Vec::new()
    };
    let cores: Vec<[f32; 4]> = (0..core.len())
        .into_par_iter()
        .map(|cell| {
            if core[cell] == 0 {
                return [0.0; 4];
            }
            let plane = &planes[0];
            let at = |n: usize, cells: usize| {
                let g = (((n * Q) as f32 + Q as f32 / 2.0) / P as f32 - 0.5).clamp(0.0, (cells - 1) as f32);
                let a = g as usize;
                Probe { a, b: (a + 1).min(cells - 1), t: g - a as f32 }
            };
            let (tx, ty) = (at(cell % grid.w, paint_grid.w), at(cell / grid.w, paint_grid.h));
            let layer = |k: usize| bilinear4(&plane.paints[k], paint_grid.w, &tx, &ty);
            let [_, _, open, light] = plane.look[cell];
            if core[cell] == 1 || core[cell] == 4 {
                let top = layer(if core[cell] == 1 { TOP } else { LAND });
                return [top[0], top[1], top[2], 0.0];
            }
            let light = light * 2.0 - 1.0;
            let (base, rim, belly) = (layer(CORE), rgb(layer(RIM)), rgb(layer(BELLY)));
            let mut tone = mix3(rgb(base), rgb(layer(LIT)), open);
            if light > 0.0 {
                tone = mix3(tone, rim, light * (c.core_lift + (c.volume_lit - c.core_lift) * open));
            } else {
                tone = mix3(tone, belly, -light * (c.core_drop + (c.volume_dark - c.core_drop) * open));
            }
            [tone[0], tone[1], tone[2], 0.0]
        })
        .collect();
    let core: Vec<u8> = (0..core.len())
        .into_par_iter()
        .map(|cell| {
            let kind = core[cell];
            if kind == 0 {
                return 0;
            }
            let (i, j) = (cell % grid.w, cell / grid.w);
            if i == 0 || j == 0 || i + 1 == grid.w || j + 1 == grid.h {
                return 0;
            }
            let bend = |a: usize, b: usize| core[a] == kind && core[b] == kind && (0..3).all(|k| (2.0 * cores[cell][k] - cores[a][k] - cores[b][k]).abs() <= BEND);
            if bend(cell - 1, cell + 1) && bend(cell - grid.w, cell + grid.w) {
                kind
            } else {
                0
            }
        })
        .collect();
    let out: &mut [u8] = &mut layer;
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, line)| {
        let row = &rows[y];
        let j = row.a;
        if !active[j * grid.w..(j + 1) * grid.w].iter().any(|a| *a) {
            return;
        }
        let tint_row = &tint_rows[y];
        let py = y as f32 + 0.5;
        let field_lines: Vec<Vec<f32>> = fields.iter().map(|f| line1(f, grid.w, row)).collect();
        let look_lines: Vec<Vec<[f32; 4]>> = planes.iter().map(|p| line4(&p.look, grid.w, row)).collect();
        let tint_lines: Vec<Vec<Vec<[f32; 4]>>> = planes.iter().map(|p| p.paints.iter().map(|layer| line4(layer, paint_grid.w, tint_row)).collect()).collect();
        let core_line = if core.is_empty() { Vec::new() } else { line4(&cores, grid.w, row) };
        let means_line = line1(&means, grid.w, row);
        for (x, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let column = &columns[x];
            if !active[j * grid.w + column.a] {
                continue;
            }
            let tint_column = &tint_columns[x];
            let grain_at = || {
                let p = &raw[(y * w + x) * 3..(y * w + x) * 3 + 3];
                let lum = 0.2126 * decode[p[0] as usize] + 0.7152 * decode[p[1] as usize] + 0.0722 * decode[p[2] as usize];
                let mean = means_line[column.a] + (means_line[column.b] - means_line[column.a]) * column.t;
                let detail = (lum - mean) * block[j * grid.w + column.a][1];
                (1.0 + (detail / c.grain_scale).clamp(-1.0, 1.0) * c.grain).powi(3)
            };
            if !core.is_empty() {
                let kind = core[row.a * grid.w + column.a];
                if kind != 0 && core[row.a * grid.w + column.b] == kind && core[row.b * grid.w + column.a] == kind && core[row.b * grid.w + column.b] == kind {
                    let v = lerp4(&core_line, column);
                    let opacity = lerp4(&tint_lines[0][if kind == 1 || kind == 4 { LAND } else { CORE }], tint_column)[3];
                    let alpha = opacity * lerp4(&look_lines[0], column)[1];
                    if alpha <= 1e-4 {
                        continue;
                    }
                    let grain = grain_at();
                    for k in 0..3 {
                        px[k] = color::encode(encode, (v[k] * grain).min(1.0));
                    }
                    px[3] = (alpha.min(1.0) * 255.0).round() as u8;
                    continue;
                }
            }
            let d = field_lines.iter().zip(&shares).map(|(f, (share, _))| (f[column.a] + (f[column.b] - f[column.a]) * column.t) * share).sum::<f32>();
            if d > c.reach {
                continue;
            }
            let fx = x as f32 + 0.5;
            let offset = |o: [f32; 2]| {
                let (gx, gy) = (grid.at(fx + o[0] * u), grid.at(py + o[1] * u));
                fields.iter().zip(&shares).map(|(f, (share, _))| grid.linear(f, gx, gy) * share).sum::<f32>()
            };
            let mut reads = Reads { d, at: offset, shadow: None, rim: None, belly: None };
            let mut grain = Grain { factor: None, make: grain_at };
            let mut total = Paint::default();
            for (k, (plane, (share, x))) in planes.iter().zip(&shares).enumerate() {
                let look = lerp4(&look_lines[k], column);
                let tints = &tint_lines[k];
                let part = paint(&x.constants, &plane.ramps, &mut reads, look, |layer| lerp4(&tints[layer], tint_column), &mut grain);
                total = total.plus(part.scaled(*share));
            }
            if total.a <= 1e-4 {
                continue;
            }
            let inverse = 1.0 / total.a;
            for (slot, value) in px.iter_mut().zip(total.rgb) {
                *slot = color::encode(encode, value * inverse);
            }
            px[3] = (total.a.min(1.0) * 255.0).round() as u8;
        }
    });
    (picture, layer)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_rounds_to_the_nearest_half_float() {
        assert_eq!(half(1.0), 1.0);
        assert_eq!(half(0.1), 0.099975586);
        assert_eq!(half(-2.5), -2.5);
        assert_eq!(half(1.0 + 1.0 / 4096.0), 1.0);
        assert_eq!(half(70000.0), 65504.0);
        assert_eq!(half(f32::NAN), 0.0);
    }

    #[test]
    fn ramps_cover_the_land_and_fade_outside() {
        let table = ramps(100.0);
        let c = Constants { unit: 100.0, ramp_low: RAMP_RANGE.0, ramp_high: RAMP_RANGE.1, reach: REACH, shadow: SHADOW_AT, rim: RIM_AT, belly: BELLY_AT, grain: 0.0, grain_scale: 1.0, grain_floor: 0.03, core_lift: 0.0, core_drop: 0.0, volume_lit: 0.0, volume_dark: 0.0 };
        let rows: Vec<[f32; 8]> = (0..RAMP).map(|k| std::array::from_fn(|c| table[((c / 4) * RAMP + k) * 4 + c % 4] as f32 / 255.0)).collect();
        let inside = ramp(&rows, -0.6, &c);
        assert!(inside.at(1) > 0.99 && inside.at(3) > 0.99 && inside.at(4) > 0.99);
        let outside = ramp(&rows, 0.6, &c);
        assert!((0..8).all(|k| outside.at(k) < 0.01));
    }
}
