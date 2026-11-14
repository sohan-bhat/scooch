use anyhow::{bail, Context, Result};
use scooch_core::Canvas;
use std::path::Path;

const MAGIC: [u8; 4] = *b"SCCV";
const VERSION: u32 = 2;
const RGBA: u32 = 4;
const SRGB: u32 = 1;
pub const HEAD: usize = 36;
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

pub fn count(width: u32, height: u32) -> u32 {
    32 - width.max(height).max(1).leading_zeros()
}

fn sizes(width: u32, height: u32) -> Vec<usize> {
    let (mut w, mut h) = (width.max(1), height.max(1));
    (0..count(width, height))
        .map(|_| {
            let size = w as usize * h as usize * 4;
            (w, h) = ((w / 2).max(1), (h / 2).max(1));
            size
        })
        .collect()
}

pub fn head(width: u32, height: u32, print: u64) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEAD);
    out.extend_from_slice(&MAGIC);
    for word in [VERSION, width, height, RGBA, SRGB, count(width, height)] {
        out.extend_from_slice(&word.to_le_bytes());
    }
    out.extend_from_slice(&print.to_le_bytes());
    out
}

pub fn write(path: &Path, width: u32, height: u32, levels: &[Vec<u8>], print: u64) -> Result<()> {
    let wanted = sizes(width, height);
    if levels.len() != wanted.len() || levels.iter().zip(&wanted).any(|(level, size)| level.len() != *size) {
        bail!("the canvas levels do not fit {width}x{height}");
    }
    let header = head(width, height, print);
    let mut parts: Vec<&[u8]> = vec![&header];
    parts.extend(levels.iter().map(Vec::as_slice));
    crate::state::write_parts(path, &parts)
}

pub struct Cached {
    pub width: u32,
    pub height: u32,
    print: u64,
    bytes: Vec<u8>,
}

impl Cached {
    pub fn levels(&self) -> Vec<&[u8]> {
        let mut at = HEAD;
        sizes(self.width, self.height)
            .into_iter()
            .map(|size| {
                let level = &self.bytes[at..at + size];
                at += size;
                level
            })
            .collect()
    }

    pub fn canvas(&self) -> Result<Canvas> {
        let size = self.width as usize * self.height as usize * 4;
        let base = &self.bytes[HEAD..HEAD + size];
        let mut pixels = vec![0u8; self.width as usize * self.height as usize * 3];
        for (target, pixel) in pixels.as_chunks_mut::<3>().0.iter_mut().zip(base.as_chunks::<4>().0) {
            *target = [pixel[0], pixel[1], pixel[2]];
        }
        if fingerprint(self.width, self.height, &pixels) != self.print {
            bail!("the canvas pixels do not match the fingerprint the tables expect");
        }
        Ok(Canvas { width: self.width, height: self.height, pixels })
    }
}

pub fn read(path: &Path) -> Result<Cached> {
    let bytes = std::fs::read(path).with_context(|| format!("the canvas {} could not be read", path.display()))?;
    if bytes.len() < HEAD || bytes[..4] != MAGIC {
        bail!("the canvas file is not a Scooch canvas");
    }
    let word = |index: usize| u32::from_le_bytes(bytes[4 + index * 4..8 + index * 4].try_into().unwrap_or_default());
    let (version, width, height, channels, colour, levels) = (word(0), word(1), word(2), word(3), word(4), word(5));
    if version != VERSION || channels != RGBA || colour != SRGB || width == 0 || height == 0 || levels != count(width, height) {
        bail!("the canvas file is version {version} with {channels} channels in colour {colour}, not what this helper reads");
    }
    let print = u64::from_le_bytes(bytes[28..36].try_into().unwrap_or_default());
    let need: usize = sizes(width, height).into_iter().sum();
    if bytes.len() != HEAD + need {
        bail!("the canvas file holds {} bytes of pixels for {width}x{height}", bytes.len() - HEAD);
    }
    Ok(Cached { width, height, print, bytes })
}

#[cfg(all(test, any(windows, target_os = "macos")))]
mod tests {
    use super::*;

    fn sample(width: u32, height: u32) -> Canvas {
        Canvas { width, height, pixels: (0..width * height * 3).map(|index| (index * 7 % 251) as u8).collect() }
    }

    fn saved(name: &str, canvas: &Canvas) -> std::path::PathBuf {
        let folder = crate::state::scratch(name);
        let path = folder.join("main.canvas");
        write(&path, canvas.width, canvas.height, &scooch_live::levels(canvas), fingerprint(canvas.width, canvas.height, &canvas.pixels)).unwrap();
        path
    }

    #[test]
    fn a_canvas_file_reads_back_the_same_pixels_and_every_level() {
        let canvas = sample(5, 3);
        let cached = read(&saved("canvas-file", &canvas)).unwrap();
        assert_eq!(cached.canvas().unwrap(), canvas);
        let levels = cached.levels();
        assert_eq!(levels.len(), 3);
        assert_eq!(levels.iter().map(|level| level.len()).collect::<Vec<_>>(), vec![60, 8, 4]);
        assert_eq!(levels.iter().map(|level| level.to_vec()).collect::<Vec<_>>(), scooch_live::levels(&canvas));
    }

    #[test]
    fn a_canvas_whose_pixels_changed_is_refused() {
        let canvas = sample(4, 4);
        let path = saved("canvas-refused", &canvas);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[HEAD] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(format!("{:#}", read(&path).unwrap().canvas().unwrap_err()).contains("fingerprint"));
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
