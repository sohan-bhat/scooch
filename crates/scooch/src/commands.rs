use crate::daemon;
use crate::platform::{self, Desktop, Screen};
use crate::state::{self, same, Entry, State, Store};
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
pub const FLIP_HINT: &str = "Icons still cover part of the picture. Run scooch flip to let Scooch flip it.";
pub const FLIP_ON: &str = "Scooch may now flip your picture when that clears your icons.";
pub const FLIP_OFF: &str = "Scooch now keeps your picture the way round it is.";
pub const NO_SCREEN: &str = "No screen was found, so connect one and try again.";
pub const NO_PICTURE: &str = "Scooch needs a picture file as your wallpaper, so pick one in your system settings or run scooch use with a picture.";
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

pub fn outdated(state: &State, running: bool, version: &str) -> bool {
    running && state.daemon.as_deref() != Some(version)
}

fn replace_outdated(store: &Store) -> Result<()> {
    if !outdated(&store.load(), store.watcher().is_some(), state::PROGRAM) {
        return Ok(());
    }
    stop(store);
    store.mark(None)?;
    println!("Scooch was updated to {}.", state::PROGRAM);
    Ok(())
}

pub fn start() -> Result<()> {
    let store = Store::open();
    replace_outdated(&store)?;
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
    wait(&store, desktop.consent_hint());
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
    if !state.options.mirror && shown.iter().any(|entry| entry.flip_helps) {
        lines.push(FLIP_HINT.into());
    }
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
    replace_outdated(&store)?;
    let mut state = store.load();
    choose_picture(&mut state, desktop.as_ref(), &store, &image)?;
    state.on = true;
    state.problem = None;
    store.save(&state)?;
    begin(desktop.as_ref(), &store)?;
    wait(&store, desktop.consent_hint());
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
    store.forget_pictures();
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
    store.mark(Some(state::PROGRAM))?;
    let desktop: Arc<dyn Desktop> = Arc::from(desktop()?);
    let result = daemon::watch(desktop, store.clone());
    store.mark(None)?;
    result
}

pub fn flip() -> Result<()> {
    let store = Store::open();
    let mut state = store.load();
    state.options.mirror = !state.options.mirror;
    store.save(&state)?;
    if let Some(process) = store.watcher() {
        poke::send(process);
    }
    println!("{}", flip_line(state.options.mirror));
    Ok(())
}

pub fn flip_line(mirror: bool) -> &'static str {
    if mirror {
        FLIP_ON
    } else {
        FLIP_OFF
    }
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

pub struct Rendered {
    pub picture: RgbImage,
    pub analysis: scooch_core::Analysis,
    pub report: serde_json::Value,
}

pub fn rendered(source: &RgbImage, scene: &Scene, fill: bool) -> Rendered {
    let options = Options::default();
    let analysis = scooch_core::analyze(source);
    let plain = scooch_core::fill(source.width(), source.height(), scene);
    let (placement, options) = if fill { (plain.clone(), Options { pads: 0.0, ..options }) } else { (scooch_core::solve(&analysis, scene, &options, None), options) };
    let picture = scooch_core::render_with(source, &analysis, &placement, scene, &options);
    let hidden = scooch_core::hidden_share(&analysis, scene, &placement);
    let hidden_plain = scooch_core::hidden_share(&analysis, scene, &plain);
    let report = serde_json::json!({ "scene": scene, "placement": placement, "analysis": analysis, "hidden": hidden, "hidden_plain": hidden_plain });
    Rendered { picture, analysis, report }
}

