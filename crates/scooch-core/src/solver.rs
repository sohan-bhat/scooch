use crate::analysis::quantile;
use crate::sat::{Place, Sat};
use crate::table::{self, Curve, STEPS};
use crate::warp::{self, Pchip, Plan, BINS};
use crate::{look, scenery, shape, Analysis, Options, Placement, Scene, TABLE};
use rayon::prelude::*;
use std::time::Duration;

const ZOOM_STEP: f64 = 0.05;
const MOST_ZOOM: f64 = 2.0;
const MOST_STRENGTH: f64 = 2.0;
const PANS: usize = 33;
const GROW: f64 = 0.08;
const HIDDEN: f64 = 4.0;
const BUSY: f64 = 0.35;
const CROPPED: f64 = 1.5;
const ZOOMED: f64 = 0.6;
const OFF_CENTER: f64 = 0.15;
const MIRRORED: f64 = 0.25;
const BORDER: f64 = 2.0;
const DISTORTION: f64 = 1.5;
const PEAK_RANK: f64 = 0.97;
const SOLID: f32 = 0.5;
const EDGE_SLACK: f64 = 0.5;
const KEEP_FLOOR: f64 = 0.02;
const KEEP_SHARE: f64 = 0.10;
const EPS: f64 = 1e-9;
const TIE: f64 = 1e-12;
const HOME_KEEP: f64 = 0.90;
const HOME_NEAR: f64 = 0.02;
const HOME_BORDER: f64 = 0.1;
const COVER_CELL: f32 = 1.0 / 3.0;
const NEAR_PANS: usize = 3;
const NEAR_SHARE: f64 = 0.8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
}

#[derive(Clone, Copy)]
struct Boxes<'a> {
    icons: &'a [Footprint],
    cover: &'a [Footprint],
}

impl<'a> Boxes<'a> {
    fn bare(icons: &'a [Footprint]) -> Self {
        Self { icons, cover: &[] }
    }
}

pub fn screen_rects(scene: &Scene) -> Vec<[f64; 4]> {
    let (width, height) = (scene.width as f64, scene.height as f64);
    scene
        .icons
        .iter()
        .filter_map(|r| {
            let (x, y, w, h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
            if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite()) || w <= 0.0 || h <= 0.0 {
                return None;
            }
            let rect = [x.max(0.0), y.max(0.0), (x + w).min(width), (y + h).min(height)];
            (rect[2] > rect[0] && rect[3] > rect[1]).then_some(rect)
        })
        .collect()
}

pub fn footprints(scene: &Scene) -> Vec<Footprint> {
    if scene.width == 0 || scene.height == 0 {
        return Vec::new();
    }
    let (width, height) = (scene.width as f64, scene.height as f64);
    screen_rects(scene)
        .into_iter()
        .map(|[x0, y0, x1, y1]| {
            let (gx, gy) = (GROW * (x1 - x0), GROW * (y1 - y0));
            Footprint {
                u0: ((x0 - gx) / width).clamp(0.0, 1.0),
                v0: ((y0 - gy) / height).clamp(0.0, 1.0),
                u1: ((x1 + gx) / width).clamp(0.0, 1.0),
                v1: ((y1 + gy) / height).clamp(0.0, 1.0),
            }
        })
        .collect()
}

struct Settings {
    strength: f64,
    max_zoom: f64,
    mirror: bool,
}

