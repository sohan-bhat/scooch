use crate::daemon;
use crate::platform::{self, Desktop, Screen};
use crate::state::{self, same, Entry, State, Store, Tally};
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
pub const STOP_PATIENCE: Duration = daemon::STOP_CAP.saturating_add(Duration::from_secs(2));
pub const UNREADABLE_OFF: &str = "Scooch is off. It could not read its record of your wallpapers, so pick them again in your system settings.";
pub const STOP_FAILED: &str = "Scooch could not stop its background program, so run scooch off again in a moment.";
pub const NO_RESTORER: &str = "Nothing is running to switch them back, so run scooch off again to give up on them or scooch to turn Scooch back on.";
const PICTURE_KINDS: [&str; 10] = ["jpg", "jpeg", "png", "heic", "heif", "webp", "tif", "tiff", "bmp", "avif"];
pub const FLIP_HINT: &str = "Icons still cover part of the picture. Run scooch flip to let Scooch flip it.";
pub const FLIP_ON: &str = "Scooch may now flip your picture when that clears your icons.";
pub const FLIP_OFF: &str = "Scooch now keeps your picture the way round it is.";
pub const GUIDE: &str = "Your wallpaper is not a picture file, so pick a picture for Scooch to move.";
pub const NO_SCREEN: &str = "No screen was found, so connect one and try again.";
pub const NO_PICTURE: &str = "Scooch needs a picture file as your wallpaper, so pick one in your system settings or run scooch use with a picture.";
pub const NO_TERMINAL: &str = "To switch the picture, run scooch use followed by the path of an image.";
pub const WAITING: &str = "Scooch is on. It starts when a desktop shows.";

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
    stop(store)?;
    store.mark(None)?;
    println!("Scooch was updated to {}.", state::PROGRAM);
    Ok(())
}

