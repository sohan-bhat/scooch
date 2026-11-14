use crate::paint::{self, Materials, CORE, LAND, LAYERS, SHORE};
use crate::area::{self, Area};
use crate::shape::{self, smoothstep, Base, Grid, FAR};
use crate::{blur, color, scenery, table, Analysis, Options, Placement, Scene, Scenery};
use rayon::prelude::*;
use std::collections::VecDeque;

pub const P: usize = 16;
pub const HOLD: (f32, f32) = (-0.6, 0.4);
const BELONG_SALIENCY: (f32, f32) = (0.10, 0.35);
const BELONG_BUSY: (f32, f32) = (0.20, 0.45);
const BELONG_SPREAD: f32 = 0.35;
const SEAM: (f32, f32) = (0.6, 0.95);
const SKY_HUE: (f32, f32) = (200.0, 290.0);
const SKY_CHROMA: f32 = 0.03;
const WHITE_CHROMA: f32 = 0.04;
const WHITE_LIGHT: f32 = 0.6;
const SKY_BUSY: f32 = 0.45;
const SKY_JUMP: f32 = 0.03;
const SKY_SPREAD: f32 = 0.15;
const SKY_EDGE: (f32, f32) = (0.4, 0.6);
const SLACK: f32 = 1e-3;
const TINT_SPREAD: f32 = 0.5;
const TINT_FLOOR: f32 = 0.02;
const RIPPLE_CALM: (f32, f32) = (0.25, 0.45);
const VOLUME: f32 = 0.35;
const OPEN_RAMP: f32 = 0.45;
const OPEN_SPREAD: f32 = 0.15;
const LIGHT_SPREAD: f32 = 0.08;

pub struct Sampled<'a> {
    pub grid: Grid,
    pub small: &'a [[f32; 3]],
    pub saliency: &'a [f32],
    pub busy: &'a [f32],
}

pub struct Scenic {
    pub field: Vec<f32>,
    pub look: Vec<u8>,
    pub paints: Vec<u8>,
    pub paint_grid: Grid,
}

pub fn belonging(saliency: f32, busy: f32) -> f32 {
    (1.0 - smoothstep(BELONG_SALIENCY.0, BELONG_SALIENCY.1, saliency)) * (1.0 - smoothstep(BELONG_BUSY.0, BELONG_BUSY.1, busy))
}

pub fn active(found: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> bool {
    if !scenery::active(scene, options) {
        return false;
    }
    let (w, h) = (found.width as usize, found.height as usize);
    if w == 0 || h == 0 || found.saliency.len() != w * h || found.busy.len() != w * h {
        return true;
    }
    let span = |table: &[f32], size: usize| {
        if !table::usable(table) {
            return (0, size - 1);
        }
        let (lo, hi) = (table[0].min(table[table.len() - 1]), table[0].max(table[table.len() - 1]));
        let lo = ((lo * size as f32 - 0.5).floor() as i64 - 1).clamp(0, size as i64 - 1) as usize;
        let hi = ((hi * size as f32 - 0.5).ceil() as i64 + 1).clamp(0, size as i64 - 1) as usize;
        (lo, hi)
    };
    let ((x0, x1), (y0, y1)) = (span(&placement.fx, w), span(&placement.fy, h));
    let clean = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 };
    let low = |plane: &[f32], x: usize, y: usize| {
        let (x2, y2) = ((x + 1).min(x1), (y + 1).min(y1));
        clean(plane[y * w + x]).min(clean(plane[y * w + x2])).min(clean(plane[y2 * w + x])).min(clean(plane[y2 * w + x2])) - SLACK
    };
    (y0..=y1).any(|y| (x0..=x1).any(|x| belonging(if found.flat { 0.0 } else { low(&found.saliency, x, y) }, low(&found.busy, x, y)) > 0.0))
}

