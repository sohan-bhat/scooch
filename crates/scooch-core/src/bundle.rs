use crate::area;
use crate::look::{self, Sampled, P};
use crate::pads::{self, Lookup};
use crate::paint::{self, Materials, CLOUD, DEEP, FOAM, GAIN, LAYERS, LIT, SAND, SHALLOW, WET};
use crate::render;
use crate::shape::{self, smoothstep, Grid, Q};
use crate::table::{self, Curve};
use crate::{color, math, scenery, Analysis, Options, Placement, RgbImage, RgbaImage, Scene};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;

pub const RAMP: usize = 512;
const RAMP_RANGE: (f32, f32) = (look::FLOOR, look::HOLD.1);
const CANVAS_MOST: u32 = 8192;
const EDGE: f32 = 0.75;
const SHALLOW_POWER: f32 = 2.2;
const FOAM_LINE: (f32, f32) = (0.05, 0.01);
const WET_BAND: (f32, f32) = (0.35, 1.5);
const BAND: f32 = 0.3;
const UNDER: f32 = 0.15;
const BROAD_MIX: f32 = 0.9;
const UNDER_MIX: f32 = 0.9;
const GRAIN_DOUBLE: f32 = 0.6;
const GLIDE_REACH: f32 = 1.8;
const GLIDE_BLEND: f32 = 0.1;
const GLIDE_EDGE: f32 = 4.0;
const GLIDE_KEEP: f32 = 0.3;
const GLIDE_GATE: f32 = 0.2;
const GLIDE_SPAN: f32 = 0.3;

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

fn ramps(u: f32) -> Vec<u8> {
    let mut out = vec![0u8; RAMP * 8];
    let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let cell = look::CELL * u;
    for k in 0..RAMP {
        let t = RAMP_RANGE.0 + (RAMP_RANGE.1 - RAMP_RANGE.0) * k as f32 / (RAMP - 1) as f32;
        let d = t * u;
        let depth = (-d).max(0.0);
        let shallow = if d <= 0.0 { 1.0 } else { math::powf((1.0 - d / cell).max(0.0), SHALLOW_POWER) };
        let foam = if d <= 0.0 { 1.0 } else { smoothstep(FOAM_LINE.0 * cell, FOAM_LINE.1 * cell, d) };
        let first = [shallow, foam, smoothstep(EDGE, -EDGE, d), math::powf(smoothstep(WET_BAND.0 * cell, 0.0, depth), WET_BAND.1)];
        let second = [(0.5 - d).clamp(0.0, 1.0), smoothstep(BAND * cell, 0.0, depth), smoothstep(UNDER * cell, 0.0, depth), 0.0];
        for c in 0..4 {
            out[k * 4 + c] = byte(first[c]);
            out[(RAMP + k) * 4 + c] = byte(second[c]);
        }
    }
    out
}

pub fn grain_hash(x: u32, y: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h >> 8) as f32 / 16_777_215.0
}

#[inline(always)]
fn grain_at(c: &Constants, x: u32, y: u32) -> f32 {
    1.0 + c.grain * ((grain_hash(x, y) - 0.5) + GRAIN_DOUBLE * (grain_hash(x / 2, y / 2) - 0.5))
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
    still(image, found, placement, scene, options, materials)
}

pub struct Framed {
    pub placement: Placement,
    pub canvas: RgbImage,
    pub canvas_width: u32,
    pub canvas_height: u32,
    pub xmap: Vec<f64>,
    pub ymap: Vec<f64>,
    pub ratio: f64,
    pub grid: Grid,
    pub small: Vec<[f32; 3]>,
    pub width: u32,
    pub height: u32,
}

impl Framed {
    pub fn empty(&self) -> bool {
        self.width == 0 || self.height == 0 || self.canvas_width == 0 || self.canvas_height == 0
    }
}

