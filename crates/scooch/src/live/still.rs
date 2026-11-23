use anyhow::{bail, Context, Result};
use std::path::Path;

const MAGIC: [u8; 4] = *b"SCST";
const VERSION: u32 = 1;
const HEAD: usize = 16;

pub struct Raw {
    pub width: u32,
    pub height: u32,
    bytes: Vec<u8>,
}

impl Raw {
    pub fn rgba(&self) -> &[u8] {
        &self.bytes[HEAD..]
    }
}

pub fn write(path: &Path, width: u32, height: u32, rgba: &[u8]) -> Result<()> {
    if rgba.len() as u64 != u64::from(width) * u64::from(height) * 4 {
        bail!("the still does not fit {width}x{height}");
    }
    let mut head = Vec::with_capacity(HEAD);
    head.extend_from_slice(&MAGIC);
    for word in [VERSION, width, height] {
        head.extend_from_slice(&word.to_le_bytes());
    }
    crate::state::write_parts_unsynced(path, &[&head, rgba])
}

pub fn write_rgb(path: &Path, image: &scooch_core::RgbImage) -> Result<()> {
    let mut rgba = vec![255u8; image.as_raw().len() / 3 * 4];
    for (target, pixel) in rgba.as_chunks_mut::<4>().0.iter_mut().zip(image.as_raw().as_chunks::<3>().0) {
        target[..3].copy_from_slice(pixel);
    }
    write(path, image.width(), image.height(), &rgba)
}

pub fn read(path: &Path) -> Result<Raw> {
    let bytes = std::fs::read(path).with_context(|| format!("the still {} could not be read", path.display()))?;
    let word = |index: usize| bytes.get(4 + index * 4..8 + index * 4).and_then(|word| word.try_into().ok()).map(u32::from_le_bytes);
    let (Some(version), Some(width), Some(height)) = (word(0), word(1), word(2)) else { bail!("the still file is too short") };
    if bytes[..4] != MAGIC || version != VERSION {
        bail!("the still file is not a Scooch still of this version");
    }
    if (bytes.len() - HEAD) as u64 != u64::from(width) * u64::from(height) * 4 {
        bail!("the still file does not hold {width}x{height} pixels");
    }
    Ok(Raw { width, height, bytes })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_still_reads_back_and_a_damaged_one_is_refused() {
        let folder = crate::state::scratch("raw-still");
        let path = folder.join("a.jpg.still");
        let rgba: Vec<u8> = (0..5u32 * 3 * 4).map(|index| (index * 7 % 256) as u8).collect();
        write(&path, 5, 3, &rgba).unwrap();
        let raw = read(&path).unwrap();
        assert_eq!((raw.width, raw.height, raw.rgba()), (5, 3, &rgba[..]));
        assert!(write(&path, 5, 4, &rgba).is_err());
        let mut cut = std::fs::read(&path).unwrap();
        cut.pop();
        std::fs::write(&path, &cut).unwrap();
        assert!(read(&path).is_err());
        let mut other = std::fs::read(&path).unwrap();
        other[0] = b'X';
        std::fs::write(&path, &other).unwrap();
        assert!(read(&path).is_err());
        std::fs::write(&path, b"SCST").unwrap();
        assert!(read(&path).is_err());
        let image = scooch_core::RgbImage::from_raw(2, 1, vec![1, 2, 3, 4, 5, 6]).unwrap();
        write_rgb(&path, &image).unwrap();
        assert_eq!(read(&path).unwrap().rgba(), &[1, 2, 3, 255, 4, 5, 6, 255]);
    }
}
