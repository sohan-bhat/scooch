use crate::table::{self, Curve, STEPS};
use crate::paint::{self, Materials};
use crate::{analysis, color, pads, scenery, Analysis, Options, Placement, RgbImage, RgbaImage, Scene};
use rayon::prelude::*;
use std::borrow::Cow;

const SHRINK_FROM: f64 = 1.25;
const CHUNK: usize = 32;

#[derive(Clone, Copy)]
struct Tap {
    at: [u32; 4],
    weight: [f32; 4],
}

fn catmull_rom(t: f32) -> [f32; 4] {
    let (t2, t3) = (t * t, t * t * t);
    [(-t3 + 2.0 * t2 - t) * 0.5, (3.0 * t3 - 5.0 * t2 + 2.0) * 0.5, (-3.0 * t3 + 4.0 * t2 + t) * 0.5, (t3 - t2) * 0.5]
}

fn taps(values: &[f32], out: usize, scale: f64, limit: usize) -> Vec<Tap> {
    let curve = Curve::new(values);
    (0..out)
        .map(|i| {
            let position = curve.at((i as f64 + 0.5) / out as f64) * scale - 0.5;
            let base = position.floor();
            let t = (position - base) as f32;
            let base = base as i64;
            let at = [0i64, 1, 2, 3].map(|k| (base - 1 + k).clamp(0, limit as i64 - 1) as u32);
            Tap { at, weight: catmull_rom(t) }
        })
        .collect()
}

fn worst_ratio(values: &[f32], source: u32, out: u32) -> f64 {
    let step = values.windows(2).map(|p| (p[1] as f64 - p[0] as f64).abs()).fold(0.0f64, f64::max);
    step * STEPS as f64 * source as f64 / out.max(1) as f64
}

fn shrink(image: &RgbImage, factor: u32) -> RgbImage {
    let (sw, sh) = image.dimensions();
    let (w, h) = (sw.div_ceil(factor), sh.div_ceil(factor));
    let decode = color::decode_table();
    let encode = color::encode_table();
    let raw = image.as_raw();
    let mut out = vec![0u8; (w * h * 3) as usize];
    out.par_chunks_mut((w * 3) as usize).enumerate().for_each(|(y, row)| {
        let (y0, y1) = (y as u32 * factor, ((y as u32 + 1) * factor).min(sh));
        let mut sums = vec![[0.0f32; 3]; w as usize];
        for sy in y0..y1 {
            let line = &raw[(sy * sw * 3) as usize..((sy + 1) * sw * 3) as usize];
            for (sx, p) in line.as_chunks::<3>().0.iter().enumerate() {
                let target = &mut sums[sx / factor as usize];
                target[0] += decode[p[0] as usize];
                target[1] += decode[p[1] as usize];
                target[2] += decode[p[2] as usize];
            }
        }
        for (x, sum) in sums.iter().enumerate() {
            let x0 = x as u32 * factor;
            let count = (((x0 + factor).min(sw) - x0) * (y1 - y0)) as f32;
            for c in 0..3 {
                row[x * 3 + c] = color::encode(encode, sum[c] / count);
            }
        }
    });
    RgbImage::from_raw(w, h, out).unwrap_or_else(|| RgbImage::new(w, h))
}

fn resample(source: &RgbImage, columns: &[Tap], rows: &[Tap], width: usize) -> Vec<u8> {
    let decode = color::decode_table();
    let encode = color::encode_table();
    let raw = source.as_raw();
    let stride = source.width() as usize * 3;
    let left = columns.iter().flat_map(|t| t.at).min().unwrap_or(0) as usize;
    let right = columns.iter().flat_map(|t| t.at).max().unwrap_or(0) as usize;
    let places: Vec<[usize; 4]> = columns.iter().map(|t| t.at.map(|a| (a as usize - left) * 3)).collect();
    let mut out = vec![0u8; width * rows.len() * 3];
    out.par_chunks_mut(width * 3 * CHUNK).enumerate().for_each(|(chunk, block)| {
        let first = chunk * CHUNK;
        let lines = &rows[first..first + block.len() / (width * 3)];
        let lo = lines.iter().flat_map(|t| t.at).min().unwrap_or(0) as usize;
        let hi = lines.iter().flat_map(|t| t.at).max().unwrap_or(0) as usize;
        let mut linear = vec![0.0f32; (right - left + 1) * 3];
        let mut across = vec![0.0f32; (hi - lo + 1) * width * 3];
        for (r, target) in across.chunks_exact_mut(width * 3).enumerate() {
            let start = (lo + r) * stride;
            for (l, v) in linear.iter_mut().zip(&raw[start + left * 3..start + (right + 1) * 3]) {
                *l = decode[*v as usize];
            }
            for ((tap, at), px) in columns.iter().zip(&places).zip(target.as_chunks_mut::<3>().0.iter_mut()) {
                let w = tap.weight;
                for c in 0..3 {
                    px[c] = w[0] * linear[at[0] + c] + w[1] * linear[at[1] + c] + w[2] * linear[at[2] + c] + w[3] * linear[at[3] + c];
                }
            }
        }
        for (tap, line) in lines.iter().zip(block.chunks_exact_mut(width * 3)) {
            let parts = tap.at.map(|a| &across[(a as usize - lo) * width * 3..(a as usize - lo + 1) * width * 3]);
            for (i, value) in line.iter_mut().enumerate() {
                let v = tap.weight[0] * parts[0][i] + tap.weight[1] * parts[1][i] + tap.weight[2] * parts[2][i] + tap.weight[3] * parts[3][i];
                *value = color::encode(encode, v);
            }
        }
    });
    out
}

