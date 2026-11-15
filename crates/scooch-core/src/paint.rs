use crate::shape::smoothstep;
use crate::{Analysis, RgbImage, RgbaImage};
use rayon::prelude::*;

pub const LAYERS: usize = 8;
pub const SHALLOW: usize = 0;
pub const FOAM: usize = 1;
pub const SAND: usize = 2;
pub const WET: usize = 3;
pub const CLOUD: usize = 4;
pub const LIT: usize = 5;
pub const DEEP: usize = 6;
pub const GUARD: usize = 7;
pub const GAIN: f32 = 4.0;

const GRAIN: f32 = 0.16;
const SHARE: f32 = 0.03;
const WARM_DEFAULT: [f32; 3] = [0.8, 0.01, 0.05];
const WARM_PICK: f32 = 0.3;
const SAND_TONE: [f32; 3] = [0.87, 0.016, 0.064];
const SAND_PULL: (f32, f32) = (0.8, 0.85);
pub(crate) const INSIDE: (f32, f32, f32) = (0.12, 0.70, 1.15);
pub(crate) const SHALLOW_MOVE: (f32, f32) = (0.5, 0.12);
const GAIN_MOST: f32 = 3.98;
const FOAM_LIGHT: f32 = 0.96;
const FOAM_TINT: f32 = 0.2;
pub(crate) const UNDER_SAND: (f32, f32) = (0.1, 0.02);
pub(crate) const SHADED: (f32, f32) = (0.81, 0.25);
const BRIGHT: (f32, f32) = (0.97, 0.07);
const WARM_LIT: f32 = 0.35;
const UNDERSIDE: (f32, f32) = (0.75, 0.28);
const TINT_MOST: f32 = 0.04;
const WARMTH: (f32, f32) = (0.01, 0.05);
const CONTRAST: (f32, f32) = (0.05, 0.10);

#[derive(Clone, Debug, PartialEq)]
pub struct Materials {
    pub palette: Vec<[f32; 3]>,
    pub grain: f32,
}

fn chroma(c: &[f32]) -> f32 {
    (c[1] * c[1] + c[2] * c[2]).sqrt()
}

pub fn from_palette(palette: &[[f32; 4]]) -> Materials {
    Materials { palette: palette.iter().filter(|c| c[3] >= SHARE).map(|c| [c[0], c[1], c[2]]).collect(), grain: GRAIN }
}

pub fn materials(analysis: &Analysis) -> Materials {
    from_palette(&analysis.palette)
}

pub fn sand(m: &Materials) -> [f32; 3] {
    let warm = m.palette.iter().copied().max_by(|a, b| (a[2] + WARM_PICK * a[1]).total_cmp(&(b[2] + WARM_PICK * b[1]))).unwrap_or(WARM_DEFAULT);
    [warm[0] + (SAND_TONE[0] - warm[0]) * SAND_PULL.0, warm[1] + (SAND_TONE[1] - warm[1]) * SAND_PULL.1, warm[2] + (SAND_TONE[2] - warm[2]) * SAND_PULL.1]
}

pub struct Tones {
    pub colors: [[f32; 3]; LAYERS],
    pub alphas: [f32; LAYERS],
}

pub fn tones(local: [f32; 3], sand: [f32; 3], mottle: f32) -> Tones {
    let c = chroma(&local[..]).max(1e-6);
    let warmth = smoothstep(WARMTH.0, WARMTH.1, local[2]);
    let tint = |l: f32, k: f32| [l, local[1] / c * (c * k).min(TINT_MOST), local[2] / c * (c * k).min(TINT_MOST)];
    let bright = tint(BRIGHT.0, BRIGHT.1 + WARM_LIT * warmth);
    let contrast = (0..3).map(|k| (bright[k] - local[k]).powi(2)).sum::<f32>().sqrt();
    let water = to_linear(local);
    let toward: [f32; 3] = std::array::from_fn(|k| local[k] + (sand[k] - local[k]) * SHALLOW_MOVE.0);
    let held = (chroma(&local[..]) / chroma(&toward[..]).max(1e-6)).min(1.0);
    let shallow = to_linear([toward[0].min(local[0] + SHALLOW_MOVE.1), toward[1] * held, toward[2] * held]);
    let gain: [f32; 3] = std::array::from_fn(|k| (shallow[k] / water[k].max(1e-3)).min(GAIN_MOST) / GAIN);
    let top = sand[0] - UNDER_SAND.0;
    let land = 1.0 - smoothstep(top - UNDER_SAND.1, top + UNDER_SAND.1, local[0]);
    let cloud = smoothstep(CONTRAST.0, CONTRAST.1, contrast);
    Tones {
        colors: [
            gain,
            to_linear([FOAM_LIGHT, sand[1] * FOAM_TINT, sand[2] * FOAM_TINT]),
            to_linear([sand[0] + mottle, sand[1], sand[2]]),
            to_linear([(sand[0] - INSIDE.0).min(INSIDE.1) + mottle, sand[1], sand[2] * INSIDE.2]),
            to_linear(tint(SHADED.0, SHADED.1)),
            to_linear(bright),
            to_linear(tint(UNDERSIDE.0, UNDERSIDE.1)),
            [0.0; 3],
        ],
        alphas: [land, 1.0, land, 1.0, cloud, 1.0, 1.0, 0.0],
    }
}

