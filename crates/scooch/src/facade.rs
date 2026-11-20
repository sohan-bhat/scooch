use crate::commands::{self, FirstRun, Off};
use crate::platform::{self, Desktop, Screen};
use crate::state::{self, Entry, Store};
use crate::{daemon, poke, scene};
use anyhow::{anyhow, bail, Result};
use scooch_core::{Options, Rect, Scene};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use crate::state::{Places, Tally, PROGRAM};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum System {
    Macos,
    Windows,
    Linux,
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
    Gnome { icons: Icons },
    Unknown { desktop: String },
    Admin,
    Startup,
    Stopped,
    Screen,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Icons {
    Missing,
    Off,
    Blocked,
    Broken,
    Unknown,
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
    pub live: bool,
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

pub fn places_of(entry: Option<&Entry>, on: bool) -> Option<Places> {
    entry.filter(|_| on).and_then(|entry| entry.places.clone())
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

pub fn icons_from(info: Option<&std::collections::HashMap<String, f64>>, user_extensions_off: bool) -> Icons {
    let Some(info) = info else { return Icons::Unknown };
    if info.is_empty() {
        return Icons::Missing;
    }
    if user_extensions_off {
        return Icons::Blocked;
    }
    match info.get("state").copied() {
        Some(state) if state == 3.0 || state == 4.0 => Icons::Broken,
        _ => Icons::Off,
    }
}

#[cfg(target_os = "linux")]
fn ding_info() -> Option<std::collections::HashMap<String, f64>> {
    let info: std::collections::HashMap<String, zbus::zvariant::OwnedValue> = platform::linux::bus::call(EXTENSIONS, EXTENSIONS_PATH, EXTENSIONS, "GetExtensionInfo", &(DING,)).ok()?;
    let mut numbers = std::collections::HashMap::new();
    for (key, value) in info {
        numbers.insert(key, value.downcast_ref::<f64>().unwrap_or(f64::NAN));
    }
    Some(numbers)
}

#[cfg(target_os = "linux")]
fn ding_icons() -> Icons {
    let off = platform::linux::settings::gsetting("org.gnome.shell", "disable-user-extensions").is_some_and(|value| value.trim() == "true");
    icons_from(ding_info().as_ref(), off)
}

#[cfg(not(target_os = "linux"))]
fn ding_icons() -> Icons {
    Icons::Unknown
}

#[cfg(target_os = "linux")]
const EXTENSIONS: &str = "org.gnome.Shell.Extensions";
#[cfg(target_os = "linux")]
const EXTENSIONS_PATH: &str = "/org/gnome/Shell/Extensions";
#[cfg(target_os = "linux")]
const DING: &str = "ding@rastersoft.com";

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
    crate::platform::linux::drops::predicts()
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
        return Some(Problem::Gnome { icons: ding_icons() });
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
    let state = store.load();
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
        live,
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
                (None, Some(entry)) => places_of(Some(entry), state.on),
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
    let desktop = platform::current().ok();
    if let Some(desktop) = &desktop {
        let store = Store::open();
        for screen in desktop.screens().unwrap_or_default() {
            if let Some(path) = desktop.wallpaper(&screen).ok().flatten().filter(|path| !store.owns(path)) {
                add(&path);
            }
        }
    }
    let gallery = desktop.map(|desktop| desktop.gallery()).unwrap_or_default();
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

pub fn set_origin(origin: Option<&Path>) -> Result<bool> {
    let store = Store::open();
    let wanted = origin.map(|path| state::canonical(path).unwrap_or_else(|_| path.to_path_buf()));
    if store.load().origin == wanted {
        return Ok(false);
    }
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    let mut state = store.load();
    if state.origin == wanted {
        return Ok(false);
    }
    state.origin = wanted;
    store.save(&state)?;
    Ok(true)
}

pub fn login_points_at(program: &Path) -> Option<bool> {
    let text = login_text()?;
    let Some(named) = login_program(&text) else { return Some(false) };
    let wanted = state::canonical(program).unwrap_or_else(|_| program.to_path_buf());
    let twin = wanted.with_file_name(if cfg!(windows) { "scoochw.exe" } else { "scooch" });
    let found = state::canonical(&named).unwrap_or(named);
    Some(same_path(&found, &wanted) || same_path(&found, &twin))
}

fn same_path(a: &Path, b: &Path) -> bool {
    if cfg!(target_os = "linux") {
        a == b
    } else {
        a.to_string_lossy().to_lowercase() == b.to_string_lossy().to_lowercase()
    }
}

#[cfg(target_os = "macos")]
fn login_program(text: &str) -> Option<PathBuf> {
    plist_program(text)
}

#[cfg(target_os = "linux")]
fn login_program(text: &str) -> Option<PathBuf> {
    entry_program(text)
}

#[cfg(windows)]
fn login_program(text: &str) -> Option<PathBuf> {
    run_program(text)
}

#[cfg(any(target_os = "macos", test))]
fn plist_program(text: &str) -> Option<PathBuf> {
    let after = &text[text.find("<key>ProgramArguments</key>")?..];
    let start = after.find("<string>")? + "<string>".len();
    let end = start + after[start..].find("</string>")?;
    Some(PathBuf::from(xml_text(&after[start..end])?))
}

#[cfg(any(target_os = "macos", test))]
fn xml_text(raw: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = raw;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        let end = at + rest[at..].find(';')?;
        let name = &rest[at + 1..end];
        let letter = match name {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ if name.starts_with("#x") => char::from_u32(u32::from_str_radix(&name[2..], 16).ok()?)?,
            _ if name.starts_with('#') => char::from_u32(name[1..].parse().ok()?)?,
            _ => return None,
        };
        out.push(letter);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

#[cfg(any(target_os = "linux", test))]
fn entry_program(text: &str) -> Option<PathBuf> {
    let value = text.lines().find_map(|line| line.trim_start().strip_prefix("Exec="))?;
    let mut plain = String::new();
    let mut letters = value.chars();
    while let Some(letter) = letters.next() {
        match letter {
            '\\' => match letters.next()? {
                's' => plain.push(' '),
                'n' => plain.push('\n'),
                't' => plain.push('\t'),
                'r' => plain.push('\r'),
                other => plain.push(other),
            },
            '%' => {
                if letters.next() == Some('%') {
                    plain.push('%');
                }
            }
            other => plain.push(other),
        }
    }
    let mut letters = plain.trim_start().chars();
    let mut program = String::new();
    if plain.trim_start().starts_with('"') {
        letters.next();
        loop {
            match letters.next()? {
                '"' => break,
                '\\' => program.push(letters.next()?),
                other => program.push(other),
            }
        }
    } else {
        program.extend(letters.take_while(|letter| !letter.is_whitespace()));
    }
    (!program.is_empty()).then(|| PathBuf::from(program))
}

#[cfg(any(windows, test))]
fn run_program(text: &str) -> Option<PathBuf> {
    let text = text.trim();
    let program = match text.strip_prefix('"') {
        Some(rest) => &rest[..rest.find('"')?],
        None => text.split_whitespace().next()?,
    };
    (!program.is_empty()).then(|| PathBuf::from(program))
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

pub fn is_on() -> bool {
    Store::open().load().on
}

pub fn repair(program: &Path) -> Result<bool> {
    let store = Store::open();
    repair_with(&store, || login_points_at(program), |older| {
        let desktop = platform::current()?;
        commands::stop(&store)?;
        if older {
            store.mark(None)?;
        }
        desktop.autostart(Some(&state::canonical(program).unwrap_or_else(|_| program.to_path_buf())))
    })
}

fn repair_with(store: &Store, points: impl FnOnce() -> Option<bool>, act: impl FnOnce(bool) -> Result<()>) -> Result<bool> {
    let _lock = store.command_lock(state::COMMAND_PATIENCE)?;
    let state = store.load();
    if !state.on {
        return Ok(false);
    }
    let elsewhere = points() == Some(false);
    let older = commands::outdated(&state, store.watcher().is_some(), PROGRAM);
    if !elsewhere && !older {
        return Ok(false);
    }
    act(older)?;
    Ok(true)
}

#[cfg(target_os = "linux")]
pub fn child_env(changes: Vec<(std::ffi::OsString, Option<std::ffi::OsString>)>) {
    platform::linux::child_env(changes);
}

#[cfg(target_os = "linux")]
pub fn show_gnome_icons() -> Result<()> {
    use std::time::{Duration, Instant};
    if ding_icons() != Icons::Off {
        bail!("gnome-icons");
    }
    let connection = platform::linux::bus::session()?;
    let rule = zbus::MatchRule::builder().msg_type(zbus::message::Type::Signal).interface(EXTENSIONS)?.member("ExtensionStateChanged")?.build();
    let signals = zbus::blocking::MessageIterator::for_match_rule(rule, &connection, Some(32))?;
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        for message in signals.flatten() {
            let Ok((uuid, info)) = message.body().deserialize::<(String, std::collections::HashMap<String, zbus::zvariant::OwnedValue>)>() else { continue };
            let state = info.get("state").and_then(|value| value.downcast_ref::<f64>().ok());
            if uuid == DING && sender.send(state).is_err() {
                break;
            }
        }
    });
    let _: bool = platform::linux::bus::call(EXTENSIONS, EXTENSIONS_PATH, EXTENSIONS, "EnableExtension", &(DING,))?;
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut active = ding_info().and_then(|info| info.get("state").copied()) == Some(1.0);
    while !active {
        let left = deadline.saturating_duration_since(Instant::now());
        match receiver.recv_timeout(left) {
            Ok(Some(state)) => active = state == 1.0,
            Ok(None) => {}
            Err(_) => break,
        }
    }
    if !active {
        bail!("gnome-icons");
    }
    while Instant::now() < deadline + Duration::from_secs(5) {
        if platform::linux::gnome::running().unwrap_or(false) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    bail!("gnome-icons")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repair_decides_on_the_state_it_reads_under_the_lock() {
        let root = std::env::temp_dir().join(format!("scooch-repair-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Store::at(&root);
        let mut saved = store.load();
        saved.on = true;
        store.save(&saved).unwrap();
        let held = store.command_lock(state::COMMAND_PATIENCE).unwrap();
        let acted = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let seen = acted.clone();
        let folder = root.clone();
        let repairing = std::thread::spawn(move || repair_with(&Store::at(&folder), || Some(false), |_| {
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let mut off = store.load();
        off.on = false;
        store.save(&off).unwrap();
        drop(held);
        let repaired = repairing.join().unwrap().unwrap();
        let _ = std::fs::remove_dir_all(&root);
        assert!(!repaired);
        assert!(!acted.load(std::sync::atomic::Ordering::SeqCst));
    }

    #[test]
    fn gnome_icons_tell_missing_off_blocked_broken_and_unknown_apart() {
        use std::collections::HashMap;
        let with = |state: f64| HashMap::from([("state".to_string(), state)]);
        assert_eq!(icons_from(None, false), Icons::Unknown);
        assert_eq!(icons_from(Some(&HashMap::new()), false), Icons::Missing);
        assert_eq!(icons_from(Some(&with(2.0)), false), Icons::Off);
        assert_eq!(icons_from(Some(&with(6.0)), false), Icons::Off);
        assert_eq!(icons_from(Some(&with(2.0)), true), Icons::Blocked);
        assert_eq!(icons_from(Some(&with(3.0)), false), Icons::Broken);
        assert_eq!(icons_from(Some(&with(4.0)), false), Icons::Broken);
        assert_eq!(serde_json::to_string(&Problem::Gnome { icons: Icons::Blocked }).unwrap(), r#"{"kind":"gnome","icons":"blocked"}"#);
    }

    #[test]
    fn login_items_name_their_program_with_any_letters() {
        for raw in ["/home/ada/.local/bin/scooch", "/home/ada/it's \"mine\"/$HOME/`x`/back\\slash/100%/a b/scooch"] {
            let path = Path::new(raw);
            let entry = crate::platform::linux::parse::entry::text(path);
            assert_eq!(entry_program(&entry).as_deref(), Some(path), "{entry}");
        }
        let plist = "<dict>\n\t<key>Label</key>\n\t<string>me.sohanbhat.scooch</string>\n\t<key>ProgramArguments</key>\n\t<array>\n\t\t<string>/Applications/R&amp;D &lt;1&gt;/Scooch.app/Contents/MacOS/scooch</string>\n\t\t<string>watch</string>\n\t</array>";
        assert_eq!(plist_program(plist).as_deref(), Some(Path::new("/Applications/R&D <1>/Scooch.app/Contents/MacOS/scooch")));
        assert_eq!(xml_text("a&#38;b&#x26;c&quot;").as_deref(), Some("a&b&c\""));
        assert_eq!(run_program("\"C:\\Users\\ada\\AppData\\Local\\Programs\\Scooch\\scoochw.exe\" watch").as_deref(), Some(Path::new("C:\\Users\\ada\\AppData\\Local\\Programs\\Scooch\\scoochw.exe")));
        assert_eq!(run_program("C:\\Scooch\\scoochw.exe watch").as_deref(), Some(Path::new("C:\\Scooch\\scoochw.exe")));
        assert_eq!(entry_program("[Desktop Entry]\nName=Scooch\n"), None);
    }

    #[test]
    fn places_show_only_while_on_and_only_when_saved() {
        let mut entry = Entry::new(Path::new("/p/a.jpg"));
        assert_eq!(places_of(Some(&entry), true), None);
        entry.places = Some(Places { width: 2880, height: 1800, icons: vec![Rect { x: 10.0, y: 20.0, w: 90.0, h: 90.0 }] });
        let places = places_of(Some(&entry), true).expect("places are read");
        assert_eq!((places.width, places.height, places.icons.len()), (2880, 1800, 1));
        assert_eq!(places_of(Some(&entry), false), None);
        assert_eq!(places_of(None, true), None);
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