fn settings(options: &Options) -> Settings {
    let strength = if options.strength.is_finite() { (options.strength as f64).clamp(0.0, MOST_STRENGTH) } else { 1.0 };
    let max_zoom = if options.max_zoom.is_finite() { (options.max_zoom as f64).clamp(1.0, MOST_ZOOM) } else { 1.3 };
    Settings { strength, max_zoom, mirror: options.mirror }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Frame {
    z: f64,
    x0: f64,
    y0: f64,
    cw: f64,
    ch: f64,
    mirrored: bool,
}

impl Frame {
    fn off(&self, center: [f64; 2]) -> f64 {
        let part = |start: f64, span: f64, middle: f64| {
            let slack = 1.0 - span;
            if slack > 1e-9 {
                ((start + span / 2.0 - middle) / (slack / 2.0)).clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        part(self.x0, self.cw, center[0]).powi(2) + part(self.y0, self.ch, center[1]).powi(2)
    }

    fn of(placement: &Placement) -> Option<Self> {
        let [x0, y0, cw, ch] = placement.crop.map(|v| v as f64);
        let ok = [x0, y0, cw, ch, placement.zoom as f64].iter().all(|v| v.is_finite()) && cw > 0.0 && ch > 0.0 && x0 >= -1e-4 && y0 >= -1e-4 && x0 + cw <= 1.0 + 1e-4 && y0 + ch <= 1.0 + 1e-4;
        ok.then_some(Self { z: placement.zoom as f64, x0, y0, cw, ch, mirrored: placement.mirrored })
    }

    fn settled(&self, center: [f64; 2]) -> f64 {
        ZOOMED * (self.z - 1.0).max(0.0) + OFF_CENTER * self.off(center) + if self.mirrored { MIRRORED } else { 0.0 }
    }
}

struct Field {
    gw: f64,
    gh: f64,
    columns: usize,
    rows: usize,
    saliency: Vec<f32>,
    busy: Vec<f32>,
    sb: Sat<3>,
    total_s: f64,
    size: f64,
    image_aspect: f64,
    screen_aspect: f64,
    flat: bool,
    middle: Frame,
    home: Frame,
    center: [f64; 2],
    kept: f64,
    edge: f64,
    exempt: [bool; 4],
}

impl Field {
    fn new(analysis: &Analysis, scene: &Scene) -> Option<Self> {
        let (columns, rows) = (analysis.width as usize, analysis.height as usize);
        let n = columns * rows;
        if n == 0 || analysis.saliency.len() != n || analysis.busy.len() != n || scene.width == 0 || scene.height == 0 {
            return None;
        }
        let clean = |v: &f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
        let saliency: Vec<f32> = if analysis.flat { vec![0.0; n] } else { analysis.saliency.iter().map(clean).collect() };
        let busy: Vec<f32> = analysis.busy.iter().map(clean).collect();
        let scenic: Vec<f32> = saliency.iter().zip(&busy).map(|(s, b)| s * look::belonging(*s, *b)).collect();
        let sb = Sat::new([&saliency, &busy, &scenic], columns, rows);
        let image_aspect = if analysis.source_width > 0 && analysis.source_height > 0 { table::aspect(analysis.source_width, analysis.source_height) } else { table::aspect(analysis.width, analysis.height) };
        let screen_aspect = table::aspect(scene.width, scene.height);
        let (cw, ch) = table::cover(image_aspect, screen_aspect, 1.0);
        let middle = Frame { z: 1.0, x0: (1.0 - cw) / 2.0, y0: (1.0 - ch) / 2.0, cw, ch, mirrored: false };
        let solid = saliency.iter().filter(|s| **s >= SOLID).count() as f64;
        let mut field = Self {
            gw: columns as f64,
            gh: rows as f64,
            columns,
            rows,
            total_s: sb.total()[0],
            saliency,
            busy,
            sb,
            size: solid.sqrt().max(1.0),
            image_aspect,
            screen_aspect,
            flat: analysis.flat,
            middle,
            home: middle,
            center: [0.5, 0.5],
            kept: 0.0,
            edge: 0.0,
            exempt: [true; 4],
        };
        if let Some(home) = field.rest_frame() {
            field.center = [home.x0 + home.cw / 2.0, home.y0 + home.ch / 2.0];
            field.home = home;
        }
        let home = field.home;
        field.kept = field.crop_sum(&home);
        field.exempt = field.sides(&home);
        field.edge = field.border(&home);
        Some(field)
    }

    fn rest_frame(&self) -> Option<Frame> {
        let middle = self.middle;
        let total = self.total_s;
        if self.flat || total <= EPS {
            return None;
        }
        let along_x = middle.cw < 1.0 - 1e-9;
        let slack = if along_x { 1.0 - middle.cw } else { 1.0 - middle.ch };
        if slack <= 1e-9 {
            return None;
        }
        let grid: Vec<Frame> = pans(slack).into_iter().map(|p| if along_x { Frame { x0: p, ..middle } } else { Frame { y0: p, ..middle } }).collect();
        let kept: Vec<f64> = grid.iter().map(|f| self.crop_sum(f)).collect();
        let reach = kept.iter().copied().fold(0.0f64, f64::max);
        let centroid = self.centroid(along_x);
        let start = |f: &Frame| if along_x { f.x0 + f.cw / 2.0 } else { f.y0 + f.ch / 2.0 };
        let best = (0..grid.len())
            .filter(|i| kept[*i] >= reach - HOME_NEAR * total)
            .min_by(|a, b| {
                let (fa, fb) = (start(&grid[*a]), start(&grid[*b]));
                (fa - centroid).abs().total_cmp(&(fb - centroid).abs()).then((fa - 0.5).abs().total_cmp(&(fb - 0.5).abs())).then(a.cmp(b))
            })
            .map(|i| grid[i])?;
        let crossed = (self.border(&middle) - self.border(&best)) / self.size;
        let leave = self.crop_sum(&middle) < HOME_KEEP * reach || crossed > HOME_BORDER;
        (leave && best != middle).then_some(best)
    }

    fn centroid(&self, along_x: bool) -> f64 {
        let solid = self.saliency.iter().any(|s| *s >= SOLID);
        let (mut sum, mut weight) = (0.0f64, 0.0f64);
        for (i, s) in self.saliency.iter().enumerate() {
            let w = if solid { if *s >= SOLID { *s as f64 } else { 0.0 } } else { *s as f64 };
            let at = if along_x { ((i % self.columns) as f64 + 0.5) / self.gw } else { ((i / self.columns) as f64 + 0.5) / self.gh };
            sum += w * at;
            weight += w;
        }
        if weight > 0.0 { sum / weight } else { 0.5 }
    }

    fn rest(&self, (sw, sh): (u32, u32), scene: &Scene) -> Placement {
        if self.home == self.middle {
            table::fill(sw, sh, scene.width, scene.height)
        } else {
            assemble(&self.home, None, None)
        }
    }

    fn crop_sum(&self, frame: &Frame) -> f64 {
        self.sb.sum(frame.x0 * self.gw, frame.y0 * self.gh, (frame.x0 + frame.cw) * self.gw, (frame.y0 + frame.ch) * self.gh)[0]
    }

    fn crop_loss(&self, frame: &Frame) -> f64 {
        let middle = &self.home;
        let (x0, x1) = (frame.x0.max(middle.x0), (frame.x0 + frame.cw).min(middle.x0 + middle.cw));
        let (y0, y1) = (frame.y0.max(middle.y0), (frame.y0 + frame.ch).min(middle.y0 + middle.ch));
        let shared = if x1 > x0 && y1 > y0 { self.sb.sum(x0 * self.gw, y0 * self.gh, x1 * self.gw, y1 * self.gh)[0] } else { 0.0 };
        (self.kept - shared).max(0.0)
    }

    fn sides(&self, frame: &Frame) -> [bool; 4] {
        let (y0, y1) = (frame.y0 * self.gh, (frame.y0 + frame.ch) * self.gh);
        let (x0, x1) = (frame.x0 * self.gw, (frame.x0 + frame.cw) * self.gw);
        [y0 <= EDGE_SLACK, y1 >= self.gh - EDGE_SLACK, x0 <= EDGE_SLACK, x1 >= self.gw - EDGE_SLACK]
    }

    fn border(&self, frame: &Frame) -> f64 {
        let (x0, x1) = (frame.x0 * self.gw, (frame.x0 + frame.cw) * self.gw);
        let (y0, y1) = (frame.y0 * self.gh, (frame.y0 + frame.ch) * self.gh);
        let on_edge = self.sides(frame);
        let [top, bottom, left, right] = std::array::from_fn(|i| !(on_edge[i] && self.exempt[i]));
        let sum = |a: f64, b: f64, c: f64, d: f64| if c > a && d > b { self.sb.sum(a, b, c, d)[0] } else { 0.0 };
        let (inner_top, inner_bottom) = (if top { y0 + 1.0 } else { y0 }, if bottom { y1 - 1.0 } else { y1 });
        let mut out = 0.0;
        if top {
            out += sum(x0, y0, x1, y0 + 1.0);
        }
        if bottom {
            out += sum(x0, y1 - 1.0, x1, y1);
        }
        if left {
            out += sum(x0, inner_top, x0 + 1.0, inner_bottom);
        }
        if right {
            out += sum(x1 - 1.0, inner_top, x1, inner_bottom);
        }
        out
    }

    fn cut(&self, frame: &Frame) -> f64 {
        (self.border(frame) - self.edge).max(0.0) / self.size
    }

    fn frame_cost(&self, frame: &Frame, hid: f64, busy: f64) -> f64 {
        let mut cost = BUSY * busy + frame.settled(self.center);
        if !self.flat {
            let total = self.total_s.max(EPS);
            cost += HIDDEN * hid / total + CROPPED * self.crop_loss(frame) / total + BORDER * self.cut(frame);
        }
        cost
    }
}

#[derive(Clone, Copy)]
struct AxisMap<'a> {
    start: f64,
    span: f64,
    mirrored: bool,
    curve: Option<&'a Pchip>,
}

impl<'a> AxisMap<'a> {
    fn x(frame: &Frame, curve: Option<&'a Pchip>) -> Self {
        Self { start: frame.x0, span: frame.cw, mirrored: frame.mirrored, curve }
    }

    fn y(frame: &Frame, curve: Option<&'a Pchip>) -> Self {
        Self { start: frame.y0, span: frame.ch, mirrored: false, curve }
    }

    fn source(&self, u: f64) -> f64 {
        let t = match self.curve {
            Some(curve) => curve.at(u),
            None => u.clamp(0.0, 1.0),
        };
        if self.mirrored {
            self.start + self.span * (1.0 - t)
        } else {
            self.start + self.span * t
        }
    }

    fn interval(&self, a: f64, b: f64) -> (f64, f64) {
        let (p, q) = (self.source(a), self.source(b));
        (p.min(q), p.max(q))
    }

    fn table(&self) -> Vec<f32> {
        let mut out: Vec<f32> = match self.curve {
            None if self.mirrored => table::line(self.start + self.span, self.start),
            None => table::line(self.start, self.start + self.span),
            Some(_) => (0..TABLE).map(|k| self.source(k as f64 / STEPS as f64) as f32).collect(),
        };
        out.iter_mut().for_each(|v| *v = v.clamp(0.0, 1.0));
        out
    }
}

#[derive(Clone, Copy, Debug)]
struct Candidate {
    zi: usize,
    xi: usize,
    yi: usize,
    frame: Frame,
    off: f64,
    cost: f64,
}

fn better(a: &Candidate, b: &Candidate) -> bool {
    if a.cost < b.cost - TIE {
        return true;
    }
    if a.cost > b.cost + TIE {
        return false;
    }
    if a.zi != b.zi {
        return a.zi < b.zi;
    }
    if (a.off - b.off).abs() > TIE {
        return a.off < b.off;
    }
    if a.frame.mirrored != b.frame.mirrored {
        return !a.frame.mirrored;
    }
    (a.xi, a.yi) < (b.xi, b.yi)
}

fn pans(slack: f64) -> Vec<f64> {
    if slack > 1e-9 {
        (0..PANS).map(|i| slack * i as f64 / (PANS - 1) as f64).collect()
    } else {
        vec![0.0]
    }
}

fn under_icons(field: &Field, boxes: &Boxes, x: &AxisMap, y: &AxisMap) -> (f64, f64) {
    let (mut hid, mut busy, mut area) = (0.0f64, 0.0f64, 0.0f64);
    for b in boxes.cover {
        let (xa, xb) = x.interval(b.u0, b.u1);
        let (ya, yb) = y.interval(b.v0, b.v1);
        hid += field.sb.sum(xa * field.gw, ya * field.gh, xb * field.gw, yb * field.gh)[2];
    }
    for b in boxes.icons {
        let (xa, xb) = x.interval(b.u0, b.u1);
        let (ya, yb) = y.interval(b.v0, b.v1);
        let [s, z, _] = field.sb.sum(xa * field.gw, ya * field.gh, xb * field.gw, yb * field.gh);
        hid += s;
        busy += z;
        area += (xb - xa) * field.gw * (yb - ya) * field.gh;
    }
    (hid, if area > 0.0 { busy / area } else { 0.0 })
}

fn zoom_levels(settings: &Settings) -> usize {
    ((settings.max_zoom - 1.0) / ZOOM_STEP + 1e-6).floor() as usize + 1
}

fn stage_one(field: &Field, boxes: &Boxes, settings: &Settings) -> Option<Vec<Frame>> {
    let middle = field.home;
    let (hid_c, busy_c) = under_icons(field, boxes, &AxisMap::x(&middle, None), &AxisMap::y(&middle, None));
    if hid_c <= 0.0 && busy_c <= 0.0 {
        return None;
    }
    let zooms = zoom_levels(settings);
    let mut rows = Vec::new();
    for zi in 0..zooms {
        for mirrored in [false, true] {
            if mirrored && !settings.mirror {
                continue;
            }
            let z = 1.0 + ZOOM_STEP * zi as f64;
            let (cw, ch) = table::cover(field.image_aspect, field.screen_aspect, z);
            for (xi, x0) in pans(1.0 - cw).into_iter().enumerate() {
                rows.push((zi, z, cw, ch, mirrored, xi, x0));
            }
        }
    }
    let lifts: Vec<Vec<Vec<(Place, Place)>>> = (0..zooms)
        .map(|zi| {
            let z = 1.0 + ZOOM_STEP * zi as f64;
            let (_, ch) = table::cover(field.image_aspect, field.screen_aspect, z);
            pans(1.0 - ch).into_iter().map(|y0| boxes.icons.iter().chain(boxes.cover).map(|b| (field.sb.place_y((y0 + ch * b.v0) * field.gh), field.sb.place_y((y0 + ch * b.v1) * field.gh))).collect()).collect()
        })
        .collect();
    let limit = hid_c * (1.0 + 1e-9) + 1e-12;
    let scored: Vec<Vec<Candidate>> = rows
        .par_iter()
        .map(|&(zi, z, cw, ch, mirrored, xi, x0)| {
            let spans: Vec<(Place, Place)> = boxes
                .icons
                .iter()
                .chain(boxes.cover)
                .map(|b| {
                    let (ta, tb) = if mirrored { (1.0 - b.u1, 1.0 - b.u0) } else { (b.u0, b.u1) };
                    (field.sb.place_x((x0 + cw * ta) * field.gw), field.sb.place_x((x0 + cw * tb) * field.gw))
                })
                .collect();
            pans(1.0 - ch)
                .into_iter()
                .enumerate()
                .filter_map(|(yi, y0)| {
                    let frame = Frame { z, x0, y0, cw, ch, mirrored };
                    let (mut hid, mut busy, mut area) = (0.0f64, 0.0f64, 0.0f64);
                    for (k, (&(xa, xb), &(ya, yb))) in spans.iter().zip(&lifts[zi][yi]).enumerate() {
                        let [s, b, h] = field.sb.sum_placed(xa, xb, ya, yb);
                        if k >= boxes.icons.len() {
                            hid += h;
                            continue;
                        }
                        hid += s;
                        if !field.flat && hid > limit {
                            return None;
                        }
                        busy += b;
                        area += (xb.at - xa.at).max(0.0) * (yb.at - ya.at).max(0.0);
                    }
                    let busy = if area > 0.0 { busy / area } else { 0.0 };
                    Some(Candidate { zi, xi, yi, frame, off: frame.off(field.center), cost: field.frame_cost(&frame, hid, busy) })
                })
                .collect()
        })
        .collect();
    let mut best: Vec<Option<Candidate>> = vec![None; zooms];
    for candidate in scored.iter().flatten() {
        let slot = &mut best[candidate.zi];
        if slot.as_ref().is_none_or(|b| better(candidate, b)) {
            *slot = Some(*candidate);
        }
    }
    let mut frames: Vec<Frame> = best.into_iter().flatten().map(|c| c.frame).collect();
    if !frames.contains(&middle) {
        frames.insert(0, middle);
    }
    Some(frames)
}

fn cells(a: f64, b: f64, n: usize) -> (usize, usize) {
    let lo = ((a * n as f64).floor().max(0.0) as usize).min(n - 1);
    let hi = (((b * n as f64) - 1e-9).ceil().max(0.0) as usize).clamp(lo + 1, n);
    (lo, hi)
}

struct Profile {
    peak: Vec<f64>,
    busy: Vec<f64>,
}

fn profile(field: &Field, frame: &Frame, along_x: bool) -> Profile {
    let (others, own) = if along_x { (cells(frame.y0, frame.y0 + frame.ch, field.rows), field.columns) } else { (cells(frame.x0, frame.x0 + frame.cw, field.columns), field.rows) };
    let bins: Vec<(f64, f64)> = (0..BINS)
        .into_par_iter()
        .map(|k| {
            let (ta, tb) = (k as f64 / BINS as f64, (k + 1) as f64 / BINS as f64);
            let (sa, sb) = if along_x {
                if frame.mirrored {
                    (frame.x0 + frame.cw * (1.0 - tb), frame.x0 + frame.cw * (1.0 - ta))
                } else {
                    (frame.x0 + frame.cw * ta, frame.x0 + frame.cw * tb)
                }
            } else {
                (frame.y0 + frame.ch * ta, frame.y0 + frame.ch * tb)
            };
            let (a0, a1) = cells(sa, sb, own);
            let mut values = Vec::with_capacity((a1 - a0) * (others.1 - others.0));
            let mut busy = 0.0f64;
            for a in a0..a1 {
                for o in others.0..others.1 {
                    let index = if along_x { o * field.columns + a } else { a * field.columns + o };
                    values.push(field.saliency[index]);
                    busy += field.busy[index] as f64;
                }
            }
            let count = values.len().max(1) as f64;
            (quantile(&values, PEAK_RANK) as f64, busy / count)
        })
        .collect();
    Profile { peak: bins.iter().map(|b| b.0).collect(), busy: bins.iter().map(|b| b.1).collect() }
}

struct Bend {
    shift: f64,
    curve: Pchip,
}

fn bend(field: &Field, boxes: &Boxes, frame: &Frame, along_x: bool, other: &AxisMap, profile: &Profile, strength: f64) -> Option<Bend> {
    let plan = Plan::new(&profile.peak, &profile.busy, strength)?;
    let total = field.total_s.max(EPS);
    let mut best: Option<(f64, f64, Pchip)> = None;
    for shift in plan.shifts() {
        let scales = plan.scales(shift);
        let curve = Pchip::from_scales(&scales);
        let own = if along_x { AxisMap::x(frame, Some(&curve)) } else { AxisMap::y(frame, Some(&curve)) };
        let (hid, busy) = if along_x { under_icons(field, boxes, &own, other) } else { under_icons(field, boxes, other, &own) };
        let cost = HIDDEN * hid / total + BUSY * busy + DISTORTION * warp::penalty(&scales, &profile.peak, &profile.busy);
        let wins = match &best {
            None => true,
            Some((c, s, _)) => cost < c - TIE || ((cost - c).abs() <= TIE && shift.abs() < s.abs()),
        };
        if wins {
            best = Some((cost, shift, curve));
        }
    }
    let (_, shift, curve) = best?;
    (shift != 0.0).then_some(Bend { shift, curve })
}

fn assemble(frame: &Frame, x: Option<&Bend>, y: Option<&Bend>) -> Placement {
    let xmap = AxisMap::x(frame, x.map(|b| &b.curve));
    let ymap = AxisMap::y(frame, y.map(|b| &b.curve));
    Placement {
        fx: xmap.table(),
        fy: ymap.table(),
        zoom: frame.z as f32,
        mirrored: frame.mirrored,
        crop: [frame.x0 as f32, frame.y0 as f32, frame.cw as f32, frame.ch as f32],
        shift: [x.map_or(0.0, |b| b.shift as f32), y.map_or(0.0, |b| b.shift as f32)],
    }
}

fn through_tables(field: &Field, boxes: &Boxes, placement: &Placement) -> (f64, f64) {
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let (mut hid, mut busy, mut area) = (0.0f64, 0.0f64, 0.0f64);
    for b in boxes.cover {
        let (p, q) = (xc.at(b.u0), xc.at(b.u1));
        let (r, s) = (yc.at(b.v0), yc.at(b.v1));
        hid += field.sb.sum(p.min(q) * field.gw, r.min(s) * field.gh, p.max(q) * field.gw, r.max(s) * field.gh)[2];
    }
    for b in boxes.icons {
        let (p, q) = (xc.at(b.u0), xc.at(b.u1));
        let (r, s) = (yc.at(b.v0), yc.at(b.v1));
        let (xa, xb, ya, yb) = (p.min(q) * field.gw, p.max(q) * field.gw, r.min(s) * field.gh, r.max(s) * field.gh);
        let [h, z, _] = field.sb.sum(xa, ya, xb, yb);
        hid += h;
        busy += z;
        area += (xb - xa) * (yb - ya);
    }
    (hid, if area > 0.0 { busy / area } else { 0.0 })
}

fn hidden_saliency(field: &Field, boxes: &Boxes, placement: &Placement) -> f64 {
    if field.flat || !(table::usable(&placement.fx) && table::usable(&placement.fy)) {
        return 0.0;
    }
    through_tables(field, boxes, placement).0 / field.total_s.max(EPS)
}

fn bin_scales(values: &[f32], start: f64, span: f64, mirrored: bool) -> Vec<f64> {
    let display: Vec<f64> = values
        .iter()
        .map(|v| {
            let t = (*v as f64 - start) / span;
            if mirrored {
                1.0 - t
            } else {
                t
            }
        })
        .collect();
    let last = display.len() - 1;
    let mut edges = Vec::with_capacity(BINS + 1);
    let mut j = 0usize;
    for k in 0..=BINS {
        let target = k as f64 / BINS as f64;
        while j + 1 < last && display[j + 1] < target {
            j += 1;
        }
        let (a, b) = (display[j], display[j + 1]);
        let frac = if b > a { ((target - a) / (b - a)).clamp(0.0, 1.0) } else { 0.0 };
        edges.push((j as f64 + frac) / last as f64);
    }
    edges.windows(2).map(|e| (1.0 / BINS as f64) / (e[1] - e[0]).max(1e-9)).collect()
}

fn distortion(placement: &Placement, frame: &Frame, x: &Profile, y: &Profile) -> f64 {
    let sx = bin_scales(&placement.fx, frame.x0, frame.cw, frame.mirrored);
    let sy = bin_scales(&placement.fy, frame.y0, frame.ch, false);
    DISTORTION * (warp::penalty(&sx, &x.peak, &x.busy) + warp::penalty(&sy, &y.peak, &y.busy))
}

fn score(field: &Field, boxes: &Boxes, placement: &Placement, profiles: Option<(&Profile, &Profile)>) -> f64 {
    let Some(frame) = Frame::of(placement) else { return f64::INFINITY };
    let (hid, busy) = through_tables(field, boxes, placement);
    let base = field.frame_cost(&frame, hid, busy);
    let bent = placement.shift != [0.0, 0.0];
    let warp = match (bent, profiles) {
        (false, _) => 0.0,
        (true, Some((x, y))) => distortion(placement, &frame, x, y),
        (true, None) => distortion(placement, &frame, &profile(field, &frame, true), &profile(field, &frame, false)),
    };
    base + warp
}

fn fits(field: &Field, previous: &Placement, settings: &Settings) -> bool {
    let Some(frame) = Frame::of(previous) else { return false };
    let shape = frame.cw * field.image_aspect / frame.ch;
    table::usable(&previous.fx)
        && table::usable(&previous.fy)
        && previous.fy[STEPS] > previous.fy[0]
        && (previous.fx[STEPS] < previous.fx[0]) == previous.mirrored
        && (settings.mirror || !previous.mirrored)
        && frame.z >= 1.0 - 1e-6
        && ((shape - field.screen_aspect) / field.screen_aspect).abs() < 1e-3
}

struct Choice {
    placement: Placement,
    cost: f64,
}

fn ranks_before(a: &Choice, b: &Choice) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    if a.cost < b.cost - TIE {
        return Ordering::Less;
    }
    if a.cost > b.cost + TIE {
        return Ordering::Greater;
    }
    let moved = |c: &Choice| (c.placement.shift[0].abs() + c.placement.shift[1].abs()) as f64;
    a.placement
        .zoom
        .total_cmp(&b.placement.zoom)
        .then(moved(a).total_cmp(&moved(b)))
        .then(a.placement.mirrored.cmp(&b.placement.mirrored))
        .then(a.placement.crop[0].total_cmp(&b.placement.crop[0]))
        .then(a.placement.crop[1].total_cmp(&b.placement.crop[1]))
}

fn choices(field: &Field, boxes: &Boxes, frame: &Frame, settings: &Settings) -> Vec<Choice> {
    let plain = assemble(frame, None, None);
    let mut out = vec![Choice { cost: score(field, boxes, &plain, None), placement: plain }];
    if field.flat || settings.strength <= 0.0 {
        return out;
    }
    let (px, py) = (profile(field, frame, true), profile(field, frame, false));
    let along = bend(field, boxes, frame, true, &AxisMap::y(frame, None), &px, settings.strength);
    let across = bend(field, boxes, frame, false, &AxisMap::x(frame, along.as_ref().map(|b| &b.curve)), &py, settings.strength);
    if along.is_some() || across.is_some() {
        let warped = assemble(frame, along.as_ref(), across.as_ref());
        if table::usable(&warped.fx) && table::usable(&warped.fy) {
            out.push(Choice { cost: score(field, boxes, &warped, Some((&px, &py))), placement: warped });
        }
    }
    out
}

fn usable<'a>(field: &'a Field, boxes: &'a Boxes<'a>, fallback: &Placement, most_lost: Option<f64>) -> impl Fn(&Choice) -> bool + 'a {
    let limits = [hidden_saliency(field, boxes, fallback), hidden_saliency(field, &Boxes::bare(boxes.icons), fallback)];
    move |c: &Choice| {
        table::usable(&c.placement.fx)
            && table::usable(&c.placement.fy)
            && hidden_saliency(field, boxes, &c.placement) <= limits[0]
            && hidden_saliency(field, &Boxes::bare(boxes.icons), &c.placement) <= limits[1]
            && most_lost.is_none_or(|most| Frame::of(&c.placement).is_some_and(|f| field.crop_loss(&f) <= most + 1e-9))
    }
}

