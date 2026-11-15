use crate::area::{self, Area};
use crate::bundle::{self, Bundle, Parts, Scope};
use crate::pack::fingerprint;
use rayon::prelude::*;
use std::borrow::Cow;
use std::sync::Arc;
use crate::paint;
use crate::{look, pads, render, scenery, shape, solver, Analysis, Options, Placement, RgbImage, Scene};

#[derive(Default)]
pub struct Session {
    held: Option<Held>,
    kept: Option<(u64, (u32, u32), Arc<look::Belonging>)>,
}

struct Held {
    image: u64,
    source: (u32, u32),
    analysis: u64,
    placement: Placement,
    options: Options,
    scene: Scene,
    parts: Parts,
    bundle: Arc<Bundle>,
    still: Option<Arc<RgbImage>>,
    picture: Option<RgbImage>,
    redraw: Option<Vec<bool>>,
    shrunk: bool,
}

fn glance(width: u32, height: u32, bytes: &[u8]) -> u64 {
    let (words, rest) = bytes.as_chunks::<32>();
    let mut lanes = [0x9e37_79b9_7f4a_7c15u64 ^ width as u64, 0xc2b2_ae3d_27d4_eb4f ^ height as u64, 0x1656_67b1_9e37_79f9, 0x27d4_eb2f_1656_67c5 ^ bytes.len() as u64];
    for block in words {
        for (k, lane) in lanes.iter_mut().enumerate() {
            let word = u64::from_le_bytes(block[k * 8..k * 8 + 8].try_into().unwrap_or([0; 8]));
            *lane = (*lane ^ word).wrapping_mul(0x9fb2_1c65_1e98_df25).rotate_left(29);
        }
    }
    fingerprint(width, height, &[lanes.iter().flat_map(|l| l.to_le_bytes()).collect::<Vec<u8>>(), rest.to_vec()].concat())
}

fn changes(old: &[[f64; 4]], new: &[[f64; 4]]) -> Option<Vec<[f64; 4]>> {
    let mut out = Vec::new();
    if old.len() == new.len() {
        for (a, b) in old.iter().zip(new) {
            if a != b {
                out.push(*a);
                out.push(*b);
            }
        }
        return Some(out);
    }
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if i < old.len() && j < new.len() && old[i] == new[j] {
            i += 1;
            j += 1;
        } else if j < new.len() && (i >= old.len() || new[j + 1..].first() == old.get(i)) {
            out.push(new[j]);
            j += 1;
        } else if i < old.len() && (j >= new.len() || old[i + 1..].first() == new.get(j)) {
            out.push(old[i]);
            i += 1;
        } else if i < old.len() && j < new.len() {
            out.push(old[i]);
            out.push(new[j]);
            i += 1;
            j += 1;
        }
        if out.len() > 64 {
            return None;
        }
    }
    Some(out)
}

fn field_margin(c: &bundle::Constants) -> usize {
    let most = [c.shadow, c.rim, c.belly].iter().flatten().fold(0.0f32, |m, v| m.max(v.abs()));
    (most * c.unit / shape::Q as f32).ceil() as usize + 4
}

impl Session {
    pub fn new() -> Session {
        Session::default()
    }

    pub fn forget(&mut self) {
        self.held = None;
        self.kept = None;
    }

    /// Whether scenery belongs anywhere on this picture for this screen, these icons and this setting. False when scenery is off, when there are no icons to size it by, or when the belonging map is zero everywhere, which is when a still draws no scenery at all. The map is worked out here or taken from an earlier call, and a later `still`, `still_shared` or `tables_shared` with the same picture reuses it.
    pub fn belongs(&mut self, image: &RgbImage, analysis: &Analysis, scene: &Scene, options: &Options) -> bool {
        let source = image.dimensions();
        let Some(g) = scenery::active(scene, options).then(|| shape::ground(scene)).flatten() else { return false };
        if source.0 == 0 || source.1 == 0 {
            return false;
        }
        let (picture, stamp) = (glance(source.0, source.1, image.as_raw()), look::stamp(analysis, scene, options));
        let held = self.held.as_ref().filter(|h| h.image == picture && h.source == source).and_then(|h| h.parts.belonging.clone());
        let kept = self.kept.as_ref().filter(|k| k.0 == picture && k.1 == source).map(|k| Arc::clone(&k.2));
        let map = match held.into_iter().chain(kept).find(|b| b.fits(source, stamp)) {
            Some(map) => map,
            None => Arc::new(bundle::map_of(&bundle::canvas(image, scene.width, scene.height), analysis, source, scene, options, &paint::materials(analysis), g.u)),
        };
        let empty = map.empty;
        self.kept = Some((picture, source, map));
        !empty
    }