pub fn framed((sw, sh): (u32, u32), canvas: RgbImage, placement: &Placement, width: u32, height: u32) -> Framed {
    let placement = if table::usable(&placement.fx) && table::usable(&placement.fy) { placement.clone() } else { table::fill(sw.max(1), sh.max(1), width.max(1), height.max(1)) };
    let (cw, ch) = canvas.dimensions();
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let xmap: Vec<f64> = (0..width).map(|i| xc.at((i as f64 + 0.5) / width as f64)).collect();
    let ymap: Vec<f64> = (0..height).map(|j| yc.at((j as f64 + 0.5) / height as f64)).collect();
    let ratio = render::worst_ratio(&placement.fx, cw, width).max(render::worst_ratio(&placement.fy, ch, height));
    let grid = Grid::of(width as usize, height as usize, Q);
    let mut out = Framed { placement, canvas, canvas_width: cw, canvas_height: ch, xmap, ymap, ratio, grid, small: Vec::new(), width, height };
    if !out.empty() {
        out.small = small_picture(&out.canvas, &out.xmap, &out.ymap, &grid);
    }
    out
}

pub struct Grounded {
    pub lookup: Lookup,
    pub usable: bool,
    pub saliency: Vec<f32>,
    pub busy: Vec<f32>,
}

impl Grounded {
    pub fn sampled<'a>(&'a self, framed: &'a Framed) -> Sampled<'a> {
        Sampled { grid: framed.grid, small: &framed.small, saliency: &self.saliency, busy: &self.busy }
    }
}

