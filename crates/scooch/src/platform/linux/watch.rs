use super::parse::desktop::Session;
use super::parse::wakes::{self, Spot};
use super::screens::{self, Shared};
use super::{set_recently, Signal, Watches};
use crate::platform::Wake;
use anyhow::{anyhow, Result};
use inotify::{EventMask, Inotify, WatchDescriptor, WatchMask};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use zbus::zvariant;

fn tagged(wake: &Shared, name: &'static str) -> Shared {
    let wake = wake.clone();
    if crate::daemon::trace() {
        Arc::new(move || {
            eprintln!("{} wake {name}", crate::daemon::stamp());
            wake()
        })
    } else {
        wake
    }
}

pub fn run(wake: Wake, watches: Watches, desktop: &Path, session: Session) -> Result<()> {
    let wake: Shared = Arc::from(wake);
    let (gave_up, lost) = channel::<&'static str>();
    let mut targets = vec![Target { path: desktop.to_path_buf(), kind: Kind::Desktop }];
    targets.extend(watches.files.iter().map(|watch| Target { path: watch.folder.clone(), kind: Kind::File(watch.matches, watch.wallpaper) }));
    let files_wake = wake.clone();
    keep("files", gave_up.clone(), move || files(&files_wake, &targets));
    for signal in watches.signals {
        let name = signal_name(&signal);
        let wake = tagged(&wake, name);
        keep(name, gave_up.clone(), move || listen(&signal, &wake));
    }
    let screen_wake = tagged(&wake, "screens");
    keep("screens", gave_up.clone(), move || screens::watch_once(session, &screen_wake));
    drop(gave_up);
    match lost.recv() {
        Ok(name) => Err(anyhow!("Scooch lost the {name} events it waits on, so start it again with scooch.")),
        Err(_) => Err(anyhow!("Scooch lost every event it waits on, so start it again with scooch.")),
    }
}

fn keep(name: &'static str, gave_up: Sender<&'static str>, mut source: impl FnMut() -> bool + Send + 'static) {
    let fallback = gave_up.clone();
    let spawned = std::thread::Builder::new().name(format!("scooch-{name}")).spawn(move || {
        let mut failures = 0usize;
        loop {
            if source() {
                failures = 0;
            }
            match wakes::retry_delay(failures) {
                Some(pause) => {
                    failures += 1;
                    std::thread::sleep(pause);
                }
                None => {
                    let _ = gave_up.send(name);
                    return;
                }
            }
        }
    });
    if spawned.is_err() {
        let _ = fallback.send(name);
    }
}

#[derive(Clone, Copy)]
enum Kind {
    Desktop,
    File(fn(&str) -> bool, bool),
}

struct Target {
    path: PathBuf,
    kind: Kind,
}

struct Armed {
    target: usize,
    spot: Spot,
}

const ENTRIES: WatchMask = WatchMask::CREATE.union(WatchMask::DELETE).union(WatchMask::MOVED_FROM).union(WatchMask::MOVED_TO);
const SELF: WatchMask = WatchMask::DELETE_SELF.union(WatchMask::MOVE_SELF);

fn mask_for(kind: Kind, waiting: bool) -> WatchMask {
    match (kind, waiting) {
        (_, true) => WatchMask::CREATE | WatchMask::MOVED_TO | SELF,
        (Kind::Desktop, false) => ENTRIES | SELF,
        (Kind::File(..), false) => WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE | SELF,
    }
}

fn arm(inotify: &mut Inotify, targets: &[Target], armed: &mut HashMap<WatchDescriptor, Vec<Armed>>) {
    for (descriptor, _) in armed.drain() {
        let _ = inotify.watches().remove(descriptor);
    }
    let mut masks: HashMap<PathBuf, WatchMask> = HashMap::new();
    let mut spots: Vec<(usize, Spot)> = Vec::new();
    for (index, target) in targets.iter().enumerate() {
        let Some(spot) = wakes::nearest(&target.path, |path| path.is_dir()) else { continue };
        *masks.entry(spot.folder.clone()).or_insert(WatchMask::empty()) |= mask_for(target.kind, spot.waiting.is_some());
        spots.push((index, spot));
    }
    let mut descriptors: HashMap<PathBuf, WatchDescriptor> = HashMap::new();
    for (folder, mask) in masks {
        if let Ok(descriptor) = inotify.watches().add(&folder, mask) {
            descriptors.insert(folder, descriptor);
        }
    }
    for (target, spot) in spots {
        if let Some(descriptor) = descriptors.get(&spot.folder) {
            armed.entry(descriptor.clone()).or_default().push(Armed { target, spot });
        }
    }
}