    /// The options to use for every later call on this picture and scene: every solve, `solve`, `solve_near` and `solve_near_timed`, and every draw. They are the given options, with the scenery setting off where nothing belongs anywhere on the picture, so a bare picture is placed and drawn exactly as with scenery off. It asks `belongs`, so the map is worked out once.
    pub fn options(&mut self, image: &RgbImage, analysis: &Analysis, scene: &Scene, options: &Options) -> Options {
        if options.scenery == crate::Scenery::Off || self.belongs(image, analysis, scene, options) {
            *options
        } else {
            Options { scenery: crate::Scenery::Off, ..*options }
        }
    }

    /// `scooch_core::solve` with the options `options` gives for this picture and scene.
    pub fn solve(&mut self, image: &RgbImage, analysis: &Analysis, scene: &Scene, options: &Options, previous: Option<&Placement>) -> Placement {
        let options = self.options(image, analysis, scene, options);
        crate::solve(analysis, scene, &options, previous)
    }

    #[doc(hidden)]
    pub fn map_ids(&self) -> (Option<usize>, Option<usize>) {
        let id = |b: &Arc<look::Belonging>| Arc::as_ptr(b) as usize;
        (self.kept.as_ref().map(|k| id(&k.2)), self.held.as_ref().and_then(|h| h.parts.belonging.as_ref()).map(id))
    }

    pub fn redrawn(&self, x: u32, y: u32) -> bool {
        match &self.held {
            Some(h) => h.redraw.as_ref().is_none_or(|cells| {
                let w = h.parts.framed.grid.w;
                cells.get((y as usize / shape::Q) * w + x as usize / shape::Q).copied().unwrap_or(false)
            }),
            None => true,
        }
    }

    #[doc(hidden)]
    pub fn sites(&self) -> usize {
        self.held.as_ref().and_then(|h| h.parts.live.as_ref()).map_or(0, |live| live.sites())
    }

    pub fn bytes(&self) -> usize {
        let held = self.held.as_ref().map_or(0, |h| h.parts.bytes() + h.bundle.canvas.len() + h.bundle.table_bytes() + h.still.as_ref().map_or(0, |s| s.as_raw().len()) + h.picture.as_ref().map_or(0, |p| p.as_raw().len()));
        let shared = |k: &Arc<look::Belonging>| self.held.as_ref().and_then(|h| h.parts.belonging.as_ref()).is_some_and(|b| Arc::ptr_eq(b, k));
        held + self.kept.as_ref().filter(|k| !shared(&k.2)).map_or(0, |k| k.2.bytes())
    }

