use crate::area;
use crate::look::{self, Sampled, P};
use crate::pads::{self, Lookup};
use crate::paint::{self, Materials, CLOUD, DEEP, FOAM, GAIN, GUARD, LAYERS, LIT, SAND, SHALLOW, WET};
use crate::render;
use crate::shape::{self, smoothstep, Grid, Q};
use crate::table::{self, Curve};
use crate::{color, math, scenery, Analysis, Options, Placement, RgbImage, RgbaImage, Scene};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::sync::Arc;

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
const GRAIN_MIX: [u32; 3] = [0x8da6_b343, 0xd816_3841, 0x5bd1_e995];
const GRAIN_SHIFT: [u32; 3] = [13, 15, 8];
const GLIDE_REACH: f32 = 1.8;
const LABEL_DEEPEN: f32 = 0.2;
const LABEL_CAP: f32 = 0.373;
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];
const GLIDE_BLEND: f32 = 0.1;
const GLIDE_EDGE: f32 = 4.0;
const GLIDE_KEEP: f32 = 0.3;
const GLIDE_GATE: f32 = 0.2;
const GLIDE_SPAN: f32 = 0.3;
const GLIDE_FELT: (f32, f32) = (-0.4, 0.06);
const FOLLOW_STEPS: usize = 2;

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
    let mut h = x.wrapping_mul(GRAIN_MIX[0]) ^ y.wrapping_mul(GRAIN_MIX[1]);
    h ^= h >> GRAIN_SHIFT[0];
    h = h.wrapping_mul(GRAIN_MIX[2]);
    h ^= h >> GRAIN_SHIFT[1];
    (h >> GRAIN_SHIFT[2]) as f32 / 16_777_215.0
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

fn centers(map: &[f64], cells: usize) -> Vec<f64> {
    (0..cells)
        .map(|cell| {
            let (a, b) = ((cell * Q + Q / 2 - 1).min(map.len() - 1), (cell * Q + Q / 2).min(map.len() - 1));
            (map[a] + map[b]) * 0.5
        })
        .collect()
}

fn small_picture(canvas: &RgbImage, columns: &[f64], rows: &[f64]) -> Vec<[f32; 3]> {
    let decode = color::decode_table();
    let (cw, ch) = (canvas.width() as f64, canvas.height() as f64);
    let (xs, ys): (Vec<f64>, Vec<f64>) = (columns.iter().map(|f| f * cw - 0.5).collect(), rows.iter().map(|f| f * ch - 0.5).collect());
    (0..xs.len() * ys.len()).into_par_iter().map(|c| sample(canvas, decode, xs[c % xs.len()], ys[c / xs.len()])).collect()
}

fn planes(found: &Analysis, ax: Vec<f32>, ay: Vec<f32>, grid: &Grid) -> (Lookup, bool, Vec<f32>, Vec<f32>) {
    let (aw, ah) = (found.width as usize, found.height as usize);
    let usable = aw > 0 && ah > 0 && found.saliency.len() == aw * ah && found.busy.len() == aw * ah;
    let lookup = Lookup { ax, ay, aw: aw.max(1), ah: ah.max(1) };
    let clean = |v: &f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let (saliency, busy) = if usable {
        let s: Vec<f32> = if found.flat { vec![0.0; aw * ah] } else { found.saliency.iter().map(clean).collect() };
        let b: Vec<f32> = found.busy.iter().map(clean).collect();
        (lookup.plane(&s, grid), lookup.plane(&b, grid))
    } else {
        (vec![0.0; grid.w * grid.h], vec![0.0; grid.w * grid.h])
    };
    (lookup, usable, saliency, busy)
}

pub struct Home {
    pub grid: Grid,
    pub small: Vec<[f32; 3]>,
    pub saliency: Vec<f32>,
    pub busy: Vec<f32>,
    pub columns: Vec<f64>,
    pub rows: Vec<f64>,
}

impl Home {
    pub fn sampled(&self) -> Sampled<'_> {
        Sampled { grid: self.grid, small: &self.small, saliency: &self.saliency, busy: &self.busy, columns: &self.columns, rows: &self.rows }
    }
}

