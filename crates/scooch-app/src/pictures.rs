use scooch::facade;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;
use tauri::http::{header, Request, Response, StatusCode};

pub const SCHEME: &str = "scooch";
const PREVIEW: u32 = 1800;
const KEEP: usize = 12;
const CACHED: usize = 96;

type Key = (PathBuf, bool, Option<SystemTime>);

static CACHE: Mutex<Vec<(Key, Vec<u8>)>> = Mutex::new(Vec::new());

fn decoded_path(request: &Request<Vec<u8>>) -> Option<(PathBuf, bool)> {
    let raw = request.uri().path().trim_start_matches('/');
    let path = percent_encoding::percent_decode_str(raw).decode_utf8().ok()?.into_owned();
    let thumb = request.uri().query().is_some_and(|query| query.split('&').any(|pair| pair == "thumb"));
    Some((PathBuf::from(path), thumb))
}

fn rendered(path: &Path, thumb: bool) -> anyhow::Result<Vec<u8>> {
    let key: Key = (path.to_path_buf(), thumb, std::fs::metadata(path).and_then(|meta| meta.modified()).ok());
    if let Some((_, bytes)) = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).iter().find(|(seen, _)| *seen == key) {
        return Ok(bytes.clone());
    }
    let bytes = if thumb { facade::thumbnail(path)? } else { facade::preview(path, PREVIEW)? };
    let mut cache = CACHE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    cache.retain(|(seen, _)| seen.0 != key.0 || seen.1 != key.1);
    cache.push((key, bytes.clone()));
    if cache.len() > CACHED {
        cache.remove(0);
    }
    Ok(bytes)
}

pub fn answer(request: &Request<Vec<u8>>) -> Response<Vec<u8>> {
    let made = decoded_path(request).ok_or_else(|| anyhow::anyhow!("bad path")).and_then(|(path, thumb)| rendered(&path, thumb));
    match made {
        Ok(bytes) => Response::builder().status(StatusCode::OK).header(header::CONTENT_TYPE, "image/jpeg").header(header::CACHE_CONTROL, "no-cache").body(bytes),
        Err(_) => Response::builder().status(StatusCode::NOT_FOUND).body(Vec::new()),
    }
    .unwrap_or_else(|_| Response::new(Vec::new()))
}

fn history_file(folder: &Path) -> PathBuf {
    folder.join("pictures.json")
}

pub fn history(folder: &Path) -> Vec<PathBuf> {
    std::fs::read(history_file(folder)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn remember(folder: &Path, path: &Path) {
    let mut kept = history(folder);
    kept.retain(|seen| seen != path);
    kept.insert(0, path.to_path_buf());
    kept.truncate(KEEP);
    if std::fs::create_dir_all(folder).is_ok() {
        if let Ok(bytes) = serde_json::to_vec(&kept) {
            let _ = std::fs::write(history_file(folder), bytes);
        }
    }
}

pub fn listed(folder: &Path) -> Vec<String> {
    facade::pictures(&history(folder)).into_iter().map(|path| path.to_string_lossy().into_owned()).collect()
}
