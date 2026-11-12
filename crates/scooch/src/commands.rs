use crate::daemon;
use crate::platform::{self, Desktop, Screen};
use crate::state::{same, Entry, State, Store};
use crate::{poke, scene};
use anyhow::{anyhow, bail, Context, Result};
use scooch_core::{Options, Rect, RgbImage, Scene};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

const PATIENCE: Duration = Duration::from_secs(60);
const HINT_AFTER: Duration = Duration::from_secs(4);
const START_PATIENCE: Duration = Duration::from_secs(8);
const POLL: Duration = Duration::from_millis(100);
const STOP_PATIENCE: Duration = Duration::from_secs(3);
const PICTURE_KINDS: [&str; 10] = ["jpg", "jpeg", "png", "heic", "heif", "webp", "tif", "tiff", "bmp", "avif"];
pub const NO_TERMINAL: &str = "To switch the picture, run scooch use followed by the path of an image.";

#[derive(serde::Deserialize)]
struct SceneFile {
    width: u32,
    height: u32,
    icons: Vec<[f32; 4]>,
}

fn desktop() -> Result<Box<dyn Desktop>> {
    platform::current()
}

pub fn start() -> Result<()> {
    let store = Store::open();
    let mut state = store.load();
    if state.on && store.watcher().is_some() {
        return status();
    }
    let desktop = desktop()?;
    if !state.on {
        record(&mut state, desktop.as_ref(), &store)?;
        state.on = true;
    }
    state.problem = None;
    store.save(&state)?;
    begin(desktop.as_ref(), &store)?;
    wait(&store);
    print_status(desktop.as_ref(), &store);
    Ok(())
}

pub fn status() -> Result<()> {
    let store = Store::open();
    match desktop() {
        Ok(desktop) => print_status(desktop.as_ref(), &store),
        Err(_) => print(&describe(&store.load(), store.watcher().is_some(), None)),
    }
    Ok(())
}

fn print_status(desktop: &dyn Desktop, store: &Store) {
    let connected = desktop.screens().ok().map(|screens| screens.into_iter().map(|screen| screen.id).collect::<Vec<_>>());
    print(&describe(&store.load(), store.watcher().is_some(), connected.as_deref()));
}

fn print(lines: &[String]) {
    for line in lines {
        println!("{line}");
    }
}

pub fn describe(state: &State, running: bool, connected: Option<&[String]>) -> Vec<String> {
    if !state.on {
        return vec!["Scooch is off. Run scooch to turn it on.".into()];
    }
    let mut lines = vec![if running { "Scooch is on.".to_string() } else { "Scooch is on but not running. Run scooch to start it again.".to_string() }];
    let shown: Vec<&Entry> = state.screens.iter().filter(|(id, _)| connected.is_none_or(|ids| ids.contains(id))).map(|(_, entry)| entry).collect();
    let mut pictures: Vec<&Path> = Vec::new();
    for entry in &shown {
        if !pictures.contains(&entry.source.as_path()) {
            pictures.push(&entry.source);
        }
    }
    for picture in pictures {
        lines.push(format!("Picture  {}", tilde(picture)));
    }
    let icons: usize = shown.iter().map(|entry| entry.icons).sum();
    lines.push(format!("Icons    {icons} on {}", plural(shown.len(), "screen")));
    if let Some(problem) = &state.problem {
        lines.push(problem.clone());
    }
    lines
}

pub fn plural(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

pub fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) if !rest.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