pub fn home(canvas: &RgbImage, found: &Analysis, source: (u32, u32), screen: (u32, u32)) -> Home {
    let (grid, size) = look::home_grid(source, screen);
    let fraction = |k: usize, span: f64| ((k * grid.cell) as f64 + grid.cell as f64 / 2.0) / span;
    let columns: Vec<f64> = (0..grid.w).map(|i| fraction(i, size[0])).collect();
    let rows: Vec<f64> = (0..grid.h).map(|j| fraction(j, size[1])).collect();
    let small = small_picture(canvas, &columns, &rows);
    let (aw, ah) = (found.width.max(1) as f64, found.height.max(1) as f64);
    let ax = columns.iter().map(|f| (f * aw - 0.5) as f32).collect();
    let ay = rows.iter().map(|f| (f * ah - 0.5) as f32).collect();
    let (_, _, saliency, busy) = planes(found, ax, ay, &grid);
    Home { grid, small, saliency, busy, columns, rows }
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
    pub columns: Vec<f64>,
    pub rows: Vec<f64>,
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
    let mut out = Framed { placement, canvas, canvas_width: cw, canvas_height: ch, xmap, ymap, ratio, grid, small: Vec::new(), columns: Vec::new(), rows: Vec::new(), width, height };
    if !out.empty() {
        out.columns = centers(&out.xmap, grid.w);
        out.rows = centers(&out.ymap, grid.h);
        out.small = small_picture(&out.canvas, &out.columns, &out.rows);
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
        Sampled { grid: framed.grid, small: &framed.small, saliency: &self.saliency, busy: &self.busy, columns: &framed.columns, rows: &framed.rows }
    }
}

pub fn grounded(framed: &Framed, found: &Analysis) -> Grounded {
    let (grid, width, height) = (framed.grid, framed.width, framed.height);
    let (aw, ah) = (found.width.max(1) as f64, found.height.max(1) as f64);
    let ax = (0..grid.w).map(|i| (framed.xmap[(i * Q + Q / 2).min(width as usize - 1)] * aw - 0.5) as f32).collect();
    let ay = (0..grid.h).map(|j| (framed.ymap[(j * Q + Q / 2).min(height as usize - 1)] * ah - 0.5) as f32).collect();
    let (lookup, usable, saliency, busy) = planes(found, ax, ay, &grid);
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
    pub belonging: Option<Arc<look::Belonging>>,
    pub fixed: Option<look::Fixed>,
    pub live: Option<look::Live>,
    pub calm: Option<pads::Calm>,
    pub pads: Option<pads::Pads>,
}