pub fn render(image: Option<PathBuf>, icons: Option<PathBuf>, out: Option<PathBuf>, json: Option<PathBuf>, saliency: Option<PathBuf>, fill: bool) -> Result<()> {
    let Some(image) = image.map_or_else(choose, |image| Ok(Some(image)))? else { return Ok(()) };
    let image = picture_file(&image)?;
    let desktop = desktop().ok();
    let scene = match icons {
        Some(file) => read_scene(&file)?,
        None => live_scene(desktop.as_deref())?,
    };
    let source = open(&image, desktop.as_deref())?;
    let Rendered { picture, analysis, report } = rendered(&source, &scene, fill);
    drop(source);
    let named = out.is_some();
    let out = out.unwrap_or_else(|| default_out(&image));
    let bytes = if out.extension().is_some_and(|kind| kind.eq_ignore_ascii_case("png")) { scooch_core::encode_png(&picture)? } else { scooch_core::encode_jpeg(&picture, daemon::QUALITY)? };
    std::fs::write(&out, bytes).with_context(|| format!("Scooch could not write {}, so check the folder.", out.display()))?;
    if let Some(path) = json {
        let body = serde_json::to_vec(&report)?;
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
    let screen = snapshot.screens.first().context(NO_SCREEN)?;
    Ok(scene::scene(screen, &snapshot.icons))
}

pub fn default_out(image: &Path) -> PathBuf {
    let stem = image.file_stem().map_or("picture".into(), |stem| stem.to_string_lossy().into_owned());
    PathBuf::from(format!("{stem}-scooch.jpg"))
}

pub fn recorded(desktop: &dyn Desktop, store: &Store, screen: &Screen, chosen: Option<&Path>) -> Result<Option<Entry>> {
    let current = desktop.wallpaper(screen)?.filter(|path| path.is_file() && !store.owns(path));
    let Some(source) = chosen.map(Path::to_path_buf).or(current) else { return Ok(None) };
    let mut entry = Entry::new(&source);
    match desktop.restorable(screen)?.filter(|path| !store.owns(path)) {
        Some(original) => entry.original = original,
        None => entry.unrestorable = true,
    }
    entry.chosen = chosen.is_some();
    Ok(Some(entry))
}

pub fn record(state: &mut State, desktop: &dyn Desktop, store: &Store) -> Result<()> {
    state.screens.clear();
    for screen in desktop.screens()? {
        if let Some(entry) = recorded(desktop, store, &screen, None)? {
            state.screens.insert(screen.id.clone(), entry);
        }
    }
    if state.screens.is_empty() {
        bail!(NO_PICTURE);
    }
    Ok(())
}

pub fn choose_picture(state: &mut State, desktop: &dyn Desktop, store: &Store, image: &Path) -> Result<()> {
    if !state.on {
        state.screens.clear();
    }
    for screen in desktop.screens()? {
        match state.screens.get_mut(&screen.id) {
            Some(entry) => (entry.source, entry.placement, entry.fingerprint, entry.chosen) = (image.to_path_buf(), None, None, true),
            None => {
                if let Some(entry) = recorded(desktop, store, &screen, Some(image))? {
                    state.screens.insert(screen.id.clone(), entry);
                }
            }
        }
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

fn wait(store: &Store, hint: Option<&str>) {
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
        if let (false, true, Some(hint)) = (hinted, started.elapsed() >= HINT_AFTER, hint) {
            println!("{hint}");
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
    fn shared_sentences_name_no_system() {
        for line in [NO_SCREEN, NO_PICTURE, NO_TERMINAL, crate::daemon::NOT_A_FILE] {
            for word in ["macOS", "Finder", "System Settings", "Windows", "GNOME", "login item"] {
                assert!(!line.contains(word), "{line}");
            }
        }
    }

    #[test]
    fn only_a_running_watcher_of_another_version_is_replaced() {
        let mut state = on();
        assert!(!outdated(&state, false, "0.2.0"));
        assert!(outdated(&state, true, "0.2.0"));
        state.daemon = Some("0.1.0".into());
        assert!(outdated(&state, true, "0.2.0"));
        assert!(!outdated(&state, false, "0.2.0"));
        state.daemon = Some("0.2.0".into());
        assert!(!outdated(&state, true, "0.2.0"));
    }

    #[test]
    fn the_running_version_is_marked_and_cleared() {
        let store = Store::at(crate::state::scratch("mark"));
        store.save(&on()).unwrap();
        store.mark(Some("9.9.9")).unwrap();
        assert_eq!(store.load().daemon.as_deref(), Some("9.9.9"));
        assert!(store.load().on);
        store.mark(None).unwrap();
        assert_eq!(store.load().daemon, None);
        assert!(!String::from_utf8(store.read().unwrap()).unwrap().contains("daemon"));
    }

    fn subject_scene() -> (RgbImage, Scene) {
        let source = crate::scene::sample(480, 100);
        (source, Scene { width: 160, height: 100, icons: vec![Rect { x: 60.0, y: 30.0, w: 40.0, h: 40.0 }] })
    }

    #[test]
    fn the_render_report_says_how_much_stays_hidden() {
        let (source, scene) = subject_scene();
        let made = rendered(&source, &scene, false);
        let hidden = made.report["hidden"].as_f64().expect("hidden is written");
        let plain = made.report["hidden_plain"].as_f64().expect("hidden_plain is written");
        assert!(hidden <= plain, "{hidden} {plain}");
        assert!(plain > 0.0);
        for key in ["scene", "placement", "analysis"] {
            assert!(made.report.get(key).is_some(), "{key}");
        }
    }

    #[test]
    fn fill_renders_the_plain_desktop() {
        let (source, scene) = subject_scene();
        let filled = rendered(&source, &scene, true);
        let empty = rendered(&source, &Scene { icons: Vec::new(), ..scene.clone() }, false);
        assert_eq!(filled.picture.as_raw(), empty.picture.as_raw());
        assert_eq!(scooch_core::encode_jpeg(&filled.picture, 95).unwrap(), scooch_core::encode_jpeg(&empty.picture, 95).unwrap());
        assert_eq!(filled.report["hidden"], filled.report["hidden_plain"]);
        assert!(filled.report["hidden"].as_f64().unwrap() > 0.0);
    }

    #[test]
    fn status_suggests_flip_only_when_it_would_help() {
        let mut state = on();
        assert_eq!(describe(&state, true, None).len(), 3);
        state.screens.get_mut("main").unwrap().flip_helps = true;
        let lines = describe(&state, true, None);
        assert_eq!(lines, vec!["Scooch is on.", "Picture  ~/Pictures/harbor.jpg", "Icons    9 on 1 screen", FLIP_HINT]);
        let none: Vec<String> = Vec::new();
        assert!(!describe(&state, true, Some(&none)).contains(&FLIP_HINT.to_string()));
        state.options.mirror = true;
        assert_eq!(describe(&state, true, None).len(), 3);
    }

    #[test]
    fn flip_toggles_the_mirror_and_says_so() {
        assert_eq!(flip_line(true), "Scooch may now flip your picture when that clears your icons.");
        assert_eq!(flip_line(false), "Scooch now keeps your picture the way round it is.");
        for line in [FLIP_ON, FLIP_OFF, FLIP_HINT] {
            assert!(!line.contains(['—', '–', ';', '(', ')']) && !line.contains(": "), "{line}");
        }
    }

    fn desk(name: &str) -> (std::sync::Arc<crate::daemon::tests::Fake>, Store, PathBuf, PathBuf) {
        let folder = crate::state::scratch(name);
        let default = crate::daemon::tests::picture(&folder, "default.heic");
        let chosen = crate::daemon::tests::picture(&folder, "chosen.png");
        let fake = crate::daemon::tests::Fake::new(vec![crate::daemon::tests::screen("main", 0.0, 1.0)], &default);
        *fake.moving.lock().unwrap() = Some(default.clone());
        (fake, Store::at(folder.join("data")), default, chosen)
    }

    #[test]
    fn a_moving_default_is_recorded_as_the_original() {
        let (fake, store, default, chosen) = desk("moving");
        let mut state = State::default();
        assert!(record(&mut state, fake.as_ref(), &store).is_err());
        choose_picture(&mut state, fake.as_ref(), &store, &chosen).unwrap();
        let entry = &state.screens["main"];
        assert_eq!((entry.source.clone(), entry.original.clone(), entry.unrestorable, entry.chosen), (chosen.clone(), default, false, true));
    }

    #[test]
    fn a_wallpaper_no_app_can_set_marks_the_screen() {
        let (fake, store, default, chosen) = desk("lost");
        fake.lost.lock().unwrap().push(default);
        let mut state = State::default();
        choose_picture(&mut state, fake.as_ref(), &store, &chosen).unwrap();
        let entry = &state.screens["main"];
        assert_eq!((entry.original.clone(), entry.unrestorable), (chosen.clone(), true));
        let text = serde_json::to_string(&state).unwrap();
        assert!(text.contains("\"unrestorable\":true"));
    }

    #[test]
    fn a_second_use_keeps_the_first_original() {
        let (fake, store, default, chosen) = desk("second");
        let mut state = State::default();
        choose_picture(&mut state, fake.as_ref(), &store, &chosen).unwrap();
        state.on = true;
        std::fs::create_dir_all(store.out()).unwrap();
        let output = store.output("main", 1, "jpg");
        std::fs::write(&output, b"x").unwrap();
        fake.show("main", &output);
        let later = crate::daemon::tests::picture(chosen.parent().unwrap(), "later.png");
        choose_picture(&mut state, fake.as_ref(), &store, &later).unwrap();
        let entry = &state.screens["main"];
        assert_eq!((entry.source.clone(), entry.original.clone()), (later, default));
    }

    #[test]
    fn a_plain_picture_is_its_own_original() {
        let (fake, store, _, chosen) = desk("plain");
        fake.show("main", &chosen);
        let mut state = State::default();
        record(&mut state, fake.as_ref(), &store).unwrap();
        let entry = &state.screens["main"];
        assert_eq!((entry.source.clone(), entry.original.clone(), entry.unrestorable, entry.chosen), (chosen.clone(), chosen, false, false));
        assert!(!serde_json::to_string(&state).unwrap().contains("unrestorable"));
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
