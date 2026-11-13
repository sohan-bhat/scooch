use crate::paint::{self, Materials, CORE, LAND, LAYERS, SHORE};
use crate::shape::{self, smoothstep, Base, Grid, FAR};
use crate::{blur, color, Options, Scenery};
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
    pub belong: Vec<f32>,
}

pub fn belonging(saliency: f32, busy: f32) -> f32 {
    (1.0 - smoothstep(BELONG_SALIENCY.0, BELONG_SALIENCY.1, saliency)) * (1.0 - smoothstep(BELONG_BUSY.0, BELONG_BUSY.1, busy))
}

fn skyish(lab: [f32; 3]) -> bool {
    let [l, a, b] = lab;
    let chroma = (a * a + b * b).sqrt();
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    (chroma >= SKY_CHROMA && hue >= SKY_HUE.0 && hue <= SKY_HUE.1) || (chroma < WHITE_CHROMA && l > WHITE_LIGHT)
}

fn blurred(values: &[f32], grid: &Grid, spread: f32) -> Vec<f32> {
    blur::gaussian(values, grid.w, grid.h, spread)
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
        belong: vec![0.0; grid.w * grid.h],
    }
}

pub fn build(base: &Base, input: &Sampled, options: &Options, materials: &Materials, width: usize, height: usize) -> Scenic {
    let (grid, u) = (input.grid, base.u);
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
    let clouds = shape::clouds(base, |i| sky[i] > 0.0);
    let mixed: Vec<f32> = base.d.iter().zip(&clouds).zip(&sky).map(|((l, c), s)| l + (c - l) * s).collect();
    let shade = blurred(&shape::shade(base), &grid, spread(OPEN_SPREAD));
    let billows = shape::billows(&grid, u, |i| sky[i] > 0.0 && base.d[i] < FAR * u * 0.5);
    let light: Vec<f32> = (0..cells)
        .into_par_iter()
        .map(|i| {
            let band = smoothstep(-VOLUME * u, 0.0, mixed[i]);
            if sky[i] > 0.0 && mixed[i] < FAR * u * 0.5 {
                band * slope(&mixed, &grid, i) + (1.0 - band) * slope(&billows, &grid, i)
            } else {
                0.0
            }
        })
        .collect();
    let light = blurred(&light, &grid, spread(LIGHT_SPREAD));
    let look: Vec<u8> = (0..cells)
        .into_par_iter()
        .flat_map_iter(|i| {
            let open = smoothstep(0.0, OPEN_RAMP * u, shade[i]);
            [byte(sky[i]), byte(belong[i]), byte(open), byte(0.5 + 0.5 * light[i])]
        })
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
    let share = blur::gaussian(&pool(&weight), paint_grid.w, paint_grid.h, wide);
    let channels: Vec<Vec<f32>> = (0..3)
        .into_par_iter()
        .map(|k| blur::gaussian(&pool(&|i| lab[i][k] * weight(i)), paint_grid.w, paint_grid.h, wide).iter().zip(&share).map(|(v, w)| v / w.max(1e-6)).collect())
        .collect();
    let rough = blur::gaussian(&pool(&|i| input.busy[i]), paint_grid.w, paint_grid.h, wide);
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
    Scenic { field: mixed.iter().map(|d| (d / u).clamp(HOLD.0, HOLD.1)).collect(), look, paints, paint_grid, belong }
}
