use anyhow::{bail, Context, Result};
use scooch_core::Canvas;
use std::io::Read;
use std::path::Path;

const MAGIC: [u8; 4] = *b"SCCV";
const VERSION: u32 = 1;
const RGB: u32 = 3;
const SRGB: u32 = 1;
pub const HEAD: usize = 32;
const TABLES_PRINT: usize = 40;

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

pub fn tables_print(tables: &[u8]) -> Option<u64> {
    tables.get(TABLES_PRINT..TABLES_PRINT + 8).and_then(|bytes| bytes.try_into().ok()).map(u64::from_le_bytes)
}

pub fn encode(width: u32, height: u32, pixels: &[u8], print: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEAD + pixels.len());
    out.extend_from_slice(&MAGIC);
    for word in [VERSION, width, height, RGB, SRGB] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&print.to_le_bytes());
    out.extend_from_slice(pixels);
    out
}

pub fn read(path: &Path) -> Result<Canvas> {
    let mut file = std::fs::File::open(path).with_context(|| format!("the canvas {} could not be opened", path.display()))?;
    let mut head = [0u8; HEAD];
    file.read_exact(&mut head).context("the canvas file ends inside its header")?;
    if head[..4] != MAGIC {
        bail!("the canvas file is not a Scooch canvas");
    }
    let word = |index: usize| u32::from_le_bytes(head[4 + index * 4..8 + index * 4].try_into().unwrap_or_default());
    let (version, width, height, channels, colour) = (word(0), word(1), word(2), word(3), word(4));
    if version != VERSION || channels != RGB || colour != SRGB {
        bail!("the canvas file is version {version} with {channels} channels in colour {colour}, not what this helper reads");
    }
    let print = u64::from_le_bytes(head[24..32].try_into().unwrap_or_default());
    let length = width as usize * height as usize * 3;
    let mut pixels = Vec::with_capacity(length);
    file.take(length as u64 + 1).read_to_end(&mut pixels).context("the canvas pixels could not be read")?;
    if pixels.len() != length {
        bail!("the canvas file holds {} bytes of pixels for {width}x{height}", pixels.len());
    }
    if fingerprint(width, height, &pixels) != print {
        bail!("the canvas pixels do not match the fingerprint the tables expect");
    }
    Ok(Canvas { width, height, pixels })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(width: u32, height: u32) -> Vec<u8> {
        (0..width * height * 3).map(|index| (index * 7 % 251) as u8).collect()
    }

    #[test]
    fn a_canvas_file_reads_back_the_same_pixels() {
        let folder = crate::state::scratch("canvas-file");
        let pixels = sample(5, 3);
        let path = folder.join("main.canvas");
        std::fs::write(&path, encode(5, 3, &pixels, fingerprint(5, 3, &pixels))).unwrap();
        let canvas = read(&path).unwrap();
        assert_eq!((canvas.width, canvas.height, canvas.pixels), (5, 3, pixels));
    }

    #[test]
    fn a_canvas_whose_pixels_changed_is_refused() {
        let folder = crate::state::scratch("canvas-refused");
        let pixels = sample(4, 4);
        let mut bytes = encode(4, 4, &pixels, fingerprint(4, 4, &pixels));
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        let path = folder.join("main.canvas");
        std::fs::write(&path, &bytes).unwrap();
        assert!(format!("{:#}", read(&path).unwrap_err()).contains("fingerprint"));
        std::fs::write(&path, &bytes[..HEAD + 10]).unwrap();
        assert!(read(&path).is_err());
        std::fs::write(&path, b"nope").unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn the_fingerprint_is_the_one_the_tables_store_for_their_canvas() {
        let image = scooch_core::RgbImage::from_fn(48, 30, |x, y| [(x * 5) as u8, (y * 8) as u8, ((x + y) * 3) as u8].into());
        let analysis = scooch_core::analyze(&image);
        let scene = scooch_core::Scene { width: 48, height: 30, icons: Vec::new() };
        let options = scooch_core::Options::default();
        let placement = scooch_core::solve(&analysis, &scene, &options, None);
        let (bundle, _) = scooch_core::still(&image, &analysis, &placement, &scene, &options);
        let tables = bundle.tables_to_bytes();
        assert_eq!(tables_print(&tables), Some(fingerprint(bundle.canvas_width, bundle.canvas_height, &bundle.canvas)));
    }
}