pub fn layers(image: &RgbImage, given: Option<&Analysis>, placement: &Placement, scene: &Scene, options: &Options, materials: Option<&Materials>) -> (RgbImage, RgbaImage) {
    let (width, height) = (scene.width, scene.height);
    let (sw, sh) = image.dimensions();
    if width == 0 || height == 0 || sw == 0 || sh == 0 {
        return (RgbImage::new(width, height), RgbaImage::new(width, height));
    }
    let placement: Cow<Placement> = if table::usable(&placement.fx) && table::usable(&placement.fy) { Cow::Borrowed(placement) } else { Cow::Owned(table::fill(sw, sh, width, height)) };
    let ratio = worst_ratio(&placement.fx, sw, width).max(worst_ratio(&placement.fy, sh, height));
    let factor = if ratio > SHRINK_FROM { ratio.floor().clamp(1.0, sw.min(sh) as f64) as u32 } else { 1 };
    let shrunk = (factor > 1).then(|| shrink(image, factor));
    let source = shrunk.as_ref().unwrap_or(image);
    let scale_x = sw as f64 / factor as f64;
    let scale_y = sh as f64 / factor as f64;
    let columns = taps(&placement.fx, width as usize, scale_x, source.width() as usize);
    let rows = taps(&placement.fy, height as usize, scale_y, source.height() as usize);
    let scenery_on = scenery::active(scene, options);
    let (raw, found) = match (scenery_on || pads::wanted(scene, options), given) {
        (false, _) => (resample(source, &columns, &rows, width as usize), None),
        (true, Some(given)) => (resample(source, &columns, &rows, width as usize), Some(Cow::Borrowed(given))),
        (true, None) => {
            let (raw, made) = rayon::join(|| resample(source, &columns, &rows, width as usize), || analysis::analyze(image));
            (raw, Some(Cow::Owned(made)))
        }
    };
    let base = RgbImage::from_raw(width, height, raw).unwrap_or_else(|| RgbImage::new(width, height));
    if !scenery_on {
        return (pads::apply(base, found.as_deref(), &placement, scene, options), RgbaImage::new(width, height));
    }
    let made;
    let materials = match (materials, found.as_deref()) {
        (Some(given), _) => given,
        (None, Some(found)) if !found.palette.is_empty() => {
            made = paint::materials(found);
            &made
        }
        _ => {
            made = paint::from_palette(&analysis::palette_of(image));
            &made
        }
    };
    let layer = scenery::layer(&base, found.as_deref(), &placement, scene, options, materials);
    (base, layer)
}

pub fn render(image: &RgbImage, given: Option<&Analysis>, placement: &Placement, scene: &Scene, options: &Options, materials: Option<&Materials>) -> RgbImage {
    let (mut picture, layer) = layers(image, given, placement, scene, options, materials);
    paint::composite(&mut picture, &layer);
    picture
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker(w: u32, h: u32) -> RgbImage {
        RgbImage::from_fn(w, h, |x, y| image::Rgb([(x * 7 % 256) as u8, (y * 13 % 256) as u8, ((x + y) % 2 * 200) as u8]))
    }

    #[test]
    fn weights_add_up_to_one() {
        for i in 0..=20 {
            let w = catmull_rom(i as f32 / 20.0);
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        }
        assert_eq!(catmull_rom(0.0), [0.0, 1.0, 0.0, 0.0]);
    }

    #[test]
    fn a_matching_screen_copies_the_source() {
        let image = checker(64, 40);
        let scene = Scene { width: 64, height: 40, icons: Vec::new() };
        let out = render(&image, None, &table::fill(64, 40, 64, 40), &scene, &Options::default(), None);
        assert_eq!(out.as_raw(), image.as_raw());
    }

    #[test]
    fn a_large_source_is_shrunk_first() {
        let image = RgbImage::from_pixel(400, 300, image::Rgb([200, 30, 90]));
        let scene = Scene { width: 40, height: 30, icons: Vec::new() };
        let out = render(&image, None, &table::fill(400, 300, 40, 30), &scene, &Options::default(), None);
        assert_eq!(out.dimensions(), (40, 30));
        assert!(out.pixels().all(|p| p.0 == [200, 30, 90]));
        let shrunk = shrink(&image, 3);
        assert_eq!(shrunk.dimensions(), (134, 100));
        assert!(shrunk.pixels().all(|p| p.0 == [200, 30, 90]));
    }

    #[test]
    fn a_mirrored_table_flips_the_picture() {
        let image = checker(32, 16);
        let scene = Scene { width: 32, height: 16, icons: Vec::new() };
        let mut placement = table::fill(32, 16, 32, 16);
        placement.fx = table::line(1.0, 0.0);
        placement.mirrored = true;
        let out = render(&image, None, &placement, &scene, &Options::default(), None);
        for y in 0..16 {
            for x in 0..32 {
                assert_eq!(out.get_pixel(x, y), image.get_pixel(31 - x, y));
            }
        }
    }

    #[test]
    fn broken_placements_fall_back_to_fill() {
        let image = checker(30, 20);
        let scene = Scene { width: 30, height: 20, icons: Vec::new() };
        let mut broken = table::fill(30, 20, 30, 20);
        broken.fx = vec![f32::NAN; 3];
        let out = render(&image, None, &broken, &scene, &Options::default(), None);
        assert_eq!(out.as_raw(), image.as_raw());
        assert_eq!(render(&image, None, &broken, &Scene { width: 0, height: 9, icons: Vec::new() }, &Options::default(), None).dimensions(), (0, 9));
    }
}
