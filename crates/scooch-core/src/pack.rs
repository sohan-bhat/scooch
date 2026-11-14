use crate::bundle::{Bundle, Constants, RAMP};
use crate::paint::LAYERS;
use crate::{Analysis, Rect};

const MAGIC: [u8; 4] = *b"SCBT";
const VERSION: u32 = 1;
const GRID: u32 = 4;
const PAINT: u32 = 16;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Canvas {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TablesError {
    Short,
    Magic,
    Version,
    Checksum,
    Shape,
    Canvas,
}

impl std::fmt::Display for TablesError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Short => "the tables end too soon",
            Self::Magic => "these bytes are not Scooch tables",
            Self::Version => "the tables come from another version",
            Self::Checksum => "the tables are damaged",
            Self::Shape => "the table sizes do not fit the screen",
            Self::Canvas => "the canvas does not match the tables",
        })
    }
}

impl std::error::Error for TablesError {}

pub fn fingerprint(width: u32, height: u32, bytes: &[u8]) -> u64 {
    let mix = |mut h: u64| {
        h ^= h >> 33;
        h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
        h ^= h >> 33;
        h = h.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
        h ^ (h >> 33)
    };
    let mut h = mix((((width as u64) << 32) | height as u64) ^ bytes.len() as u64);
    let (words, rest) = bytes.as_chunks::<8>();
    for word in words {
        h = mix(h ^ u64::from_le_bytes(*word)).wrapping_add(0x9e37_79b9_7f4a_7c15);
    }
    let mut tail = [0u8; 8];
    tail[..rest.len()].copy_from_slice(rest);
    mix(h ^ u64::from_le_bytes(tail) ^ rest.len() as u64)
}

pub fn half_bits(v: f32) -> u16 {
    let bits = v.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exponent = ((bits >> 23) & 0xff) as i32 - 127 + 15;
    if v == 0.0 || v.is_nan() {
        sign
    } else if exponent >= 31 {
        sign | 0x7bff
    } else if exponent <= 0 {
        sign | ((v.abs() / 2.0f32.powi(-24)).round() as u16).min(0x3ff)
    } else {
        sign | ((exponent as u16) << 10) | (((bits & 0x7f_ffff) >> 13) as u16)
    }
}

pub fn from_half_bits(h: u16) -> f32 {
    let sign = if h & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exponent = ((h >> 10) & 0x1f) as i32;
    let mantissa = (h & 0x3ff) as f32;
    sign * if exponent == 0 { mantissa * 2.0f32.powi(-24) } else { (1.0 + mantissa / 1024.0) * 2.0f32.powi(exponent - 15) }
}

fn constants(c: &Constants) -> [f32; 17] {
    [c.unit, c.ramp_low, c.ramp_high, c.reach, c.shadow[0], c.shadow[1], c.rim[0], c.rim[1], c.belly[0], c.belly[1], c.grain, c.grain_scale, c.grain_floor, c.core_lift, c.core_drop, c.volume_lit, c.volume_dark]
}

