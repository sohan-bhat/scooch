use crate::analysis::quantile;
use crate::sat::{Place, Sat};
use crate::table::{self, Curve, STEPS};
use crate::warp::{self, Pchip, Plan, BINS};
use crate::{Analysis, Options, Placement, Scene, TABLE};
use rayon::prelude::*;

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
const DISTORTION: f64 = 0.6;
const BUSY_SHARE: f32 = 0.5;
const PROFILE_RANK: f64 = 0.95;
const GAIN_FLOOR: f64 = 0.02;
const GAIN_SHARE: f64 = 0.15;
const KEEP_FLOOR: f64 = 0.02;
const KEEP_SHARE: f64 = 0.10;
const EPS: f64 = 1e-9;
const TIE: f64 = 1e-12;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub u0: f64,
    pub v0: f64,
    pub u1: f64,
    pub v1: f64,
}

pub fn screen_rects(scene: &Scene) -> Vec<[f64; 4]> {
    let (width, height) = (scene.width as f64, scene.height as f64);
    scene
        .icons
        .iter()
        .filter_map(|r| {
            let (x, y, w, h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
            if !(x.is_finite() && y.is_finite() && w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
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

struct Field {
    gw: f64,
    gh: f64,
    columns: usize,
    rows: usize,
    importance: Vec<f32>,
    sb: Sat<2>,
    imp: Sat<1>,
    total_s: f64,
    total_i: f64,
    image_aspect: f64,
    screen_aspect: f64,
    flat: bool,
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
        let importance: Vec<f32> = saliency.iter().zip(&busy).map(|(s, b)| s.max(BUSY_SHARE * b)).collect();
        let sb = Sat::new([&saliency, &busy], columns, rows);
        let imp = Sat::new([&importance], columns, rows);
        let image_aspect = if analysis.source_width > 0 && analysis.source_height > 0 { table::aspect(analysis.source_width, analysis.source_height) } else { table::aspect(analysis.width, analysis.height) };
        Some(Self {
            gw: columns as f64,
            gh: rows as f64,
            columns,
            rows,
            total_s: sb.total()[0],
            total_i: imp.total()[0],
            importance,
            sb,
            imp,
            image_aspect,
            screen_aspect: table::aspect(scene.width, scene.height),
            flat: analysis.flat,
        })
    }

    fn crop_sum(&self, frame: &Frame) -> f64 {
        self.sb.sum(frame.x0 * self.gw, frame.y0 * self.gh, (frame.x0 + frame.cw) * self.gw, (frame.y0 + frame.ch) * self.gh)[0]
    }
}

#[derive(Clone, Copy, Debug)]
struct Frame {
    z: f64,
    x0: f64,
    y0: f64,
    cw: f64,
    ch: f64,
    mirrored: bool,
}

impl Frame {
    fn off(&self) -> f64 {
        let part = |start: f64, span: f64| {
            let slack = 1.0 - span;
            if slack > 1e-9 {
                ((start + span / 2.0 - 0.5) / (slack / 2.0)).clamp(-1.0, 1.0)
            } else {
                0.0
            }
        };
        part(self.x0, self.cw).powi(2) + part(self.y0, self.ch).powi(2)
    }

    fn of(placement: &Placement) -> Option<Self> {
        let [x0, y0, cw, ch] = placement.crop.map(|v| v as f64);
        let ok = [x0, y0, cw, ch, placement.zoom as f64].iter().all(|v| v.is_finite()) && cw > 0.0 && ch > 0.0 && x0 >= -1e-4 && y0 >= -1e-4 && x0 + cw <= 1.0 + 1e-4 && y0 + ch <= 1.0 + 1e-4;
        ok.then_some(Self { z: placement.zoom as f64, x0, y0, cw, ch, mirrored: placement.mirrored })
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

fn centered(field: &Field) -> Frame {
    let (cw, ch) = table::cover(field.image_aspect, field.screen_aspect, 1.0);
    Frame { z: 1.0, x0: (1.0 - cw) / 2.0, y0: (1.0 - ch) / 2.0, cw, ch, mirrored: false }
}

fn under_icons(field: &Field, boxes: &[Footprint], x: &AxisMap, y: &AxisMap) -> (f64, f64) {
    let (mut hid, mut busy, mut area) = (0.0f64, 0.0f64, 0.0f64);
    for b in boxes {
        let (xa, xb) = x.interval(b.u0, b.u1);
        let (ya, yb) = y.interval(b.v0, b.v1);
        let [s, z] = field.sb.sum(xa * field.gw, ya * field.gh, xb * field.gw, yb * field.gh);
        hid += s;
        busy += z;
        area += (xb - xa) * field.gw * (yb - ya) * field.gh;
    }
    (hid, if area > 0.0 { busy / area } else { 0.0 })
}

fn stage_one(field: &Field, boxes: &[Footprint], settings: &Settings) -> Option<Frame> {
    let middle = centered(field);
    let (hid_c, busy_c) = under_icons(field, boxes, &AxisMap::x(&middle, None), &AxisMap::y(&middle, None));
    if hid_c <= 0.0 && busy_c <= 0.0 {
        return None;
    }
    let vis_c = field.crop_sum(&middle);
    let total = field.total_s.max(EPS);
    let zooms = ((settings.max_zoom - 1.0) / ZOOM_STEP + 1e-6).floor() as usize + 1;
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
            pans(1.0 - ch).into_iter().map(|y0| boxes.iter().map(|b| (field.sb.place_y((y0 + ch * b.v0) * field.gh), field.sb.place_y((y0 + ch * b.v1) * field.gh))).collect()).collect()
        })
        .collect();
    let scored: Vec<Vec<Candidate>> = rows
        .par_iter()
        .map(|&(zi, z, cw, ch, mirrored, xi, x0)| {
            let spans: Vec<(Place, Place)> = boxes
                .iter()
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
                    for (&(xa, xb), &(ya, yb)) in spans.iter().zip(&lifts[zi][yi]) {
                        let [s, b] = field.sb.sum_placed(xa, xb, ya, yb);
                        hid += s;
                        busy += b;
                        area += (xb.at - xa.at).max(0.0) * (yb.at - ya.at).max(0.0);
                    }
                    let busy = if area > 0.0 { busy / area } else { 0.0 };
                    let off = frame.off();
                    let mut cost = BUSY * busy + ZOOMED * (z - 1.0) + OFF_CENTER * off + if mirrored { MIRRORED } else { 0.0 };
                    if !field.flat {
                        if hid > hid_c * (1.0 + 1e-9) + 1e-12 {
                            return None;
                        }
                        let vis = field.crop_sum(&frame);
                        cost += HIDDEN * hid / total + CROPPED * (vis_c - vis).max(0.0) / total;
                    }
                    Some(Candidate { zi, xi, yi, frame, off, cost })
                })
                .collect()
        })
        .collect();
    let mut best: Option<Candidate> = None;
    for candidate in scored.iter().flatten() {
        if best.as_ref().map_or(true, |b| better(candidate, b)) {
            best = Some(*candidate);
        }
    }
    best.map(|b| b.frame)
}

fn cells(a: f64, b: f64, n: usize) -> (usize, usize) {
    let lo = ((a * n as f64).floor().max(0.0) as usize).min(n - 1);
    let hi = (((b * n as f64) - 1e-9).ceil().max(0.0) as usize).clamp(lo + 1, n);
    (lo, hi)
}

fn profile(field: &Field, frame: &Frame, along_x: bool) -> Vec<f64> {
    let (others, own) = if along_x { (cells(frame.y0, frame.y0 + frame.ch, field.rows), field.columns) } else { (cells(frame.x0, frame.x0 + frame.cw, field.columns), field.rows) };
    let mut values = Vec::new();
    (0..BINS)
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
            values.clear();
            for a in a0..a1 {
                for o in others.0..others.1 {
                    let index = if along_x { o * field.columns + a } else { a * field.columns + o };
                    values.push(field.importance[index]);
                }
            }
            quantile(&values, PROFILE_RANK) as f64
        })
        .collect()
}

