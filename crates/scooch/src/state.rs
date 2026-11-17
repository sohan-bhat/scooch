use anyhow::{bail, Context, Result};
use scooch_core::{Options, Placement, Scenery};
use serde::{Deserialize, Deserializer, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const VERSION: u32 = 1;
pub const UNREAD: &str = "Some saved settings did not read.";
pub const NEWER: &str = "Saved settings came from a newer Scooch.";
pub const BUSY: &str = "Another scooch command is still running, so try again in a moment.";
pub const COMMAND_PATIENCE: Duration = Duration::from_secs(20);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    pub version: u32,
    pub on: bool,
    #[serde(default = "fresh_options", deserialize_with = "options_or_scenery")]
    pub options: Options,
    pub glide: bool,
    pub predict: bool,
    pub screens: BTreeMap<String, Entry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daemon: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub restoring: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub picture: Option<PathBuf>,
    #[serde(skip_serializing_if = "Tally::is_empty")]
    pub tally: Tally,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub kept: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stuck: Vec<PathBuf>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub scales: BTreeMap<String, f32>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub starts: BTreeMap<String, Start>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Start {
    pub ready: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub colds: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub canvases: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub windows: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub draws: Vec<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub reveals: Vec<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missed: Option<u32>,
    pub at: u64,
}

pub fn fresh_options() -> Options {
    Options { scenery: Scenery::Auto, ..Options::default() }
}

fn options_or_scenery<'de, D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Options, D::Error> {
    let mut value = serde_json::Value::deserialize(deserializer)?;
    if let Some(fields) = value.as_object_mut() {
        fields.entry("scenery").or_insert_with(|| serde_json::json!(Scenery::Auto));
    }
    serde_json::from_value(value).map_err(serde::de::Error::custom)
}

