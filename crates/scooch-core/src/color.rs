use std::sync::OnceLock;

pub const ENCODE_SIZE: usize = 4096;
const ENCODE_TOP: f32 = (ENCODE_SIZE - 1) as f32;

pub fn decode_table() -> &'static [f32; 256] {
    static CELL: OnceLock<[f32; 256]> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut table = [0.0f32; 256];
        for (i, value) in table.iter_mut().enumerate() {
            let c = i as f64 / 255.0;
            *value = if c <= 0.04045 { c / 12.92 } else { crate::math::pow((c + 0.055) / 1.055, 2.4) } as f32;
        }
        table
    })
}

pub fn encode_table() -> &'static [u8; ENCODE_SIZE] {
    static CELL: OnceLock<[u8; ENCODE_SIZE]> = OnceLock::new();
    CELL.get_or_init(|| {
        let mut table = [0u8; ENCODE_SIZE];
        for (i, value) in table.iter_mut().enumerate() {
            let v = i as f64 / (ENCODE_SIZE - 1) as f64;
            let c = if v <= 0.0031308 { v * 12.92 } else { 1.055 * crate::math::pow(v, 1.0 / 2.4) - 0.055 };
            *value = (c * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        }
        table
    })
}

#[inline(always)]
pub fn encode(table: &[u8; ENCODE_SIZE], linear: f32) -> u8 {
    table[(linear * ENCODE_TOP + 0.5).clamp(0.0, ENCODE_TOP) as usize]
}

fn cube_root(x: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    let mut y = f32::from_bits(x.to_bits() / 3 + 709_921_077);
    for _ in 0..4 {
        y = (2.0 * y + x / (y * y)) / 3.0;
    }
    y
}

pub fn oklab(r: f32, g: f32, b: f32) -> [f32; 3] {
    let l = cube_root(0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b);
    let m = cube_root(0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b);
    let s = cube_root(0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b);
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eight_bit_values_survive_a_round_trip() {
        let decode = decode_table();
        let table = encode_table();
        for v in 0..=255u8 {
            assert_eq!(encode(table, decode[v as usize]), v);
        }
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let table = encode_table();
        assert_eq!(encode(table, -3.0), 0);
        assert_eq!(encode(table, 7.0), 255);
        assert_eq!(encode(table, f32::NAN), 0);
    }

    #[test]
    fn cube_root_is_accurate() {
        for i in 1..=4000 {
            let x = i as f32 / 4000.0;
            let y = cube_root(x);
            assert!((y * y * y - x).abs() < 2e-6, "{x} {y}");
        }
        assert_eq!(cube_root(0.0), 0.0);
    }

    #[test]
    fn white_has_full_lightness_and_no_chroma() {
        let [l, a, b] = oklab(1.0, 1.0, 1.0);
        assert!((l - 1.0).abs() < 1e-3 && a.abs() < 1e-3 && b.abs() < 1e-3);
    }
}