pub fn to_linear(lab: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = lab;
    let l_ = (l + 0.396_337_78 * a + 0.215_803_76 * b).powi(3);
    let m_ = (l - 0.105_561_346 * a - 0.063_854_17 * b).powi(3);
    let s_ = (l - 0.089_484_18 * a - 1.291_485_5 * b).powi(3);
    [
        (4.076_741_7 * l_ - 3.307_711_6 * m_ + 0.230_969_94 * s_).clamp(0.0, 1.0),
        (-1.268_438 * l_ + 2.609_757_4 * m_ - 0.341_319_38 * s_).clamp(0.0, 1.0),
        (-0.004_196_086_3 * l_ - 0.703_418_6 * m_ + 1.707_614_7 * s_).clamp(0.0, 1.0),
    ]
}

pub fn composite(picture: &mut RgbImage, layer: &RgbaImage) {
    if picture.dimensions() != layer.dimensions() {
        return;
    }
    let below: &mut [u8] = picture;
    below.par_chunks_mut(3).zip(layer.as_raw().par_chunks(4)).for_each(|(p, s)| blend(p, s));
}

#[inline(always)]
pub fn blend(p: &mut [u8], s: &[u8]) {
    let a = s[3] as u32;
    if a == 0 {
        return;
    }
    for k in 0..3 {
        p[k] = ((s[k] as u32 * a + p[k] as u32 * (255 - a) + 127) / 255) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color;

    fn lab_of(linear: [f32; 3]) -> [f32; 3] {
        color::oklab(linear[0], linear[1], linear[2])
    }

    #[test]
    fn lab_round_trips_through_linear() {
        for lin in [[0.2f32, 0.5, 0.8], [0.9, 0.9, 0.9], [0.05, 0.3, 0.1]] {
            let back = to_linear(color::oklab(lin[0], lin[1], lin[2]));
            assert!(back.iter().zip(&lin).all(|(a, b)| (a - b).abs() < 2e-3), "{back:?} {lin:?}");
        }
    }

    #[test]
    fn scenery_tones_keep_sand_pale_and_clouds_white() {
        let sky = [0.62, -0.03, -0.09];
        let t = tones(sky, sand(&from_palette(&[[0.45, -0.05, 0.02, 0.5], [0.6, 0.02, 0.08, 0.5]])), 0.0);
        let shaded = lab_of(t.colors[CLOUD]);
        let bright = lab_of(t.colors[LIT]);
        assert!(shaded[0] > sky[0] + 0.1 && chroma(&shaded[..]) < chroma(&sky[..]), "{shaded:?}");
        assert!(bright[0] > 0.95 && chroma(&bright[..]) <= TINT_MOST + 0.005, "{bright:?}");
        let body = lab_of(t.colors[SAND]);
        assert!(body[0] > 0.75 && body[0] < 0.95 && body[2] > 0.0, "{body:?}");
        assert!(lab_of(t.colors[WET])[0] < body[0]);
        assert!(t.alphas[CLOUD] > 0.99);
        for local in [[0.45, -0.08, 0.03], [0.72, -0.07, -0.03], [0.4, -0.02, -0.08]] {
            let water = tones(local, sand(&from_palette(&[])), 0.0);
            let lin = to_linear(local);
            let shallow = lab_of(std::array::from_fn(|k| lin[k] * water.colors[SHALLOW][k] * GAIN));
            assert!(shallow[0] > local[0] && shallow[0] <= local[0] + SHALLOW_MOVE.1 + 0.005, "{local:?} {shallow:?}");
            assert!(chroma(&shallow[..]) <= chroma(&local[..]) + 0.005, "{local:?} {shallow:?}");
        }
        assert!(tones([0.95, 0.0, 0.0], [0.87, 0.016, 0.064], 0.0).alphas[CLOUD] < 0.01);
        assert!(tones([0.9, 0.0, 0.0], [0.87, 0.016, 0.064], 0.0).alphas[SAND] < 0.01);
    }

    #[test]
    fn composite_is_plain_source_over() {
        let mut picture = RgbImage::from_pixel(2, 1, image::Rgb([100, 150, 200]));
        let mut layer = RgbaImage::new(2, 1);
        layer.put_pixel(0, 0, image::Rgba([0, 0, 0, 255]));
        layer.put_pixel(1, 0, image::Rgba([255, 255, 255, 128]));
        composite(&mut picture, &layer);
        assert_eq!(picture.get_pixel(0, 0).0, [0, 0, 0]);
        assert_eq!(picture.get_pixel(1, 0).0, [178, 203, 228]);
    }
}