pub fn scenery_on(options: &Options) -> bool {
    options.scenery != Scenery::Off
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Tally {
    pub back: usize,
    pub plain: usize,
    pub stuck: usize,
    pub pending: usize,
    pub abandoned: usize,
    pub strays: usize,
}

impl Tally {
    pub fn is_empty(&self) -> bool {
        *self == Tally::default()
    }
}

impl Default for State {
    fn default() -> Self {
        Self { version: VERSION, on: false, options: Options::default(), glide: true, predict: true, screens: BTreeMap::new(), problem: None, daemon: None, restoring: false, picture: None, tally: Tally::default(), kept: Vec::new(), stuck: Vec::new(), scales: BTreeMap::new(), starts: BTreeMap::new() }
    }
}

impl State {
    pub fn fresh() -> Self {
        Self { options: fresh_options(), ..Self::default() }
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
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub painted: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unrestorable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub absent_since: Option<u64>,
    #[serde(skip_serializing_if = "is_zero")]
    pub absent_ticks: u32,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub belongs: Option<Belongs>,
    #[serde(skip_serializing_if = "Option::is_none", deserialize_with = "lenient")]
    pub drawn: Option<String>,
}

fn lenient<'de, D: Deserializer<'de>, T: serde::de::DeserializeOwned>(deserializer: D) -> std::result::Result<Option<T>, D::Error> {
    Ok(serde_json::from_value(serde_json::Value::deserialize(deserializer)?).ok())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Belongs {
    pub key: u64,
    pub stamp: u64,
    pub answer: bool,
}

fn is_zero(count: &u32) -> bool {
    *count == 0
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
    pub fn pictures(&self) -> Vec<PathBuf> {
        let Ok(entries) = fs::read_dir(self.out()) else { return Vec::new() };
        entries.flatten().map(|entry| entry.path()).filter(|path| path.is_file()).collect()
    }

    pub fn mark(&self, running: Option<&str>) -> Result<()> {
        let mut state = self.load();
        if state.daemon.as_deref() == running {
            return Ok(());
        }
        state.daemon = running.map(str::to_string);
        self.save(&state)
    }
}

pub fn folder_name(linux: bool) -> &'static str {
    if linux {
        "scooch"
    } else {
        "Scooch"
    }
}

pub fn folder() -> PathBuf {
    match std::env::var_os("SCOOCH_HOME") {
        Some(custom) if !custom.is_empty() => PathBuf::from(custom),
        _ => dirs::data_local_dir().unwrap_or_else(std::env::temp_dir).join(folder_name(cfg!(target_os = "linux"))),
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    pub root: PathBuf,
}

pub struct Lock {
    _file: fs::File,
    pid: PathBuf,
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.pid);
    }
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

    pub fn pid_file(&self) -> PathBuf {
        self.root.join("watch.pid")
    }

    pub fn recorded_pid(&self) -> Option<u32> {
        fs::read_to_string(self.pid_file()).ok()?.trim().parse().ok()
    }

    pub fn out(&self) -> PathBuf {
        self.root.join("out")
    }

    pub fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    pub fn tables(&self) -> PathBuf {
        self.cache().join("tables")
    }

    pub fn canvas_for(&self, screen: &str, key: u64) -> PathBuf {
        self.tables().join(format!("{}.{key:016x}.canvas", file_safe(screen)))
    }

    pub fn tables_for(&self, output: &Path, key: u64) -> PathBuf {
        let name = output.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
        self.tables().join(format!("{name}.{key:016x}.tables"))
    }

    pub fn aside_file(&self) -> PathBuf {
        self.root.join("state.unreadable.json")
    }

    pub fn kept_file(&self) -> PathBuf {
        self.root.join("state.kept.json")
    }

    fn keep(&self, bytes: &[u8], first: &str) {
        if fs::read(self.kept_file()).ok().as_deref() != Some(bytes) && write_atomic(&self.kept_file(), bytes).is_ok() {
            eprintln!("{first} Each screen's own picture is kept. The old file is at {}.", self.kept_file().display());
        }
    }

    pub fn read(&self) -> Option<Vec<u8>> {
        let bytes = fs::read(self.state_file()).ok()?;
        if let Ok(state) = serde_json::from_slice::<State>(&bytes) {
            if state.version > VERSION {
                self.keep(&bytes, NEWER);
            }
            return Some(bytes);
        }
        if let Some(saved) = salvage(&bytes) {
            self.keep(&bytes, UNREAD);
            return serde_json::to_vec_pretty(&saved).ok();
        }
        if fs::rename(self.state_file(), self.aside_file()).is_err() {
            let _ = write_atomic(&self.aside_file(), &bytes);
            let _ = fs::remove_file(self.state_file());
        }
        None
    }

    pub fn load(&self) -> State {
        parse(self.read().as_deref())
    }

    pub fn retire_aside(&self) {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|since| since.as_secs()).unwrap_or(0);
        let _ = fs::rename(self.aside_file(), self.root.join(format!("state.unreadable-{stamp}.json")));
    }

    pub fn aside_line(&self) -> Option<String> {
        self.aside_file().is_file().then(|| format!("Scooch could not read its saved settings, so it started fresh and kept them at {}.", self.aside_file().display()))
    }

    pub fn save(&self, state: &State) -> Result<()> {
        self.write(state).map(drop)
    }

    pub fn write(&self, state: &State) -> Result<Vec<u8>> {
        fs::create_dir_all(&self.root).with_context(|| format!("Scooch could not make its folder at {}.", self.root.display()))?;
        let mut state = state.clone();
        state.version = VERSION;
        let bytes = serde_json::to_vec_pretty(&state)?;
        write_atomic(&self.state_file(), &bytes)?;
        Ok(bytes)
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

    pub fn outputs_of(&self, screen: &str) -> Vec<PathBuf> {
        let prefix = format!("{}-", file_safe(screen));
        let Ok(entries) = fs::read_dir(self.out()) else { return Vec::new() };
        entries.flatten().filter(|entry| entry.file_name().to_string_lossy().starts_with(&prefix)).map(|entry| entry.path()).collect()
    }

    pub fn lock(&self) -> Result<Option<Lock>> {
        fs::create_dir_all(&self.root)?;
        let file = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.lock_file())?;
        if file.try_lock().is_err() {
            return Ok(None);
        }
        if let Err(error) = write_atomic(&self.pid_file(), std::process::id().to_string().as_bytes()) {
            eprintln!("{error}");
        }
        Ok(Some(Lock { _file: file, pid: self.pid_file() }))
    }

    pub fn command_lock(&self, patience: Duration) -> Result<fs::File> {
        fs::create_dir_all(&self.root).with_context(|| format!("Scooch could not make its folder at {}.", self.root.display()))?;
        let file = fs::OpenOptions::new().create(true).truncate(false).read(true).write(true).open(self.root.join("command.lock"))?;
        let started = Instant::now();
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(file),
                Err(fs::TryLockError::WouldBlock) if started.elapsed() < patience => std::thread::sleep(Duration::from_millis(50)),
                Err(fs::TryLockError::WouldBlock) => bail!(BUSY),
                Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            }
        }
    }

    pub fn watcher(&self) -> Option<u32> {
        self.recorded_pid().filter(|process| crate::poke::alive(*process))
    }
}