    pub fn still(&mut self, image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> (Bundle, RgbImage) {
        let Some(held) = self.advance::<true>(image, analysis, placement, scene, options) else { return render::still(image, analysis, placement, scene, options) };
        let mut bundle = (*held.bundle).clone();
        if !held.shrunk && bundle.canvas.is_empty() {
            bundle.canvas = image.as_raw().clone();
        }
        let still = held.still.as_deref().cloned().unwrap_or_default();
        (bundle, still)
    }

    pub fn still_shared(&mut self, image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> (Arc<Bundle>, Arc<RgbImage>) {
        let Some(held) = self.advance::<true>(image, analysis, placement, scene, options) else {
            let (bundle, still) = render::still(image, analysis, placement, scene, options);
            return (Arc::new(bundle), Arc::new(still));
        };
        if !held.shrunk && held.bundle.canvas.is_empty() {
            Arc::make_mut(&mut held.bundle).canvas = image.as_raw().clone();
        }
        let still = held.still.clone().unwrap_or_default();
        (Arc::clone(&held.bundle), still)
    }

    pub fn tables_shared(&mut self, image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> Arc<Bundle> {
        let Some(held) = self.advance::<false>(image, analysis, placement, scene, options) else {
            return Arc::new(crate::bundle(image, analysis, placement, scene, options));
        };
        if !held.shrunk && held.bundle.canvas.is_empty() {
            Arc::make_mut(&mut held.bundle).canvas = image.as_raw().clone();
        }
        Arc::clone(&held.bundle)
    }

    fn advance<const DRAW: bool>(&mut self, image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> Option<&mut Held> {
        let (sw, sh) = image.dimensions();
        if sw == 0 || sh == 0 || scene.width == 0 || scene.height == 0 {
            self.held = None;
            return None;
        }
        let key = (glance(sw, sh, image.as_raw()), look::print(analysis));
        let spare = self.kept.as_ref().filter(|k| k.0 == key.0 && k.1 == (sw, sh)).map(|k| Arc::clone(&k.2));
        let stamp = look::stamp(analysis, scene, options);
        let held = match self.held.take() {
            Some(mut h) if h.image == key.0 && h.source == (sw, sh) && h.scene.width == scene.width && h.scene.height == scene.height => {
                if h.analysis == key.1 && h.placement == *placement && h.options == *options {
                    shift::<DRAW>(h, image, analysis, scene, key, spare)
                } else {
                    let kept = (reuse(&mut h), fitting(h.parts.belonging.take().filter(|_| h.analysis == key.1), spare, (sw, sh), stamp));
                    build(image, analysis, placement, scene, options, key, kept)
                }
            }
            _ => build(image, analysis, placement, scene, options, key, (None, fitting(None, spare, (sw, sh), stamp))),
        };
        let mut held = held;
        if !DRAW {
            held.still = None;
        } else if held.still.is_none() {
            paint_still(&mut held, image, analysis, scene, options);
        }
        self.held = Some(held);
        self.held.as_mut()
    }
}

fn reuse(h: &mut Held) -> Option<RgbImage> {
    if !h.shrunk {
        return None;
    }
    let raw = match Arc::get_mut(&mut h.bundle) {
        Some(bundle) => std::mem::take(&mut bundle.canvas),
        None => h.bundle.canvas.clone(),
    };
    RgbImage::from_raw(h.bundle.canvas_width, h.bundle.canvas_height, raw)
}

fn build(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options, (image_key, analysis_key): (u64, u64), (canvas, kept): (Option<RgbImage>, Option<Arc<look::Belonging>>)) -> Held {
    let materials = paint::materials(analysis);
    let (canvas, shrunk) = match canvas {
        Some(canvas) => (canvas, true),
        None => match bundle::canvas(image, scene.width, scene.height) {
            Cow::Owned(canvas) => (canvas, true),
            Cow::Borrowed(image) => (image.clone(), false),
        },
    };
    let mut parts = bundle::parts(image.dimensions(), (canvas, kept), analysis, placement, scene, options, &materials);
    let mut out = parts.tables(&materials);
    let canvas = std::mem::replace(&mut parts.framed.canvas, RgbImage::new(0, 0)).into_raw();
    out.canvas = if shrunk { canvas } else { Vec::new() };
    parts.settle();
    Held { image: image_key, source: image.dimensions(), analysis: analysis_key, placement: placement.clone(), options: *options, scene: scene.clone(), parts, bundle: Arc::new(out), still: None, picture: None, redraw: None, shrunk }
}

fn paint_still(h: &mut Held, image: &RgbImage, analysis: &Analysis, scene: &Scene, options: &Options) {
    let framed = &h.parts.framed;
    let picture = h.picture.take().unwrap_or_else(|| {
        if scenery::active(scene, options) {
            let canvas: &[u8] = if h.shrunk { &h.bundle.canvas } else { image.as_raw() };
            render::picture((canvas, h.bundle.canvas_width, h.bundle.canvas_height), (&framed.xmap, &framed.ymap), framed.ratio, (scene.width, scene.height))
        } else {
            render::plain(image, &framed.placement, scene.width, scene.height)
        }
    });
    let still = if scenery::active(scene, options) {
        let (mut shown, layer) = bundle::finish(&h.bundle, &h.bundle, 0.0, picture.clone(), None);
        paint::composite(&mut shown, &layer);
        shown
    } else {
        render::finish_plain(picture.clone(), analysis, &framed.placement, scene, options)
    };
    h.picture = Some(picture);
    h.still = Some(Arc::new(still));
    h.redraw = None;
}

fn fitting(held: Option<Arc<look::Belonging>>, spare: Option<Arc<look::Belonging>>, source: (u32, u32), stamp: u64) -> Option<Arc<look::Belonging>> {
    held.into_iter().chain(spare).find(|b| b.fits(source, stamp))
}

fn shift<const DRAW: bool>(mut h: Held, image: &RgbImage, analysis: &Analysis, scene: &Scene, key: (u64, u64), spare: Option<Arc<look::Belonging>>) -> Held {
    let (old, new) = (solver::screen_rects(&h.scene), solver::screen_rects(scene));
    if old == new {
        h.scene = scene.clone();
        h.redraw = Some(Vec::new());
        return h;
    }
    let options = h.options;
    let (was, now) = (scenery::active(&h.scene, &options), scenery::active(scene, &options));
    if !was && !now {
        h.parts.repad(analysis, scene, &options);
        let grid = h.parts.framed.grid;
        let pad = h.parts.pad().unwrap_or_else(|| vec![0; grid.w * grid.h * 4]);
        if pad != h.bundle.pad {
            Arc::make_mut(&mut h.bundle).pad = pad;
        }
        let unit = shape::ground(scene).map_or(1.0, |g| g.u);
        if unit.to_bits() != h.bundle.constants.unit.to_bits() {
            h.parts.unit = unit;
            let out = Arc::make_mut(&mut h.bundle);
            out.constants.unit = unit;
            out.ramps = bundle::ramps(unit);
        }
        h.still = None;
        h.scene = scene.clone();
        h.redraw = None;
        return h;
    }
    match (was && now).then(|| changes(&old, &new)).flatten() {
        Some(moved) if lean::<DRAW>(&mut h, analysis, scene, &moved) => h,
        _ => {
            let placement = h.placement.clone();
            let stamp = look::stamp(analysis, scene, &options);
            let kept = (reuse(&mut h), fitting(h.parts.belonging.take(), spare, h.source, stamp));
            build(image, analysis, &placement, scene, &options, key, kept)
        }
    }
}

fn lean<const DRAW: bool>(h: &mut Held, analysis: &Analysis, scene: &Scene, moved: &[[f64; 4]]) -> bool {
    let Some(g) = shape::ground(scene) else { return false };
    let options = h.options;
    let parts = &mut h.parts;
    let bare = parts.fixed.is_none() && parts.belonging.as_ref().is_some_and(|b| b.empty && b.fits(h.source, look::stamp(analysis, scene, &options)));
    let Some(grounded) = parts.grounded.as_ref() else { return false };
    if parts.pads.as_ref().is_some_and(|p| p.feather().to_bits() != pads::feather(scene).to_bits()) {
        return false;
    }
    let _ = analysis;
    let grid = parts.framed.grid;
    let (w, hh) = (grid.w, grid.h);
    let cells = w * hh;
    let (mut far, mut halves) = (vec![false; cells], Vec::new());
    let (mut near, mut boxes) = (vec![false; cells], Vec::new());
    if !bare {
        let (Some(fixed), Some(live)) = (parts.fixed.as_ref(), parts.live.as_mut()) else { return false };
        if g.u.to_bits() != fixed.u().to_bits() {
            return false;
        }
        let icons: Vec<[f32; 4]> = moved.iter().map(|r| r.map(|v| v as f32)).collect();
        let Some((field, look)) = look::relive(fixed, live, &g, &icons) else { return false };
        for a in area::merge(field) {
            let before = area::take(&h.bundle.field, w, a);
            let fresh: Vec<f32> = live.field_in(w, a).into_iter().map(bundle::half).collect();
            if before.iter().zip(&fresh).all(|(p, q)| area::bits(p, q)) {
                continue;
            }
            let out = Arc::make_mut(&mut h.bundle);
            for y in a[1]..a[3] {
                out.field[y * w + a[0]..y * w + a[2]].copy_from_slice(&fresh[(y - a[1]) * (a[2] - a[0])..(y - a[1] + 1) * (a[2] - a[0])]);
            }
            halves.extend(area::mark(&before, &out.field, w, a, area::bits, &mut far));
        }
        for a in area::merge(look) {
            let before = area::take(h.bundle.look.as_chunks::<4>().0, w, a);
            if before == area::take(live.look.as_chunks::<4>().0, w, a) {
                continue;
            }
            let out = Arc::make_mut(&mut h.bundle);
            for y in a[1]..a[3] {
                out.look[(y * w + a[0]) * 4..(y * w + a[2]) * 4].copy_from_slice(&live.look[(y * w + a[0]) * 4..(y * w + a[2]) * 4]);
            }
            boxes.extend(area::mark(&before, out.look.as_chunks::<4>().0, w, a, area::equal, &mut near));
        }
    }
    if let (Some(pads), Some(calm)) = (parts.pads.as_mut(), parts.calm.as_ref()) {
        let was = pads.any();
        let touched = area::merge(pads.update(calm, &grid, &grounded.lookup, scene, parts.fixed.as_ref().map(|f| &f.belong[..]), moved));
        let before: Vec<Vec<[u8; 4]>> = touched.iter().map(|a| area::take(h.bundle.pad.as_chunks::<4>().0, w, *a)).collect();
        if was != pads.any() {
            Arc::make_mut(&mut h.bundle).pad = pads.table(calm).unwrap_or_else(|| vec![0; cells * 4]);
        } else if was && !touched.is_empty() {
            pads.patch(&mut Arc::make_mut(&mut h.bundle).pad, w, &touched);
        }
        for (a, old) in touched.iter().zip(&before) {
            boxes.extend(area::mark(old, h.bundle.pad.as_chunks::<4>().0, w, *a, |p, q| p[3] == q[3], &mut near));
        }
    }
    h.scene = scene.clone();
    let margin = field_margin(&h.bundle.constants);
    let mut redraw = vec![false; cells];
    area::spread(&far, w, hh, margin, &halves, &mut redraw);
    area::spread(&near, w, hh, 3, &boxes, &mut redraw);
    let mut around: Vec<Area> = halves.iter().map(|a| area::grow(*a, margin, w, hh)).collect();
    around.extend(boxes.iter().map(|a| area::grow(*a, 3, w, hh)));
    let around = area::merge(around);
    let Some(picture) = h.picture.as_ref().filter(|_| DRAW && h.still.is_some()) else {
        h.still = None;
        h.redraw = None;
        return true;
    };
    if around.is_empty() {
        h.redraw = Some(redraw);
        return true;
    }
    let width = scene.width as usize;
    let scope = Scope::new(redraw, &around, w, hh);
    let mut work = RgbImage::new(scene.width, scene.height);
    let from = picture.as_raw();
    work.par_chunks_mut(width * 3).enumerate().for_each(|(y, line)| {
        for (x0, x1) in Scope::ranges(&scope.need, w, y, width) {
            line[x0 * 3..x1 * 3].copy_from_slice(&from[(y * width + x0) * 3..(y * width + x1) * 3]);
        }
    });
    let (shown, layer) = bundle::finish(&h.bundle, &h.bundle, 0.0, work, Some(&scope));
    let (shown, layer) = (shown.as_raw(), layer.as_raw());
    let Some(still) = h.still.as_mut() else { return true };
    let still: &mut RgbImage = Arc::make_mut(still);
    let still: &mut [u8] = still;
    still.par_chunks_mut(width * 3).enumerate().for_each(|(y, line)| {
        for (x0, x1) in Scope::ranges(&scope.redraw, w, y, width) {
            for x in x0..x1 {
                let (i, p) = (y * width + x, &mut line[x * 3..x * 3 + 3]);
                p.copy_from_slice(&shown[i * 3..i * 3 + 3]);
                paint::blend(p, &layer[i * 4..i * 4 + 4]);
            }
        }
    });
    h.redraw = Some(scope.redraw);
    true
}
