use crate::shape::{smoothstep, Shapes, CLOUD_SHADOW, ISLAND_SHADOW, Q};
use crate::{color, Analysis, RgbImage, RgbaImage};
use rayon::prelude::*;

const EDGE: f32 = 1.2;
const LABEL_LIGHT: f32 = 0.62;
const RIPPLES: [(f32, f32); 2] = [(0.16, 0.55), (0.34, 0.30)];
const RIPPLE_WIDTH: f32 = 0.018;
const RIPPLE_WHITE: f32 = 0.22;
const FOAM: f32 = 0.03;
const FOAM_LIGHT: f32 = 0.05;
const LAND: f32 = 0.13;
const TOP: f32 = 0.42;
const LIGHT_BAND: f32 = 0.14;
const BELLY_BAND: f32 = 0.14;
const GRAIN_SCALE: f32 = 0.10;
const GRAIN: f32 = 0.06;
const SHARE: f32 = 0.03;
const FAMILY: f32 = 0.5;
const DARK: f32 = 0.30;
const DARK_RISE: f32 = 0.04;
const DARK_LAND: f32 = 0.06;
const DARK_TOP: f32 = 0.04;
const SHORE_RISE: f32 = 0.10;
const LAND_DROP: f32 = 0.12;
const TOP_STEP: f32 = 0.07;
const SHORE_CHROMA: (f32, f32) = (0.8, 0.07);
const LAND_CHROMA: (f32, f32) = (1.15, 0.12);
const BODY_CHROMA: (f32, f32) = (0.8, 0.05);
const RIM_RISE: f32 = 0.20;
const RIM_RANGE: (f32, f32) = (0.88, 0.97);
const RIM_CHROMA: f32 = 0.012;
const BODY_DROP: f32 = 0.04;
const BELLY_DROP: f32 = 0.06;
const CLOUD_FEATHER: f32 = 0.05;
const LIT_RISE: f32 = 0.14;
const LIT_RANGE: (f32, f32) = (0.82, 0.94);
const OPEN_RAMP: f32 = 0.45;
const VOLUME_LIT: f32 = 0.6;
const VOLUME_DARK: f32 = 0.5;
const SHADOW_SPREAD: f32 = 1.7;
const FAR: f32 = 0.42;
const FAR_RIPPLES: f32 = 0.37;
const SHADOW_SLOPE: f32 = 1.5;
const FAR_CALM: f32 = 0.28;
const DEEP: f32 = 1.5;
const GRAIN_REACH: f32 = 0.1;
const GRAIN_FLOOR: f32 = 0.03;

#[derive(Clone, Debug, PartialEq)]
pub struct Materials {
    pub palette: Vec<[f32; 3]>,
    pub grain: f32,
    pub island_texture: Option<RgbaImage>,
    pub cloud_texture: Option<RgbaImage>,
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
    Materials { palette: palette.iter().filter(|c| c[3] >= SHARE).map(|c| [c[0], c[1], c[2]]).collect(), grain: GRAIN, island_texture: None, cloud_texture: None }
}

pub fn materials(analysis: &Analysis) -> Materials {
    from_palette(&analysis.palette)
}

pub struct Tones {
    island: [[f32; 3]; 4],
    cloud: [[f32; 3]; 4],
    pub cloud_here: bool,
    pub ripples: bool,
}

