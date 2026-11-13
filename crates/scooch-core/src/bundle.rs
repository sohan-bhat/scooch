use crate::look::{self, Sampled, P};
use crate::pads::{self, Lookup};
use crate::paint::{self, Materials, BELLY, CORE, FOAM, LAND, LIT, RIM, SHORE, TOP};
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
const REACH: f32 = 0.45;

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

fn canvas(image: &RgbImage, width: u32, height: u32) -> Cow<'_, RgbImage> {
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
    let at = |map: &[f64], cell: usize, k: usize, size: f64| map[(cell * Q + 1 + 2 * k).min(map.len() - 1)] * size - 0.5;
    let columns: Vec<[f64; 2]> = (0..grid.w).map(|i| [at(xmap, i, 0, cw), at(xmap, i, 1, cw)]).collect();
    let rows: Vec<[f64; 2]> = (0..grid.h).map(|j| [at(ymap, j, 0, ch), at(ymap, j, 1, ch)]).collect();
    (0..grid.w * grid.h)
        .into_par_iter()
        .map(|c| {
            let (col, row) = (&columns[c % grid.w], &rows[c / grid.w]);
            let mut sum = [0.0f32; 3];
            for y in row {
                for x in col {
                    let v = sample(canvas, decode, *x, *y);
                    for k in 0..3 {
                        sum[k] += v[k] * 0.25;
                    }
                }
            }
            sum
        })
        .collect()
}

