use crate::paint::{self, Materials};
use crate::shape;
use crate::table::{self, Curve};
use crate::{color, Analysis, Options, Placement, RgbImage, RgbaImage, Scene, Scenery};

const SKY_TOP: f32 = 0.55;
const SKY_BUSY: f32 = 0.2;
const SKY_HUE: (f32, f32) = (200.0, 290.0);
const SKY_CHROMA: f32 = 0.03;
const WHITE_CHROMA: f32 = 0.04;
const WHITE_LIGHT: f32 = 0.6;
const RIPPLE_BUSY: f32 = 0.35;
const SKY_STEP: usize = 8;
const SKY_JUMP: f32 = 0.03;

pub fn active(scene: &Scene, options: &Options) -> bool {
    options.scenery != Scenery::Off && shape::ground(scene).is_some()
}

pub fn footprints(scene: &Scene) -> Vec<crate::Rect> {
    shape::footprints(scene)
}

fn skyish(lab: [f32; 3]) -> bool {
    let [l, a, b] = lab;
    let chroma = (a * a + b * b).sqrt();
    let hue = b.atan2(a).to_degrees().rem_euclid(360.0);
    let blue = chroma >= SKY_CHROMA && hue >= SKY_HUE.0 && hue <= SKY_HUE.1;
    let white = chroma < WHITE_CHROMA && l > WHITE_LIGHT;
    blue || white
}

fn calm_to_top(color_at: &dyn Fn(usize, usize) -> [f32; 3], busy_at: &dyn Fn(f32, f32) -> f32, x: usize, top: usize) -> bool {
    let mut last = color_at(x, top);
    for y in (0..top).rev().step_by(SKY_STEP) {
        let c = color_at(x, y);
        let jump = (0..3).map(|k| (c[k] - last[k]).powi(2)).sum::<f32>().sqrt();
        if jump > SKY_JUMP || busy_at(x as f32, y as f32) >= SKY_BUSY {
            return false;
        }
        last = c;
    }
    skyish(last)
}

fn busy_sampler<'a>(found: Option<&'a Analysis>, placement: &Placement, width: usize, height: usize) -> Box<dyn Fn(f32, f32) -> f32 + 'a> {
    let Some(found) = found else { return Box::new(|_, _| 0.0) };
    let (bw, bh) = (found.width as usize, found.height as usize);
    if bw == 0 || bh == 0 || found.busy.len() != bw * bh || !table::usable(&placement.fx) || !table::usable(&placement.fy) {
        return Box::new(|_, _| 0.0);
    }
    let (xc, yc) = (Curve::new(&placement.fx), Curve::new(&placement.fy));
    let busy = &found.busy;
    Box::new(move |px, py| {
        let sx = (xc.at(px as f64 / width as f64) * bw as f64 - 0.5).clamp(0.0, (bw - 1) as f64) as usize;
        let sy = (yc.at(py as f64 / height as f64) * bh as f64 - 0.5).clamp(0.0, (bh - 1) as f64) as usize;
        busy[sy * bw + sx]
    })
}

pub fn layer(picture: &RgbImage, found: Option<&Analysis>, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> RgbaImage {
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let empty = || RgbaImage::new(picture.width(), picture.height());
    if !active(scene, options) {
        return empty();
    }
    let Some(base) = shape::base(scene, width, height) else { return empty() };
    if base.groups.is_empty() {
        return empty();
    }
    let decode = color::decode_table();
    let raw = picture.as_raw();
    let busy_at = busy_sampler(found, placement, width, height);
    let stats: Vec<([f32; 3], f32, (f32, f32))> = base
        .groups
        .iter()
        .map(|cells| {
            let even = |i: &&usize| (*i % base.grid.w).is_multiple_of(2) && (*i / base.grid.w).is_multiple_of(2);
            let sparse = cells.iter().any(|i| even(&i));
            let picked: Vec<usize> = cells.iter().filter(|i| !sparse || even(i)).copied().collect();
            let n = picked.len() as f32;
            let (mut sx, mut sy, mut lab, mut busy) = (0.0f32, 0.0f32, [0.0f32; 3], 0.0f32);
            for i in picked {
                let (px, py) = base.grid.center(i);
                sx += px;
                sy += py;
                let (x, y) = ((px as usize).min(width - 1), (py as usize).min(height - 1));
                let p = &raw[(y * width + x) * 3..(y * width + x) * 3 + 3];
                let c = color::oklab(decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]);
                for k in 0..3 {
                    lab[k] += c[k];
                }
                busy += busy_at(px, py);
            }
            (lab.map(|v| v / n), busy / n, (sx / n, sy / n))
        })
        .collect();
    let color_at = |x: usize, y: usize| {
        let mut sum = [0.0f32; 3];
        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let (x, y) = ((x + dx).min(width - 1), (y + dy).min(height - 1));
            let p = &raw[(y * width + x) * 3..(y * width + x) * 3 + 3];
            let c = color::oklab(decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]);
            for k in 0..3 {
                sum[k] += c[k] / 4.0;
            }
        }
        sum
    };
    let bounds: Vec<[usize; 4]> = base
        .groups
        .iter()
        .map(|cells| {
            let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
            for i in cells {
                let (x, y) = (i % base.grid.w, i / base.grid.w);
                (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x), y1.max(y));
            }
            [x0, y0, x1, y1]
        })
        .collect();
    let clouds: Vec<bool> = stats
        .iter()
        .zip(&bounds)
        .map(|((lab, busy, center), b)| match options.scenery {
            Scenery::Clouds => true,
            Scenery::Islands => false,
            _ if !skyish(*lab) || *busy >= SKY_BUSY => false,
            _ if center.1 < SKY_TOP * height as f32 => true,
            _ => {
                let top = (b[1] * shape::Q).min(height - 1);
                [b[0], (b[0] + b[2]) / 2, b[2]].into_iter().any(|gx| calm_to_top(&color_at, &*busy_at, (gx * shape::Q).min(width - 1), top))
            }
        })
        .collect();
    let tones: Vec<paint::Tones> = stats.iter().zip(&clouds).map(|((lab, busy, _), cloud)| paint::tones(materials, *lab, *cloud, *busy < RIPPLE_BUSY)).collect();
    let centers: Vec<(f32, f32)> = stats.iter().map(|s| s.2).collect();
    let wide: Vec<bool> = bounds.iter().map(|b| b[2] - b[0] > b[3] - b[1]).collect();
    let shapes = shape::shapes(base, &clouds, &centers, &wide);
    paint::layer(picture, &shapes, &tones, materials)
}