pub fn switch(image: Option<PathBuf>) -> Result<()> {
    let Some(image) = image.map_or_else(choose, |image| Ok(Some(image)))? else { return Ok(()) };
    let image = picture_file(&image)?;
    let desktop = desktop()?;
    desktop.decode(&image).map_err(|_| anyhow!("Scooch cannot read {} as a picture, so pick a JPEG, PNG or HEIC file.", tilde(&image)))?;
    let store = Store::open();
    let mut state = store.load();
    if !state.on {
        record(&mut state, desktop.as_ref(), &store).ok();
    }
    let screens = desktop.screens()?;
    for screen in &screens {
        let entry = state.screens.entry(screen.id.clone()).or_insert_with(|| Entry::new(&image));
        (entry.source, entry.placement, entry.fingerprint, entry.chosen) = (image.clone(), None, None, true);
    }
    state.on = true;
    state.problem = None;
    store.save(&state)?;
    begin(desktop.as_ref(), &store)?;
    wait(&store);
    print_status(desktop.as_ref(), &store);
    Ok(())
}

fn picture_file(image: &Path) -> Result<PathBuf> {
    let image = expand(image);
    let found = image.canonicalize().map_err(|_| anyhow!("{} was not found, so check the path and try again.", image.display()))?;
    if !found.is_file() {
        bail!("{} is a folder, so pass the path of a picture inside it.", tilde(&found));
    }
    Ok(found)
}

fn expand(path: &Path) -> PathBuf {
    match (path.strip_prefix("~"), dirs::home_dir()) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => path.to_path_buf(),
    }
}

fn choose() -> Result<Option<PathBuf>> {
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        println!("{NO_TERMINAL}");
        std::process::exit(2);
    }
    let mut folders: Vec<PathBuf> = [dirs::picture_dir(), dirs::download_dir(), dirs::desktop_dir()].into_iter().flatten().collect();
    folders.extend(std::env::current_dir().ok());
    let found = pictures(&folders);
    if found.is_empty() {
        bail!("No pictures were found in Pictures, Downloads, Desktop or this folder, so run scooch use with the path of an image.");
    }
    let labels: Vec<String> = found.iter().map(|path| tilde(path)).collect();
    let picked = dialoguer::FuzzySelect::new().with_prompt("Pick a picture").items(&labels).default(0).max_length(12).interact_opt()?;
    Ok(picked.map(|index| found[index].clone()))
}

pub fn pictures(folders: &[PathBuf]) -> Vec<PathBuf> {
    let mut found: Vec<(SystemTime, PathBuf)> = Vec::new();
    for folder in folders {
        let Ok(entries) = std::fs::read_dir(folder) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            let hidden = entry.file_name().to_string_lossy().starts_with('.');
            if hidden || !is_picture(&path) || found.iter().any(|(_, seen)| same(seen, &path)) {
                continue;
            }
            if let Some(changed) = entry.metadata().ok().filter(|meta| meta.is_file()).and_then(|meta| meta.modified().ok()) {
                found.push((changed, path));
            }
        }
    }
    found.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    found.into_iter().map(|(_, path)| path).collect()
}

pub fn is_picture(path: &Path) -> bool {
    path.extension().and_then(|kind| kind.to_str()).is_some_and(|kind| PICTURE_KINDS.contains(&kind.to_ascii_lowercase().as_str()))
}

pub fn off() -> Result<()> {
    let store = Store::open();
    let desktop = desktop()?;
    desktop.autostart(None)?;
    stop(&store);
    let mut state = store.load();
    for screen in desktop.screens().unwrap_or_default() {
        let Some(entry) = state.screens.get(&screen.id) else { continue };
        restore(desktop.as_ref(), &store, &screen, entry)?;
    }
    let _ = std::fs::remove_dir_all(store.out());
    if store.state_file().exists() {
        state.on = false;
        state.problem = None;
        for entry in state.screens.values_mut() {
            (entry.output, entry.fingerprint, entry.placement) = (None, None, None);
        }
        store.save(&state)?;
    }
    println!("Scooch is off. Your wallpaper is back.");
    Ok(())
}