fn fresh(field: &Field, boxes: &Boxes, settings: &Settings, fallback: &Placement, most_lost: Option<f64>) -> Placement {
    let Some(frames) = stage_one(field, boxes, settings) else { return fallback.clone() };
    let mut all: Vec<Choice> = frames.par_iter().flat_map(|frame| choices(field, boxes, frame, settings)).collect();
    all.sort_by(ranks_before);
    let ok = usable(field, boxes, fallback, most_lost);
    all.into_iter().find(|c| ok(c)).map_or_else(|| fallback.clone(), |c| c.placement)
}

fn cover_boxes(scene: &Scene) -> Vec<Footprint> {
    let Some(ground) = shape::ground(scene) else { return Vec::new() };
    let cell = ((ground.u * COVER_CELL) as usize).max(8);
    let Some((grid, mask)) = shape::coverage(scene, cell) else { return Vec::new() };
    let (width, height) = (scene.width as f64, scene.height as f64);
    let footprint = |x0: usize, x1: usize, y0: usize, y1: usize| Footprint {
        u0: ((x0 * cell) as f64 / width).clamp(0.0, 1.0),
        v0: ((y0 * cell) as f64 / height).clamp(0.0, 1.0),
        u1: ((x1 * cell) as f64 / width).clamp(0.0, 1.0),
        v1: ((y1 * cell) as f64 / height).clamp(0.0, 1.0),
    };
    let mut out = Vec::new();
    let mut open: Vec<(usize, usize, usize)> = Vec::new();
    for j in 0..=grid.h {
        let mut runs: Vec<(usize, usize)> = Vec::new();
        if j < grid.h {
            for i in 0..grid.w {
                if mask[j * grid.w + i] {
                    match runs.last_mut() {
                        Some(run) if run.1 == i => run.1 = i + 1,
                        _ => runs.push((i, i + 1)),
                    }
                }
            }
        }
        let mut next = Vec::new();
        for (x0, x1, y0) in open.drain(..) {
            if runs.contains(&(x0, x1)) {
                next.push((x0, x1, y0));
            } else {
                out.push(footprint(x0, x1, y0, j));
            }
        }
        for (x0, x1) in runs {
            if !next.iter().any(|o| (o.0, o.1) == (x0, x1)) {
                next.push((x0, x1, j));
            }
        }
        open = next;
    }
    out.retain(|f| f.u1 > f.u0 && f.v1 > f.v0);
    out
}

