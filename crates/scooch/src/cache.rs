use crate::state::{write_atomic, write_parts, Store};
use anyhow::{Context, Result};
use scooch_core::{Analysis, RgbImage};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const KEPT: usize = 4;
const MAGIC: &[u8; 8] = b"SCRGB001";
const PIXEL_LIMIT: u64 = 120 << 20;
const SEED: u64 = 0x9E37_79B9_7F4A_7C15;
const MIX: u64 = 0xFF51_AFD7_ED55_8CCD;

pub fn digest(bytes: &[u8]) -> u64 {
    let mut state = SEED ^ (bytes.len() as u64).wrapping_mul(MIX);
    let (words, rest) = bytes.as_chunks::<8>();
    for word in words {
        state = stir(state ^ u64::from_le_bytes(*word));
    }
    let mut tail = [0u8; 8];
    tail[..rest.len()].copy_from_slice(rest);
    let state = stir(state ^ u64::from_le_bytes(tail));
    state ^ (state >> 32)
}

fn stir(value: u64) -> u64 {
    (value ^ (value >> 29)).wrapping_mul(MIX).rotate_left(27)
}

pub fn key(source: &Path) -> Result<u64> {
    let bytes = fs::read(source).with_context(|| format!("Scooch could not read {}, so check that the picture is still there.", source.display()))?;
    Ok(digest(&bytes) ^ digest(concat!(env!("CARGO_PKG_VERSION"), "-srgb").as_bytes()).rotate_left(17))
}

pub fn entry(store: &Store, key: u64) -> PathBuf {
    store.cache().join(format!("{key:016x}.json"))
}

pub fn pixel_file(store: &Store, key: u64) -> PathBuf {
    store.cache().join(format!("{key:016x}.rgb"))
}

pub fn analysis(store: &Store, key: u64, fresh: impl FnOnce() -> Result<Analysis>) -> Result<Analysis> {
    let file = entry(store, key);
    if let Some(found) = fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Analysis>(&bytes).ok()) {
        return Ok(found);
    }
    let made = fresh()?;
    fs::create_dir_all(store.cache())?;
    write_atomic(&file, &serde_json::to_vec(&made)?)?;
    trim(&store.cache(), "json", KEPT);
    Ok(made)
}

pub fn pixels(store: &Store, key: u64) -> Option<RgbImage> {
    let mut file = fs::File::open(pixel_file(store, key)).ok()?;
    let mut header = [0u8; 16];
    file.read_exact(&mut header).ok()?;
    if &header[..8] != MAGIC {
        return None;
    }
    let width = u32::from_le_bytes(header[8..12].try_into().ok()?);
    let height = u32::from_le_bytes(header[12..16].try_into().ok()?);
    let size = u64::from(width) * u64::from(height) * 3;
    if size == 0 || size > PIXEL_LIMIT || file.metadata().ok()?.len() != size + 16 {
        return None;
    }
    let mut raw = vec![0u8; size as usize];
    file.read_exact(&mut raw).ok()?;
    RgbImage::from_raw(width, height, raw)
}

pub fn keep_pixels(store: &Store, key: u64, image: &RgbImage) {
    let size = u64::from(image.width()) * u64::from(image.height()) * 3;
    if size > PIXEL_LIMIT || fs::create_dir_all(store.cache()).is_err() {
        return;
    }
    let mut header = MAGIC.to_vec();
    header.extend_from_slice(&image.width().to_le_bytes());
    header.extend_from_slice(&image.height().to_le_bytes());
    let file = pixel_file(store, key);
    if write_parts(&file, &[&header, image.as_raw()]).is_ok() {
        trim(&store.cache(), "rgb", 1);
    }
}

pub fn trim(folder: &Path, kind: &str, kept: usize) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    let mut files: Vec<(SystemTime, PathBuf)> = entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|extension| extension == kind))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect();
    files.sort();
    let extra = files.len().saturating_sub(kept);
    for (_, path) in files.into_iter().take(extra) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::scratch;

    #[test]
    fn digest_is_stable_and_spreads() {
        assert_eq!(digest(b"scooch"), digest(b"scooch"));
        assert_ne!(digest(b"scooch"), digest(b"scoocH"));
        assert_ne!(digest(b""), digest(b"\0"));
        assert_ne!(digest(&[1u8; 16]), digest(&[1u8; 17]));
    }

    #[test]
    fn analysis_is_computed_once_per_picture() {
        let store = Store::at(scratch("cache"));
        let picture = store.root.join("picture.png");
        let image = crate::scene::sample(64, 40);
        fs::write(&picture, scooch_core::encode_png(&image).unwrap()).unwrap();
        let key = key(&picture).unwrap();
        let first = analysis(&store, key, || Ok(scooch_core::analyze(&image))).unwrap();
        assert!(entry(&store, key).exists());
        let again = analysis(&store, key, || panic!("the cache was skipped")).unwrap();
        assert_eq!(first, again);
    }

    #[test]
    fn a_changed_picture_gets_a_new_entry() {
        let store = Store::at(scratch("cache-change"));
        let picture = store.root.join("picture.bin");
        fs::write(&picture, b"one").unwrap();
        let before = key(&picture).unwrap();
        fs::write(&picture, b"two").unwrap();
        assert_ne!(before, key(&picture).unwrap());
    }

    #[test]
    fn trim_keeps_the_newest_files() {
        let folder = scratch("trim");
        for name in ["a.json", "b.json", "c.json", "d.rgb"] {
            fs::write(folder.join(name), name).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        trim(&folder, "json", 2);
        assert!(!folder.join("a.json").exists() && folder.join("b.json").exists() && folder.join("c.json").exists() && folder.join("d.rgb").exists());
    }

    #[test]
    fn decoded_pixels_round_trip_and_only_the_newest_stays() {
        let store = Store::at(scratch("pixels"));
        let image = crate::scene::sample(37, 21);
        keep_pixels(&store, 1, &image);
        assert_eq!(pixels(&store, 1), Some(image.clone()));
        assert_eq!(pixels(&store, 2), None);
        std::thread::sleep(std::time::Duration::from_millis(20));
        keep_pixels(&store, 2, &image);
        assert!(!pixel_file(&store, 1).exists() && pixel_file(&store, 2).exists());
        fs::write(pixel_file(&store, 2), b"SCRGB001short").unwrap();
        assert_eq!(pixels(&store, 2), None);
    }
}