fn restore(desktop: &dyn Desktop, store: &Store, screen: &Screen, entry: &Entry) -> Result<()> {
    let Some(current) = desktop.wallpaper(screen)? else { return Ok(()) };
    let ours = store.owns(&current) || same(&current, &entry.source);
    if ours && !same(&current, &entry.original) && entry.original.is_file() {
        desktop.set_wallpaper(screen, &entry.original)?;
    }
    Ok(())
}

fn stop(store: &Store) {
    let Some(process) = store.watcher() else { return };
    poke::stop(process);
    let started = Instant::now();
    while store.watcher().is_some() && started.elapsed() < STOP_PATIENCE {
        std::thread::sleep(POLL);
    }
}

pub fn watch() -> Result<()> {
    let store = Store::open();
    if !store.load().on {
        return Ok(());
    }
    let Some(_lock) = store.lock()? else {
        println!("Scooch is already running.");
        return Ok(());
    };
    let desktop: Arc<dyn Desktop> = Arc::from(desktop()?);
    daemon::watch(desktop, store)
}

pub fn doctor() -> Result<()> {
    let desktop = desktop()?;
    println!("{}", desktop.name());
    for (name, result) in desktop.check() {
        match result {
            Ok(detail) => println!("ok    {name:<11} {detail}"),
            Err(error) => println!("fail  {name:<11} {error}"),
        }
    }
    let store = Store::open();
    let state = store.load();
    let running = if store.watcher().is_some() { "running" } else { "not running" };
    println!("ok    {:<11} {running}", "Watcher");
    println!("ok    {:<11} {}", "Data", tilde(&store.root));
    if let Some(problem) = state.problem {
        println!("fail  {:<11} {problem}", "Last tick");
    }
    Ok(())
}

pub fn render(image: Option<PathBuf>, icons: Option<PathBuf>, out: Option<PathBuf>, json: Option<PathBuf>, saliency: Option<PathBuf>) -> Result<()> {
    let Some(image) = image.map_or_else(choose, |image| Ok(Some(image)))? else { return Ok(()) };
    let image = picture_file(&image)?;
    let desktop = desktop().ok();
    let scene = match icons {
        Some(file) => read_scene(&file)?,
        None => live_scene(desktop.as_deref())?,
    };
    let options = Options::default();
    let source = open(&image, desktop.as_deref())?;
    let analysis = scooch_core::analyze(&source);
    let placement = scooch_core::solve(&analysis, &scene, &options, None);
    let picture = scooch_core::render(&source, &placement, &scene, &options);
    drop(source);
    let named = out.is_some();
    let out = out.unwrap_or_else(|| default_out(&image));
    let bytes = if out.extension().is_some_and(|kind| kind.eq_ignore_ascii_case("png")) { scooch_core::encode_png(&picture)? } else { scooch_core::encode_jpeg(&picture, daemon::QUALITY)? };
    std::fs::write(&out, bytes).with_context(|| format!("Scooch could not write {}, so check the folder.", out.display()))?;
    if let Some(path) = json {
        let body = serde_json::to_vec(&serde_json::json!({ "scene": scene, "placement": placement, "analysis": analysis }))?;
        std::fs::write(&path, body).with_context(|| format!("Scooch could not write {}, so check the folder.", path.display()))?;
    }
    if let Some(path) = saliency {
        let map = gray(&scooch_core::saliency_image(&analysis));
        std::fs::write(&path, scooch_core::encode_png(&map)?).with_context(|| format!("Scooch could not write {}, so check the folder.", path.display()))?;
    }
    if !named {
        println!("Wrote {}", tilde(&out.canonicalize().unwrap_or(out)));
    }
    Ok(())
}

fn gray(map: &scooch_core::GrayImage) -> RgbImage {
    let bytes = map.as_raw().iter().flat_map(|value| [*value; 3]).collect();
    RgbImage::from_raw(map.width(), map.height(), bytes).unwrap_or_default()
}