pub fn grounded(framed: &Framed, found: &Analysis) -> Grounded {
    let (grid, width, height) = (framed.grid, framed.width, framed.height);
    let (aw, ah) = (found.width as usize, found.height as usize);
    let usable = aw > 0 && ah > 0 && found.saliency.len() == aw * ah && found.busy.len() == aw * ah;
    let lookup = Lookup {
        ax: (0..grid.w).map(|i| (framed.xmap[(i * Q + Q / 2).min(width as usize - 1)] * aw as f64 - 0.5) as f32).collect(),
        ay: (0..grid.h).map(|j| (framed.ymap[(j * Q + Q / 2).min(height as usize - 1)] * ah as f64 - 0.5) as f32).collect(),
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
    Grounded { lookup, usable, saliency, busy }
}

pub fn constants(materials: &Materials, unit: f32) -> Constants {
    Constants {
        unit,
        ramp_low: RAMP_RANGE.0,
        ramp_high: RAMP_RANGE.1,
        reach: look::REACH,
        shadow: [GLIDE_KEEP, GLIDE_GATE],
        rim: [0.0; 2],
        belly: [0.0; 2],
        grain: materials.grain,
        grain_scale: GLIDE_REACH,
        grain_floor: GLIDE_BLEND,
        core_lift: look::LIFT + look::DIM,
        core_drop: look::DIM,
        volume_lit: BROAD_MIX,
        volume_dark: UNDER_MIX,
    }
}

pub fn shell(framed: &Framed, materials: &Materials) -> Bundle {
    Bundle {
        width: framed.width,
        height: framed.height,
        canvas_width: framed.canvas_width,
        canvas_height: framed.canvas_height,
        canvas: Vec::new(),
        xmap: framed.xmap.clone(),
        ymap: framed.ymap.clone(),
        ratio: framed.ratio,
        grid_width: framed.grid.w as u32,
        grid_height: framed.grid.h as u32,
        field: Vec::new(),
        look: Vec::new(),
        pad: Vec::new(),
        paint_width: 0,
        paint_height: 0,
        paints: Vec::new(),
        ramps: Vec::new(),
        constants: constants(materials, 1.0),
    }
}

pub fn fill(out: &mut Bundle, framed: &Framed, fixed: Option<&look::Fixed>, live: Option<&look::Live>, pad: Option<Vec<u8>>) {
    let (grid, width, height) = (framed.grid, framed.width as usize, framed.height as usize);
    let unit = fixed.map_or(1.0, |f| f.u());
    match (fixed, live) {
        (Some(fixed), Some(live)) => {
            out.field = live.field().into_iter().map(half).collect();
            out.look = live.look.clone();
            out.paint_width = fixed.paint_grid.w as u32;
            out.paint_height = fixed.paint_grid.h as u32;
            out.paints = fixed.paints.clone();
        }
        _ => {
            let none = look::none(grid, width, height);
            out.field = none.field.iter().map(|v| half(*v)).collect();
            out.look = none.look;
            out.paint_width = none.paint_grid.w as u32;
            out.paint_height = none.paint_grid.h as u32;
            out.paints = none.paints;
        }
    }
    out.pad = pad.unwrap_or_else(|| vec![0; grid.w * grid.h * 4]);
    out.ramps = ramps(unit);
    out.constants.unit = unit;
}

pub struct Parts {
    pub framed: Framed,
    pub grounded: Option<Grounded>,
    pub fixed: Option<look::Fixed>,
    pub live: Option<look::Live>,
    pub calm: Option<pads::Calm>,
    pub pads: Option<pads::Pads>,
}

impl Parts {
    pub fn bytes(&self) -> usize {
        let f = &self.framed;
        let framed = f.canvas.as_raw().len() + (f.xmap.len() + f.ymap.len()) * 8 + f.small.len() * 12;
        let grounded = self.grounded.as_ref().map_or(0, |g| (g.saliency.len() + g.busy.len() + g.lookup.ax.len() + g.lookup.ay.len()) * 4);
        framed + grounded + self.fixed.as_ref().map_or(0, |x| x.bytes()) + self.live.as_ref().map_or(0, |x| x.bytes()) + self.calm.as_ref().map_or(0, |x| x.bytes()) + self.pads.as_ref().map_or(0, |x| x.bytes())
    }

    pub fn pad(&self) -> Option<Vec<u8>> {
        self.pads.as_ref().zip(self.calm.as_ref()).and_then(|(p, c)| p.table(c))
    }

    pub fn tables(&self, materials: &Materials) -> Bundle {
        let mut out = shell(&self.framed, materials);
        if !self.framed.empty() {
            fill(&mut out, &self.framed, self.fixed.as_ref(), self.live.as_ref(), self.pad());
        }
        out
    }

    pub fn settle(&mut self) {
        let mut keep = false;
        if let Some(grounded) = &mut self.grounded {
            keep = grounded.usable && self.calm.is_none();
            grounded.saliency = Vec::new();
            grounded.busy = Vec::new();
        }
        if !keep {
            self.framed.small = Vec::new();
        }
    }

    pub fn repad(&mut self, found: &Analysis, scene: &Scene, options: &Options) {
        let Some(grounded) = &self.grounded else { return };
        let padded = grounded.usable && (self.live.is_some() || pads::active(found, &self.framed.placement, scene, options));
        if !padded {
            self.pads = None;
            return;
        }
        if self.calm.is_none() {
            self.calm = pads::calm(&self.framed.small, &self.framed.grid, found, &grounded.lookup, scene.width as usize);
        }
        self.pads = self.calm.as_ref().and_then(|calm| pads::Pads::new(calm, &self.framed.grid, &grounded.lookup, scene, options, self.fixed.as_ref().map(|f| &f.belong[..])));
    }
}

pub fn parts(source: (u32, u32), canvas: RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> Parts {
    let (width, height) = (scene.width, scene.height);
    let framed = framed(source, canvas, placement, width, height);
    let mut out = Parts { framed, grounded: None, fixed: None, live: None, calm: None, pads: None };
    if out.framed.empty() {
        return out;
    }
    let grounded = grounded(&out.framed, found);
    let ground = scenery::active(scene, options).then(|| shape::ground(scene)).flatten();
    out.fixed = ground.as_ref().map(|g| look::fixed(&grounded.sampled(&out.framed), options, materials, g.u, width as usize, height as usize));
    out.live = ground.as_ref().zip(out.fixed.as_ref()).map(|(g, f)| look::live(f, g));
    out.grounded = Some(grounded);
    out.repad(found, scene, options);
    out
}

pub fn still(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> Bundle {
    let parts = parts(image.dimensions(), canvas(image, scene.width, scene.height).into_owned(), found, placement, scene, options, materials);
    let mut out = parts.tables(materials);
    out.canvas = parts.framed.canvas.into_raw();
    out
}

pub fn regions(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> (Vec<[u8; 4]>, u32, u32) {
    let parts = parts(image.dimensions(), canvas(image, scene.width, scene.height).into_owned(), found, placement, scene, options, materials);
    let grid = &parts.framed.grid;
    let cells = match &parts.fixed {
        Some(f) => f.sky.iter().zip(&f.belong).map(|(s, b)| [look::byte(*s), look::byte(*b), 0, 0]).collect(),
        None => vec![[0; 4]; grid.w * grid.h],
    };
    (cells, grid.w as u32, grid.h as u32)
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

fn line4(values: &[[f32; 4]], w: usize, row: &Probe, (lo, hi): (usize, usize)) -> Vec<[f32; 4]> {
    let (top, bottom) = (&values[row.a * w..(row.a + 1) * w], &values[row.b * w..(row.b + 1) * w]);
    let mut out = vec![[0.0f32; 4]; w];
    for (o, (p, q)) in out[lo..hi].iter_mut().zip(top[lo..hi].iter().zip(&bottom[lo..hi])) {
        *o = std::array::from_fn(|k| p[k] + (q[k] - p[k]) * row.t);
    }
    out
}

fn line1(values: &[f32], w: usize, row: &Probe, (lo, hi): (usize, usize)) -> Vec<f32> {
    let (top, bottom) = (&values[row.a * w..(row.a + 1) * w], &values[row.b * w..(row.b + 1) * w]);
    let mut out = vec![0.0f32; w];
    for (o, (p, q)) in out[lo..hi].iter_mut().zip(top[lo..hi].iter().zip(&bottom[lo..hi])) {
        *o = p + (q - p) * row.t;
    }
    out
}

fn field_at(field: &[f32], grid: &Grid, x: f32, y: f32) -> f32 {
    let probe = |v: f32, n: usize| {
        let g = (v / Q as f32 - 0.5).clamp(0.0, (n - 1) as f32);
        let a = g as usize;
        (a, (a + 1).min(n - 1), g - a as f32)
    };
    let ((xa, xb, tx), (ya, yb, ty)) = (probe(x, grid.w), probe(y, grid.h));
    let top = field[ya * grid.w + xa] + (field[ya * grid.w + xb] - field[ya * grid.w + xa]) * tx;
    let bottom = field[yb * grid.w + xa] + (field[yb * grid.w + xb] - field[yb * grid.w + xa]) * tx;
    top + (bottom - top) * ty
}

fn gap(a: &[f32], b: &[f32], grid: &Grid, (x, y): (f32, f32), here: f32, reach: f32) -> f32 {
    let change = |dx: f32, dy: f32| (field_at(a, grid, x + dx, y + dy) - field_at(b, grid, x + dx, y + dy)).abs();
    here.max(change(reach, 0.0)).max(change(-reach, 0.0)).max(change(0.0, reach)).max(change(0.0, -reach))
}

struct Plane {
    look: Vec<[f32; 4]>,
    pad: Vec<[f32; 4]>,
    paints: Vec<Vec<[f32; 4]>>,
    ramps: Vec<[f32; 8]>,
}

fn plane(x: &Bundle, decode: &[f32; 256], only: Option<&[bool]>) -> Plane {
    let unit = |p: &[u8; 4]| p.map(|v| v as f32 / 255.0);
    let linear = |p: &[u8; 4]| [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize], p[3] as f32 / 255.0];
    let size = x.paint_grid().w * x.paint_grid().h * 4;
    let wanted = |c: usize| only.is_none_or(|o| o[c]);
    Plane {
        look: x.look.as_chunks::<4>().0.par_iter().enumerate().map(|(c, p)| if wanted(c) { unit(p) } else { [0.0; 4] }).collect(),
        pad: x.pad.as_chunks::<4>().0.par_iter().enumerate().map(|(c, p)| if wanted(c) { linear(p) } else { [0.0; 4] }).collect(),
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

#[inline(always)]
fn pieces<L: Fn(usize) -> [f32; 4]>(c: &Constants, ramps: &[[f32; 8]], (d, own): (f32, f32), look: [f32; 4], layer: L, picture: [f32; 3], grain: f32) -> (Paint, Paint) {
    let [kind, mark, broad, facing] = look;
    let facing = facing * 2.0 - 1.0;
    let here = ramp(ramps, d, c);
    let (mut shallow, mut rest) = (Paint::default(), Paint::default());
    if kind < 1.0 {
        let gain = layer(SHALLOW);
        let sand = layer(SAND);
        let keep = sand[3] * (1.0 - kind);
        shallow.over(std::array::from_fn(|k| (picture[k] * gain[k] * GAIN).min(1.0)), ramp(ramps, own, c).at(0) * gain[3]);
        shallow = shallow.scaled(keep);
        let mut land = Paint::default();
        land.over(rgb(layer(FOAM)), here.at(1) * mark);
        let body = mix3(rgb(sand), rgb(layer(WET)), here.at(3));
        land.over(body.map(|v| (v * grain).min(1.0)), here.at(2));
        rest = land.scaled(keep);
    }
    if kind > 0.0 && here.at(4) > 0.0 {
        let shaded = layer(CLOUD);
        let (lit, deep) = (rgb(layer(LIT)), rgb(layer(DEEP)));
        let mut tone = mix3(rgb(shaded), lit, c.volume_lit * broad);
        tone = mix3(tone, deep, here.at(6) * (-facing).max(0.0) * c.volume_dark);
        tone = mix3(tone, lit, here.at(5) * facing.max(0.0));
        let lift = 1.0 + mark * c.core_lift - c.core_drop;
        let mut cloud = Paint::default();
        cloud.over(tone.map(|v| (v * lift).min(1.0)), here.at(4));
        rest = rest.plus(cloud.scaled(shaded[3] * kind));
    }
    (shallow, rest)
}

fn under((shallow, rest): (Paint, Paint)) -> Paint {
    rest.plus(shallow.scaled(1.0 - rest.a))
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
    frame_with(a, b, s, Glide::ByKind)
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Glide {
    ByKind,
    Dissolve,
}

pub fn frame_with(a: &Bundle, b: &Bundle, s: f32, glide: Glide) -> RgbImage {
    if !compatible(a, b) {
        return crossfade(a, b, s.clamp(0.0, 1.0));
    }
    let (mut picture, layer) = draw_with(a, b, s, &a.canvas, glide);
    paint::composite(&mut picture, &layer);
    picture
}

pub fn layers(a: &Bundle, b: &Bundle, s: f32) -> (RgbImage, RgbaImage) {
    draw(a, b, s, &a.canvas)
}

pub fn layers_with(a: &Bundle, b: &Bundle, s: f32, glide: Glide) -> (RgbImage, RgbaImage) {
    draw_with(a, b, s, &a.canvas, glide)
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
    draw_with(a, b, s, canvas, Glide::ByKind)
}

fn draw_with(a: &Bundle, b: &Bundle, s: f32, canvas: &[u8], glide: Glide) -> (RgbImage, RgbaImage) {
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
    let picture = render::picture((canvas, a.canvas_width, a.canvas_height), (&xmap, &ymap), ratio, (width, height));
    finish_with(a, b, s, picture, None, glide)
}

pub struct Scope {
    pub redraw: Vec<bool>,
    pub need: Vec<bool>,
    pub decode: Vec<bool>,
}

impl Scope {
    pub fn new(redraw: Vec<bool>, boxes: &[area::Area], grid_width: usize, grid_height: usize) -> Scope {
        let grown = |r: usize| {
            let mut out = vec![false; redraw.len()];
            area::spread(&redraw, grid_width, grid_height, r, boxes, &mut out);
            out
        };
        let (need, decode) = (grown(2), grown(4));
        Scope { redraw, need, decode }
    }

    pub fn ranges(cells: &[bool], grid_width: usize, y: usize, width: usize) -> Vec<(usize, usize)> {
        let row = &cells[(y / Q) * grid_width..(y / Q + 1) * grid_width];
        let mut out = Vec::new();
        let mut i = 0;
        while i < row.len() {
            if row[i] {
                let start = i;
                while i < row.len() && row[i] {
                    i += 1;
                }
                out.push((start * Q, (i * Q).min(width)));
            } else {
                i += 1;
            }
        }
        out
    }
}

pub fn finish(a: &Bundle, b: &Bundle, s: f32, picture: RgbImage, scope: Option<&Scope>) -> (RgbImage, RgbaImage) {
    finish_with(a, b, s, picture, scope, Glide::ByKind)
}

fn finish_with(a: &Bundle, b: &Bundle, s: f32, mut picture: RgbImage, scope: Option<&Scope>, glide: Glide) -> (RgbImage, RgbaImage) {
    let same = std::ptr::eq(a, b) || s == 0.0;
    let (width, height) = (a.width, a.height);
    let mut layer = RgbaImage::new(width, height);
    let decode = color::decode_table();
    let encode = color::encode_table();
    let pad_on = |x: &Bundle| x.pad.as_chunks::<4>().0.iter().any(|p| p[3] > 0);
    let field_on = |x: &Bundle| x.field.iter().any(|f| *f < x.constants.reach);
    let wanted = |x: &Bundle| pad_on(x) || field_on(x);
    if !(wanted(a) || (!same && wanted(b))) {
        return (picture, layer);
    }
    let grid = a.grid();
    let (w, h) = (width as usize, height as usize);
    let columns = probes(w, Q, grid.w);
    let rows = probes(h, Q, grid.h);
    let only = scope.map(|sc| &sc.decode[..]);
    let planes: Vec<Plane> = if same { vec![plane(a, decode, only)] } else { vec![plane(a, decode, only), plane(b, decode, only)] };
    let shares: Vec<(f32, &Bundle)> = if same { vec![(1.0, a)] } else { vec![(1.0 - s, a), (s, b)] };
    let padded = dilate(&(0..grid.w * grid.h).map(|c| planes.iter().any(|p| p.pad[c][3] > 0.0)).collect::<Vec<bool>>(), &grid);
    if padded.iter().any(|p| *p) {
        let buffer: &mut [u8] = &mut picture;
        buffer.par_chunks_mut(w * 3).enumerate().for_each(|(y, line)| {
            let row = &rows[y];
            if !padded[row.a * grid.w..(row.a + 1) * grid.w].iter().any(|p| *p) {
                return;
            }
            let ranges = scope.map_or_else(|| vec![(0, w)], |sc| Scope::ranges(&sc.need, grid.w, y, w));
            for x in ranges.into_iter().flat_map(|(x0, x1)| x0..x1) {
                let px = &mut line[x * 3..x * 3 + 3];
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
    let active = dilate(&(0..grid.w * grid.h).map(|cell| fields.iter().any(|f| f[cell] < c.reach)).collect::<Vec<bool>>(), &grid);
    if !active.iter().any(|a| *a) {
        return (picture, layer);
    }
    let raw = picture.as_raw();
    let paint_grid = a.paint_grid();
    let tint_columns = probes(w, P, paint_grid.w);
    let tint_rows = probes(h, P, paint_grid.h);
    let out: &mut [u8] = &mut layer;
    out.par_chunks_mut(w * 4).enumerate().for_each(|(y, line)| {
        let row = &rows[y];
        let j = row.a;
        if !active[j * grid.w..(j + 1) * grid.w].iter().any(|a| *a) {
            return;
        }
        let ranges = scope.map_or_else(|| vec![(0, w)], |sc| Scope::ranges(&sc.redraw, grid.w, y, w));
        if ranges.is_empty() {
            return;
        }
        let tint_row = &tint_rows[y];
        let (first, last) = (ranges[0].0, ranges[ranges.len() - 1].1);
        let span = ((first / Q).saturating_sub(1), (last / Q + 2).min(grid.w));
        let tint_span = ((first / P).saturating_sub(1), (last / P + 2).min(paint_grid.w));
        let field_lines: Vec<Vec<f32>> = fields.iter().map(|f| line1(f, grid.w, row, span)).collect();
        let look_lines: Vec<Vec<[f32; 4]>> = planes.iter().map(|p| line4(&p.look, grid.w, row, span)).collect();
        let tint_lines: Vec<Vec<Vec<[f32; 4]>>> = planes.iter().map(|p| p.paints.iter().map(|layer| line4(layer, paint_grid.w, tint_row, tint_span)).collect()).collect();
        let cells = line.as_chunks_mut::<4>().0;
        for x in ranges.into_iter().flat_map(|(x0, x1)| x0..x1) {
            let px = &mut cells[x];
            let column = &columns[x];
            if !active[j * grid.w + column.a] {
                continue;
            }
            let mut ds = [f32::MAX; 2];
            for (slot, f) in ds.iter_mut().zip(&field_lines) {
                *slot = f[column.a] + (f[column.b] - f[column.a]) * column.t;
            }
            if ds.iter().all(|d| *d > c.reach) {
                continue;
            }
            let tint_column = &tint_columns[x];
            let p = &raw[(y * w + x) * 3..(y * w + x) * 3 + 3];
            let here = [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]];
            let grain = grain_at(&c, x as u32, y as u32);
            let part = |k: usize, d: f32, own: f32| {
                let tints = &tint_lines[k];
                pieces(&shares[k].1.constants, &planes[k].ramps, (d, own), lerp4(&look_lines[k], column), |layer| lerp4(&tints[layer], tint_column), here, grain)
            };
            let faded = || {
                let mut sum = Paint::default();
                for (k, (share, _)) in shares.iter().enumerate() {
                    if ds[k] <= c.reach {
                        sum = sum.plus(under(part(k, ds[k], ds[k])).scaled(*share));
                    }
                }
                sum
            };
            let moved = || {
                let (da, db) = (ds[0], ds[1]);
                let (ta, tb) = (da + s * c.grain_scale, db + (1.0 - s) * c.grain_scale);
                let change = gap(fields[0], fields[1], &grid, (x as f32 + 0.5, y as f32 + 0.5), (da - db).abs(), GLIDE_SPAN * c.unit);
                let bump = 4.0 * s * (1.0 - s);
                let kept = da.max(db) + c.shadow[0] * bump * bump * smoothstep(0.0, c.shadow[1], change);
                let d = kept.min(ta.min(tb));
                let mut w = smoothstep(-c.grain_floor, c.grain_floor, ta - tb);
                w *= (GLIDE_EDGE * s).min(1.0);
                w = 1.0 - (1.0 - w) * (GLIDE_EDGE * (1.0 - s)).min(1.0);
                let ((sa, ra), (sb, rb)) = (part(0, d, da), part(1, d, db));
                under((sa.scaled(1.0 - s).plus(sb.scaled(s)), ra.scaled(1.0 - w).plus(rb.scaled(w))))
            };
            let tide = if planes.len() == 1 || glide == Glide::Dissolve { 0.0 } else { 1.0 - lerp4(&look_lines[0], column)[0].max(lerp4(&look_lines[1], column)[0]) };
            let total = if planes.len() == 1 {
                under(part(0, ds[0], ds[0]))
            } else if tide <= 0.0 {
                faded()
            } else if tide >= 1.0 {
                moved()
            } else {
                faded().scaled(1.0 - tide).plus(moved().scaled(tide))
            };
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
        let c = constants(&paint::from_palette(&[]), 100.0);
        let rows: Vec<[f32; 8]> = (0..RAMP).map(|k| std::array::from_fn(|c| table[((c / 4) * RAMP + k) * 4 + c % 4] as f32 / 255.0)).collect();
        let inside = ramp(&rows, -0.5, &c);
        assert!(inside.at(0) > 0.99 && inside.at(2) > 0.99 && inside.at(4) > 0.99 && inside.at(3) < 0.01);
        let shore = ramp(&rows, 0.011, &c);
        assert!(shore.at(0) > 0.9 && shore.at(1) > 0.9 && shore.at(2) < 0.01 && shore.at(4) < 0.01);
        let outside = ramp(&rows, look::REACH, &c);
        assert!([0, 1, 2, 4].iter().all(|k| outside.at(*k) < 0.01));
    }
}