fn source_size(analysis: &Analysis) -> (u32, u32) {
    if analysis.source_width > 0 && analysis.source_height > 0 {
        (analysis.source_width, analysis.source_height)
    } else {
        (analysis.width, analysis.height)
    }
}

pub fn home(analysis: &Analysis, scene: &Scene) -> Placement {
    let (sw, sh) = source_size(analysis);
    match Field::new(analysis, scene) {
        Some(field) => field.rest((sw, sh), scene),
        None => table::fill(sw, sh, scene.width, scene.height),
    }
}

pub fn solve(analysis: &Analysis, scene: &Scene, options: &Options, previous: Option<&Placement>) -> Placement {
    let (sw, sh) = source_size(analysis);
    let Some(field) = Field::new(analysis, scene) else { return table::fill(sw, sh, scene.width, scene.height) };
    let fallback = field.rest((sw, sh), scene);
    let bare = footprints(scene);
    if bare.is_empty() {
        return fallback;
    }
    let cover = if scenery::active(scene, options) { cover_boxes(scene) } else { Vec::new() };
    let boxes = Boxes { icons: &bare, cover: &cover };
    let settings = settings(options);
    let new = if cover.is_empty() {
        fresh(&field, &boxes, &settings, &fallback, None)
    } else {
        let plain = fresh(&field, &Boxes::bare(&bare), &settings, &fallback, None);
        let lost = Frame::of(&plain).map(|f| field.crop_loss(&f));
        fresh(&field, &boxes, &settings, &fallback, lost)
    };
    let Some(old) = previous.filter(|p| fits(&field, p, &settings)) else { return new };
    if *old == new {
        return new;
    }
    let (cost_new, cost_old) = (score(&field, &boxes, &new, None), score(&field, &boxes, old, None));
    if cost_new < cost_old - (KEEP_FLOOR + KEEP_SHARE * cost_old) {
        new
    } else {
        old.clone()
    }
}