struct Bend {
    shift: f64,
    curve: Pchip,
}

fn hidden_importance(field: &Field, boxes: &[Footprint], x: &AxisMap, y: &AxisMap) -> f64 {
    let mut sum = 0.0;
    for b in boxes {
        let (xa, xb) = x.interval(b.u0, b.u1);
        let (ya, yb) = y.interval(b.v0, b.v1);
        sum += field.imp.sum(xa * field.gw, ya * field.gh, xb * field.gw, yb * field.gh)[0];
    }
    sum / field.total_i.max(EPS)
}

fn bend(field: &Field, boxes: &[Footprint], frame: &Frame, along_x: bool, other: &AxisMap, strength: f64) -> Option<Bend> {
    let plan = Plan::new(&profile(field, frame, along_x), strength)?;
    let own = |curve: Option<&Pchip>| -> f64 {
        if along_x {
            hidden_importance(field, boxes, &AxisMap::x(frame, curve), other)
        } else {
            hidden_importance(field, boxes, other, &AxisMap::y(frame, curve))
        }
    };
    let base = own(None);
    let mut best: Option<(f64, f64, f64, Pchip)> = None;
    for shift in plan.shifts() {
        let scales = plan.scales(shift);
        let curve = Pchip::from_scales(&scales);
        let hid = if shift == 0.0 { base } else { own(Some(&curve)) };
        let cost = hid + DISTORTION * plan.penalty(&scales);
        let wins = match &best {
            None => true,
            Some((c, _, s, _)) => cost < c - TIE || ((cost - c).abs() <= TIE && shift.abs() < s.abs()),
        };
        if wins {
            best = Some((cost, hid, shift, curve));
        }
    }
    let (_, hid, shift, curve) = best?;
    let gain = base - hid;
    (shift != 0.0 && gain >= GAIN_FLOOR && gain >= GAIN_SHARE * base).then_some(Bend { shift, curve })
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

fn hidden_saliency(field: &Field, boxes: &[Footprint], placement: &Placement) -> f64 {
    if field.flat || !(table::usable(&placement.fx) && table::usable(&placement.fy)) {
        return 0.0;
    }
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let mut hid = 0.0;
    for b in boxes {
        let (p, q) = (xc.at(b.u0), xc.at(b.u1));
        let (r, s) = (yc.at(b.v0), yc.at(b.v1));
        hid += field.sb.sum(p.min(q) * field.gw, r.min(s) * field.gh, p.max(q) * field.gw, r.max(s) * field.gh)[0];
    }
    hid / field.total_s.max(EPS)
}

fn distortion(field: &Field, frame: &Frame, values: &[f32], along_x: bool) -> f64 {
    let profile = profile(field, frame, along_x);
    let (start, span, mirrored) = if along_x { (frame.x0, frame.cw, frame.mirrored) } else { (frame.y0, frame.ch, false) };
    let display = |v: f32| {
        let t = (v as f64 - start) / span;
        if mirrored {
            1.0 - t
        } else {
            t
        }
    };
    let mut sum = 0.0;
    for pair in values.windows(2) {
        let (a, b) = (display(pair[0]), display(pair[1]));
        let dt = (b - a).abs();
        if dt <= 0.0 {
            continue;
        }
        let scale = (1.0 / STEPS as f64) / dt;
        let bin = (((a + b) / 2.0) * BINS as f64).floor().clamp(0.0, (BINS - 1) as f64) as usize;
        sum += warp::penalty(&[scale], &[profile[bin]]) * dt;
    }
    sum
}

fn score(field: &Field, boxes: &[Footprint], placement: &Placement, vis_c: f64) -> f64 {
    let Some(frame) = Frame::of(placement) else { return f64::INFINITY };
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let (mut hid, mut busy, mut area) = (0.0f64, 0.0f64, 0.0f64);
    for b in boxes {
        let (p, q) = (xc.at(b.u0), xc.at(b.u1));
        let (r, s) = (yc.at(b.v0), yc.at(b.v1));
        let (xa, xb, ya, yb) = (p.min(q) * field.gw, p.max(q) * field.gw, r.min(s) * field.gh, r.max(s) * field.gh);
        let [h, z] = field.sb.sum(xa, ya, xb, yb);
        hid += h;
        busy += z;
        area += (xb - xa) * (yb - ya);
    }
    let busy = if area > 0.0 { busy / area } else { 0.0 };
    let total = field.total_s.max(EPS);
    let mut cost = BUSY * busy + ZOOMED * (frame.z - 1.0).max(0.0) + OFF_CENTER * frame.off() + if frame.mirrored { MIRRORED } else { 0.0 };
    if !field.flat {
        cost += HIDDEN * hid / total + CROPPED * (vis_c - field.crop_sum(&frame)).max(0.0) / total;
    }
    cost + DISTORTION * (distortion(field, &frame, &placement.fx, true) + distortion(field, &frame, &placement.fy, false))
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

fn fresh(field: &Field, boxes: &[Footprint], settings: &Settings, fallback: &Placement) -> Placement {
    let Some(frame) = stage_one(field, boxes, settings) else { return fallback.clone() };
    let plain = assemble(&frame, None, None);
    if field.flat || settings.strength <= 0.0 {
        return plain;
    }
    let along = bend(field, boxes, &frame, true, &AxisMap::y(&frame, None), settings.strength);
    let across = bend(field, boxes, &frame, false, &AxisMap::x(&frame, along.as_ref().map(|b| &b.curve)), settings.strength);
    let limit = hidden_saliency(field, boxes, fallback);
    let warped = assemble(&frame, along.as_ref(), across.as_ref());
    for choice in [warped, plain] {
        if table::usable(&choice.fx) && table::usable(&choice.fy) && hidden_saliency(field, boxes, &choice) <= limit {
            return choice;
        }
    }
    fallback.clone()
}

fn source_size(analysis: &Analysis) -> (u32, u32) {
    if analysis.source_width > 0 && analysis.source_height > 0 {
        (analysis.source_width, analysis.source_height)
    } else {
        (analysis.width, analysis.height)
    }
}

pub fn solve(analysis: &Analysis, scene: &Scene, options: &Options, previous: Option<&Placement>) -> Placement {
    let (sw, sh) = source_size(analysis);
    let fallback = table::fill(sw, sh, scene.width, scene.height);
    let Some(field) = Field::new(analysis, scene) else { return fallback };
    let boxes = footprints(scene);
    if boxes.is_empty() {
        return fallback;
    }
    let settings = settings(options);
    let new = fresh(&field, &boxes, &settings, &fallback);
    let Some(old) = previous.filter(|p| fits(&field, p, &settings)) else { return new };
    if *old == new {
        return new;
    }
    let vis_c = field.crop_sum(&centered(&field));
    let (cost_new, cost_old) = (score(&field, &boxes, &new, vis_c), score(&field, &boxes, old, vis_c));
    if cost_new < cost_old - (KEEP_FLOOR + KEEP_SHARE * cost_old) {
        new
    } else {
        old.clone()
    }
}

pub fn hidden_share(analysis: &Analysis, scene: &Scene, placement: &Placement) -> f32 {
    match Field::new(analysis, scene) {
        Some(field) => hidden_saliency(&field, &footprints(scene), placement) as f32,
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
}