fn open(image: &Path, desktop: Option<&dyn Desktop>) -> Result<RgbImage> {
    let opened = match desktop {
        Some(desktop) => desktop.decode(image),
        None => scooch_core::open(image).map_err(anyhow::Error::from),
    };
    opened.map_err(|_| anyhow!("Scooch cannot read {} as a picture, so pick a JPEG, PNG or HEIC file.", tilde(image)))
}

fn read_scene(file: &Path) -> Result<Scene> {
    let bytes = std::fs::read(file).map_err(|_| anyhow!("{} was not found, so check the path of the icons file.", file.display()))?;
    let parsed: SceneFile = serde_json::from_slice(&bytes).map_err(|_| anyhow!("{} should hold a width, a height and a list of icons as x, y, w and h.", file.display()))?;
    Ok(Scene { width: parsed.width, height: parsed.height, icons: parsed.icons.iter().map(|r| Rect { x: r[0], y: r[1], w: r[2], h: r[3] }).collect() })
}

fn live_scene(desktop: Option<&dyn Desktop>) -> Result<Scene> {
    let desktop = desktop.context("Scooch cannot read this desktop yet, so pass the icons with --icons and a file.")?;
    let snapshot = desktop.snapshot()?;
    let screen = snapshot.screens.first().context("macOS reports no screens, so connect one and try again.")?;
    Ok(scene::scene(screen, &snapshot.icons))
}

pub fn default_out(image: &Path) -> PathBuf {
    let stem = image.file_stem().map_or("picture".into(), |stem| stem.to_string_lossy().into_owned());
    PathBuf::from(format!("{stem}-scooch.jpg"))
}

fn record(state: &mut State, desktop: &dyn Desktop, store: &Store) -> Result<()> {
    let mut recorded = 0;
    for screen in desktop.screens()? {
        let current = desktop.wallpaper(&screen)?.filter(|path| path.is_file() && !store.owns(path));
        match (current, state.screens.get_mut(&screen.id)) {
            (Some(current), Some(entry)) => {
                (entry.source, entry.original, entry.fingerprint, entry.placement) = (current.clone(), current, None, None);
                recorded += 1;
            }
            (Some(current), None) => {
                state.screens.insert(screen.id.clone(), Entry::new(&current));
                recorded += 1;
            }
            (None, Some(_)) => recorded += 1,
            (None, None) => {}
        }
    }
    if recorded == 0 {
        bail!("Scooch needs a picture file as your wallpaper, so pick one in System Settings or run scooch use with a picture.");
    }
    Ok(())
}

fn begin(desktop: &dyn Desktop, store: &Store) -> Result<()> {
    if let Some(process) = store.watcher() {
        if poke::send(process) {
            return Ok(());
        }
    }
    let exe = std::env::current_exe().context("Scooch cannot find its own program file, so reinstall it.")?;
    desktop.autostart(Some(&exe.canonicalize().unwrap_or(exe)))
}

fn wait(store: &Store) {
    let started = Instant::now();
    let mut hinted = false;
    while started.elapsed() < PATIENCE {
        let running = store.watcher().is_some();
        if settled(&store.load()) && running {
            return;
        }
        if !running && started.elapsed() > START_PATIENCE {
            return;
        }
        if !hinted && started.elapsed() >= HINT_AFTER {
            println!("If macOS asks to let scooch control Finder, choose Allow.");
            hinted = true;
        }
        std::thread::sleep(POLL);
    }
}