fn nearest_pan(slack: f64, at: f64) -> usize {
    let p = pans(slack);
    (0..p.len()).min_by(|a, b| (p[*a] - at).abs().total_cmp(&(p[*b] - at).abs())).unwrap_or(0)
}

pub fn solve_near(analysis: &Analysis, scene: &Scene, options: &Options, previous: &Placement, budget: Duration, elapsed: &dyn Fn() -> Duration) -> Placement {
    let Some(field) = Field::new(analysis, scene) else { return previous.clone() };
    let bare = footprints(scene);
    let settings = settings(options);
    let Some(held) = Frame::of(previous).filter(|_| !bare.is_empty() && fits(&field, previous, &settings)) else {
        return solve(analysis, scene, options, None);
    };
    let cover = if scenery::active(scene, options) { cover_boxes(scene) } else { Vec::new() };
    let boxes = Boxes { icons: &bare, cover: &cover };
    let held_cost = score(&field, &boxes, previous, None);
    let most_lost = (!cover.is_empty()).then(|| field.crop_loss(&held));
    let zooms = zoom_levels(&settings);
    let zi = ((held.z - 1.0) / ZOOM_STEP).round().clamp(0.0, (zooms - 1) as f64) as usize;
    let mut frames = vec![held];
    for z in zi.saturating_sub(1)..(zi + 2).min(zooms) {
        let zoom = 1.0 + ZOOM_STEP * z as f64;
        let (cw, ch) = table::cover(field.image_aspect, field.screen_aspect, zoom);
        let (xs, ys) = (pans(1.0 - cw), pans(1.0 - ch));
        let (xc, yc) = (nearest_pan(1.0 - cw, held.x0), nearest_pan(1.0 - ch, held.y0));
        for x0 in &xs[xc.saturating_sub(NEAR_PANS)..(xc + NEAR_PANS + 1).min(xs.len())] {
            for y0 in &ys[yc.saturating_sub(NEAR_PANS)..(yc + NEAR_PANS + 1).min(ys.len())] {
                frames.push(Frame { z: zoom, x0: *x0, y0: *y0, cw, ch, mirrored: held.mirrored });
            }
        }
    }
    let mut ranked: Vec<(f64, Frame)> = frames
        .par_iter()
        .map(|f| {
            let (hid, busy) = under_icons(&field, &boxes, &AxisMap::x(f, None), &AxisMap::y(f, None));
            (field.frame_cost(f, hid, busy), *f)
        })
        .collect();
    ranked.sort_by(|a, b| a.0.total_cmp(&b.0));
    let ok = usable(&field, &boxes, previous, most_lost);
    let mut best = Choice { placement: previous.clone(), cost: held_cost };
    for (_, frame) in ranked {
        if elapsed().as_secs_f64() > budget.as_secs_f64() * NEAR_SHARE {
            break;
        }
        for c in choices(&field, &boxes, &frame, &settings) {
            if ok(&c) && ranks_before(&c, &best) == std::cmp::Ordering::Less {
                best = c;
            }
        }
    }
    best.placement
}