impl Bundle {
    pub fn tables_to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.table_bytes() + 256);
        out.extend_from_slice(&MAGIC);
        let words = [VERSION, self.width, self.height, self.canvas_width, self.canvas_height, self.grid_width, self.grid_height, self.paint_width, self.paint_height];
        words.iter().for_each(|w| out.extend_from_slice(&w.to_le_bytes()));
        out.extend_from_slice(&fingerprint(self.canvas_width, self.canvas_height, &self.canvas).to_le_bytes());
        out.extend_from_slice(&self.ratio.to_le_bytes());
        constants(&self.constants).iter().for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        self.xmap.iter().chain(&self.ymap).for_each(|v| out.extend_from_slice(&(*v as f32).to_le_bytes()));
        self.field.iter().for_each(|v| out.extend_from_slice(&half_bits(*v).to_le_bytes()));
        out.extend_from_slice(&self.look);
        out.extend_from_slice(&self.pad);
        out.extend_from_slice(&self.paints);
        out.extend_from_slice(&self.ramps);
        let sum = fingerprint(0, 0, &out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    pub fn tables_from_bytes(bytes: &[u8], canvas: Canvas) -> Result<Bundle, TablesError> {
        let body = bytes.len().checked_sub(8).ok_or(TablesError::Short)?;
        if bytes.len() < 4 || bytes[..4] != MAGIC {
            return Err(TablesError::Magic);
        }
        let mut stored = [0u8; 8];
        stored.copy_from_slice(&bytes[body..]);
        if fingerprint(0, 0, &bytes[..body]) != u64::from_le_bytes(stored) {
            return Err(TablesError::Checksum);
        }
        let mut reader = Reader { bytes: &bytes[..body], at: 4 };
        if reader.u32()? != VERSION {
            return Err(TablesError::Version);
        }
        let [width, height, canvas_width, canvas_height, grid_width, grid_height, paint_width, paint_height] = std::array::from_fn(|_| reader.u32().unwrap_or(u32::MAX));
        if [width, height, canvas_width, canvas_height, grid_width, grid_height, paint_width, paint_height].contains(&u32::MAX) {
            return Err(TablesError::Short);
        }
        if grid_width != width.div_ceil(GRID) || grid_height != height.div_ceil(GRID) || paint_width != width.div_ceil(PAINT) || paint_height != height.div_ceil(PAINT) {
            return Err(TablesError::Shape);
        }
        let print = reader.u64()?;
        if canvas.width != canvas_width || canvas.height != canvas_height || canvas.pixels.len() as u64 != canvas_width as u64 * canvas_height as u64 * 3 || fingerprint(canvas.width, canvas.height, &canvas.pixels) != print {
            return Err(TablesError::Canvas);
        }
        let ratio = f64::from_le_bytes(reader.take(8)?.try_into().map_err(|_| TablesError::Short)?);
        let c: Vec<f32> = (0..17).map(|_| reader.f32()).collect::<Result<_, _>>()?;
        let need = (width as u128 + height as u128) * 4 + grid_width as u128 * grid_height as u128 * 10 + paint_width as u128 * paint_height as u128 * 4 * LAYERS as u128 + RAMP as u128 * 8;
        if (reader.bytes.len() - reader.at) as u128 != need {
            return Err(TablesError::Shape);
        }
        let cells = grid_width as usize * grid_height as usize;
        let paints = paint_width as usize * paint_height as usize * 4 * LAYERS;
        let xmap = (0..width).map(|_| reader.f32().map(|v| v as f64)).collect::<Result<Vec<f64>, _>>()?;
        let ymap = (0..height).map(|_| reader.f32().map(|v| v as f64)).collect::<Result<Vec<f64>, _>>()?;
        let field = (0..cells).map(|_| reader.take(2).map(|b| from_half_bits(u16::from_le_bytes([b[0], b[1]])))).collect::<Result<Vec<f32>, _>>()?;
        let look = reader.take(cells * 4)?.to_vec();
        let pad = reader.take(cells * 4)?.to_vec();
        let paints = reader.take(paints)?.to_vec();
        let ramps = reader.take(RAMP * 8)?.to_vec();
        Ok(Bundle {
            width,
            height,
            canvas_width,
            canvas_height,
            canvas: canvas.pixels,
            xmap,
            ymap,
            ratio,
            grid_width,
            grid_height,
            field,
            look,
            pad,
            paint_width,
            paint_height,
            paints,
            ramps,
            constants: Constants {
                unit: c[0],
                ramp_low: c[1],
                ramp_high: c[2],
                reach: c[3],
                shadow: [c[4], c[5]],
                rim: [c[6], c[7]],
                belly: [c[8], c[9]],
                grain: c[10],
                grain_scale: c[11],
                grain_floor: c[12],
                core_lift: c[13],
                core_drop: c[14],
                volume_lit: c[15],
                volume_dark: c[16],
            },
        })
    }
}

const ANALYSIS_MAGIC: [u8; 4] = *b"SCAN";
const ANALYSIS_VERSION: u32 = 1;

