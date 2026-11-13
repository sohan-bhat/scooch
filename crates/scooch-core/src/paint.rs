use crate::shape::smoothstep;
use crate::{Analysis, RgbImage, RgbaImage};
use rayon::prelude::*;

pub const LAYERS: usize = 8;
pub const SHORE: usize = 0;
pub const FOAM: usize = 1;
pub const LAND: usize = 2;
pub const TOP: usize = 3;
pub const CORE: usize = 4;
pub const LIT: usize = 5;
pub const RIM: usize = 6;
pub const BELLY: usize = 7;

const LABEL_LIGHT: f32 = 0.62;
const CORE_LIGHT: f32 = 0.60;
const GRAIN: f32 = 0.06;
const SHARE: f32 = 0.03;
const FAMILY: f32 = 0.5;
const DARK: f32 = 0.30;
const DARK_RISE: f32 = 0.04;
const DARK_LAND: f32 = 0.06;
const DARK_TOP: f32 = 0.10;
const SHORE_RISE: f32 = 0.10;
const FOAM_LIGHT: f32 = 0.05;
const LAND_DROP: f32 = 0.08;
const TOP_DROP: f32 = 0.12;
const DEEPEST: f32 = 0.12;
const CHROMA_OVER: f32 = 0.03;
const LAND_FADE: (f32, f32) = (0.66, 0.72);
const SHORE_CHROMA: (f32, f32) = (0.8, 0.07);
const LAND_CHROMA: (f32, f32) = (1.15, 0.12);
const BODY_CHROMA: (f32, f32) = (0.8, 0.05);
const RIM_RISE: f32 = 0.20;
const RIM_RANGE: (f32, f32) = (0.88, 0.97);
const RIM_CHROMA: f32 = 0.012;
const LIT_RISE: f32 = 0.14;
const LIT_RANGE: (f32, f32) = (0.82, 0.94);
const BODY_DROP: f32 = 0.04;
const BELLY_DROP: f32 = 0.06;
const CONTRAST: (f32, f32) = (0.05, 0.10);

#[derive(Clone, Debug, PartialEq)]
pub struct Materials {
    pub palette: Vec<[f32; 3]>,
    pub grain: f32,
}

fn chroma(c: &[f32]) -> f32 {
    (c[1] * c[1] + c[2] * c[2]).sqrt()
}

fn mix(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn shade(c: [f32; 3], l: f32, chroma_scale: f32, most: f32) -> [f32; 3] {
    let s = (chroma_scale * chroma(&c[..])).min(most) / chroma(&c[..]).max(1e-6);
    [l, c[1] * s, c[2] * s]
}

pub fn from_palette(palette: &[[f32; 4]]) -> Materials {
    Materials { palette: palette.iter().filter(|c| c[3] >= SHARE).map(|c| [c[0], c[1], c[2]]).collect(), grain: GRAIN }
}

pub fn materials(analysis: &Analysis) -> Materials {
    from_palette(&analysis.palette)
}

pub struct Tones {
    pub colors: [[f32; 3]; LAYERS],
    pub land: f32,
    pub cloud: f32,
}

pub fn tones(m: &Materials, local: [f32; 3]) -> Tones {
    let distance = |c: &[f32; 3]| (0..3).map(|k| (c[k] - local[k]).powi(2)).sum::<f32>();
    let nearest = m.palette.iter().min_by(|a, b| distance(a).total_cmp(&distance(b))).copied().unwrap_or(local);
    let family = mix(local, nearest, FAMILY);
    let lb = local[0];
    let most = chroma(&local[..]) + CHROMA_OVER;
    let (shore_l, foam_l, land_l, top_l) = if lb < DARK {
        (lb + DARK_RISE * 0.6, lb + DARK_RISE, (lb - DARK_LAND).max(0.04), (lb - DARK_TOP).max(0.03))
    } else {
        ((lb + SHORE_RISE).min(0.92), (lb + SHORE_RISE + FOAM_LIGHT).min(0.95), (lb - LAND_DROP).min(LABEL_LIGHT), (lb - TOP_DROP).min(LABEL_LIGHT - 0.04))
    };
    let (land_l, top_l) = (land_l.max(lb - DEEPEST), top_l.max(lb - DEEPEST));
    let (rim_l, lit_l, body_l) = if lb < DARK {
        (lb + DARK_RISE, lb + DARK_RISE * 0.75, (lb + 0.01).min(CORE_LIGHT))
    } else {
        ((lb + RIM_RISE).clamp(RIM_RANGE.0, RIM_RANGE.1), (lb + LIT_RISE).clamp(LIT_RANGE.0, LIT_RANGE.1), (lb - BODY_DROP).clamp(0.40, CORE_LIGHT))
    };
    let land = |l: f32, (scale, top): (f32, f32)| to_linear(shade(family, l, scale, top.min(most)));
    let lit = shade(family, lit_l, 0.3, RIM_CHROMA * 1.5);
    let contrast = (0..3).map(|k| (lit[k] - local[k]).powi(2)).sum::<f32>().sqrt();
    Tones {
        colors: [
            land(shore_l, SHORE_CHROMA),
            land(foam_l, SHORE_CHROMA),
            land(land_l, LAND_CHROMA),
            land(top_l, (LAND_CHROMA.0 * 1.1, LAND_CHROMA.1)),
            to_linear(shade(family, body_l, BODY_CHROMA.0, BODY_CHROMA.1)),
            to_linear(lit),
            to_linear(shade(family, rim_l, 0.2, RIM_CHROMA)),
            to_linear(shade(family, body_l - BELLY_DROP, BODY_CHROMA.0, BODY_CHROMA.1)),
        ],
        land: 1.0 - smoothstep(LAND_FADE.0, LAND_FADE.1, lb),
        cloud: smoothstep(CONTRAST.0, CONTRAST.1, contrast),
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
    below.par_chunks_mut(3).zip(layer.as_raw().par_chunks(4)).for_each(|(p, s)| {
        let a = s[3] as u32;
        if a == 0 {
            return;
        }
        for k in 0..3 {
            p[k] = ((s[k] as u32 * a + p[k] as u32 * (255 - a) + 127) / 255) as u8;
        }
    });
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
    fn land_stays_near_the_picture_and_keeps_labels_readable() {
        let m = from_palette(&[[0.3, 0.02, -0.08, 0.5], [0.7, 0.18, 0.15, 0.5]]);
        for local in [[0.6, 0.0, -0.05], [0.15, 0.0, -0.03], [0.45, -0.05, 0.10], [0.7, 0.01, 0.0]] {
            let t = tones(&m, local);
            for layer in [LAND, TOP] {
                let c = lab_of(t.colors[layer]);
                assert!(c[0] >= local[0] - DEEPEST - 0.01, "{local:?} {c:?}");
                assert!(c[0] <= LABEL_LIGHT + 0.01, "{local:?} {c:?}");
                assert!(chroma(&c[..]) <= chroma(&local[..]) + CHROMA_OVER + 0.01, "{local:?} {c:?}");
            }
        }
        assert!(tones(&m, [0.9, 0.0, 0.0]).land < 0.01);
        assert!(tones(&m, [0.95, 0.0, 0.0]).cloud < 0.01);
        assert!(tones(&m, [0.6, -0.03, -0.09]).cloud > 0.99);
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