fn files(wake: &Shared, targets: &[Target]) -> bool {
    let Ok(mut inotify) = Inotify::init() else { return false };
    let mut armed: HashMap<WatchDescriptor, Vec<Armed>> = HashMap::new();
    arm(&mut inotify, targets, &mut armed);
    let on_desktop = tagged(wake, "desktop");
    let on_file = tagged(wake, "settings file");
    let mut buffer = [0u8; 8192];
    loop {
        let Ok(events) = inotify.read_events_blocking(&mut buffer) else { return true };
        let (mut desktop_hit, mut file_hit, mut rearm) = (false, false, false);
        for event in events {
            if event.mask.contains(EventMask::Q_OVERFLOW) {
                (desktop_hit, file_hit, rearm) = (true, true, true);
                continue;
            }
            let Some(list) = armed.get(&event.wd) else { continue };
            if event.mask.intersects(EventMask::IGNORED | EventMask::DELETE_SELF | EventMask::MOVE_SELF) {
                for each in list {
                    match targets[each.target].kind {
                        Kind::Desktop => desktop_hit = true,
                        Kind::File(..) => file_hit = true,
                    }
                }
                rearm = true;
                continue;
            }
            for each in list {
                let target = &targets[each.target];
                if let Some(waiting) = &each.spot.waiting {
                    if event.name.is_some_and(|name| name == waiting.as_os_str()) {
                        rearm = true;
                        match target.kind {
                            Kind::Desktop => desktop_hit = true,
                            Kind::File(..) => file_hit = true,
                        }
                    }
                    continue;
                }
                match (target.kind, event.name.map(|name| name.to_str())) {
                    (Kind::Desktop, Some(Some(name))) => desktop_hit |= wakes::desktop_entry(name),
                    (Kind::Desktop, Some(None)) => desktop_hit = true,
                    (Kind::File(matches, wallpaper), Some(Some(name))) => file_hit |= matches(name) && !(wallpaper && set_recently()),
                    _ => {}
                }
            }
        }
        if rearm {
            arm(&mut inotify, targets, &mut armed);
        }
        if desktop_hit {
            on_desktop();
        }
        if file_hit {
            on_file();
        }
    }
}

fn signal_name(signal: &Signal) -> &'static str {
    match signal {
        Signal::Metadata(_) => "metadata",
        Signal::Dconf(_) => "dconf",
        Signal::Ding => "ding geometry",
        Signal::Xfconf => "xfconf",
    }
}

fn rule(signal: &Signal) -> &'static str {
    match signal {
        Signal::Metadata(_) => wakes::GVFS_RULE,
        Signal::Dconf(_) => wakes::DCONF_RULE,
        Signal::Ding => wakes::DING_RULE,
        Signal::Xfconf => wakes::XFCONF_RULE,
    }
}

fn listen(signal: &Signal, wake: &Shared) -> bool {
    let Ok(connection) = zbus::blocking::Connection::session() else { return false };
    let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule(signal), &connection, Some(256)) else { return false };
    for message in messages {
        let Ok(message) = message else { break };
        if wanted(signal, &message) {
            wake();
        }
    }
    true
}

pub fn wanted(signal: &Signal, message: &zbus::Message) -> bool {
    let body = message.body();
    match signal {
        Signal::Metadata(desktop) => body.deserialize::<(String, String)>().is_ok_and(|(tree, path)| wakes::metadata(&tree, &path, desktop.as_deref())),
        Signal::Dconf(keys) => !set_recently() && body.deserialize::<(String, Vec<String>, String)>().is_ok_and(|(prefix, changes, _)| wakes::dconf(&prefix, &changes, keys)),
        Signal::Ding => true,
        Signal::Xfconf => body.deserialize::<(String, String, zvariant::OwnedValue)>().is_ok_and(|(channel, property, _)| wakes::xfconf(&channel, &property) && !(wakes::wallpaper_property(&channel, &property) && set_recently())),
    }
}