pub fn make(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> Bundle {
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
        out.canvas = canvas.as_raw().clone();
        return out;
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
    out.canvas = canvas.as_raw().clone();
    out.field = scenic.field.iter().map(|v| half(*v)).collect();
    out.look = scenic.look;
    out.pad = pad.unwrap_or_else(|| vec![0; grid.w * grid.h * 4]);
    out.paint_width = scenic.paint_grid.w as u32;
    out.paint_height = scenic.paint_grid.h as u32;
    out.paints = scenic.paints;
    out.ramps = ramps(unit);
    out.constants.unit = unit;
    out
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

fn bilinear(values: &[f32], w: usize, column: &Probe, row: &Probe) -> f32 {
    let top = values[row.a * w + column.a] + (values[row.a * w + column.b] - values[row.a * w + column.a]) * column.t;
    let bottom = values[row.b * w + column.a] + (values[row.b * w + column.b] - values[row.b * w + column.a]) * column.t;
    top + (bottom - top) * row.t
}

fn bytes4(values: &[u8], w: usize, column: &Probe, row: &Probe) -> [f32; 4] {
    let at = |r: usize, c: usize, k: usize| values[(r * w + c) * 4 + k] as f32;
    std::array::from_fn(|k| {
        let top = at(row.a, column.a, k) + (at(row.a, column.b, k) - at(row.a, column.a, k)) * column.t;
        let bottom = at(row.b, column.a, k) + (at(row.b, column.b, k) - at(row.b, column.a, k)) * column.t;
        (top + (bottom - top) * row.t) / 255.0
    })
}

fn color4(values: &[u8], w: usize, column: &Probe, row: &Probe, decode: &[f32; 256]) -> [f32; 4] {
    let at = |r: usize, c: usize| {
        let i = (r * w + c) * 4;
        [decode[values[i] as usize], decode[values[i + 1] as usize], decode[values[i + 2] as usize], values[i + 3] as f32 / 255.0]
    };
    let (p, q, r, s) = (at(row.a, column.a), at(row.a, column.b), at(row.b, column.a), at(row.b, column.b));
    std::array::from_fn(|k| {
        let top = p[k] + (q[k] - p[k]) * column.t;
        let bottom = r[k] + (s[k] - r[k]) * column.t;
        top + (bottom - top) * row.t
    })
}

fn ramp(table: &[u8], row: usize, value: f32, c: &Constants) -> [f32; 4] {
    let f = ((value - c.ramp_low) / (c.ramp_high - c.ramp_low) * (RAMP - 1) as f32).clamp(0.0, (RAMP - 1) as f32);
    let (a, t) = (f as usize, f - f.floor());
    let b = (a + 1).min(RAMP - 1);
    std::array::from_fn(|k| {
        let (p, q) = (table[(row * RAMP + a) * 4 + k] as f32, table[(row * RAMP + b) * 4 + k] as f32);
        (p + (q - p) * t) / 255.0
    })
}

#[derive(Clone, Copy, Default)]
struct Paint {
    rgb: [f32; 3],
    a: f32,
}

impl Paint {
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

struct Reads {
    d: f32,
    shadow: f32,
    rim: f32,
    belly: f32,
}

struct At<'a> {
    column: &'a Probe,
    row: &'a Probe,
    tint_column: &'a Probe,
    tint_row: &'a Probe,
}

fn paint(x: &Bundle, reads: &Reads, at: &At, grain: f32, decode: &[f32; 256]) -> Paint {
    let (column, row, tint_column, tint_row) = (at.column, at.row, at.tint_column, at.tint_row);
    let c = &x.constants;
    let grid = x.grid();
    let [sky, belong, open, light] = bytes4(&x.look, grid.w, column, row);
    if belong <= 0.0 {
        return Paint::default();
    }
    let light = light * 2.0 - 1.0;
    let first = ramp(&x.ramps, 0, reads.d, c);
    let second = ramp(&x.ramps, 1, reads.d, c);
    let shade = ramp(&x.ramps, 1, reads.shadow, c);
    let plane = x.paint_grid().w * x.paint_grid().h * 4;
    let layer = |k: usize| color4(&x.paints[k * plane..(k + 1) * plane], x.paint_grid().w, tint_column, tint_row, decode);
    let grained = |rgb: [f32; 4]| [(rgb[0] * grain).min(1.0), (rgb[1] * grain).min(1.0), (rgb[2] * grain).min(1.0)];
    let mut land = Paint::default();
    if sky < 1.0 {
        let shore = layer(SHORE);
        land.over([1.0; 3], first[0] * shore[3]);
        land.over([0.0; 3], shade[2]);
        if first[1] > 0.0 {
            land.over(grained(shore), first[1]);
            if first[2] > 0.0 {
                land.over(grained(layer(FOAM)), first[2]);
            }
            let fill = layer(LAND);
            if first[3] > 0.0 {
                land.over(grained(fill), first[3]);
            }
            if second[0] > 0.0 {
                land.over(grained(layer(TOP)), second[0]);
            }
            land = land.scaled(fill[3]);
        } else {
            land = land.scaled(layer(LAND)[3]);
        }
    }
    let mut cloud = Paint::default();
    if sky > 0.0 {
        let core = layer(CORE);
        cloud.over([0.0; 3], shade[3]);
        if second[1] > 0.0 {
            let (rim, belly) = (layer(RIM), layer(BELLY));
            let mut tone = mix3([core[0], core[1], core[2]], { let l = layer(LIT); [l[0], l[1], l[2]] }, open);
            if light > 0.0 {
                tone = mix3(tone, [rim[0], rim[1], rim[2]], light * (c.core_lift + (c.volume_lit - c.core_lift) * open));
            } else {
                tone = mix3(tone, [belly[0], belly[1], belly[2]], -light * (c.core_drop + (c.volume_dark - c.core_drop) * open));
            }
            tone = mix3(tone, [rim[0], rim[1], rim[2]], 1.0 - ramp(&x.ramps, 0, reads.rim, c)[1]);
            tone = mix3(tone, [belly[0], belly[1], belly[2]], 1.0 - ramp(&x.ramps, 0, reads.belly, c)[1]);
            cloud.over(grained([tone[0], tone[1], tone[2], 1.0]), second[1]);
        }
        cloud = cloud.scaled(core[3]);
    }
    land.scaled(1.0 - sky).plus(cloud.scaled(sky)).scaled(belong)
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
    if width == 0 || height == 0 || a.canvas_width == 0 || a.canvas_height == 0 {
        return (RgbImage::new(width, height), RgbaImage::new(width, height));
    }
    let same = std::ptr::eq(a, b) || s == 0.0;
    let canvas = RgbImage::from_raw(a.canvas_width, a.canvas_height, a.canvas.clone()).unwrap_or_else(|| RgbImage::new(a.canvas_width, a.canvas_height));
    let (xmap, ymap): (Cow<[f64]>, Cow<[f64]>) = if same {
        (Cow::Borrowed(&a.xmap), Cow::Borrowed(&a.ymap))
    } else {
        let blend = |p: &[f64], q: &[f64]| p.iter().zip(q).map(|(x, y)| x + (y - x) * s as f64).collect::<Vec<f64>>();
        (Cow::Owned(blend(&a.xmap, &b.xmap)), Cow::Owned(blend(&a.ymap, &b.ymap)))
    };
    let ratio = if same { a.ratio } else { a.ratio.max(b.ratio) };
    let mut picture = render::picture(&canvas, &xmap, &ymap, ratio, width, height);
    let grid = a.grid();
    let decode = color::decode_table();
    let encode = color::encode_table();
    let (w, h) = (width as usize, height as usize);
    let columns = probes(w, Q, grid.w);
    let rows = probes(h, Q, grid.h);
    let weights = |x: &Bundle| -> Vec<f32> { x.pad.as_chunks::<4>().0.iter().map(|p| p[3] as f32 / 255.0).collect() };
    let pad_on = |x: &Bundle| x.pad.as_chunks::<4>().0.iter().any(|p| p[3] > 0);
    if pad_on(a) || (!same && pad_on(b)) {
        let (wa, wb) = (weights(a), if same { Vec::new() } else { weights(b) });
        let buffer: &mut [u8] = &mut picture;
        buffer.par_chunks_mut(w * 3).enumerate().for_each(|(y, line)| {
            let row = &rows[y];
            for (x, px) in line.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let column = &columns[x];
                let mut weight = bilinear(&wa, grid.w, column, row) * if same { 1.0 } else { 1.0 - s };
                let mut calm = color4(&a.pad, grid.w, column, row, decode).map(|v| v * weight);
                if !same {
                    let wb_here = bilinear(&wb, grid.w, column, row) * s;
                    let other = color4(&b.pad, grid.w, column, row, decode);
                    for k in 0..3 {
                        calm[k] += other[k] * wb_here;
                    }
                    weight += wb_here;
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
    let mut layer = RgbaImage::new(width, height);
    let belonging = |x: &Bundle| x.look.as_chunks::<4>().0.iter().any(|p| p[1] > 0);
    if !(belonging(a) || (!same && belonging(b))) {
        return (picture, layer);
    }
    let c = a.constants;
    let u = c.unit;
    let near = |x: &Bundle, cell: usize| x.look[cell * 4 + 1] > 0 && x.field[cell] < c.reach + 0.25;
    let active: Vec<bool> = (0..grid.w * grid.h)
        .map(|cell| {
            let (i, j) = (cell % grid.w, cell / grid.w);
            (j.saturating_sub(1)..(j + 2).min(grid.h)).any(|r| (i.saturating_sub(1)..(i + 2).min(grid.w)).any(|q| near(a, r * grid.w + q) || (!same && near(b, r * grid.w + q))))
        })
        .collect();
    let raw = picture.as_raw();
    let block: Vec<[f32; 2]> = (0..grid.w * grid.h)
        .into_par_iter()
        .map(|cell| {
            if !active[cell] {
                return [0.0, 0.0];
            }
            let (i, j) = (cell % grid.w, cell / grid.w);
            let (mut sum, mut count) = (0.0f32, 0.0f32);
            for y in j * Q..((j + 1) * Q).min(h) {
                for x in i * Q..((i + 1) * Q).min(w) {
                    let p = &raw[(y * w + x) * 3..(y * w + x) * 3 + 3];
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
    let field_at = |x: &Bundle, px: f32, py: f32| grid.linear(&x.field, grid.at(px), grid.at(py));
    let read = |px: f32, py: f32| if same { field_at(a, px, py) } else { field_at(a, px, py) * (1.0 - s) + field_at(b, px, py) * s };
    let out: &mut [u8] = &mut layer;
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, line)| {
        let row = &rows[y];
        let j = row.a.min(grid.h - 1);
        if !(0..grid.w).any(|i| active[j * grid.w + i]) {
            return;
        }
        let tint_row = &tint_rows[y];
        let py = y as f32 + 0.5;
        for (x, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let column = &columns[x];
            if !active[j * grid.w + column.a] {
                continue;
            }
            let d = if same { bilinear(&a.field, grid.w, column, row) } else { bilinear(&a.field, grid.w, column, row) * (1.0 - s) + bilinear(&b.field, grid.w, column, row) * s };
            if d > c.reach {
                continue;
            }
            let fx = x as f32 + 0.5;
            let reads = Reads {
                d,
                shadow: read(fx + c.shadow[0] * u, py + c.shadow[1] * u),
                rim: read(fx + c.rim[0] * u, py + c.rim[1] * u),
                belly: read(fx + c.belly[0] * u, py + c.belly[1] * u),
            };
            let p = &raw[(y * w + x) * 3..(y * w + x) * 3 + 3];
            let lum = 0.2126 * decode[p[0] as usize] + 0.7152 * decode[p[1] as usize] + 0.0722 * decode[p[2] as usize];
            let detail = (lum - bilinear(&means, grid.w, column, row)) * block[j * grid.w + column.a][1];
            let grain = (1.0 + (detail / c.grain_scale).clamp(-1.0, 1.0) * c.grain).powi(3);
            let at = At { column, row, tint_column: &tint_columns[x], tint_row };
            let mut total = paint(a, &reads, &at, grain, decode);
            if !same {
                total = total.scaled(1.0 - s).plus(paint(b, &reads, &at, grain, decode).scaled(s));
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
        let inside = ramp(&table, 0, -0.6, &c);
        assert!(inside[1] > 0.99 && inside[3] > 0.99 && ramp(&table, 1, -0.6, &c)[0] > 0.99);
        let outside = ramp(&table, 0, 0.6, &c);
        assert!(outside.iter().all(|v| *v < 0.01));
    }
}