pub fn start() -> Result<()> {
    let store = Store::open();
    store.load();
    if let Some(line) = store.aside_line() {
        println!("{line}");
    }
    replace_outdated(&store)?;
    let mut state = store.load();
    if state.on && store.watcher().is_some() {
        return status();
    }
    let desktop = desktop()?;
    match turn_on(&mut state, desktop.as_ref(), &store, terminal())? {
        FirstRun::Go => {}
        FirstRun::Fail => bail!(NO_PICTURE),
        FirstRun::Pick => {
            println!("{GUIDE}");
            let Some(pick) = choose(&desktop.gallery())? else { return Ok(()) };
            return switch(Some(pick));
        }
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
    store.load();
    if let Some(line) = store.aside_line() {
        println!("{line}");
    }
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
    if state.restoring && !state.on {
        let mut lines = vec![restoring_line(state.screens.len())];
        lines.extend(tally_lines(&state.tally));
        if !running {
            lines.push(NO_RESTORER.into());
        }
        return lines;
    }
    if !state.on {
        let mut lines = vec!["Scooch is off. Run scooch to turn it on.".to_string()];
        lines.extend(tally_lines(&state.tally));
        return lines;
    }
    if running && connected.is_some_and(<[String]>::is_empty) {
        return vec![WAITING.to_string()];
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
    let desktop = desktop()?;
    let Some(image) = image.map_or_else(|| choose(&desktop.gallery()), |image| Ok(Some(image)))? else { return Ok(()) };
    let image = picture_file(&image)?;
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
    let found = state::canonical(&image).map_err(|_| anyhow!("{} was not found, so check the path and try again.", image.display()))?;
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

fn terminal() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

fn choose(gallery: &[PathBuf]) -> Result<Option<PathBuf>> {
    if !terminal() {
        println!("{NO_TERMINAL}");
        std::process::exit(2);
    }
    let mut folders: Vec<PathBuf> = [dirs::picture_dir(), dirs::download_dir(), dirs::desktop_dir()].into_iter().flatten().collect();
    folders.extend(std::env::current_dir().ok());
    let found = listing(&folders, gallery);
    if found.is_empty() {
        bail!("No pictures were found in Pictures, Downloads, Desktop or this folder, so run scooch use with the path of an image.");
    }
    let labels: Vec<String> = found.iter().map(|path| tilde(path)).collect();
    let picked = dialoguer::FuzzySelect::new().with_prompt("Pick a picture").items(&labels).default(0).max_length(12).interact_opt()?;
    Ok(picked.map(|index| found[index].clone()))
}

pub fn listing(own: &[PathBuf], gallery: &[PathBuf]) -> Vec<PathBuf> {
    let mut found = pictures(own);
    let mut extra = pictures(gallery);
    extra.sort_by_cached_key(|path| (path.file_name().map(|name| name.to_string_lossy().to_lowercase()), path.clone()));
    for path in extra {
        if !found.iter().any(|seen| same(seen, &path)) {
            found.push(path);
        }
    }
    found
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
    desktop.screens()?;
    stop(&store)?;
    let off = wind_down(desktop.as_ref(), &store)?;
    match off {
        Off::Unreadable => {
            desktop.autostart(None)?;
            if let Some(line) = store.aside_line() {
                println!("{line}");
            }
            println!("{UNREADABLE_OFF}");
        }
        Off::Done(tally) => {
            desktop.autostart(None)?;
            println!("{}", closing(&tally));
        }
        Off::Pending(tally) => {
            begin(desktop.as_ref(), &store)?;
            println!("{}", closing(&tally));
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Off {
    Unreadable,
    Done(Tally),
    Pending(Tally),
}

pub fn wind_down(desktop: &dyn Desktop, store: &Store) -> Result<Off> {
    let screens = desktop.screens()?;
    let known = desktop.known().ok().flatten();
    let mut state = store.load();
    if state.screens.is_empty() && store.aside_file().is_file() {
        state.on = false;
        state.restoring = false;
        store.save(&state)?;
        return Ok(Off::Unreadable);
    }
    let was_restoring = state.restoring;
    let mut tally = daemon::settle_screens(&mut state, desktop, store, &screens, known.as_deref(), true).tally;
    let left = state.screens.len();
    state.on = false;
    state.problem = None;
    let off = if left > 0 && !was_restoring {
        state.restoring = true;
        state.tally = Tally { pending: 0, ..tally };
        tally.pending = left;
        let _ = std::fs::remove_dir_all(store.cache());
        Off::Pending(tally)
    } else {
        tally.pending = 0;
        tally.abandoned = left;
        let mut unreached: Vec<PathBuf> = state.screens.values().filter_map(|entry| entry.output.clone()).collect();
        unreached.extend(state.screens.keys().flat_map(|id| store.outputs_of(id)));
        for path in unreached {
            daemon::keep(&mut state, path);
        }
        state.screens.clear();
        state.restoring = false;
        state.picture = None;
        state.tally = tally;
        daemon::forget(desktop, store, &mut state);
        Off::Done(tally)
    };
    if store.state_file().exists() {
        store.save(&state)?;
    }
    Ok(off)
}

pub fn wake_up(state: &mut State) -> bool {
    if !state.restoring {
        return false;
    }
    state.restoring = false;
    state.on = true;
    state.tally = Tally::default();
    true
}

pub fn plain_sentence(count: usize) -> String {
    format!("{count} of your old wallpapers were not picture files and cannot be put back by an app, so pick them again in your system settings.")
}

pub fn stuck_sentence(count: usize) -> String {
    format!("{count} desktops still show Scooch's picture because the original file is gone, so pick a wallpaper there.")
}

pub fn closing(tally: &Tally) -> String {
    let mut parts = vec!["Scooch is off.".to_string()];
    let nothing_else = tally.plain == 0 && tally.stuck == 0 && tally.pending == 0 && tally.abandoned == 0 && tally.strays == 0;
    if tally.back > 0 || nothing_else {
        parts.push("Your wallpaper is back.".into());
    }
    if tally.plain > 0 {
        parts.push(plain_sentence(tally.plain));
    }
    if tally.stuck > 0 {
        parts.push(stuck_sentence(tally.stuck));
    }
    if tally.pending > 0 {
        parts.push(format!("{} more switch back the next time those desktops or screens show.", tally.pending));
    }
    if tally.abandoned > 0 {
        parts.push(format!("{} desktops or screens it could not reach still show Scooch's picture, so pick a wallpaper there.", tally.abandoned));
    }
    if tally.strays > 0 {
        parts.push(daemon::ORPHAN.into());
    }
    parts.join(" ")
}

pub fn restoring_line(left: usize) -> String {
    format!("Scooch is off. {left} desktops or screens switch back the next time they show.")
}

fn tally_lines(tally: &Tally) -> Vec<String> {
    let mut lines = Vec::new();
    if tally.plain > 0 {
        lines.push(plain_sentence(tally.plain));
    }
    if tally.stuck > 0 {
        lines.push(stuck_sentence(tally.stuck));
    }
    if tally.strays > 0 {
        lines.push(daemon::ORPHAN.into());
    }
    lines
}

fn stop(store: &Store) -> Result<()> {
    let Some(process) = store.watcher() else { return Ok(()) };
    poke::stop(process);
    if !wait_gone(|| poke::alive(process), STOP_PATIENCE, POLL, std::thread::sleep) {
        bail!(STOP_FAILED);
    }
    Ok(())
}

pub fn wait_gone(mut alive: impl FnMut() -> bool, patience: Duration, poll: Duration, mut pause: impl FnMut(Duration)) -> bool {
    let mut waited = Duration::ZERO;
    while alive() {
        if waited >= patience {
            return false;
        }
        pause(poll);
        waited += poll;
    }
    true
}

pub fn watch() -> Result<()> {
    let store = Store::open();
    let state = store.load();
    if !state.on && !state.restoring {
        return Ok(());
    }
    let desktop: Arc<dyn Desktop> = Arc::from(desktop()?);
    daemon::watch(desktop, store)
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
    let desktop = desktop().ok();
    let gallery = desktop.as_ref().map(|desktop| desktop.gallery()).unwrap_or_default();
    let Some(image) = image.map_or_else(|| choose(&gallery), |image| Ok(Some(image)))? else { return Ok(()) };
    let image = picture_file(&image)?;
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
        println!("Wrote {}", tilde(&state::canonical(&out).unwrap_or(out)));
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

pub fn record(state: &mut State, desktop: &dyn Desktop, store: &Store) -> Result<bool> {
    state.screens.clear();
    state.picture = None;
    state.tally = Tally::default();
    for screen in desktop.screens()? {
        if let Some(entry) = recorded(desktop, store, &screen, None)? {
            state.screens.insert(screen.id.clone(), entry);
        }
    }
    Ok(!state.screens.is_empty())
}

#[derive(Debug, PartialEq)]
pub enum FirstRun {
    Go,
    Pick,
    Fail,
}

pub fn turn_on(state: &mut State, desktop: &dyn Desktop, store: &Store, terminal: bool) -> Result<FirstRun> {
    if wake_up(state) || state.on {
        return Ok(FirstRun::Go);
    }
    let showing = !desktop.screens()?.is_empty();
    let run = first_run(record(state, desktop, store)? || !showing, terminal);
    state.on = run == FirstRun::Go;
    Ok(run)
}

pub fn first_run(recorded: bool, terminal: bool) -> FirstRun {
    match (recorded, terminal) {
        (true, _) => FirstRun::Go,
        (false, true) => FirstRun::Pick,
        (false, false) => FirstRun::Fail,
    }
}

pub fn choose_picture(state: &mut State, desktop: &dyn Desktop, store: &Store, image: &Path) -> Result<()> {
    let screens = desktop.screens()?;
    if !wake_up(state) && !state.on {
        state.screens.clear();
        state.tally = Tally::default();
    }
    for entry in state.screens.values_mut() {
        (entry.source, entry.placement, entry.fingerprint, entry.chosen) = (image.to_path_buf(), None, None, true);
    }
    for screen in &screens {
        if !state.screens.contains_key(&screen.id) {
            if let Some(entry) = recorded(desktop, store, screen, Some(image))? {
                state.screens.insert(screen.id.clone(), entry);
            }
        }
    }
    state.picture = Some(image.to_path_buf());
    Ok(())
}

fn begin(desktop: &dyn Desktop, store: &Store) -> Result<()> {
    if let Some(process) = store.watcher() {
        if poke::send(process) {
            return Ok(());
        }
    }
    let exe = std::env::current_exe().context("Scooch cannot find its own program file, so reinstall it.")?;
    desktop.autostart(Some(&state::canonical(&exe).unwrap_or(exe)))
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
    fn a_first_run_without_a_picture_guides_or_fails() {
        assert_eq!(first_run(true, false), FirstRun::Go);
        assert_eq!(first_run(true, true), FirstRun::Go);
        assert_eq!(first_run(false, true), FirstRun::Pick);
        assert_eq!(first_run(false, false), FirstRun::Fail);
        for line in [GUIDE, NO_PICTURE] {
            assert!(!line.contains(['—', '–', ';', '(', ')']) && !line.contains(": "), "{line}");
        }
    }

    #[test]
    fn a_moving_default_is_recorded_as_the_original() {
        let (fake, store, default, chosen) = desk("moving");
        let mut state = State::default();
        assert!(!record(&mut state, fake.as_ref(), &store).unwrap());
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

    fn shown_output(fake: &crate::daemon::tests::Fake, store: &Store) -> PathBuf {
        std::fs::create_dir_all(store.out()).unwrap();
        let output = store.output("main", 1, "jpg");
        std::fs::write(&output, b"x").unwrap();
        fake.show("main", &output);
        output
    }

    fn tally(back: usize, plain: usize, stuck: usize, pending: usize, abandoned: usize) -> Tally {
        Tally { back, plain, stuck, pending, abandoned, strays: 0 }
    }

    fn off_of(fake: &crate::daemon::tests::Fake, store: &Store) -> Off {
        wind_down(fake, store).unwrap()
    }

    fn turned_on(fake: &crate::daemon::tests::Fake, store: &Store, chosen: &Path) {
        let mut state = State::default();
        choose_picture(&mut state, fake, store, chosen).unwrap();
        state.on = true;
        store.save(&state).unwrap();
    }

    #[test]
    fn off_brings_back_a_moving_default() {
        let (fake, store, default, chosen) = desk("offmoving");
        turned_on(&fake, &store, &chosen);
        shown_output(&fake, &store);
        assert_eq!(off_of(&fake, &store), Off::Done(tally(1, 0, 0, 0, 0)));
        assert_eq!(fake.showing("main"), default);
        assert_eq!(fake.wallpaper(&crate::daemon::tests::screen("main", 0.0, 1.0)).unwrap(), None);
        assert_eq!(closing(&tally(1, 0, 0, 0, 0)), "Scooch is off. Your wallpaper is back.");
        let state = store.load();
        assert!(!state.on && !state.restoring && state.screens.is_empty() && state.picture.is_none());
        assert!(!store.out().exists());
    }

    #[test]
    fn off_leaves_the_plain_picture_when_nothing_can_be_put_back() {
        let (fake, store, default, chosen) = desk("offlost");
        fake.lost.lock().unwrap().push(default);
        turned_on(&fake, &store, &chosen);
        shown_output(&fake, &store);
        assert_eq!(off_of(&fake, &store), Off::Done(tally(0, 1, 0, 0, 0)));
        assert_eq!(fake.showing("main"), chosen);
    }

    #[test]
    fn off_leaves_a_wallpaper_the_user_picked_since() {
        let (fake, store, _, chosen) = desk("offpicked");
        turned_on(&fake, &store, &chosen);
        let mine = crate::daemon::tests::picture(chosen.parent().unwrap(), "mine.png");
        fake.show("main", &mine);
        assert_eq!(off_of(&fake, &store), Off::Done(tally(1, 0, 0, 0, 0)));
        assert_eq!(fake.showing("main"), mine);
    }

    #[test]
    fn off_falls_back_to_the_picture_when_the_original_is_gone() {
        let (fake, store, _, chosen) = desk("offfallback");
        let gone = crate::daemon::tests::picture(chosen.parent().unwrap(), "gone.png");
        fake.show("main", &gone);
        turned_on(&fake, &store, &chosen);
        shown_output(&fake, &store);
        std::fs::remove_file(&gone).unwrap();
        assert_eq!(off_of(&fake, &store), Off::Done(tally(0, 1, 0, 0, 0)));
        assert_eq!(fake.showing("main"), chosen);
    }

    #[test]
    fn off_leaves_scoochs_picture_up_when_nothing_else_is_left() {
        let (fake, store, _, chosen) = desk("offnothing");
        let gone = crate::daemon::tests::picture(chosen.parent().unwrap(), "gone.png");
        fake.show("main", &gone);
        let lone = crate::daemon::tests::picture(chosen.parent().unwrap(), "lone.png");
        turned_on(&fake, &store, &lone);
        let output = shown_output(&fake, &store);
        std::fs::remove_file(&gone).unwrap();
        std::fs::remove_file(&lone).unwrap();
        assert_eq!(off_of(&fake, &store), Off::Done(tally(0, 0, 1, 0, 0)));
        assert!(output.exists());
        assert_eq!(fake.showing("main"), output);
        let state = store.load();
        assert!(!state.restoring && state.screens.is_empty() && state.stuck == vec![output]);
    }

    fn pair(name: &str) -> (std::sync::Arc<crate::daemon::tests::Fake>, Store, PathBuf, PathBuf) {
        let folder = crate::state::scratch(name);
        let left = crate::daemon::tests::picture(&folder, "left.png");
        let right = crate::daemon::tests::picture(&folder, "right.png");
        let fake = crate::daemon::tests::Fake::new(vec![crate::daemon::tests::screen("L", 0.0, 1.0), crate::daemon::tests::screen("R", 160.0, 1.0)], &left);
        fake.show("R", &right);
        let store = Store::at(folder.join("data"));
        let mut state = State::default();
        assert!(record(&mut state, fake.as_ref(), &store).unwrap());
        state.on = true;
        store.save(&state).unwrap();
        std::fs::create_dir_all(store.out()).unwrap();
        std::fs::create_dir_all(store.cache()).unwrap();
        for id in ["L", "R"] {
            let output = store.output(id, 1, "jpg");
            std::fs::write(&output, b"x").unwrap();
            fake.show(id, &output);
        }
        (fake, store, left, right)
    }

    #[test]
    fn off_with_a_screen_unplugged_finishes_when_it_comes_back() {
        let (fake, store, left, right) = pair("unplugged");
        fake.connect(vec![crate::daemon::tests::screen("L", 0.0, 1.0)]);
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(1, 0, 0, 1, 0)));
        assert_eq!(fake.showing("L"), left);
        let state = store.load();
        assert!(state.restoring && !state.on && state.screens.contains_key("R") && !state.screens.contains_key("L"));
        assert!(store.output("R", 1, "jpg").exists() && !store.output("L", 1, "jpg").exists() && !store.cache().exists());
        assert_eq!(describe(&state, true, None), vec!["Scooch is off. 1 desktops or screens switch back the next time they show."]);
        fake.connect(vec![crate::daemon::tests::screen("L", 0.0, 1.0), crate::daemon::tests::screen("R", 160.0, 1.0)]);
        let daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        let report = daemon.tick();
        assert!(report.unload);
        assert_eq!(fake.showing("R"), right);
        let state = store.load();
        assert!(!state.restoring && state.screens.is_empty() && !store.out().exists());
    }

    #[test]
    fn off_twice_quickly_gives_up_on_screens_it_cannot_reach() {
        let (fake, store, _, _) = pair("twice");
        fake.connect(vec![crate::daemon::tests::screen("L", 0.0, 1.0)]);
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(1, 0, 0, 1, 0)));
        assert_eq!(off_of(&fake, &store), Off::Done(tally(0, 0, 0, 0, 1)));
        let state = store.load();
        assert!(!state.restoring && state.screens.is_empty());
        assert!(store.output("R", 1, "jpg").exists() && !store.output("L", 1, "jpg").exists());
        assert_eq!(closing(&tally(0, 0, 0, 0, 1)), "Scooch is off. 1 desktops or screens it could not reach still show Scooch's picture, so pick a wallpaper there.");
    }

    #[test]
    fn turning_on_while_restoring_keeps_the_originals() {
        let (fake, store, left, right) = pair("resume");
        fake.connect(vec![crate::daemon::tests::screen("L", 0.0, 1.0)]);
        off_of(&fake, &store);
        let mut state = store.load();
        assert!(wake_up(&mut state));
        assert!(state.on && !state.restoring);
        assert_eq!(state.screens["R"].original, right);
        let mut chosen = store.load();
        choose_picture(&mut chosen, fake.as_ref(), &store, &left).unwrap();
        assert!(chosen.on && !chosen.restoring);
        assert_eq!((chosen.screens["R"].source.clone(), chosen.screens["R"].original.clone()), (left.clone(), right));
        assert_eq!(chosen.picture, Some(left));
    }

    #[test]
    fn a_space_that_no_longer_exists_is_forgotten() {
        let (fake, store, _, chosen) = desk("gonespace");
        let mut state = State::default();
        let mut gone = Entry::new(&chosen);
        gone.output = Some(store.output("main@S9", 1, "jpg"));
        state.screens.insert("main@S9".into(), gone);
        state.on = true;
        store.save(&state).unwrap();
        std::fs::create_dir_all(store.out()).unwrap();
        std::fs::write(store.output("main@S9", 1, "jpg"), b"x").unwrap();
        fake.connect(vec![crate::daemon::tests::screen("main@S1", 0.0, 1.0)]);
        *fake.known.lock().unwrap() = Some(vec!["main@S1".into(), "main@S2".into()]);
        assert_eq!(off_of(&fake, &store), Off::Done(tally(0, 0, 0, 0, 0)));
        assert!(store.load().screens.is_empty() && !store.out().exists());
    }

    #[test]
    fn off_changes_nothing_when_screens_cannot_be_read() {
        let (fake, store, _, _) = pair("blind");
        *fake.blind.lock().unwrap() = true;
        let before = store.read();
        assert!(wind_down(fake.as_ref(), &store).is_err());
        assert_eq!(store.read(), before);
        assert!(store.output("L", 1, "jpg").exists() && store.cache().exists());
    }

    #[test]
    fn use_sets_every_desktop_and_remembers_the_picture() {
        let (fake, store, left, right) = pair("useall");
        let mut state = store.load();
        state.screens.insert("hidden@S2".into(), Entry::new(&right));
        choose_picture(&mut state, fake.as_ref(), &store, &left).unwrap();
        assert!(state.screens.values().all(|entry| entry.source == left));
        assert_eq!(state.screens["hidden@S2"].original, right);
        assert_eq!(state.picture, Some(left));
    }

    #[test]
    fn a_plain_picture_is_its_own_original() {
        let (fake, store, _, chosen) = desk("plain");
        fake.show("main", &chosen);
        let mut state = State::default();
        assert!(record(&mut state, fake.as_ref(), &store).unwrap());
        let entry = &state.screens["main"];
        assert_eq!((entry.source.clone(), entry.original.clone(), entry.unrestorable, entry.chosen), (chosen.clone(), chosen, false, false));
        assert!(!serde_json::to_string(&state).unwrap().contains("unrestorable"));
    }

    #[test]
    fn the_picker_lists_own_pictures_first_then_the_gallery_by_name() {
        let own = crate::state::scratch("own");
        let system = crate::state::scratch("system");
        let nested = system.join("Nature");
        std::fs::create_dir_all(&nested).unwrap();
        let aged = |path: &Path, age: u64| {
            std::fs::write(path, b"x").unwrap();
            std::fs::File::options().write(true).open(path).unwrap().set_modified(SystemTime::now() - Duration::from_secs(age)).unwrap();
        };
        aged(&own.join("old.jpg"), 300);
        aged(&own.join("new.png"), 10);
        aged(&system.join("Zebra.heic"), 1);
        aged(&system.join("apple.jpg"), 5000);
        aged(&system.join("Look.madesktop"), 1);
        aged(&nested.join("Meadow.jpg"), 1);
        let gallery = crate::platform::folder_and_children(&system);
        assert_eq!(gallery, vec![system.clone(), nested.clone()]);
        let found = listing(std::slice::from_ref(&own), &[gallery.clone(), vec![own.clone(), system.clone()]].concat());
        assert_eq!(found, vec![own.join("new.png"), own.join("old.jpg"), system.join("apple.jpg"), nested.join("Meadow.jpg"), system.join("Zebra.heic")]);
        assert!(listing(&[], &[]).is_empty());
        assert_eq!(listing(&[], &gallery).len(), 3);
    }

    #[test]
    fn picked_paths_are_clean_on_every_system() {
        let folder = crate::state::scratch("clean");
        let picture = folder.join("a.jpg");
        std::fs::write(&picture, b"x").unwrap();
        let found = picture_file(&folder.join(".").join("a.jpg")).unwrap();
        assert_eq!(found, state::canonical(&picture).unwrap());
        assert!(!found.to_string_lossy().starts_with(r"\\?\"), "{}", found.display());
    }

    #[test]
    fn off_waits_out_the_daemon_and_gives_up_on_one_that_lives_on() {
        assert!(STOP_PATIENCE >= daemon::STOP_CAP + Duration::from_secs(2));
        let mut lives = 5;
        let mut paused = Duration::ZERO;
        assert!(wait_gone(|| { lives -= 1; lives > 0 }, STOP_PATIENCE, POLL, |length| paused += length));
        assert_eq!(paused, POLL * 4);
        let mut stubborn = Duration::ZERO;
        assert!(!wait_gone(|| true, STOP_PATIENCE, POLL, |length| stubborn += length));
        assert!(stubborn >= STOP_PATIENCE);
    }

    #[test]
    fn status_while_restoring_says_when_nothing_is_running() {
        let mut state = State { restoring: true, ..State::default() };
        state.screens.insert("R".into(), Entry::new(Path::new("/p/a.jpg")));
        assert_eq!(describe(&state, true, None), vec!["Scooch is off. 1 desktops or screens switch back the next time they show."]);
        assert_eq!(describe(&state, false, None)[1], NO_RESTORER);
    }

    #[test]
    fn off_after_an_unreadable_record_deletes_nothing() {
        let (fake, store, _, _) = pair("unreadableoff");
        std::fs::write(store.state_file(), b"{ broken").unwrap();
        assert_eq!(off_of(&fake, &store), Off::Unreadable);
        assert!(store.output("L", 1, "jpg").exists() && store.output("R", 1, "jpg").exists());
        assert!(store.aside_file().is_file());
        assert!(!store.load().on);
        assert!(!UNREADABLE_OFF.contains(['—', ';', '(']) && !UNREADABLE_OFF.contains(": "));
    }

    #[test]
    fn the_closing_line_tells_every_mix_apart() {
        let plain = "2 of your old wallpapers were not picture files and cannot be put back by an app, so pick them again in your system settings.";
        let stuck = "2 desktops still show Scooch's picture because the original file is gone, so pick a wallpaper there.";
        let pending = "2 more switch back the next time those desktops or screens show.";
        for mix in 0..16_usize {
            let count = |bit: usize| if mix & (1 << bit) == 0 { 0 } else { 2 };
            let counts = tally(count(0), count(1), count(2), count(3), 0);
            let mut expected = vec!["Scooch is off."];
            if counts.back > 0 || mix == 0 {
                expected.push("Your wallpaper is back.");
            }
            for (number, sentence) in [(counts.plain, plain), (counts.stuck, stuck), (counts.pending, pending)] {
                if number > 0 {
                    expected.push(sentence);
                }
            }
            let line = closing(&counts);
            assert_eq!(line, expected.join(" "));
            assert!(!line.contains(['\u{2014}', '\u{2013}', ';', '(', ')']) && !line.contains(": "), "{line}");
        }
        assert_eq!(closing(&tally(3, 0, 0, 0, 0)), "Scooch is off. Your wallpaper is back.");
        assert_eq!(closing(&Tally { strays: 1, ..Tally::default() }), format!("Scooch is off. {}", daemon::ORPHAN));
    }

    fn screens(ids: &[&str]) -> Vec<Screen> {
        ids.iter().zip([0.0, 160.0, 320.0]).map(|(id, x)| crate::daemon::tests::screen(id, x, 1.0)).collect()
    }

    fn trio(name: &str) -> (std::sync::Arc<crate::daemon::tests::Fake>, Store, Vec<PathBuf>) {
        let folder = crate::state::scratch(name);
        let originals: Vec<PathBuf> = ["a.png", "b.png", "c.png"].iter().map(|file| crate::daemon::tests::picture(&folder, file)).collect();
        let fake = crate::daemon::tests::Fake::new(screens(&["A", "B", "C"]), &originals[0]);
        fake.show("B", &originals[1]);
        fake.show("C", &originals[2]);
        let store = Store::at(folder.join("data"));
        let mut state = State::default();
        assert!(record(&mut state, fake.as_ref(), &store).unwrap());
        state.on = true;
        store.save(&state).unwrap();
        std::fs::create_dir_all(store.out()).unwrap();
        for id in ["A", "B", "C"] {
            let output = store.output(id, 1, "jpg");
            std::fs::write(&output, b"x").unwrap();
            fake.show(id, &output);
        }
        (fake, store, originals)
    }

    fn ids(state: &State) -> Vec<String> {
        state.screens.keys().cloned().collect()
    }

    #[test]
    fn off_where_the_second_of_three_fails_puts_back_the_rest() {
        let (fake, store, originals) = trio("secondfails");
        fake.refuse.lock().unwrap().push("B".into());
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(2, 0, 0, 1, 0)));
        assert_eq!((fake.showing("A"), fake.showing("C")), (originals[0].clone(), originals[2].clone()));
        assert_eq!(fake.showing("B"), store.output("B", 1, "jpg"));
        let state = store.load();
        assert!(state.restoring && ids(&state) == ["B"]);
        assert!(store.output("B", 1, "jpg").exists() && !store.output("A", 1, "jpg").exists());
        assert_eq!(closing(&tally(2, 0, 0, 1, 0)), "Scooch is off. Your wallpaper is back. 1 more switch back the next time those desktops or screens show.");
    }

    #[test]
    fn a_restoring_tick_where_the_second_of_three_fails_saves_the_rest() {
        let (fake, store, originals) = trio("secondfailstick");
        fake.connect(Vec::new());
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(0, 0, 0, 3, 0)));
        fake.connect(screens(&["A", "B", "C"]));
        fake.refuse.lock().unwrap().push("B".into());
        let daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        assert!(!daemon.tick().unload);
        assert_eq!((fake.showing("A"), fake.showing("C")), (originals[0].clone(), originals[2].clone()));
        let state = store.load();
        assert!(state.restoring && ids(&state) == ["B"] && state.tally.back == 2);
        fake.refuse.lock().unwrap().clear();
        assert!(daemon.tick().unload);
        assert_eq!(fake.showing("B"), originals[1]);
        assert_eq!(store.load().tally.back, 3);
    }

    #[test]
    fn the_restoring_daemon_keeps_its_counts_for_status() {
        let (fake, store, _, right) = pair("restorecounts");
        fake.connect(screens(&["L"]));
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(1, 0, 0, 1, 0)));
        std::fs::remove_file(&right).unwrap();
        fake.connect(screens(&["L", "R"]));
        assert!(crate::daemon::Daemon::new(fake.clone(), store.clone()).tick().unload);
        let output = store.output("R", 1, "jpg");
        assert_eq!(fake.showing("R"), output);
        assert!(output.exists());
        let state = store.load();
        assert_eq!(state.tally, Tally { back: 1, stuck: 1, ..Tally::default() });
        assert_eq!(describe(&state, false, None), vec!["Scooch is off. Run scooch to turn it on.".to_string(), stuck_sentence(1)]);
    }

    fn spaces(name: &str) -> (std::sync::Arc<crate::daemon::tests::Fake>, Store, PathBuf) {
        let folder = crate::state::scratch(name);
        let first = crate::daemon::tests::picture(&folder, "first.png");
        let fake = crate::daemon::tests::Fake::new(screens(&["D@S1"]), &first);
        let store = Store::at(folder.join("data"));
        std::fs::create_dir_all(store.out()).unwrap();
        (fake, store, first)
    }

    #[test]
    fn the_restoring_daemon_never_drops_a_space_from_one_read() {
        let (fake, store, first) = spaces("onereadgone");
        let second = crate::daemon::tests::picture(first.parent().unwrap(), "second.png");
        let mut state = State { on: true, ..State::default() };
        state.screens.insert("D@S1".into(), Entry::new(&first));
        state.screens.insert("D@S2".into(), Entry::new(&second));
        store.save(&state).unwrap();
        let shown = store.output("D@S1", 1, "jpg");
        std::fs::write(&shown, b"x").unwrap();
        fake.show("D@S1", &shown);
        std::fs::write(store.output("D@S2", 1, "jpg"), b"x").unwrap();
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(1, 0, 0, 1, 0)));
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into()]);
        let daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        for _ in 0..3 {
            assert!(!daemon.tick().unload);
        }
        let state = store.load();
        assert!(state.restoring && ids(&state) == ["D@S2"] && store.output("D@S2", 1, "jpg").exists());
        assert_eq!(off_of(&fake, &store), Off::Done(tally(0, 0, 0, 0, 0)));
        assert!(store.load().screens.is_empty() && !store.out().exists());
    }

    #[test]
    fn turning_on_during_a_restoring_tick_keeps_the_daemon_and_its_files() {
        let (fake, store, left, right) = pair("wakeduring");
        fake.connect(screens(&["L"]));
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(1, 0, 0, 1, 0)));
        fake.connect(screens(&["L", "R"]));
        let waking = store.clone();
        fake.during(move || {
            let mut state = waking.load();
            assert!(wake_up(&mut state));
            waking.save(&state).unwrap();
        });
        let daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        let report = daemon.tick();
        assert!(!report.unload && !report.exit);
        let state = store.load();
        assert!(state.on && !state.restoring && store.out().exists());
        assert!(!daemon.tick().exit);
        let state = store.load();
        assert_eq!((state.screens["L"].original.clone(), state.screens["R"].original.clone()), (left, right));
    }

    #[test]
    fn off_finds_an_entry_made_under_a_bare_id() {
        let (fake, store, first) = spaces("offrelink");
        let output = store.output("D", 1, "jpg");
        std::fs::write(&output, b"x").unwrap();
        let mut state = State { on: true, ..State::default() };
        state.screens.insert("D".into(), Entry { output: Some(output.clone()), ..Entry::new(&first) });
        store.save(&state).unwrap();
        fake.show("D@S1", &output);
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into()]);
        assert_eq!(off_of(&fake, &store), Off::Done(tally(1, 0, 0, 0, 0)));
        assert_eq!(fake.showing("D@S1"), first);
        assert!(!output.exists());
    }

    #[test]
    fn off_leaves_its_own_picture_alone_where_it_has_no_record() {
        let (fake, store, _) = spaces("offstray");
        store.save(&State { on: true, ..State::default() }).unwrap();
        let stray = store.output("X", 2, "jpg");
        std::fs::write(&stray, b"x").unwrap();
        fake.show("D@S1", &stray);
        let counts = Tally { strays: 1, ..Tally::default() };
        assert_eq!(off_of(&fake, &store), Off::Done(counts));
        assert!(stray.exists());
        assert_eq!(fake.showing("D@S1"), stray);
        assert!(!closing(&counts).contains("is back"));
    }

    #[test]
    fn off_during_a_slow_tick_wins() {
        let (fake, store, left, right) = pair("offslow");
        fake.place(vec![Rect { x: 60.0, y: 30.0, w: 40.0, h: 40.0 }]);
        let (desktop, stopping) = (fake.clone(), store.clone());
        fake.during(move || {
            assert_eq!(wind_down(desktop.as_ref(), &stopping).unwrap(), Off::Done(tally(2, 0, 0, 0, 0)));
        });
        let mut daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        daemon.grace = Duration::ZERO;
        let report = daemon.tick();
        assert!(!report.unload);
        assert_eq!((fake.showing("L"), fake.showing("R")), (left, right));
        let state = store.load();
        assert!(!state.on && !state.restoring && state.screens.is_empty());
        assert!(std::fs::read_dir(store.out()).map_or(true, |mut files| files.next().is_none()));
        assert!(daemon.tick().exit);
    }

    #[test]
    fn scooch_right_after_off_with_pending_entries_turns_back_on() {
        let (fake, store, left, right) = pair("onagain");
        fake.connect(screens(&["L"]));
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(1, 0, 0, 1, 0)));
        let mut state = store.load();
        assert!(wake_up(&mut state));
        store.save(&state).unwrap();
        fake.connect(screens(&["L", "R"]));
        let daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        let report = daemon.tick();
        assert!(!report.unload && !report.exit);
        let state = store.load();
        assert!(state.on && !state.restoring && state.tally.is_empty());
        assert_eq!((state.screens["L"].original.clone(), state.screens["R"].original.clone()), (left, right));
    }

    #[test]
    fn a_daemon_killed_while_restoring_finishes_when_started_again() {
        let (fake, store, originals) = trio("killed");
        fake.connect(Vec::new());
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(0, 0, 0, 3, 0)));
        fake.connect(screens(&["A"]));
        let first = crate::daemon::Daemon::new(fake.clone(), store.clone());
        assert!(!first.tick().unload);
        drop(first);
        fake.show("B", &originals[1]);
        let state = store.load();
        assert!(state.restoring && ids(&state) == ["B", "C"] && state.tally.back == 1);
        fake.connect(screens(&["A", "B", "C"]));
        let again = crate::daemon::Daemon::new(fake.clone(), store.clone());
        assert!(again.tick().unload);
        assert_eq!((fake.showing("B"), fake.showing("C")), (originals[1].clone(), originals[2].clone()));
        let state = store.load();
        assert!(!state.restoring && state.screens.is_empty() && !store.out().exists());
        assert_eq!(state.tally.back, 3);
    }

    #[test]
    fn a_kill_right_after_the_set_still_leaves_a_record_that_off_keeps() {
        let (fake, store, first) = spaces("killset");
        store.save(&State { on: true, ..State::default() }).unwrap();
        fake.place(vec![Rect { x: 60.0, y: 30.0, w: 40.0, h: 40.0 }]);
        *fake.crash_after_set.lock().unwrap() = true;
        let mut daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        daemon.grace = Duration::ZERO;
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| daemon.tick())).is_err());
        let shown = fake.showing("D@S1");
        assert!(store.owns(&shown));
        assert_eq!(store.load().screens["D@S1"].output.as_deref(), Some(shown.as_path()));
        fake.connect(screens(&["D@S2"]));
        fake.show("D@S2", &first);
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(0, 0, 0, 1, 0)));
        assert!(shown.exists());
    }

    #[test]
    fn off_counts_a_desktop_pointing_at_a_picture_no_record_knows() {
        let (fake, store, first) = spaces("norecord");
        store.save(&State { on: true, ..State::default() }).unwrap();
        let lost = store.output("D@S1", 4, "jpg");
        std::fs::write(&lost, b"x").unwrap();
        fake.show("D@S1", &lost);
        fake.connect(screens(&["D@S2"]));
        fake.show("D@S2", &first);
        let counts = Tally { strays: 1, ..Tally::default() };
        assert_eq!(off_of(&fake, &store), Off::Done(counts));
        assert!(lost.exists());
        assert!(!closing(&counts).contains("is back"));
    }

    #[test]
    fn every_command_works_while_no_desktop_shows() {
        let (fake, store, first) = spaces("fullscreen");
        fake.connect(Vec::new());
        let mut state = State::default();
        assert_eq!(turn_on(&mut state, fake.as_ref(), &store, false).unwrap(), FirstRun::Go);
        assert!(state.on && state.screens.is_empty());
        store.save(&state).unwrap();
        assert_eq!(describe(&state, true, Some(&[])), vec![WAITING.to_string()]);
        assert!(!WAITING.contains(['\u{2014}', '\u{2013}', ';', '(', ')']) && !WAITING.contains(": "));
        let mut daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        daemon.grace = Duration::ZERO;
        let report = daemon.tick();
        assert!(report.problem.is_none() && report.outcomes.is_empty() && store.load().problem.is_none());
        let mut chosen = store.load();
        choose_picture(&mut chosen, fake.as_ref(), &store, &first).unwrap();
        assert!(chosen.on && chosen.screens.is_empty() && chosen.picture == Some(first.clone()));
        store.save(&chosen).unwrap();
        fake.connect(screens(&["D@S1"]));
        daemon.tick();
        assert_eq!(store.load().screens["D@S1"].source, first);
        fake.show("D@S1", &store.output("D@S1", 1, "jpg"));
        std::fs::write(store.output("D@S1", 1, "jpg"), b"x").unwrap();
        fake.connect(Vec::new());
        assert_eq!(off_of(&fake, &store), Off::Pending(tally(0, 0, 0, 1, 0)));
        assert!(store.load().restoring);
    }

    #[test]
    fn off_with_an_entry_that_changed_nothing_is_back_at_once() {
        let (fake, store, first) = spaces("nooutput");
        let mut state = State { on: true, ..State::default() };
        state.screens.insert("D@S9".into(), Entry::new(&first));
        store.save(&state).unwrap();
        assert_eq!(off_of(&fake, &store), Off::Done(tally(1, 0, 0, 0, 0)));
        assert!(!store.load().restoring && store.load().screens.is_empty());
        state.restoring = true;
        state.on = false;
        store.save(&state).unwrap();
        let report = crate::daemon::Daemon::new(fake.clone(), store.clone()).tick();
        assert!(report.unload);
        assert_eq!(store.load().tally.back, 1);
    }

    #[test]
    fn a_pending_space_closed_while_restoring_lets_restoring_end() {
        let (fake, store, first) = spaces("closedpending");
        let output = store.output("D@S2", 1, "jpg");
        std::fs::write(&output, b"x").unwrap();
        let mut state = State { restoring: true, ..State::default() };
        state.screens.insert("D@S2".into(), Entry { output: Some(output.clone()), ..Entry::new(&first) });
        store.save(&state).unwrap();
        *fake.known.lock().unwrap() = Some(vec!["D@S1".into()]);
        let daemon = crate::daemon::Daemon::new(fake.clone(), store.clone());
        assert!(!daemon.tick().unload);
        let marked = store.load().screens["D@S2"].clone();
        assert!(marked.absent_since.is_some() && marked.absent_ticks == 1 && output.exists());
        let mut state = store.load();
        let entry = state.screens.get_mut("D@S2").unwrap();
        (entry.absent_since, entry.absent_ticks) = (Some(1), 4);
        store.save(&state).unwrap();
        assert!(daemon.tick().unload);
        let state = store.load();
        assert!(!state.restoring && state.screens.is_empty() && !output.exists());
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
