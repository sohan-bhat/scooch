use crate::commands::{self, FirstRun, Off};
use crate::platform::{self, Desktop, Screen};
use crate::state::{self, Store};
use crate::{daemon, poke, scene};
use anyhow::{anyhow, bail, Result};
use scooch_core::{Options, Rect, Scene};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use crate::state::{Tally, PROGRAM};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    Macos,
    Windows,
    Linux,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Places {
    pub width: u32,
    pub height: u32,
    pub icons: Vec<Rect>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Monitor {
    pub id: String,
    pub frame: Rect,
    pub picture: Option<PathBuf>,
    pub places: Option<Places>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Wallpaper {
    Slideshow,
    Video,
    Color,
    Moving,
    Unreadable,
    Made,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Problem {
    Automation,
    Asking,
    Finder,
    Move,
    Wallpaper { what: Wallpaper },
    Gnome,
    Unknown { desktop: String },
    Admin,
    Startup,
    Stopped,
    Screen,
    Other,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
pub struct Switches {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub glide: Option<bool>,
    pub scenery: bool,
    pub flip: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub predict: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Counted {
    pub icons: usize,
    pub screens: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub system: System,
    pub on: bool,
    pub running: bool,
    pub restoring: bool,
    pub pending: usize,
    pub monitors: Vec<Monitor>,
    pub switches: Switches,
    pub problem: Option<Problem>,
    pub flip_helps: bool,
    pub current: Option<PathBuf>,
    pub counted: Option<Counted>,
    pub reduce_motion: bool,
    pub text_scale: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Switch {
    Glide,
    Scenery,
    Flip,
    Predict,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Started {
    Go,
    Picture,
}

pub const THUMB: (u32, u32) = (320, 200);

pub fn system() -> System {
    if cfg!(target_os = "macos") {
        System::Macos
    } else if cfg!(windows) {
        System::Windows
    } else {
        System::Linux
    }
}

pub fn folder() -> PathBuf {
    Store::open().root
}

pub fn host() {
    #[cfg(target_os = "macos")]
    platform::macos::RUNNING.store(true, std::sync::atomic::Ordering::SeqCst);
}

pub fn places_of(bytes: Option<&[u8]>, id: &str) -> Option<Places> {
    let value: serde_json::Value = serde_json::from_slice(bytes?).ok()?;
    serde_json::from_value(value.get("screens")?.get(id)?.get("places")?.clone()).ok()
}

fn local(screen: &Screen, icons: &[Rect]) -> Places {
    let Scene { width, height, icons } = scene::scene(screen, icons);
    Places { width, height, icons }
}

struct Layout {
    all: Vec<Screen>,
    arranged: Vec<String>,
}

fn layout(desktop: &dyn Desktop) -> Result<Layout> {
    #[cfg(target_os = "linux")]
    {
        let linux = platform::linux::Linux::open()?;
        let found = linux.all_screens()?;
        let arranged: Vec<String> = desktop.screens()?.into_iter().map(|screen| screen.id).collect();
        let mut all: Vec<Screen> = found.iter().filter(|each| each.primary).map(|each| each.screen.clone()).collect();
        all.extend(found.iter().filter(|each| !each.primary).map(|each| each.screen.clone()));
        Ok(Layout { all, arranged })
    }
    #[cfg(not(target_os = "linux"))]
    {
        let all = desktop.screens()?;
        let arranged = all.iter().map(|screen| screen.id.clone()).collect();
        Ok(Layout { all, arranged })
    }
}

fn wallpaper_kind(desktop: &dyn Desktop, screen: &Screen, store: &Store) -> Option<Wallpaper> {
    match desktop.wallpaper(screen) {
        Ok(Some(path)) if store.owns(&path) => Some(Wallpaper::Made),
        Ok(Some(path)) if path.is_file() => None,
        Ok(Some(_)) => Some(Wallpaper::Unreadable),
        Ok(None) => Some(unpictured(screen)),
        Err(_) => None,
    }
}

#[cfg(target_os = "macos")]
fn unpictured(screen: &Screen) -> Wallpaper {
    use platform::macos::store::{describe, Kind};
    match describe(&screen.id) {
        Some(Kind::Default) => Wallpaper::Moving,
        Some(Kind::Picture(_)) => Wallpaper::Unreadable,
        Some(Kind::Other(provider)) if provider.to_lowercase().contains("color") => Wallpaper::Color,
        Some(Kind::Other(provider)) if provider.to_lowercase().contains("shuffle") => Wallpaper::Slideshow,
        Some(Kind::Other(_)) => Wallpaper::Video,
        None => Wallpaper::Other,
    }
}

#[cfg(not(target_os = "macos"))]
fn unpictured(_screen: &Screen) -> Wallpaper {
    Wallpaper::Other
}

fn saved_problem(text: &str) -> Problem {
    #[cfg(target_os = "macos")]
    {
        use platform::macos::finder;
        if text == finder::DENIED {
            return Problem::Automation;
        }
        if text == finder::ASKING {
            return Problem::Asking;
        }
        if text == finder::FINDER_OFF {
            return Problem::Finder;
        }
    }
    if text == daemon::NOT_A_FILE {
        return Problem::Wallpaper { what: Wallpaper::Other };
    }
    if text == daemon::ORPHAN {
        return Problem::Wallpaper { what: Wallpaper::Made };
    }
    if text == commands::NO_SCREEN || text == commands::WAITING {
        return Problem::Screen;
    }
    Problem::Other
}

#[cfg(target_os = "linux")]
fn unknown_desktop() -> Option<Problem> {
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    if platform::linux::parse::desktop::pick(&current).is_some() {
        return None;
    }
    let name = current.split(':').find(|part| !part.trim().is_empty()).map(|part| part.trim().to_string()).unwrap_or_else(|| "This desktop".into());
    Some(Problem::Unknown { desktop: name })
}

#[cfg(not(target_os = "linux"))]
fn unknown_desktop() -> Option<Problem> {
    None
}

#[cfg(target_os = "linux")]
fn gnome_without_icons() -> bool {
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    platform::linux::parse::desktop::pick(&current) == Some(platform::linux::parse::desktop::Kind::Gnome) && platform::linux::gnome::running().is_ok_and(|running| !running)
}

#[cfg(not(target_os = "linux"))]
fn gnome_without_icons() -> bool {
    false
}

#[cfg(windows)]
fn admin() -> bool {
    platform::windows::startup::refused()
}

#[cfg(not(windows))]
fn admin() -> bool {
    false
}

#[cfg(windows)]
fn startup_off() -> bool {
    platform::windows::startup::approval().is_some_and(|bytes| bytes.first().is_some_and(|first| first & 1 == 1))
}

#[cfg(not(windows))]
fn startup_off() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn predicts() -> bool {
    use platform::linux::parse::desktop::{pick, session, Kind, Session};
    let current = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    pick(&current) == Some(Kind::Xfce) && session(std::env::var("WAYLAND_DISPLAY").ok().as_deref()) == Session::X11
}

#[cfg(not(target_os = "linux"))]
fn predicts() -> bool {
    false
}

#[cfg(windows)]
pub fn text_scale() -> f32 {
    use ::windows::core::w;
    use ::windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut value = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let read = unsafe { RegGetValueW(HKEY_CURRENT_USER, w!(r"Software\Microsoft\Accessibility"), w!("TextScaleFactor"), RRF_RT_REG_DWORD, None, Some(&mut value as *mut u32 as *mut std::ffi::c_void), Some(&mut size)) };
    if read.is_ok() && (100..=225).contains(&value) {
        value as f32 / 100.0
    } else {
        1.0
    }
}

#[cfg(target_os = "linux")]
pub fn text_scale() -> f32 {
    platform::linux::settings::gsetting("org.gnome.desktop.interface", "text-scaling-factor").and_then(|text| text.trim().parse::<f32>().ok()).filter(|scale| (0.5..=3.0).contains(scale)).unwrap_or(1.0)
}

#[cfg(target_os = "macos")]
pub fn text_scale() -> f32 {
    1.0
}

fn problem_of(view: &View, layout: Option<&Layout>, desktop: &dyn Desktop, store: &Store, saved: Option<&str>) -> Option<Problem> {
    if let Some(problem) = unknown_desktop() {
        return Some(problem);
    }
    if admin() {
        return Some(Problem::Admin);
    }
    let layout = layout?;
    if layout.all.is_empty() {
        return Some(Problem::Screen);
    }
    if view.on && !view.running && !view.restoring {
        return Some(Problem::Stopped);
    }
    if gnome_without_icons() {
        return Some(Problem::Gnome);
    }
    if !view.on {
        let arranged = layout.all.iter().filter(|screen| layout.arranged.contains(&screen.id));
        if let Some(what) = arranged.filter_map(|screen| wallpaper_kind(desktop, screen, store)).next() {
            return Some(Problem::Wallpaper { what });
        }
        return None;
    }
    if let Some(text) = saved {
        return Some(saved_problem(text));
    }
    if startup_off() {
        return Some(Problem::Startup);
    }
    None
}

pub fn view() -> View {
    let store = Store::open();
    let bytes = store.read();
    let state = state::parse(bytes.as_deref());
    let running = store.watcher().is_some();
    let desktop = platform::current().ok();
    let reduce_motion = desktop.as_ref().is_some_and(|desktop| desktop.reduce_motion());
    let live = desktop.as_ref().is_some_and(|desktop| desktop.live());
    let mut view = View {
        system: system(),
        on: state.on,
        running,
        restoring: state.restoring && !state.on,
        pending: if state.restoring && !state.on { state.screens.len() } else { 0 },
        monitors: Vec::new(),
        switches: Switches {
            glide: (live && !reduce_motion).then_some(state.glide),
            scenery: state::scenery_on(&state.options),
            flip: state.options.mirror,
            predict: predicts().then_some(state.predict),
        },
        problem: None,
        flip_helps: false,
        current: None,
        counted: None,
        reduce_motion,
        text_scale: text_scale(),
    };
    let Some(desktop) = desktop else {
        view.problem = unknown_desktop().or(Some(Problem::Other));
        return view;
    };
    let layout = layout(desktop.as_ref()).ok();
    if let Some(layout) = &layout {
        let snapshot_icons = if state.on || cfg!(target_os = "macos") { None } else { desktop.snapshot().ok().map(|snapshot| snapshot.icons) };
        let lead = layout.arranged.first().and_then(|id| state.screens.get(id)).map(|entry| entry.shown().to_path_buf());
        let mut counted = Counted { icons: 0, screens: 0 };
        for screen in &layout.all {
            let entry = state.screens.get(&screen.id).filter(|_| state.on);
            let picture = match entry {
                Some(entry) => Some(entry.shown().to_path_buf()),
                None if state.on => lead.clone(),
                None => desktop.wallpaper(screen).ok().flatten(),
            };
            let places = match (&snapshot_icons, entry) {
                (Some(icons), _) => Some(local(screen, icons)),
                (None, Some(_)) => places_of(bytes.as_deref(), &screen.id),
                (None, None) => None,
            };
            if let Some(entry) = entry {
                counted.icons += entry.icons;
                counted.screens += 1;
                view.flip_helps |= entry.flip_helps && !state.options.mirror;
                if view.current.is_none() {
                    view.current = Some(entry.source.clone());
                }
            }
            view.monitors.push(Monitor { id: screen.id.clone(), frame: screen.frame, picture, places });
        }
        if view.current.is_none() {
            view.current = view.monitors.iter().filter_map(|monitor| monitor.picture.clone()).find(|path| !store.owns(path));
        }
        if state.on && running && counted.screens > 0 {
            view.counted = Some(counted);
        }
    }
    view.problem = problem_of(&view, layout.as_ref(), desktop.as_ref(), &store, state.problem.as_deref());
    view
}

fn begin(desktop: &dyn Desktop, store: &Store, program: &Path) -> Result<()> {
    if let Some(process) = store.watcher() {
        if poke::send(process) {
            return Ok(());
        }
    }
    desktop.autostart(Some(&state::canonical(program).unwrap_or_else(|_| program.to_path_buf())))
}

fn replace_outdated(store: &Store) -> Result<()> {
    if !commands::outdated(&store.load(), store.watcher().is_some(), state::PROGRAM) {
        return Ok(());
    }
    commands::stop(store)?;
    store.mark(None)
}

pub fn turn_on(program: &Path) -> Result<Started> {
    let store = Store::open();
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    store.load();
    commands::aside(&store);
    replace_outdated(&store)?;
    let mut state = store.load();
    let desktop = platform::current()?;
    if state.on && store.watcher().is_some() {
        return Ok(Started::Go);
    }
    match commands::turn_on(&mut state, desktop.as_ref(), &store, false)? {
        FirstRun::Go => {}
        FirstRun::Pick | FirstRun::Fail => return Ok(Started::Picture),
    }
    state.problem = None;
    store.save(&state)?;
    begin(desktop.as_ref(), &store, program)?;
    Ok(Started::Go)
}

pub fn turn_off(program: &Path) -> Result<Tally> {
    let store = Store::open();
    let desktop = platform::current()?;
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    desktop.screens()?;
    commands::stop(&store)?;
    Ok(match commands::wind_down(desktop.as_ref(), &store)? {
        Off::Unreadable => {
            desktop.autostart(None)?;
            Tally::default()
        }
        Off::Done(tally) => {
            desktop.autostart(None)?;
            tally
        }
        Off::Pending(tally) => {
            begin(desktop.as_ref(), &store, program)?;
            tally
        }
    })
}

pub fn set(switch: Switch, on: bool) -> Result<()> {
    if switch == Switch::Glide && !platform::current().is_ok_and(|desktop| desktop.live()) {
        bail!(commands::GLIDE_NONE);
    }
    let store = Store::open();
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    let mut state = store.load();
    match switch {
        Switch::Glide => state.glide = on,
        Switch::Scenery => state.options.scenery = if on { scooch_core::Scenery::Auto } else { scooch_core::Scenery::Off },
        Switch::Flip => state.options.mirror = on,
        Switch::Predict => state.predict = on,
    }
    store.save(&state)?;
    if let Some(process) = store.watcher() {
        poke::send(process);
    }
    Ok(())
}

pub fn use_picture(program: &Path, path: &Path) -> Result<()> {
    let image = state::canonical(path).map_err(|_| anyhow!("{} was not found.", path.display()))?;
    if !image.is_file() || !commands::is_picture(&image) {
        bail!("{} is not a picture file.", image.display());
    }
    let desktop = platform::current()?;
    desktop.decode(&image).map_err(|_| anyhow!("{} cannot be read as a picture.", image.display()))?;
    let store = Store::open();
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    replace_outdated(&store)?;
    let mut state = store.load();
    commands::choose_picture(&mut state, desktop.as_ref(), &store, &image)?;
    state.on = true;
    state.problem = None;
    store.save(&state)?;
    begin(desktop.as_ref(), &store, program)
}

pub fn pictures(history: &[PathBuf]) -> Vec<PathBuf> {
    let state = Store::open().load();
    let mut found: Vec<PathBuf> = Vec::new();
    let mut add = |path: &Path| {
        if path.is_file() && commands::is_picture(path) && !found.iter().any(|seen| state::same(seen, path)) {
            found.push(path.to_path_buf());
        }
    };
    for path in history {
        add(path);
    }
    for entry in state.screens.values() {
        add(&entry.source);
        add(&entry.original);
    }
    let gallery = platform::current().map(|desktop| desktop.gallery()).unwrap_or_default();
    let mut listed = commands::pictures(&gallery);
    listed.sort_by_cached_key(|path| path.file_name().map(|name| name.to_string_lossy().to_lowercase()));
    for path in &listed {
        add(path);
    }
    found
}

fn decoded(path: &Path) -> Result<scooch_core::RgbImage> {
    if !commands::is_picture(path) {
        bail!("{} is not a picture file.", path.display());
    }
    Ok(match platform::current() {
        Ok(desktop) => desktop.decode(path)?,
        Err(_) => scooch_core::open(path)?,
    })
}

fn framed(source: &scooch_core::RgbImage, width: u32, height: u32) -> Result<Vec<u8>> {
    let scene = Scene { width: width.max(1), height: height.max(1), icons: Vec::new() };
    let placement = scooch_core::fill(source.width(), source.height(), &scene);
    let picture = scooch_core::render(source, &placement, &scene, &Options { pads: 0.0, ..Options::default() });
    Ok(scooch_core::encode_jpeg(&picture, 84)?)
}

pub fn thumbnail(path: &Path) -> Result<Vec<u8>> {
    framed(&decoded(path)?, THUMB.0, THUMB.1)
}

pub fn preview(path: &Path, long_side: u32) -> Result<Vec<u8>> {
    let source = decoded(path)?;
    let (width, height) = (source.width().max(1), source.height().max(1));
    let shrink = (long_side as f32 / width.max(height) as f32).min(1.0);
    framed(&source, (width as f32 * shrink).round() as u32, (height as f32 * shrink).round() as u32)
}

pub fn login_points_at(program: &Path) -> Option<bool> {
    let wanted = state::canonical(program).unwrap_or_else(|_| program.to_path_buf());
    let text = login_text()?;
    let plain = wanted.to_string_lossy().to_lowercase();
    let twin = wanted.with_file_name(if cfg!(windows) { "scoochw.exe" } else { "scooch" }).to_string_lossy().to_lowercase();
    let text = text.to_lowercase();
    Some(text.contains(&plain) || text.contains(&twin))
}

#[cfg(target_os = "macos")]
fn login_text() -> Option<String> {
    std::fs::read_to_string(platform::macos::launchd::plist_path().ok()?).ok()
}

#[cfg(windows)]
fn login_text() -> Option<String> {
    platform::windows::startup::entry()
}

#[cfg(target_os = "linux")]
fn login_text() -> Option<String> {
    std::fs::read_to_string(platform::linux::autostart::path().ok()?).ok()
}

pub fn repair(program: &Path) -> Result<bool> {
    let store = Store::open();
    let state = store.load();
    if !state.on || login_points_at(program) != Some(false) {
        return Ok(false);
    }
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    let desktop = platform::current()?;
    desktop.autostart(Some(&state::canonical(program).unwrap_or_else(|_| program.to_path_buf())))?;
    Ok(true)
}

#[cfg(target_os = "linux")]
pub fn show_gnome_icons() -> Result<()> {
    let _: bool = platform::linux::bus::call("org.gnome.Shell.Extensions", "/org/gnome/Shell/Extensions", "org.gnome.Shell.Extensions", "EnableExtension", &("ding@rastersoft.com",))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn places_read_from_the_saved_entry_and_nothing_else() {
        let bytes = br#"{"on":true,"screens":{"main":{"source":"/p/a.jpg","places":{"width":2880,"height":1800,"icons":[{"x":10,"y":20,"w":90,"h":90}]}},"side":{"source":"/p/a.jpg"}}}"#;
        let places = places_of(Some(bytes), "main").expect("places are read");
        assert_eq!((places.width, places.height, places.icons.len()), (2880, 1800, 1));
        assert_eq!(places.icons[0], Rect { x: 10.0, y: 20.0, w: 90.0, h: 90.0 });
        assert_eq!(places_of(Some(bytes), "side"), None);
        assert_eq!(places_of(Some(bytes), "gone"), None);
        assert_eq!(places_of(None, "main"), None);
        assert_eq!(places_of(Some(b"not json"), "main"), None);
    }

    #[test]
    fn saved_sentences_map_to_typed_problems() {
        assert_eq!(saved_problem(daemon::NOT_A_FILE), Problem::Wallpaper { what: Wallpaper::Other });
        assert_eq!(saved_problem(daemon::ORPHAN), Problem::Wallpaper { what: Wallpaper::Made });
        assert_eq!(saved_problem(commands::NO_SCREEN), Problem::Screen);
        assert_eq!(saved_problem("anything else"), Problem::Other);
    }

    #[test]
    fn the_view_serializes_the_way_the_window_reads_it() {
        let problem = serde_json::to_value(Problem::Wallpaper { what: Wallpaper::Slideshow }).unwrap();
        assert_eq!(problem, serde_json::json!({ "kind": "wallpaper", "what": "slideshow" }));
        let switches = serde_json::to_value(Switches { glide: None, scenery: true, flip: false, predict: None }).unwrap();
        assert_eq!(switches, serde_json::json!({ "scenery": true, "flip": false }));
    }
}