pub fn tones(m: &Materials, local: [f32; 3], cloud_here: bool, ripples: bool) -> Tones {
    let distance = |c: &[f32; 3]| (0..3).map(|k| (c[k] - local[k]).powi(2)).sum::<f32>();
    let nearest = m.palette.iter().min_by(|a, b| distance(a).total_cmp(&distance(b))).copied().unwrap_or(local);
    let family = mix(local, nearest, FAMILY);
    let lb = local[0];
    let dark = lb < DARK;
    let (shore_l, foam_l, land_l, top_l) = if dark {
        let land = (lb - DARK_LAND).max(0.05);
        (lb + DARK_RISE * 0.6, lb + DARK_RISE, land, (land - DARK_TOP).max(0.03))
    } else {
        let land = (lb - LAND_DROP).clamp(0.18, LABEL_LIGHT);
        ((lb + SHORE_RISE).min(0.92), (lb + SHORE_RISE + FOAM_LIGHT).min(0.95), land, (land - TOP_STEP).max(0.12))
    };
    let (rim_l, lit_l, body_l) = if dark {
        (lb + DARK_RISE, lb + DARK_RISE * 0.75, (lb + 0.01).min(LABEL_LIGHT))
    } else {
        ((lb + RIM_RISE).clamp(RIM_RANGE.0, RIM_RANGE.1), (lb + LIT_RISE).clamp(LIT_RANGE.0, LIT_RANGE.1), (lb - BODY_DROP).clamp(0.40, LABEL_LIGHT))
    };
    let to = |c: [f32; 3]| to_linear(c);
    Tones {
        island: [
            to(shade(family, shore_l, SHORE_CHROMA.0, SHORE_CHROMA.1)),
            to(shade(family, foam_l, SHORE_CHROMA.0, SHORE_CHROMA.1)),
            to(shade(family, land_l, LAND_CHROMA.0, LAND_CHROMA.1)),
            to(shade(family, top_l, LAND_CHROMA.0 * 1.1, LAND_CHROMA.1)),
        ],
        cloud: [
            to(shade(family, body_l, BODY_CHROMA.0, BODY_CHROMA.1)),
            to(shade(family, rim_l, 0.2, RIM_CHROMA)),
            to(shade(family, body_l - BELLY_DROP, BODY_CHROMA.0, BODY_CHROMA.1)),
            to(shade(family, lit_l, 0.3, RIM_CHROMA * 1.5)),
        ],
        cloud_here,
        ripples,
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

fn cover(d: f32) -> f32 {
    smoothstep(EDGE / 2.0, -EDGE / 2.0, d)
}

fn luminance(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

struct Layer {
    rgb: [f32; 3],
    a: f32,
}

impl Layer {
    fn over(&mut self, rgb: [f32; 3], a: f32) {
        if a <= 0.0 {
            return;
        }
        let a = a.min(1.0);
        for (mine, theirs) in self.rgb.iter_mut().zip(rgb) {
            *mine = theirs * a + *mine * (1.0 - a);
        }
        self.a = a + self.a * (1.0 - a);
    }
}

fn texture(t: &Option<RgbaImage>, x: usize, y: usize, under: [f32; 3]) -> [f32; 3] {
    let Some(t) = t else { return under };
    if t.width() == 0 || t.height() == 0 {
        return under;
    }
    let decode = color::decode_table();
    let p = t.get_pixel(x as u32 % t.width(), y as u32 % t.height()).0;
    let a = p[3] as f32 / 255.0;
    mix(under, [decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]], a)
}

fn grained(c: [f32; 3], factor: f32) -> [f32; 3] {
    c.map(|v| (v * factor).clamp(0.0, 1.0))
}

pub fn layer(picture: &RgbImage, shapes: &Shapes, tones: &[Tones], m: &Materials) -> RgbaImage {
    let (width, height) = (picture.width() as usize, picture.height() as usize);
    let grid = shapes.base.grid;
    let u = shapes.base.u;
    let decode = color::decode_table();
    let encode = color::encode_table();
    let raw = picture.as_raw();
    let block: Vec<[f32; 2]> = if m.grain > 0.0 {
        (0..grid.w * grid.h)
            .into_par_iter()
            .map(|i| {
                let Some(t) = usize::try_from(shapes.owner[i]).ok().map(|id| &tones[id]) else { return [0.0, 0.0] };
                if (if t.cloud_here { shapes.cloud[i] } else { shapes.base.d[i] }) > GRAIN_REACH * u {
                    return [0.0, 0.0];
                }
                let (cx, cy) = ((i % grid.w) * Q, (i / grid.w) * Q);
                let (mut sum, mut count) = (0.0f32, 0.0f32);
                for y in cy..(cy + Q).min(height) {
                    for x in cx..(cx + Q).min(width) {
                        let p = &raw[(y * width + x) * 3..(y * width + x) * 3 + 3];
                        sum += luminance(decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]);
                        count += 1.0;
                    }
                }
                let mean = sum / count.max(1.0);
                [mean, 1.0 / (3.0 * mean.cbrt().powi(2)).max(GRAIN_FLOOR)]
            })
            .collect()
    } else {
        Vec::new()
    };
    let means: Vec<f32> = block.iter().map(|b| b[0]).collect();
    let active = |cell: usize| {
        let Some(t) = usize::try_from(shapes.owner[cell]).ok().map(|id| &tones[id]) else { return false };
        let near = if t.cloud_here { shapes.cloud[cell] } else { shapes.base.d[cell] };
        let reach = match (t.cloud_here, t.ripples) {
            (true, _) => FAR,
            (false, true) => FAR_RIPPLES,
            (false, false) => FAR_CALM,
        };
        near <= reach * u
    };
    let runs: Vec<Vec<(usize, usize)>> = (0..grid.h)
        .into_par_iter()
        .map(|j| {
            let mut runs: Vec<(usize, usize)> = Vec::new();
            for i in (0..grid.w).filter(|i| active(j * grid.w + i)) {
                match runs.last_mut() {
                    Some(run) if run.1 + 1 == i => run.1 = i,
                    _ => runs.push((i, i)),
                }
            }
            runs
        })
        .collect();
    let mut out = vec![0u8; width * height * 4];
    out.par_chunks_mut(width * 4).enumerate().for_each(|(y, line)| {
        let gy = (y as f32 + 0.5) / Q as f32 - 0.5;
        let j = (gy.round().max(0.0) as usize).min(grid.h - 1);
        for x in runs[j].iter().flat_map(|&(a, b)| a * Q..((b + 1) * Q).min(width)) {
            let gx = (x as f32 + 0.5) / Q as f32 - 0.5;
            let probe = grid.probe(gx, gy);
            let i = (gx.round().max(0.0) as usize).min(grid.w - 1);
            let Some(t) = usize::try_from(shapes.owner[j * grid.w + i]).ok().map(|id| &tones[id]) else { continue };
            let factor = || {
                if block.is_empty() {
                    return 1.0;
                }
                let p = &raw[(y * width + x) * 3..(y * width + x) * 3 + 3];
                let detail = (luminance(decode[p[0] as usize], decode[p[1] as usize], decode[p[2] as usize]) - probe.at(&means)) * block[j * grid.w + i][1];
                (1.0 + (detail / GRAIN_SCALE).clamp(-1.0, 1.0) * m.grain).powi(3)
            };
            let mut paint = Layer { rgb: [0.0; 3], a: 0.0 };
            let shade = |field: &[f32], here: f32, (offset, sigma, strength): (f32, f32, f32)| {
                if here > (SHADOW_SPREAD * sigma + SHADOW_SLOPE * offset) * u {
                    return 0.0;
                }
                strength * smoothstep(SHADOW_SPREAD * sigma * u, -SHADOW_SPREAD * sigma * u, grid.linear(field, gx, gy - offset * u / Q as f32))
            };
            if t.cloud_here {
                let dc = probe.at(&shapes.cloud);
                let body = smoothstep(CLOUD_FEATHER * u, -CLOUD_FEATHER * u, dc);
                let deep = dc < -LIGHT_BAND.max(BELLY_BAND) * DEEP * u - EDGE;
                if !deep {
                    paint.over([0.0; 3], shade(&shapes.cloud, dc, CLOUD_SHADOW));
                }
                if body > 0.0 {
                    let open = smoothstep(0.0, OPEN_RAMP * u, probe.at(&shapes.icons));
                    let mut tone = texture(&m.cloud_texture, x, y, mix(t.cloud[0], t.cloud[3], open));
                    let light = probe.at(&shapes.light) * open;
                    tone = if light > 0.0 { mix(tone, t.cloud[1], light * VOLUME_LIT) } else { mix(tone, t.cloud[2], -light * VOLUME_DARK) };
                    if !deep {
                        let toward = LIGHT_BAND * u / Q as f32 * std::f32::consts::FRAC_1_SQRT_2;
                        tone = mix(tone, t.cloud[1], 1.0 - cover(grid.linear(&shapes.cloud, gx - toward, gy - toward)));
                        tone = mix(tone, t.cloud[2], 1.0 - cover(grid.linear(&shapes.cloud, gx, gy + BELLY_BAND * u / Q as f32)));
                    }
                    paint.over(grained(tone, factor()), body);
                }
            } else {
                let d = probe.at(&shapes.base.d);
                if d < -TOP * u - EDGE {
                    paint.over(grained(texture(&m.island_texture, x, y, t.island[3]), factor()), 1.0);
                } else {
                    if t.ripples {
                        for (at, strength) in RIPPLES {
                            let gap = (d - at * u).abs() - RIPPLE_WIDTH * u / 2.0;
                            if gap < EDGE {
                                paint.over([1.0; 3], cover(gap) * strength * RIPPLE_WHITE);
                            }
                        }
                    }
                    paint.over([0.0; 3], shade(&shapes.base.d, d, ISLAND_SHADOW));
                    let shore = cover(d);
                    if shore > 0.0 {
                        let factor = factor();
                        paint.over(grained(t.island[0], factor), shore);
                        let foam = (shore - cover(d + FOAM * u)).max(0.0);
                        if foam > 0.0 {
                            paint.over(grained(t.island[1], factor), foam);
                        }
                        let land = cover(probe.at(&shapes.land) + LAND * u);
                        if land > 0.0 {
                            paint.over(grained(texture(&m.island_texture, x, y, t.island[2]), factor), land);
                        }
                        let top = cover(d + TOP * u);
                        if top > 0.0 {
                            paint.over(grained(texture(&m.island_texture, x, y, t.island[3]), factor), top);
                        }
                    }
                }
            }
            if paint.a <= 0.0 {
                continue;
            }
            let px = &mut line[x * 4..x * 4 + 4];
            let inverse = 1.0 / paint.a;
            for (slot, value) in px.iter_mut().zip(paint.rgb) {
                *slot = color::encode(encode, value * inverse);
            }
            px[3] = (paint.a * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    });
    RgbaImage::from_raw(width as u32, height as u32, out).unwrap_or_else(|| RgbaImage::new(width as u32, height as u32))
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

    #[test]
    fn lab_round_trips_through_linear() {
        for lin in [[0.2f32, 0.5, 0.8], [0.9, 0.9, 0.9], [0.05, 0.3, 0.1]] {
            let back = to_linear(color::oklab(lin[0], lin[1], lin[2]));
            assert!(back.iter().zip(&lin).all(|(a, b)| (a - b).abs() < 2e-3), "{back:?} {lin:?}");
        }
    }

    #[test]
    fn tones_stay_in_the_photos_family_and_keep_labels_readable() {
        let m = from_palette(&[[0.3, 0.02, -0.08, 0.5], [0.7, 0.05, 0.03, 0.5]]);
        for local in [[0.75, 0.0, -0.05], [0.15, 0.0, -0.03], [0.95, 0.0, 0.0]] {
            let t = tones(&m, local, false, true);
            let land = color::oklab(t.island[2][0], t.island[2][1], t.island[2][2]);
            assert!(land[0] <= LABEL_LIGHT + 0.01, "{land:?}");
            let shore = color::oklab(t.island[0][0], t.island[0][1], t.island[0][2]);
            if local[0] < DARK {
                assert!(shore[0] <= local[0] + DARK_RISE + 0.01 && land[0] < local[0]);
            }
        }
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
