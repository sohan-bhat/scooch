use crate::state::{write_atomic, Store};
use anyhow::{Context, Result};
use scooch_core::Analysis;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

const KEPT: usize = 4;
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

pub fn entry(store: &Store, source: &Path) -> Result<PathBuf> {
    let bytes = fs::read(source).with_context(|| format!("Scooch could not read {}, so check that the picture is still there.", source.display()))?;
    let key = digest(&bytes) ^ digest(env!("CARGO_PKG_VERSION").as_bytes()).rotate_left(17);
    Ok(store.cache().join(format!("{key:016x}.json")))
}

pub fn analysis(store: &Store, source: &Path, fresh: impl FnOnce() -> Result<Analysis>) -> Result<Analysis> {
    let file = entry(store, source)?;
    if let Some(found) = fs::read(&file).ok().and_then(|bytes| serde_json::from_slice::<Analysis>(&bytes).ok()) {
        return Ok(found);
    }
    let made = fresh()?;
    fs::create_dir_all(store.cache())?;
    write_atomic(&file, &serde_json::to_vec(&made)?)?;
    trim(&store.cache(), KEPT);
    Ok(made)
}

pub fn trim(folder: &Path, kept: usize) {
    let Ok(entries) = fs::read_dir(folder) else { return };
    let mut files: Vec<(SystemTime, PathBuf)> = entries.flatten().filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path()))).collect();
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
        let first = analysis(&store, &picture, || Ok(scooch_core::analyze(&image))).unwrap();
        assert!(entry(&store, &picture).unwrap().exists());
        let again = analysis(&store, &picture, || panic!("the cache was skipped")).unwrap();
        assert_eq!(first, again);
    }

    #[test]
    fn a_changed_picture_gets_a_new_entry() {
        let store = Store::at(scratch("cache-change"));
        let picture = store.root.join("picture.bin");
        fs::write(&picture, b"one").unwrap();
        let before = entry(&store, &picture).unwrap();
        fs::write(&picture, b"two").unwrap();
        assert_ne!(before, entry(&store, &picture).unwrap());
    }

    #[test]
    fn trim_keeps_the_newest_files() {
        let folder = scratch("trim");
        for name in ["a", "b", "c"] {
            fs::write(folder.join(name), name).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        trim(&folder, 2);
        assert!(!folder.join("a").exists() && folder.join("b").exists() && folder.join("c").exists());
    }
}