impl Analysis {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity((self.saliency.len() + self.busy.len() + self.palette.len() * 4 + 16) * 4 + 16);
        out.extend_from_slice(&ANALYSIS_MAGIC);
        let words = [ANALYSIS_VERSION, self.width, self.height, self.source_width, self.source_height, self.flat as u32, self.saliency.len() as u32, self.busy.len() as u32, self.palette.len() as u32];
        words.iter().for_each(|w| out.extend_from_slice(&w.to_le_bytes()));
        let s = self.subject;
        [s.x, s.y, s.w, s.h].iter().chain(&self.saliency).chain(&self.busy).chain(self.palette.iter().flatten()).for_each(|v| out.extend_from_slice(&v.to_le_bytes()));
        let sum = fingerprint(0, 0, &out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Analysis, TablesError> {
        let body = bytes.len().checked_sub(8).ok_or(TablesError::Short)?;
        if bytes.len() < 4 || bytes[..4] != ANALYSIS_MAGIC {
            return Err(TablesError::Magic);
        }
        let mut stored = [0u8; 8];
        stored.copy_from_slice(&bytes[body..]);
        if fingerprint(0, 0, &bytes[..body]) != u64::from_le_bytes(stored) {
            return Err(TablesError::Checksum);
        }
        let mut reader = Reader { bytes: &bytes[..body], at: 4 };
        if reader.u32()? != ANALYSIS_VERSION {
            return Err(TablesError::Version);
        }
        let [width, height, source_width, source_height, flat, saliency, busy, palette] = std::array::from_fn(|_| reader.u32().unwrap_or(u32::MAX));
        if [width, height, source_width, source_height, flat, saliency, busy, palette].contains(&u32::MAX) {
            return Err(TablesError::Short);
        }
        let need = (4 + saliency as u128 + busy as u128 + palette as u128 * 4) * 4;
        if (reader.bytes.len() - reader.at) as u128 != need || flat > 1 {
            return Err(TablesError::Shape);
        }
        let mut floats = |n: u32| (0..n).map(|_| reader.f32()).collect::<Result<Vec<f32>, _>>();
        let subject = floats(4)?;
        let saliency = floats(saliency)?;
        let busy = floats(busy)?;
        let palette = floats(palette * 4)?.as_chunks::<4>().0.to_vec();
        Ok(Analysis { width, height, saliency, busy, subject: Rect { x: subject[0], y: subject[1], w: subject[2], h: subject[3] }, flat: flat == 1, source_width, source_height, palette })
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], TablesError> {
        let end = self.at.checked_add(n).filter(|e| *e <= self.bytes.len()).ok_or(TablesError::Short)?;
        let out = &self.bytes[self.at..end];
        self.at = end;
        Ok(out)
    }

    fn u32(&mut self) -> Result<u32, TablesError> {
        self.take(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, TablesError> {
        self.take(8).map(|b| u64::from_le_bytes(std::array::from_fn(|i| b[i])))
    }

    fn f32(&mut self) -> Result<f32, TablesError> {
        self.u32().map(f32::from_bits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_bits_round_trip_every_half_value() {
        for h in 0..=u16::MAX {
            let v = from_half_bits(h);
            if v.is_finite() && (h & 0x7c00) != 0x7c00 {
                assert_eq!(half_bits(v), h, "{h:#x}");
            }
        }
        assert_eq!(from_half_bits(half_bits(0.25)), 0.25);
        assert_eq!(half_bits(1e9), 0x7bff);
    }

    #[test]
    fn an_analysis_survives_its_bytes() {
        let found = Analysis {
            width: 3,
            height: 2,
            saliency: vec![0.1, 0.2, f32::NAN, 0.4, -0.0, 1.0],
            busy: vec![0.5; 6],
            subject: Rect { x: 0.25, y: 0.5, w: 0.125, h: 0.75 },
            flat: true,
            source_width: 1200,
            source_height: 800,
            palette: vec![[0.5, 0.01, -0.02, 0.4], [0.7, 0.0, 0.03, 0.6]],
        };
        let bytes = found.to_bytes();
        let back = Analysis::from_bytes(&bytes).expect("bytes");
        assert_eq!(back.to_bytes(), bytes);
        assert_eq!((back.width, back.height, back.flat, back.source_width, back.palette.len()), (3, 2, true, 1200, 2));
        for cut in [0, 7, 12, bytes.len() - 1] {
            assert!(Analysis::from_bytes(&bytes[..cut]).is_err());
        }
        let mut broken = bytes.clone();
        broken[40] ^= 1;
        assert_eq!(Analysis::from_bytes(&broken).err(), Some(TablesError::Checksum));
    }

    #[test]
    fn the_fingerprint_sees_every_byte() {
        let a = vec![7u8; 1001];
        let mut b = a.clone();
        b[1000] = 8;
        assert_ne!(fingerprint(1, 1, &a), fingerprint(1, 1, &b));
        assert_ne!(fingerprint(1, 1, &a), fingerprint(2, 1, &a));
        assert_eq!(fingerprint(3, 4, &a), fingerprint(3, 4, &a));
    }
}
