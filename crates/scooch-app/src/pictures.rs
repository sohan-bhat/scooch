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

fn held(error: &std::io::Error) -> bool {
    cfg!(windows) && matches!(error.raw_os_error(), Some(5 | 32 | 33))
}

pub fn save(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let staged = path.with_file_name(format!(".{name}.{}", std::process::id()));
    std::fs::write(&staged, bytes)?;
    let mut tries = 0;
    loop {
        match std::fs::rename(&staged, path) {
            Ok(()) => return Ok(()),
            Err(error) if held(&error) && tries < 40 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(error) => {
                let _ = std::fs::remove_file(&staged);
                return Err(error);
            }
        }
    }
}

fn selection_file(folder: &Path) -> PathBuf {
    folder.join("selected.json")
}

pub fn selected(folder: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(selection_file(folder)).ok()?;
    let path: PathBuf = serde_json::from_slice(&bytes).ok()?;
    path.is_file().then_some(path)
}

pub fn select(folder: &Path, path: Option<&Path>) {
    match path {
        Some(path) => {
            if std::fs::create_dir_all(folder).is_ok() {
                if let Ok(bytes) = serde_json::to_vec(path) {
                    if let Err(error) = save(&selection_file(folder), &bytes) {
                        eprintln!("scooch-app the selection was not kept, {error}");
                    }
                }
            }
        }
        None => {
            let _ = std::fs::remove_file(selection_file(folder));
        }
    }
}

pub fn shown(view: &mut facade::View, selection: Option<&Path>) {
    if view.on {
        return;
    }
    if let Some(selection) = selection {
        put(selection, &mut view.current, view.monitors.iter_mut().map(|monitor| (&mut monitor.picture, &mut monitor.stamp)));
    }
}

fn put<'a>(selection: &Path, current: &mut Option<PathBuf>, monitors: impl Iterator<Item = (&'a mut Option<PathBuf>, &'a mut Option<u64>)>) {
    *current = Some(selection.to_path_buf());
    for (picture, stamp) in monitors {
        *picture = Some(selection.to_path_buf());
        *stamp = facade::stamp(Some(selection));
    }
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
            if let Err(error) = save(&history_file(folder), &bytes) {
                eprintln!("scooch-app the picture list was not kept, {error}");
            }
        }
    }
}

pub fn listed(folder: &Path) -> Vec<String> {
    facade::pictures(&history(folder)).into_iter().map(|path| path.to_string_lossy().into_owned()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_selection_is_kept_across_openings_until_it_is_cleared_or_its_file_is_gone() {
        let folder = std::env::temp_dir().join(format!("scooch-selected-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        let picture = folder.join("sky.jpg");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(&picture, b"x").unwrap();
        assert_eq!(selected(&folder), None);
        select(&folder, Some(&picture));
        assert_eq!(selected(&folder), Some(picture.clone()));
        select(&folder, None);
        assert_eq!(selected(&folder), None);
        select(&folder, Some(&picture));
        std::fs::remove_file(&picture).unwrap();
        assert_eq!(selected(&folder), None);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_save_replaces_the_file_and_leaves_nothing_staged() {
        let folder = std::env::temp_dir().join(format!("scooch-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("pictures.json");
        std::fs::write(&file, b"old").unwrap();
        save(&file, b"new").unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[cfg(windows)]
    #[test]
    fn a_save_waits_while_another_process_holds_the_file_without_delete_sharing() {
        use std::os::windows::fs::OpenOptionsExt;
        let folder = std::env::temp_dir().join(format!("scooch-held-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let file = folder.join("selected.json");
        std::fs::write(&file, b"old").unwrap();
        let holder = std::fs::OpenOptions::new().read(true).share_mode(1).open(&file).unwrap();
        let releasing = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            drop(holder);
        });
        save(&file, b"new").unwrap();
        releasing.join().unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(&folder).unwrap().count(), 1);
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_selection_becomes_the_current_picture_and_every_preview_with_its_own_stamp() {
        let folder = std::env::temp_dir().join(format!("scooch-put-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let sky = folder.join("sky.jpg");
        std::fs::write(&sky, b"x").unwrap();
        std::fs::File::options().write(true).open(&sky).unwrap().set_modified(SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(1_700_000_000_789)).unwrap();
        let (mut current, mut left, mut right) = (Some(PathBuf::from("/old.jpg")), (None, None), (Some(PathBuf::from("/old.jpg")), Some(1_600_000_000_000)));
        put(&sky, &mut current, [(&mut left.0, &mut left.1), (&mut right.0, &mut right.1)].into_iter());
        let _ = std::fs::remove_dir_all(&folder);
        let stamped = (Some(sky.clone()), Some(1_700_000_000_789));
        assert_eq!((current, left, right), (Some(sky.clone()), stamped.clone(), stamped));
    }

    fn request(uri: &str) -> Request<Vec<u8>> {
        Request::builder().uri(uri).body(Vec::new()).unwrap()
    }

    #[test]
    fn the_stamp_after_a_picture_path_changes_neither_the_path_nor_the_thumbnail_flag() {
        let path = PathBuf::from("/home/ada/.local/share/scooch/out/main-shown.jpg");
        let at = |query: &str| decoded_path(&request(&format!("scooch://localhost/%2Fhome%2Fada%2F.local%2Fshare%2Fscooch%2Fout%2Fmain-shown.jpg{query}")));
        assert_eq!(at(""), Some((path.clone(), false)));
        assert_eq!(at("?v=123"), Some((path.clone(), false)));
        assert_eq!(at("?thumb"), Some((path.clone(), true)));
        assert_eq!(at("?thumb&v=123"), Some((path.clone(), true)));
        assert_eq!(at("?v=123&thumb"), Some((path, true)));
    }
}