fn salvage(bytes: &[u8]) -> Option<State> {
    let value: serde_json::Value = serde_json::from_slice(bytes).ok()?;
    let screens = value.get("screens")?.as_object()?;
    let mut state = State::fresh();
    if let Some(on) = value.get("on").and_then(serde_json::Value::as_bool) {
        state.on = on;
    }
    for (id, entry) in screens {
        let read = serde_json::from_value::<Entry>(entry.clone()).ok().or_else(|| entry.get("original").and_then(serde_json::Value::as_str).map(|original| Entry::new(Path::new(original))));
        if let Some(read) = read {
            state.screens.insert(id.clone(), read);
        }
    }
    Some(state)
}

pub fn parse(bytes: Option<&[u8]>) -> State {
    let mut state = bytes.and_then(|bytes| serde_json::from_slice::<State>(bytes).ok()).unwrap_or_else(State::fresh);
    state.version = VERSION;
    state
}

pub fn display_of(id: &str) -> &str {
    id.split('@').next().unwrap_or(id)
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
    write_with(path, parts, true)
}

pub fn write_unsynced(path: &Path, bytes: &[u8]) -> Result<()> {
    write_with(path, &[bytes], false)
}

fn write_with(path: &Path, parts: &[&[u8]], sync: bool) -> Result<()> {
    let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
    static WRITES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let serial = WRITES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let partial = path.with_file_name(format!(".{name}.{}.{serial}.part", std::process::id()));
    let written = fs::File::create(&partial)
        .and_then(|mut file| {
            parts.iter().try_for_each(|part| file.write_all(part))?;
            if sync {
                file.sync_all()?;
            }
            Ok(())
        })
        .and_then(|()| fs::rename(&partial, path));
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

    fn read_with(field: &str, odd: serde_json::Value) -> State {
        let mut state = State { on: true, ..State::default() };
        state.screens.insert("main".into(), Entry { output: Some("/d/out/main-3.jpg".into()), counter: 3, belongs: Some(Belongs { key: 1, stamp: 2, answer: false }), drawn: Some("f".into()), ..Entry::new(Path::new("/p/mine.jpg")) });
        let mut text = serde_json::to_value(&state).unwrap();
        text["screens"]["main"][field] = odd;
        parse(Some(serde_json::to_vec(&text).unwrap().as_slice()))
    }

    fn kept(state: &State) -> bool {
        let entry = &state.screens["main"];
        state.on && entry.original == Path::new("/p/mine.jpg") && entry.output.as_deref() == Some(Path::new("/d/out/main-3.jpg")) && entry.counter == 3
    }

    #[test]
    fn a_belonging_answer_with_a_missing_field_keeps_every_entry() {
        let state = read_with("belongs", serde_json::json!({"key": 1, "stamp": 2}));
        assert!(kept(&state));
        assert_eq!(state.screens["main"].belongs, None);
    }

    #[test]
    fn a_belonging_answer_that_is_text_keeps_every_entry() {
        let state = read_with("belongs", serde_json::json!("x"));
        assert!(kept(&state));
        assert_eq!(state.screens["main"].belongs, None);
    }

    #[test]
    fn a_drawn_mark_that_is_a_number_keeps_every_entry() {
        let state = read_with("drawn", serde_json::json!(7));
        assert!(kept(&state));
        assert_eq!(state.screens["main"].drawn, None);
        assert_eq!(state.screens["main"].belongs.map(|known| known.stamp), Some(2));
    }

    fn written(name: &str, edit: impl FnOnce(&mut serde_json::Value)) -> (Store, Vec<u8>) {
        let store = Store::at(scratch(name));
        let mut state = State { on: true, ..State::default() };
        state.screens.insert("main".into(), Entry { output: Some("/d/out/main-3.jpg".into()), counter: 3, ..Entry::new(Path::new("/p/mine.jpg")) });
        state.screens.insert("side".into(), Entry { output: Some("/d/out/side-5.jpg".into()), counter: 5, ..Entry::new(Path::new("/p/yours.jpg")) });
        store.save(&state).unwrap();
        let mut value: serde_json::Value = serde_json::from_slice(&fs::read(store.state_file()).unwrap()).unwrap();
        edit(&mut value);
        let bytes = serde_json::to_vec_pretty(&value).unwrap();
        fs::write(store.state_file(), &bytes).unwrap();
        (store, bytes)
    }

    #[test]
    fn an_entry_that_does_not_read_keeps_its_original_and_the_others_whole() {
        let (store, _) = written("salvage-entry", |value| value["screens"]["main"]["counter"] = serde_json::json!("x"));
        let state = store.load();
        assert!(state.on);
        assert_eq!(state.screens["main"].original, Path::new("/p/mine.jpg"));
        let side = &state.screens["side"];
        assert_eq!((side.original.as_path(), side.output.as_deref(), side.counter), (Path::new("/p/yours.jpg"), Some(Path::new("/d/out/side-5.jpg")), 5));
    }

    #[test]
    fn a_top_level_field_that_does_not_read_keeps_every_entry_whole() {
        let (store, _) = written("salvage-tally", |value| value["tally"] = serde_json::json!("x"));
        let state = store.load();
        assert!(state.on);
        assert_eq!((state.screens["main"].output.as_deref(), state.screens["main"].counter), (Some(Path::new("/d/out/main-3.jpg")), 3));
        assert_eq!((state.screens["side"].output.as_deref(), state.screens["side"].counter), (Some(Path::new("/d/out/side-5.jpg")), 5));
    }

    #[test]
    fn a_file_that_is_not_json_loads_fresh_and_goes_aside() {
        let store = Store::at(scratch("salvage-none"));
        fs::create_dir_all(&store.root).unwrap();
        fs::write(store.state_file(), b"not json at all").unwrap();
        let state = store.load();
        assert!(!state.on && state.screens.is_empty());
        assert_eq!(fs::read(store.aside_file()).unwrap(), b"not json at all");
        assert!(!store.kept_file().exists());
    }

    #[test]
    fn the_kept_copy_holds_the_bytes_that_were_read() {
        let (store, bytes) = written("salvage-kept", |value| value["screens"]["main"]["counter"] = serde_json::json!("x"));
        let state = store.load();
        store.save(&state).unwrap();
        assert_eq!(fs::read(store.kept_file()).unwrap(), bytes);
        assert!(store.load().screens.contains_key("main"));
    }

    #[test]
    fn a_file_from_a_newer_version_is_kept_byte_for_byte_before_the_first_save() {
        let (store, bytes) = written("newer", |value| {
            value["version"] = serde_json::json!(VERSION + 1);
            value["screens"]["main"]["later"] = serde_json::json!({"kind": "unknown here"});
        });
        let state = store.load();
        store.save(&state).unwrap();
        assert_eq!(fs::read(store.kept_file()).unwrap(), bytes);
        assert_eq!(state.screens["main"].original, Path::new("/p/mine.jpg"));
    }

    #[test]
    fn a_file_at_our_own_version_is_not_copied() {
        let (store, _) = written("own-version", |value| value["screens"]["main"]["later"] = serde_json::json!(1));
        let state = store.load();
        store.save(&state).unwrap();
        assert!(!store.kept_file().exists());
    }

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
    fn the_data_folder_is_lower_case_only_on_linux() {
        assert_eq!(folder_name(true), "scooch");
        assert_eq!(folder_name(false), "Scooch");
    }

    #[test]
    fn a_broken_file_is_moved_aside_and_never_overwritten() {
        let store = Store::at(scratch("broken"));
        fs::write(store.state_file(), b"{ not json").unwrap();
        assert!(!store.load().on);
        assert_eq!(fs::read(store.aside_file()).unwrap(), b"{ not json");
        assert!(!store.state_file().exists());
        store.save(&State { on: true, ..State::default() }).unwrap();
        assert_eq!(fs::read(store.aside_file()).unwrap(), b"{ not json");
        let line = store.aside_line().unwrap();
        assert!(line.contains("state.unreadable.json") && line.ends_with('.') && !line.contains(": "), "{line}");
        assert!(Store::at(scratch("fine")).aside_line().is_none());
    }

    #[test]
    fn writes_from_many_threads_never_share_a_temporary_file() {
        let folder = scratch("many");
        let target = folder.join("state.json");
        let writers: Vec<_> = (0..16).map(|index| {
            let target = target.clone();
            std::thread::spawn(move || (0..20).all(|round| write_atomic(&target, format!("{index}-{round}").as_bytes()).is_ok()))
        }).collect();
        assert!(writers.into_iter().all(|writer| writer.join().unwrap()));
        let names: Vec<_> = fs::read_dir(&folder).unwrap().flatten().map(|entry| entry.file_name()).collect();
        assert_eq!(names, vec![std::ffi::OsString::from("state.json")]);
    }

    #[test]
    fn older_files_with_missing_fields_still_load() {
        let store = Store::at(scratch("older"));
        fs::write(store.state_file(), br#"{"on":true,"screens":{"main":{"source":"/p/a.jpg","counter":3}}}"#).unwrap();
        let state = store.load();
        assert!(state.on);
        assert_eq!(state.screens["main"].counter, 3);
        assert_eq!(state.options, fresh_options());
    }

    #[test]
    fn a_file_without_the_glide_setting_loads_with_glide_on() {
        let store = Store::at(scratch("glide-old"));
        fs::write(store.state_file(), br#"{"on":true}"#).unwrap();
        assert!(store.load().glide);
        let state = State { on: true, glide: false, ..State::default() };
        store.save(&state).unwrap();
        assert_eq!(store.load(), state);
    }

    #[test]
    fn a_file_without_the_scenery_setting_loads_with_scenery_on() {
        let store = Store::at(scratch("scenery-old"));
        fs::write(store.state_file(), br#"{"on":true,"options":{"strength":1.0,"max_zoom":1.3,"mirror":true,"pads":0.7}}"#).unwrap();
        let state = store.load();
        assert_eq!(state.options.scenery, Scenery::Auto);
        assert!(state.options.mirror);
        assert_eq!(Store::at(scratch("scenery-none")).load().options.scenery, Scenery::Auto);
    }

    #[test]
    fn both_scenery_values_survive_a_round_trip() {
        let store = Store::at(scratch("scenery-trip"));
        for scenery in [Scenery::Auto, Scenery::Off] {
            let state = State { on: true, options: Options { scenery, ..Options::default() }, ..State::default() };
            store.save(&state).unwrap();
            assert_eq!(store.load(), state);
        }
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
        fs::remove_file(&target).unwrap();
        fs::create_dir(&target).unwrap();
        fs::write(target.join("inside"), b"old").unwrap();
        assert!(write_atomic(&target, b"new").is_err());
        assert_eq!(fs::read(target.join("inside")).unwrap(), b"old");
        assert_eq!(fs::read_dir(&folder).unwrap().count(), 1);
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
    fn one_command_runs_at_a_time() {
        let store = Store::at(scratch("commandlock"));
        let held = store.command_lock(Duration::ZERO).unwrap();
        assert_eq!(store.command_lock(Duration::from_millis(150)).unwrap_err().to_string(), BUSY);
        let waiting = {
            let store = store.clone();
            std::thread::spawn(move || store.command_lock(Duration::from_secs(5)).map(drop))
        };
        std::thread::sleep(Duration::from_millis(300));
        drop(held);
        assert!(waiting.join().unwrap().is_ok());
        assert!(store.lock().unwrap().is_some());
    }

    #[test]
    fn only_one_watcher_holds_the_lock() {
        let store = Store::at(scratch("lock"));
        assert_eq!(store.watcher(), None);
        let held = store.lock().unwrap();
        assert!(held.is_some());
        assert!(store.lock().unwrap().is_none());
        assert_eq!(store.recorded_pid(), Some(std::process::id()));
        #[cfg(unix)]
        assert_eq!(store.watcher(), Some(std::process::id()));
        drop(held);
        assert_eq!(store.recorded_pid(), None);
        assert_eq!(store.watcher(), None);
        assert!(store.lock().unwrap().is_some());
    }

    #[test]
    fn a_stale_pid_file_is_not_a_watcher() {
        let store = Store::at(scratch("stale"));
        fs::write(store.pid_file(), b"1").unwrap();
        assert_eq!(store.watcher(), None);
        fs::write(store.pid_file(), b"not a number").unwrap();
        assert_eq!(store.watcher(), None);
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