fn skyish(lab: [f32; 3]) -> bool {
    let [l, a, b] = lab;
    let chroma = (a * a + b * b).sqrt();
    let hue = crate::math::atan2(b, a).to_degrees().rem_euclid(360.0);
    (chroma >= SKY_CHROMA && hue >= SKY_HUE.0 && hue <= SKY_HUE.1) || (chroma < WHITE_CHROMA && l > WHITE_LIGHT)
}

fn blurred(values: &[f32], grid: &Grid, spread: f32) -> Vec<f32> {
    blur::gaussian_parallel(values, grid.w, grid.h, spread)
}

fn sky_region(lab: &[[f32; 3]], busy: &[f32], grid: &Grid) -> Vec<f32> {
    let open: Vec<bool> = lab.iter().zip(busy).map(|(c, b)| skyish(*c) && *b < SKY_BUSY).collect();
    let mut reached = vec![false; lab.len()];
    let mut queue: VecDeque<usize> = (0..grid.w).filter(|i| open[*i]).collect();
    for i in &queue {
        reached[*i] = true;
    }
    while let Some(i) = queue.pop_front() {
        let (x, y) = (i % grid.w, i / grid.w);
        let next = [(x > 0).then(|| i - 1), (x + 1 < grid.w).then(|| i + 1), (y > 0).then(|| i - grid.w), (y + 1 < grid.h).then(|| i + grid.w)];
        for n in next.into_iter().flatten() {
            let jump = (0..3).map(|k| (lab[n][k] - lab[i][k]).powi(2)).sum::<f32>().sqrt();
            if !reached[n] && open[n] && jump < SKY_JUMP {
                reached[n] = true;
                queue.push_back(n);
            }
        }
    }
    reached.into_iter().map(|r| if r { 1.0 } else { 0.0 }).collect()
}