impl Parts {
    pub fn bytes(&self) -> usize {
        let f = &self.framed;
        let framed = f.canvas.as_raw().len() + (f.xmap.len() + f.ymap.len() + f.columns.len() + f.rows.len()) * 8 + f.small.len() * 12;
        let grounded = self.grounded.as_ref().map_or(0, |g| (g.saliency.len() + g.busy.len() + g.lookup.ax.len() + g.lookup.ay.len()) * 4);
        framed + grounded + self.belonging.as_ref().map_or(0, |b| b.bytes()) + self.fixed.as_ref().map_or(0, |x| x.bytes()) + self.live.as_ref().map_or(0, |x| x.bytes()) + self.calm.as_ref().map_or(0, |x| x.bytes()) + self.pads.as_ref().map_or(0, |x| x.bytes())
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

pub fn parts(source: (u32, u32), (canvas, kept): (RgbImage, Option<Arc<look::Belonging>>), found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> Parts {
    let (width, height) = (scene.width, scene.height);
    let framed = framed(source, canvas, placement, width, height);
    let mut out = Parts { framed, grounded: None, belonging: None, fixed: None, live: None, calm: None, pads: None };
    if out.framed.empty() {
        return out;
    }
    let grounded = grounded(&out.framed, found);
    let ground = scenery::active(scene, options).then(|| shape::ground(scene)).flatten();
    out.belonging = ground.as_ref().map(|g| match kept.filter(|b| b.fits(source, (width, height), g.u, options)) {
        Some(b) => b,
        None => Arc::new(look::belonging_map(&home(&out.framed.canvas, found, source, (width, height)).sampled(), source, (width, height), options, g.u, paint::sand(materials)[0])),
    });
    out.fixed = ground.as_ref().zip(out.belonging.as_deref().filter(|b| !b.empty)).map(|(g, b)| look::fixed(&grounded.sampled(&out.framed), b, materials, g.u, (width as usize, height as usize)));
    out.live = ground.as_ref().zip(out.fixed.as_ref()).map(|(g, f)| look::live(f, g));
    out.grounded = Some(grounded);
    out.repad(found, scene, options);
    out
}

pub fn still(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> Bundle {
    let parts = parts(image.dimensions(), (canvas(image, scene.width, scene.height).into_owned(), None), found, placement, scene, options, materials);
    let mut out = parts.tables(materials);
    out.canvas = parts.framed.canvas.into_raw();
    out
}

pub fn regions(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> (Vec<[u8; 4]>, u32, u32) {
    let parts = parts(image.dimensions(), (canvas(image, scene.width, scene.height).into_owned(), None), found, placement, scene, options, materials);
    let grid = &parts.framed.grid;
    let cells = match &parts.fixed {
        Some(f) => f.sky.iter().zip(&f.belong).map(|(s, b)| [look::byte(*s), look::byte(*b), 0, 0]).collect(),
        None => vec![[0; 4]; grid.w * grid.h],
    };
    (cells, grid.w as u32, grid.h as u32)
}

pub fn solid_cells(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> (Vec<bool>, u32, u32) {
    let parts = parts(image.dimensions(), (canvas(image, scene.width, scene.height).into_owned(), None), found, placement, scene, options, materials);
    match &parts.fixed {
        Some(f) => (f.solid_cells(), f.paint_grid.w as u32, f.paint_grid.h as u32),
        None => (Vec::new(), 0, 0),
    }
}

pub fn belonging_map(image: &RgbImage, found: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> (Vec<[f32; 2]>, u32, u32) {
    let parts = parts(image.dimensions(), (canvas(image, scene.width, scene.height).into_owned(), None), found, placement, scene, options, materials);
    match &parts.belonging {
        Some(b) => (b.sky.iter().zip(&b.belong).map(|(s, k)| [*s, *k]).collect(), b.grid.w as u32, b.grid.h as u32),
        None => (Vec::new(), 0, 0),
    }
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
    (0..count).map(|x| probe_at(x as f32 + 0.5, cell, cells)).collect()
}

fn probe_at(at: f32, cell: usize, cells: usize) -> Probe {
    let g = (at / cell as f32 - 0.5).clamp(0.0, (cells - 1) as f32);
    let a = g as usize;
    Probe { a, b: (a + 1).min(cells - 1), t: g - a as f32 }
}

fn followed(own: &[f64], other: &[f64], s: f32, cell: usize, cells: usize) -> Vec<Probe> {
    let own: Vec<f32> = own.iter().map(|v| *v as f32).collect();
    let other: Vec<f32> = other.iter().map(|v| *v as f32).collect();
    let last = own.len().saturating_sub(1);
    let seek = |want: f32, start: f32| {
        let mut q = start;
        for _ in 0..FOLLOW_STEPS {
            let g = (q - 0.5).clamp(0.0, last as f32);
            let i = (g as usize).min(last.saturating_sub(1));
            let a = own[i];
            let slope = own[(i + 1).min(last)] - a;
            if slope != 0.0 {
                q += (want - (a + slope * (g - i as f32))) / slope;
            }
        }
        q
    };
    (0..own.len())
        .map(|i| {
            let (lo, hi) = (i.saturating_sub(1), (i + 1).min(last));
            let slope = (own[hi] - own[lo]) / (hi - lo).max(1) as f32;
            let want = own[i] + (other[i] - own[i]) * s;
            let start = i as f32 + 0.5 + if slope != 0.0 { (want - own[i]) / slope } else { 0.0 };
            probe_at(seek(want, start), cell, cells)
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

fn line4(values: &[[f32; 4]], w: usize, row: &Probe, (lo, hi): (usize, usize), out: &mut Vec<[f32; 4]>) {
    let (top, bottom) = (&values[row.a * w..(row.a + 1) * w], &values[row.b * w..(row.b + 1) * w]);
    out.resize(w, [0.0; 4]);
    for (o, (p, q)) in out[lo..hi].iter_mut().zip(top[lo..hi].iter().zip(&bottom[lo..hi])) {
        *o = std::array::from_fn(|k| p[k] + (q[k] - p[k]) * row.t);
    }
}

fn line1(values: &[f32], w: usize, row: &Probe, (lo, hi): (usize, usize), out: &mut Vec<f32>) {
    let (top, bottom) = (&values[row.a * w..(row.a + 1) * w], &values[row.b * w..(row.b + 1) * w]);
    out.resize(w, 0.0);
    for (o, (p, q)) in out[lo..hi].iter_mut().zip(top[lo..hi].iter().zip(&bottom[lo..hi])) {
        *o = p + (q - p) * row.t;
    }
}

#[derive(Default)]
struct Lines {
    fields: Vec<Vec<f32>>,
    looks: Vec<Vec<[f32; 4]>>,
    tints: Vec<Vec<Vec<[f32; 4]>>>,
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
    let side = reach * std::f32::consts::FRAC_1_SQRT_2;
    here.max(change(side, side)).max(change(side, -side)).max(change(-side, side)).max(change(-side, -side))
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
        let body = mix3(rgb(sand), rgb(layer(WET)), here.at(3)).map(|v| v * grain);
        let luma = LUMA[0] * body[0] + LUMA[1] * body[1] + LUMA[2] * body[2];
        let deeper = 1.0 - broad * LABEL_DEEPEN.max(1.0 - LABEL_CAP / luma.max(1e-4));
        land.over(body.map(|v| (v * deeper).min(1.0)), here.at(2));
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

pub fn frame_through(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    if !compatible(a, b) {
        return crossfade(a, b, s.clamp(0.0, 1.0));
    }
    let (mut picture, layer) = draw_both(a, b, s, &a.canvas, Glide::ByKind, false);
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
    draw_both(a, b, s, canvas, glide, true)
}

fn draw_both(a: &Bundle, b: &Bundle, s: f32, canvas: &[u8], glide: Glide, ends: bool) -> (RgbImage, RgbaImage) {
    let s = if s.is_finite() { s.clamp(0.0, 1.0) } else { 0.0 };
    let (a, b, s) = if !ends && compatible(a, b) {
        (a, b, s)
    } else if !compatible(a, b) {
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
    let same = std::ptr::eq(a, b) || (ends && s == 0.0);
    let (xmap, ymap): (Cow<[f64]>, Cow<[f64]>) = if same {
        (Cow::Borrowed(&a.xmap), Cow::Borrowed(&a.ymap))
    } else {
        let blend = |p: &[f64], q: &[f64]| p.iter().zip(q).map(|(x, y)| x + (y - x) * s as f64).collect::<Vec<f64>>();
        (Cow::Owned(blend(&a.xmap, &b.xmap)), Cow::Owned(blend(&a.ymap, &b.ymap)))
    };
    let ratio = if same { a.ratio } else { a.ratio.max(b.ratio) };
    let picture = render::picture((canvas, a.canvas_width, a.canvas_height), (&xmap, &ymap), ratio, (width, height));
    finish_with(a, b, s, picture, None, glide, ends)
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
    finish_with(a, b, s, picture, scope, Glide::ByKind, true)
}

fn finish_with(a: &Bundle, b: &Bundle, s: f32, mut picture: RgbImage, scope: Option<&Scope>, glide: Glide, ends: bool) -> (RgbImage, RgbaImage) {
    let same = std::ptr::eq(a, b) || (ends && s == 0.0);
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
    let follows = !same && (a.xmap != b.xmap || a.ymap != b.ymap);
    let held = follows.then(|| {
        [
            (followed(&a.xmap, &b.xmap, s, P, paint_grid.w), followed(&a.ymap, &b.ymap, s, P, paint_grid.h)),
            (followed(&b.xmap, &a.xmap, 1.0 - s, P, paint_grid.w), followed(&b.ymap, &a.ymap, 1.0 - s, P, paint_grid.h)),
        ]
    });
    let out: &mut [u8] = &mut layer;
    out.par_chunks_mut(w * 4).enumerate().for_each_init(Lines::default, |lines, (y, line)| {
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
        lines.fields.resize_with(fields.len(), Vec::new);
        lines.looks.resize_with(planes.len(), Vec::new);
        lines.tints.resize_with(planes.len(), Vec::new);
        for (f, out) in fields.iter().zip(&mut lines.fields) {
            line1(f, grid.w, row, span, out);
        }
        for ((p, look), tint) in planes.iter().zip(&mut lines.looks).zip(&mut lines.tints) {
            line4(&p.look, grid.w, row, span, look);
            tint.resize_with(p.paints.len(), Vec::new);
            for (layer, out) in p.paints.iter().zip(tint.iter_mut()) {
                line4(layer, paint_grid.w, tint_row, tint_span, out);
            }
        }
        let (field_lines, look_lines, tint_lines) = (&lines.fields, &lines.looks, &lines.tints);
        let cells = line.as_chunks_mut::<4>().0;
        for x in ranges.into_iter().flat_map(|(x0, x1)| x0..x1) {
            let px = &mut cells[x];
            let column = &columns[x];
            if !active[j * grid.w + column.a] {
                continue;
            }
            let mut ds = [f32::MAX; 2];
            for (slot, f) in ds.iter_mut().zip(field_lines.iter()) {
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
                let bump = 4.0 * s * (1.0 - s);
                let (shared, near, most) = (da.max(db), ta.min(tb), c.shadow[0] * bump * bump);
                let felt = near > shared && near.min(shared + most) > GLIDE_FELT.0 && shared < GLIDE_FELT.1;
                let change = if felt { gap(fields[0], fields[1], &grid, (x as f32 + 0.5, y as f32 + 0.5), (da - db).abs(), GLIDE_SPAN * c.unit) } else { 0.0 };
                let kept = shared + most * smoothstep(0.0, c.shadow[1], change);
                let d = kept.min(ta.min(tb));
                let mut w = smoothstep(-c.grain_floor, c.grain_floor, ta - tb);
                w *= (GLIDE_EDGE * s).min(1.0);
                w = 1.0 - (1.0 - w) * (GLIDE_EDGE * (1.0 - s)).min(1.0);
                let ((sa, ra), (sb, rb)) = (part(0, d, da), part(1, d, db));
                under((sa.scaled(1.0 - s).plus(sb.scaled(s)), ra.scaled(1.0 - w).plus(rb.scaled(w))))
            };
            let sky = |k: usize| lerp4(&look_lines[k], column)[0] * (1.0 - smoothstep(0.5 * c.reach, c.reach, ds[k]));
            let tide = if planes.len() == 1 || glide == Glide::Dissolve { 0.0 } else { 1.0 - sky(0).max(sky(1)) };
            let total = if planes.len() == 1 {
                under(part(0, ds[0], ds[0]))
            } else if tide <= 0.0 {
                faded()
            } else if tide >= 1.0 {
                moved()
            } else {
                faded().scaled(1.0 - tide).plus(moved().scaled(tide))
            };
            let guard = |k: usize| match &held {
                Some(held) => bilinear4(&planes[k].paints[GUARD], paint_grid.w, &held[k].0[x], &held[k].1[y])[3],
                None => lerp4(&tint_lines[k][GUARD], tint_column)[3],
            };
            let block = if planes.len() == 1 { guard(0) } else { (guard(0) * (GLIDE_EDGE * (1.0 - s)).min(1.0)).max(guard(1) * (GLIDE_EDGE * s).min(1.0)) };
            let total = total.scaled(1.0 - block);
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
    fn the_shaders_hold_the_numbers_the_cpu_frame_uses() {
        let (wgsl, glsl) = (include_str!("../shaders/frame.wgsl"), include_str!("../shaders/frame.glsl"));
        let side = std::f32::consts::FRAC_1_SQRT_2;
        let both = [
            (format!("> {:?} && both < {:?}", GLIDE_FELT.0, GLIDE_FELT.1), 1),
            (format!("{:?} * (grain_hash(", GRAIN_DOUBLE), 1),
            ("* grain;".to_string(), 1),
            (format!("1.0 - broad * max({LABEL_DEEPEN:?}, 1.0 - {LABEL_CAP:?} / max(dot(body, "), 1),
            (format!("({:?}, {:?}, {:?})), 0.0001));", LUMA[0], LUMA[1], LUMA[2]), 1),
            (format!("(x * {:#x}u) ^ (y * {:#x}u)", GRAIN_MIX[0], GRAIN_MIX[1]), 1),
            (format!("h * {:#x}u", GRAIN_MIX[2]), 1),
            (format!("h ^ (h >> {}u)", GRAIN_SHIFT[0]), 1),
            (format!("h ^ (h >> {}u)", GRAIN_SHIFT[1]), 1),
            (format!("(h >> {}u) / 16777215.0", GRAIN_SHIFT[2]), 1),
        ];
        for (text, count) in &both {
            assert_eq!(wgsl.matches(text.as_str()).count(), *count, "wgsl {text}");
            assert_eq!(glsl.matches(text.as_str()).count(), *count, "glsl {text}");
        }
        let edge = format!("min({:?} * ", GLIDE_EDGE);
        assert_eq!((wgsl.matches(edge.as_str()).count(), glsl.matches(edge.as_str()).count()), (4, 4), "{edge}");
        let guard = format!(", {GUARD})");
        assert_eq!((wgsl.matches(guard.as_str()).count(), glsl.matches(guard.as_str()).count()), (2, 2), "{guard}");
        let guards = [
            (format!("let guard_a = layer(paints_a, follow(map_a, pixel, from_a, uv, p), {GUARD}).a;"), format!("float guard_a = layer(0, follow(0, pixel, from_a, uv, p), {GUARD}).a;")),
            (format!("let guard_b = layer(paints_b, follow(map_b, pixel, from_b, uv, p), {GUARD}).a;"), format!("float guard_b = layer(1, follow(1, pixel, from_b, uv, p), {GUARD}).a;")),
            (
                format!("let block = max(guard_a * min({GLIDE_EDGE:?} * (1.0 - frame.s), 1.0), guard_b * min({GLIDE_EDGE:?} * frame.s, 1.0));"),
                format!("float block = max(guard_a * min({GLIDE_EDGE:?} * (1.0 - s), 1.0), guard_b * min({GLIDE_EDGE:?} * s, 1.0));"),
            ),
        ];
        for (w, g) in &guards {
            assert_eq!(wgsl.matches(w.as_str()).count(), 1, "wgsl {w}");
            assert_eq!(glsl.matches(g.as_str()).count(), 1, "glsl {g}");
        }
        assert_eq!(wgsl.matches(&format!("for (var k = 0; k < {FOLLOW_STEPS}; k = k + 1)")).count(), 1);
        assert_eq!(glsl.matches(&format!("for (int k = 0; k < {FOLLOW_STEPS}; k++)")).count(), 1);
        assert_eq!(wgsl.matches(&format!("let side = {:?} * frame.unit * {side:?};", GLIDE_SPAN)).count(), 1);
        assert_eq!(glsl.matches(&format!("float side = {:?} * unit * {side:?};", GLIDE_SPAN)).count(), 1);
    }

    #[test]
    fn the_glide_gate_is_skipped_only_where_the_ramps_are_flat() {
        for u in [shape::UNIT_FLOOR, 24.0, 60.0, 110.0, 240.0] {
            let table = ramps(u);
            let entry = |k: usize| [table[k * 4 + 1], table[k * 4 + 2], table[k * 4 + 3], table[(RAMP + k) * 4], table[(RAMP + k) * 4 + 1], table[(RAMP + k) * 4 + 2]];
            let at = |k: usize| RAMP_RANGE.0 + (RAMP_RANGE.1 - RAMP_RANGE.0) * k as f32 / (RAMP - 1) as f32;
            let step = (RAMP_RANGE.1 - RAMP_RANGE.0) / (RAMP - 1) as f32;
            let low: Vec<usize> = (0..RAMP).filter(|k| at(*k) <= GLIDE_FELT.0 + step).collect();
            let high: Vec<usize> = (0..RAMP).filter(|k| at(*k) >= GLIDE_FELT.1 - step).collect();
            assert!(low.iter().all(|k| entry(*k) == entry(0)), "u {u} the ramps still change below {}", GLIDE_FELT.0);
            assert!(high.iter().all(|k| entry(*k) == entry(RAMP - 1)), "u {u} the ramps still change above {}", GLIDE_FELT.1);
        }
    }

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
