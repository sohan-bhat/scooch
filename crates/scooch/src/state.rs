use anyhow::{Context, Result};
use scooch_core::{Options, Placement};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub on: bool,
    pub options: Options,
    pub screens: BTreeMap<String, Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon: Option<String>,
}

impl Default for State {
    fn default() -> Self {
        Self { version: VERSION, on: false, options: Options::default(), screens: BTreeMap::new(), problem: None, daemon: None }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Entry {
    pub source: PathBuf,
    pub original: PathBuf,
    pub output: Option<PathBuf>,
    pub counter: u64,
    pub fingerprint: Option<String>,
    pub placement: Option<Placement>,
    pub icons: usize,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub chosen: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub flip_helps: bool,
}

impl Entry {
    pub fn new(picture: &Path) -> Self {
        Self { source: picture.to_path_buf(), original: picture.to_path_buf(), ..Self::default() }
    }

    pub fn shown(&self) -> &Path {
        self.output.as_deref().unwrap_or(&self.source)
    }
}

pub const PROGRAM: &str = env!("CARGO_PKG_VERSION");

impl Store {
    pub fn mark(&self, running: Option<&str>) -> Result<()> {
        let mut state = self.load();
        if state.daemon.as_deref() == running {
            return Ok(());
        }
        state.daemon = running.map(str::to_string);
        self.save(&state)
    }
}

pub fn folder() -> PathBuf {
    match std::env::var_os("SCOOCH_HOME") {
        Some(custom) if !custom.is_empty() => PathBuf::from(custom),
        _ => dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join("Scooch"),
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

pub struct Lock {
    _file: fs::File,
}

impl Store {
    pub fn open() -> Self {
        Self::at(folder())
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self { root: canonical(&root).unwrap_or(root) }
    }

    pub fn state_file(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn lock_file(&self) -> PathBuf {
        self.root.join("watch.lock")
    }

    pub fn out(&self) -> PathBuf {
        self.root.join("out")
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn read(&self) -> Option<Vec<u8>> {
        fs::read(self.state_file()).ok()
    }

    pub fn load(&self) -> State {
        parse(self.read().as_deref())
    }

    pub fn save(&self, state: &State) -> Result<()> {
        fs::create_dir_all(&self.root).with_context(|| format!("Scooch could not make its folder at {}.", self.root.display()))?;
        let mut state = state.clone();
        state.version = VERSION;
        write_atomic(&self.state_file(), &serde_json::to_vec_pretty(&state)?)
    }

    pub fn owns(&self, path: &Path) -> bool {
        let out = self.out();
        if path.starts_with(&out) {
            return true;
        }
        let out = canonical(&out).unwrap_or(out);
        canonical(path).is_ok_and(|real| real.starts_with(&out)) || path.starts_with(&out)
    }

    pub fn output(&self, screen: &str, counter: u64, extension: &str) -> PathBuf {
        self.out().join(format!("{}-{counter}.{extension}", file_safe(screen)))
    }

    pub fn sweep(&self, screen: &str, keep: Option<&Path>) {
        let prefix = format!("{}-", file_safe(screen));
        let Ok(entries) = fs::read_dir(self.out()) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if Some(path.as_path()) != keep && entry.file_name().to_string_lossy().starts_with(&prefix) {
                let _ = fs::remove_file(path);
            }
        }
    }

    pub fn lock(&self) -> Result<Option<Lock>> {
        fs::create_dir_all(&self.root)?;
        let mut file = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.lock_file())?;
        if file.try_lock().is_err() {
            return Ok(None);
        }
        file.set_len(0)?;
        file.write_all(std::process::id().to_string().as_bytes())?;
        Ok(Some(Lock { _file: file }))
    }

    pub fn watcher(&self) -> Option<u32> {
        let file = fs::File::open(self.lock_file()).ok()?;
        if file.try_lock().is_ok() {
            return None;
        }
        Some(fs::read_to_string(self.lock_file()).ok().and_then(|text| text.trim().parse().ok()).unwrap_or(0))
    }
}

pub fn parse(bytes: Option<&[u8]>) -> State {
    let mut state = bytes.and_then(|bytes| serde_json::from_slice::<State>(bytes).ok()).unwrap_or_default();
    state.version = VERSION;
    state
}

pub fn same(a: &Path, b: &Path) -> bool {
    a == b || matches!((canonical(a), canonical(b)), (Ok(a), Ok(b)) if a == b)
}

pub fn canonical(path: &Path) -> std::io::Result<PathBuf> {
    #[cfg(windows)]
    return dunce::canonicalize(path);
    #[cfg(not(windows))]
    path.canonicalize()
}

pub fn file_safe(name: &str) -> String {
    name.chars().map(|letter| if letter.is_ascii_alphanumeric() || letter == '-' || letter == '_' { letter } else { '_' }).collect()
}

pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    write_parts(path, &[bytes])
}

pub fn write_parts(path: &Path, parts: &[&[u8]]) -> Result<()> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    let partial = path.with_file_name(format!(".{name}.{}.part", std::process::id()));
    let written = fs::File::create(&partial).and_then(|mut file| parts.iter().try_for_each(|part| file.write_all(part))).and_then(|()| fs::rename(&partial, path));
    if written.is_err() {
        let _ = fs::remove_file(&partial);
    }
    written.with_context(|| format!("Scooch could not write {}, so check that the disk has room.", path.display()))
}

#[cfg(test)]
pub fn scratch(name: &str) -> PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let base = std::env::var_os("SCOOCH_TEST_TMP").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/scooch-tests"));
    let folder = base.join(format!("{name}-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::SeqCst)));
    let _ = fs::remove_dir_all(&folder);
    fs::create_dir_all(&folder).unwrap();
    folder.canonicalize().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_loads_as_off() {
        let store = Store::at(scratch("fresh"));
        let state = store.load();
        assert!(!state.on);
        assert!(state.screens.is_empty());
        assert_eq!(state.version, VERSION);
    }

    #[test]
    fn state_survives_a_round_trip() {
        let store = Store::at(scratch("round"));
        let mut state = State { on: true, ..State::default() };
        let entry = Entry { output: Some("/data/out/main-3.jpg".into()), counter: 3, fingerprint: Some("00ff".into()), icons: 9, ..Entry::new(Path::new("/pictures/harbor.jpg")) };
        state.screens.insert("main".into(), entry);
        store.save(&state).unwrap();
        assert_eq!(store.load(), state);
    }

    #[test]
    fn the_file_has_the_documented_shape() {
        let store = Store::at(scratch("shape"));
        let mut state = State { on: true, ..State::default() };
        state.screens.insert("ABC".into(), Entry::new(Path::new("/p/a.jpg")));
        store.save(&state).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&store.read().unwrap()).unwrap();
        assert_eq!(value["version"], 1);
        assert_eq!(value["on"], true);
        assert_eq!(value["options"]["max_zoom"].as_f64().map(|zoom| (zoom * 10.0).round()), Some(13.0));
        let entry = &value["screens"]["ABC"];
        for key in ["source", "original", "output", "counter", "fingerprint", "placement"] {
            assert!(entry.get(key).is_some(), "{key}");
        }
        assert!(value.get("problem").is_none());
    }

    #[test]
    fn a_broken_file_loads_as_off() {
        let store = Store::at(scratch("broken"));
        fs::write(store.state_file(), b"{ not json").unwrap();
        assert!(!store.load().on);
    }

    #[test]
    fn older_files_with_missing_fields_still_load() {
        let store = Store::at(scratch("older"));
        fs::write(store.state_file(), br#"{"on":true,"screens":{"main":{"source":"/p/a.jpg","counter":3}}}"#).unwrap();
        let state = store.load();
        assert!(state.on);
        assert_eq!(state.screens["main"].counter, 3);
        assert_eq!(state.options, Options::default());
    }

    #[test]
    fn saving_is_atomic_and_leaves_no_partial_file() {
        let store = Store::at(scratch("atomic"));
        store.save(&State::default()).unwrap();
        store.save(&State { on: true, ..State::default() }).unwrap();
        let names: Vec<_> = fs::read_dir(&store.root).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("state.json")]);
        assert!(store.load().on);
    }

    #[test]
    fn a_failed_write_keeps_the_old_file() {
        let folder = scratch("failed");
        let target = folder.join("state.json");
        fs::write(&target, b"old").unwrap();
        let blocked = folder.join(format!(".state.json.{}.part", std::process::id()));
        fs::create_dir(&blocked).unwrap();
        assert!(write_atomic(&target, b"new").is_err());
        assert_eq!(fs::read(&target).unwrap(), b"old");
    }

    #[test]
    fn outputs_are_named_by_screen_and_counter() {
        let store = Store::at("/data");
        assert_eq!(store.output("AB-12", 4, "jpg"), PathBuf::from("/data/out/AB-12-4.jpg"));
        assert_eq!(store.output("odd/name:1", 1, "png"), PathBuf::from("/data/out/odd_name_1-1.png"));
    }

    #[test]
    fn only_the_out_folder_counts_as_ours() {
        let store = Store::at(scratch("owns"));
        fs::create_dir_all(store.out()).unwrap();
        assert!(store.owns(&store.output("main", 1, "jpg")));
        assert!(!store.owns(Path::new("/Library/Desktop Pictures/lake.jpg")));
        assert!(!store.owns(&store.state_file()));
    }

    #[test]
    fn sweep_keeps_the_newest_output_and_other_screens() {
        let store = Store::at(scratch("sweep"));
        fs::create_dir_all(store.out()).unwrap();
        let old = store.output("main", 1, "jpg");
        let new = store.output("main", 2, "jpg");
        let other = store.output("main2", 1, "jpg");
        for path in [&old, &new, &other] {
            fs::write(path, b"x").unwrap();
        }
        store.sweep("main", Some(&new));
        assert!(!old.exists() && new.exists() && other.exists());
        store.sweep("main", None);
        assert!(!new.exists() && other.exists());
    }

    #[test]
    fn only_one_watcher_holds_the_lock() {
        let store = Store::at(scratch("lock"));
        assert_eq!(store.watcher(), None);
        let held = store.lock().unwrap();
        assert!(held.is_some());
        assert!(store.lock().unwrap().is_none());
        assert_eq!(store.watcher(), Some(std::process::id()));
        drop(held);
        assert_eq!(store.watcher(), None);
        assert!(store.lock().unwrap().is_some());
    }

    #[test]
    fn same_sees_through_links() {
        let folder = scratch("same");
        let real = folder.join("real.jpg");
        fs::write(&real, b"x").unwrap();
        assert!(same(&real, &real));
        assert!(!same(&real, &folder.join("other.jpg")));
        #[cfg(unix)]
        {
            let link = folder.join("link.jpg");
            std::os::unix::fs::symlink(&real, &link).unwrap();
            assert!(same(&real, &link));
        }
    }

    #[test]
    fn canonical_paths_have_no_verbatim_prefix() {
        let folder = scratch("canonical");
        let real = canonical(&folder.join(".")).unwrap();
        assert!(!real.to_string_lossy().starts_with(r"\\?\"), "{}", real.display());
        assert!(same(&real, &folder));
    }

    #[test]
    fn shown_is_the_output_or_else_the_source() {
        let mut entry = Entry::new(Path::new("/p/a.jpg"));
        assert_eq!(entry.shown(), Path::new("/p/a.jpg"));
        entry.output = Some("/d/out/x-1.jpg".into());
        assert_eq!(entry.shown(), Path::new("/d/out/x-1.jpg"));
    }
}