pub fn hidden_share(analysis: &Analysis, scene: &Scene, placement: &Placement) -> f32 {
    match Field::new(analysis, scene) {
        Some(field) => hidden_saliency(&field, &Boxes::bare(&footprints(scene)), placement) as f32,
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Rect;

    #[test]
    fn bad_rectangles_are_dropped() {
        let scene = Scene {
            width: 1000,
            height: 500,
            icons: vec![
                Rect { x: 10.0, y: 10.0, w: 0.0, h: 50.0 },
                Rect { x: 10.0, y: 10.0, w: -40.0, h: 50.0 },
                Rect { x: f32::NAN, y: 10.0, w: 40.0, h: 50.0 },
                Rect { x: 2000.0, y: 10.0, w: 40.0, h: 50.0 },
                Rect { x: -300.0, y: -300.0, w: 40.0, h: 50.0 },
                Rect { x: 10.0, y: 10.0, w: f32::INFINITY, h: 50.0 },
            ],
        };
        assert!(footprints(&scene).is_empty());
    }

    #[test]
    fn footprints_grow_and_stay_on_screen() {
        let scene = Scene { width: 1000, height: 500, icons: vec![Rect { x: 0.0, y: 100.0, w: 100.0, h: 100.0 }] };
        let f = footprints(&scene)[0];
        assert_eq!(f.u0, 0.0);
        assert!((f.u1 - 0.108).abs() < 1e-12 && (f.v0 - 0.184).abs() < 1e-12 && (f.v1 - 0.416).abs() < 1e-12);
    }

    #[test]
    fn pans_are_even_and_include_the_middle() {
        let p = pans(0.4);
        assert_eq!(p.len(), 33);
        assert_eq!(p[16], 0.2);
        assert_eq!(pans(0.0), vec![0.0]);
    }

    #[test]
    fn cells_always_hold_one_cell() {
        assert_eq!(cells(0.5, 0.5, 10), (5, 6));
        assert_eq!(cells(0.0, 1.0, 10), (0, 10));
        assert_eq!(cells(0.999, 1.0, 10), (9, 10));
        assert_eq!(cells(0.2, 0.3, 10), (2, 3));
    }

    #[test]
    fn straight_tables_have_unit_scales() {
        let scales = bin_scales(&table::line(0.2, 0.8), 0.2, 0.6, false);
        assert_eq!(scales.len(), BINS);
        assert!(scales.iter().all(|s| (s - 1.0).abs() < 1e-4), "{scales:?}");
        let flipped = bin_scales(&table::line(0.8, 0.2), 0.2, 0.6, true);
        assert!(flipped.iter().all(|s| (s - 1.0).abs() < 1e-4));
    }

    #[test]
    fn the_border_skips_sides_on_the_image_edge() {
        let (w, h) = (40usize, 20usize);
        let saliency = vec![1.0f32; w * h];
        let analysis = Analysis { width: w as u32, height: h as u32, saliency, busy: vec![0.0; w * h], subject: Rect { x: 0.0, y: 0.0, w: 1.0, h: 1.0 }, flat: false, source_width: 400, source_height: 200, palette: Vec::new() };
        let field = Field::new(&analysis, &Scene { width: 400, height: 200, icons: Vec::new() }).unwrap();
        assert_eq!(field.border(&field.middle), 0.0);
        let inside = Frame { z: 2.0, x0: 0.25, y0: 0.25, cw: 0.5, ch: 0.5, mirrored: false };
        assert_eq!(field.border(&inside), 56.0);
        let low = Frame { z: 2.0, x0: 0.25, y0: 0.5, cw: 0.5, ch: 0.5, mirrored: false };
        assert_eq!(field.border(&low), 38.0);
        assert!((field.cut(&low) - 38.0 / field.size).abs() < 1e-9);
        let tall = Analysis { source_width: 200, source_height: 400, ..analysis.clone() };
        let narrow = Field::new(&tall, &Scene { width: 400, height: 200, icons: Vec::new() }).unwrap();
        assert_eq!(narrow.exempt, [false, false, true, true]);
        assert!(narrow.edge > 0.0);
        let bottom = Frame { z: 1.0, x0: 0.0, y0: 0.75, cw: 1.0, ch: 0.25, mirrored: false };
        assert!(narrow.border(&bottom) > 0.0);
        assert!((field.size - 800f64.sqrt()).abs() < 1e-9);
    }
}