pub fn settled(state: &State) -> bool {
    state.problem.is_some() || state.screens.values().all(|entry| entry.fingerprint.is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn on() -> State {
        let mut state = State { on: true, ..State::default() };
        let home = dirs::home_dir().unwrap_or_default();
        state.screens.insert("main".into(), Entry { icons: 9, fingerprint: Some("f".into()), ..Entry::new(&home.join("Pictures/harbor.jpg")) });
        state
    }

    #[test]
    fn on_reads_exactly_like_the_spec() {
        assert_eq!(describe(&on(), true, None), vec!["Scooch is on.", "Picture  ~/Pictures/harbor.jpg", "Icons    9 on 1 screen"]);
    }

    #[test]
    fn off_points_the_way_back() {
        assert_eq!(describe(&State::default(), false, None), vec!["Scooch is off. Run scooch to turn it on."]);
    }

    #[test]
    fn two_screens_add_up() {
        let mut state = on();
        state.screens.insert("side".into(), Entry { icons: 3, ..state.screens["main"].clone() });
        assert_eq!(describe(&state, true, None)[2], "Icons    12 on 2 screens");
        let only = vec!["side".to_string()];
        assert_eq!(describe(&state, true, Some(&only))[2], "Icons    3 on 1 screen");
    }

    #[test]
    fn a_stopped_watcher_and_a_problem_are_called_out() {
        let mut state = on();
        state.problem = Some(crate::daemon::NOT_A_FILE.into());
        let lines = describe(&state, false, None);
        assert_eq!(lines[0], "Scooch is on but not running. Run scooch to start it again.");
        assert_eq!(lines.last().unwrap(), crate::daemon::NOT_A_FILE);
    }

    #[test]
    fn output_stays_free_of_fussy_punctuation() {
        let mut state = on();
        state.problem = Some(crate::daemon::NOT_A_FILE.into());
        let lines = [describe(&state, true, None), describe(&state, false, None), describe(&State::default(), false, None), vec![NO_TERMINAL.to_string()]].concat();
        for line in lines {
            assert!(!line.contains(['—', '–', ';', '(', ')']), "{line}");
            assert!(!line.contains(": "), "{line}");
            assert!(!line.contains(" - "), "{line}");
        }
    }

    #[test]
    fn pictures_are_found_newest_first_without_hidden_files() {
        let folder = crate::state::scratch("pictures");
        let other = crate::state::scratch("pictures-other");
        for (path, age) in [(folder.join("old.jpg"), 300), (folder.join("new.PNG"), 10), (other.join("mid.heic"), 100), (folder.join(".hidden.jpg"), 1), (folder.join("notes.txt"), 1)] {
            std::fs::write(&path, b"x").unwrap();
            let when = SystemTime::now() - Duration::from_secs(age);
            std::fs::File::options().write(true).open(&path).unwrap().set_modified(when).unwrap();
        }
        std::fs::create_dir(folder.join("album.jpg")).unwrap();
        let found = pictures(&[folder.clone(), other.clone(), folder.clone()]);
        assert_eq!(found, vec![folder.join("new.PNG"), other.join("mid.heic"), folder.join("old.jpg")]);
    }

    #[test]
    fn tilde_shortens_the_home_folder() {
        let home = dirs::home_dir().unwrap();
        assert_eq!(tilde(&home.join("Pictures/harbor.jpg")), "~/Pictures/harbor.jpg");
        assert_eq!(tilde(Path::new("/System/Library/CoreServices/DefaultDesktop.heic")), "/System/Library/CoreServices/DefaultDesktop.heic");
    }

    #[test]
    fn waiting_ends_on_a_render_or_a_problem() {
        let mut state = on();
        state.screens.get_mut("main").unwrap().fingerprint = None;
        assert!(!settled(&state));
        state.problem = Some("x".into());
        assert!(settled(&state));
        state.problem = None;
        state.screens.get_mut("main").unwrap().fingerprint = Some("f".into());
        assert!(settled(&state));
    }

    #[test]
    fn render_names_its_output_after_the_picture() {
        assert_eq!(default_out(Path::new("/p/lake.heic")), PathBuf::from("lake-scooch.jpg"));
    }

    #[test]
    fn a_saliency_map_becomes_a_gray_picture() {
        let map = scooch_core::GrayImage::from_raw(2, 1, vec![0, 200]).unwrap();
        assert_eq!(gray(&map).into_raw(), vec![0, 0, 0, 200, 200, 200]);
    }
}