fn slope(values: &[f32], grid: &Grid, i: usize) -> f32 {
    let (x, y) = (i % grid.w, i / grid.w);
    let at = |x: usize, y: usize| values[y * grid.w + x];
    let dx = at((x + 1).min(grid.w - 1), y) - at(x.saturating_sub(1), y);
    let dy = at(x, (y + 1).min(grid.h - 1)) - at(x, y.saturating_sub(1));
    let length = (dx * dx + dy * dy).sqrt();
    if length < 1e-6 {
        0.0
    } else {
        -(dx + dy) * std::f32::consts::FRAC_1_SQRT_2 / length
    }
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

pub fn none(grid: Grid, width: usize, height: usize) -> Scenic {
    let paint_grid = Grid::of(width, height, P);
    Scenic {
        field: vec![HOLD.1; grid.w * grid.h],
        look: vec![0; grid.w * grid.h * 4],
        paints: vec![0; paint_grid.w * paint_grid.h * 4 * LAYERS],
        paint_grid,
    }
}

pub struct Fixed {
    pub sky: Vec<f32>,
    pub belong: Vec<f32>,
    pub paints: Vec<u8>,
    pub paint_grid: Grid,
    grid: Grid,
    u: f32,
    billows: shape::Billows,
}

pub struct Live {
    calmer: Vec<f32>,
    puffs: shape::Puffs,
    clouds: Vec<f32>,
    mixed: Vec<f32>,
    shade: Vec<f32>,
    open: Vec<f32>,
    billows: Vec<f32>,
    light: Vec<f32>,
    lit: Vec<f32>,
    pub look: Vec<u8>,
    pub field: Vec<f32>,
}

impl Live {
    pub fn bytes(&self) -> usize {
        let floats = [&self.calmer, &self.clouds, &self.mixed, &self.shade, &self.open, &self.billows, &self.light, &self.lit, &self.field].iter().map(|v| v.len()).sum::<usize>();
        floats * 4 + self.look.len() + self.puffs.bytes()
    }
}

impl Fixed {
    pub fn bytes(&self) -> usize {
        (self.sky.len() + self.belong.len()) * 4 + self.paints.len() + self.billows.bytes()
    }

    pub fn u(&self) -> f32 {
        self.u
    }

    fn spread(&self, k: f32) -> f32 {
        k * self.u / self.grid.cell as f32
    }

    fn mix_into(&self, base: &Base, clouds: &[f32], area: Area, out: &mut [f32]) {
        let w = self.grid.w;
        for y in area[1]..area[3] {
            for i in y * w + area[0]..y * w + area[2] {
                let (l, c, s) = (base.d[i], clouds[i], self.sky[i]);
                out[i] = l + (c - l) * s;
            }
        }
    }

    fn light_into(&self, mixed: &[f32], billows: &[f32], area: Area, out: &mut [f32]) {
        let (grid, u) = (self.grid, self.u);
        out.par_chunks_mut(grid.w).enumerate().skip(area[1]).take(area[3] - area[1]).for_each(|(y, row)| {
            for (x, cell) in row.iter_mut().enumerate().take(area[2]).skip(area[0]) {
                let i = y * grid.w + x;
                let band = smoothstep(-VOLUME * u, 0.0, mixed[i]);
                *cell = if self.sky[i] > 0.0 && mixed[i] < FAR * u * 0.5 { band * slope(mixed, &grid, i) + (1.0 - band) * slope(billows, &grid, i) } else { 0.0 };
            }
        });
    }

    fn look_into(&self, open: &[f32], lit: &[f32], area: Area, look: &mut [u8]) {
        let (w, u) = (self.grid.w, self.u);
        let cells = look.as_chunks_mut::<4>().0;
        for y in area[1]..area[3] {
            for i in y * w + area[0]..y * w + area[2] {
                let shown = smoothstep(0.0, OPEN_RAMP * u, open[i]);
                cells[i] = [byte(self.sky[i]), byte(self.belong[i]), byte(shown), byte(0.5 + 0.5 * lit[i])];
            }
        }
    }

    fn field_into(&self, mixed: &[f32], area: Area, out: &mut [f32]) {
        let (w, u) = (self.grid.w, self.u);
        for y in area[1]..area[3] {
            for i in y * w + area[0]..y * w + area[2] {
                out[i] = (mixed[i] / u).clamp(HOLD.0, HOLD.1);
            }
        }
    }
}

pub fn live(base: &Base, fixed: &Fixed) -> Live {
    let (grid, u) = (base.grid, base.u);
    let (cells, all) = (grid.w * grid.h, shape::whole(&grid));
    let calmer: Vec<f32> = base.d.iter().zip(&base.meanders).map(|(d, m)| shape::calmer(*d, *m)).collect();
    let puffs = shape::puffs(base, &calmer);
    let mut clouds = vec![0.0f32; cells];
    shape::clouds_into(base, &calmer, &puffs, |i| fixed.sky[i] > 0.0, all, &mut clouds);
    let mut mixed = vec![0.0f32; cells];
    fixed.mix_into(base, &clouds, all, &mut mixed);
    let mut shade = vec![0.0f32; cells];
    shape::shade_into(&base.icons, &grid, u, all, &mut shade);
    let mut open = vec![0.0f32; cells];
    blur::fixed_into(&shade, &mut open, grid.w, grid.h, fixed.spread(OPEN_SPREAD), all);
    let mut billows = vec![0.0f32; cells];
    shape::billows_into(&fixed.billows, &grid, u, |i| fixed.sky[i] > 0.0 && base.d[i] < FAR * u * 0.5, all, &mut billows);
    let mut light = vec![0.0f32; cells];
    fixed.light_into(&mixed, &billows, all, &mut light);
    let mut lit = vec![0.0f32; cells];
    blur::fixed_into(&light, &mut lit, grid.w, grid.h, fixed.spread(LIGHT_SPREAD), all);
    let mut look = vec![0u8; cells * 4];
    fixed.look_into(&open, &lit, all, &mut look);
    let mut field = vec![0.0f32; cells];
    fixed.field_into(&mixed, all, &mut field);
    Live { calmer, puffs, clouds, mixed, shade, open, billows, light, lit, look, field }
}

fn refresh(values: &mut [f32], w: usize, areas: &[Area], compute: impl Fn(Area, &mut [f32])) -> Vec<Area> {
    let mut out = Vec::new();
    for a in areas {
        let before = area::take(values, w, *a);
        compute(*a, values);
        out.extend(area::moved(&before, values, w, *a, area::bits));
    }
    out
}

pub fn relive(base: &Base, fixed: &Fixed, live: &mut Live, changed: &[Area], owned: &[Area], icons: &[[f32; 4]]) -> (Vec<Area>, Vec<Area>) {
    let (grid, u) = (base.grid, base.u);
    let (w, h) = (grid.w, grid.h);
    let grown = |areas: &[Area], r: usize| area::merge(areas.iter().map(|a| area::grow(*a, r, w, h)).collect());
    for a in changed {
        for y in a[1]..a[3] {
            for i in y * w + a[0]..y * w + a[2] {
                live.calmer[i] = shape::calmer(base.d[i], base.meanders[i]);
            }
        }
    }
    let puffs = shape::puffs(base, &live.calmer);
    let mut cloudy = changed.to_vec();
    cloudy.extend_from_slice(owned);
    cloudy.extend(live.puffs.moved(&puffs, &grid));
    live.puffs = puffs;
    let cloudy = area::merge(cloudy);
    for a in &cloudy {
        shape::clouds_into(base, &live.calmer, &live.puffs, |i| fixed.sky[i] > 0.0, *a, &mut live.clouds);
    }
    let clouds = &live.clouds;
    let mixed = refresh(&mut live.mixed, w, &cloudy, |a, out| fixed.mix_into(base, clouds, a, out));
    let mixed_values = &live.mixed;
    let field = refresh(&mut live.field, w, &area::merge(mixed.clone()), |a, out| fixed.field_into(mixed_values, a, out));
    let near = area::merge(icons.iter().map(|r| shape::cells_near(&grid, r, shape::shade_reach(u))).collect());
    let shade = refresh(&mut live.shade, w, &near, |a, out| shape::shade_into(&base.icons, &grid, u, a, out));
    let (open_sigma, lit_sigma) = (fixed.spread(OPEN_SPREAD), fixed.spread(LIGHT_SPREAD));
    let shaded = &live.shade;
    let open = refresh(&mut live.open, w, &grown(&shade, blur::reach(open_sigma)), |a, out| blur::fixed_into(shaded, out, w, h, open_sigma, a));
    let billows = refresh(&mut live.billows, w, &area::merge(changed.to_vec()), |a, out| shape::billows_into(&fixed.billows, &grid, u, |i| fixed.sky[i] > 0.0 && base.d[i] < FAR * u * 0.5, a, out));
    let mut sloped = mixed;
    sloped.extend(billows);
    let (mixed_values, billow_values) = (&live.mixed, &live.billows);
    let light = refresh(&mut live.light, w, &grown(&sloped, 1), |a, out| fixed.light_into(mixed_values, billow_values, a, out));
    let light_values = &live.light;
    let lit = refresh(&mut live.lit, w, &grown(&light, blur::reach(lit_sigma)), |a, out| blur::fixed_into(light_values, out, w, h, lit_sigma, a));
    let mut looks = open;
    looks.extend(lit);
    let mut look = Vec::new();
    let (open_values, lit_values) = (&live.open, &live.lit);
    for a in area::merge(looks) {
        let before = area::take(live.look.as_chunks::<4>().0, w, a);
        fixed.look_into(open_values, lit_values, a, &mut live.look);
        look.extend(area::moved(&before, live.look.as_chunks::<4>().0, w, a, area::equal));
    }
    (field, look)
}

pub fn fixed(input: &Sampled, options: &Options, materials: &Materials, u: f32, width: usize, height: usize) -> Fixed {
    let grid = input.grid;
    let cells = grid.w * grid.h;
    let encode = color::encode_table();
    let lab: Vec<[f32; 3]> = input.small.par_iter().map(|c| color::oklab(c[0], c[1], c[2])).collect();
    let raw: Vec<f32> = input
        .saliency
        .iter()
        .zip(input.busy)
        .map(|(s, b)| belonging(*s, *b))
        .collect();
    let spread = |k: f32| k * u / grid.cell as f32;
    let calm = blurred(&raw, &grid, spread(BELONG_SPREAD));
    let sky: Vec<f32> = match options.scenery {
        Scenery::Clouds => vec![1.0; cells],
        Scenery::Islands => vec![0.0; cells],
        _ => blurred(&sky_region(&lab, input.busy, &grid), &grid, spread(SKY_SPREAD)).into_iter().map(|v| smoothstep(SKY_EDGE.0, SKY_EDGE.1, v)).collect(),
    };
    let belong: Vec<f32> = calm
        .iter()
        .zip(&sky)
        .map(|(c, s)| if options.scenery == Scenery::Auto { c * smoothstep(SEAM.0, SEAM.1, (2.0 * s - 1.0).abs()) } else { *c })
        .collect();
    let paint_grid = Grid::of(width, height, P);
    let step = P / grid.cell;
    let pool = |values: &dyn Fn(usize) -> f32| -> Vec<f32> {
        (0..paint_grid.w * paint_grid.h)
            .map(|i| {
                let (px, py) = ((i % paint_grid.w) * step, (i / paint_grid.w) * step);
                let (mut sum, mut count) = (0.0f32, 0.0f32);
                for y in py..(py + step).min(grid.h) {
                    for x in px..(px + step).min(grid.w) {
                        sum += values(y * grid.w + x);
                        count += 1.0;
                    }
                }
                sum / count.max(1.0)
            })
            .collect()
    };
    let wide = spread(TINT_SPREAD) / step as f32;
    let weight = |i: usize| raw[i] + TINT_FLOOR;
    let share = blur::gaussian_parallel(&pool(&weight), paint_grid.w, paint_grid.h, wide);
    let channels: Vec<Vec<f32>> = (0..3)
        .into_par_iter()
        .map(|k| blur::gaussian_parallel(&pool(&|i| lab[i][k] * weight(i)), paint_grid.w, paint_grid.h, wide).iter().zip(&share).map(|(v, w)| v / w.max(1e-6)).collect())
        .collect();
    let rough = blur::gaussian_parallel(&pool(&|i| input.busy[i]), paint_grid.w, paint_grid.h, wide);
    let tones: Vec<(paint::Tones, f32)> = (0..paint_grid.w * paint_grid.h)
        .into_par_iter()
        .map(|i| (paint::tones(materials, [channels[0][i], channels[1][i], channels[2][i]]), 1.0 - smoothstep(RIPPLE_CALM.0, RIPPLE_CALM.1, rough[i])))
        .collect();
    let mut paints = vec![0u8; paint_grid.w * paint_grid.h * 4 * LAYERS];
    let plane = paint_grid.w * paint_grid.h * 4;
    for (i, (t, still)) in tones.iter().enumerate() {
        for (layer, rgb) in t.colors.iter().enumerate() {
            let alpha = match layer {
                SHORE => *still,
                LAND => t.land,
                CORE => t.cloud,
                _ => 1.0,
            };
            let at = layer * plane + i * 4;
            for k in 0..3 {
                paints[at + k] = color::encode(encode, rgb[k]);
            }
            paints[at + 3] = byte(alpha);
        }
    }
    Fixed { sky, belong, paints, paint_grid, grid, u, billows: shape::billow_lattice(&grid, u) }
}
