use super::parse::desktop::Session;
use super::parse::wakes;
use super::screens::{self, Shared};
use super::{bus, set_recently, Signal, Watches};
use crate::platform::Wake;
use zbus::zvariant;
use anyhow::{Context, Result};
use inotify::{Inotify, WatchDescriptor, WatchMask};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

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
    files(&wake, &watches, desktop)?;
    for signal in watches.signals {
        listen(signal, &wake);
    }
    screens::watch(session, tagged(&wake, "screens"));
    loop {
        std::thread::park();
    }
}

enum Target {
    Desktop,
    File(fn(&str) -> bool, bool),
}

fn files(wake: &Shared, watches: &Watches, desktop: &Path) -> Result<()> {
    let mut inotify = Inotify::init().context("Scooch could not watch files, so check the inotify limits.")?;
    let mut targets: HashMap<WatchDescriptor, Vec<Target>> = HashMap::new();
    let entries = WatchMask::CREATE | WatchMask::DELETE | WatchMask::MOVED_FROM | WatchMask::MOVED_TO;
    if let Ok(descriptor) = inotify.watches().add(desktop, entries) {
        targets.entry(descriptor).or_default().push(Target::Desktop);
    }
    for watch in &watches.files {
        let _ = std::fs::create_dir_all(&watch.folder);
        if let Ok(descriptor) = inotify.watches().add(&watch.folder, WatchMask::CLOSE_WRITE | WatchMask::MOVED_TO | WatchMask::CREATE) {
            targets.entry(descriptor).or_default().push(Target::File(watch.matches, watch.wallpaper));
        }
    }
    let on_desktop = tagged(wake, "desktop");
    let on_file = tagged(wake, "settings file");
    std::thread::Builder::new().name("scooch-files".into()).spawn(move || {
        let mut buffer = [0u8; 8192];
        loop {
            let Ok(events) = inotify.read_events_blocking(&mut buffer) else { break };
            let (mut desktop_hit, mut file_hit) = (false, false);
            for event in events {
                let Some(name) = event.name.and_then(|name| name.to_str()) else { continue };
                for target in targets.get(&event.wd).map(Vec::as_slice).unwrap_or_default() {
                    match target {
                        Target::Desktop => desktop_hit |= wakes::desktop_entry(name),
                        Target::File(matches, wallpaper) => file_hit |= matches(name) && !(*wallpaper && set_recently()),
                    }
                }
            }
            if desktop_hit {
                on_desktop();
            }
            if file_hit {
                on_file();
            }
        }
    })?;
    Ok(())
}

fn listen(signal: Signal, wake: &Shared) {
    let (rule, name): (&str, &'static str) = match &signal {
        Signal::Metadata(_) => (wakes::GVFS_RULE, "metadata"),
        Signal::Dconf(_) => (wakes::DCONF_RULE, "dconf"),
        Signal::Ding => (wakes::DING_RULE, "ding geometry"),
        Signal::Xfconf => (wakes::XFCONF_RULE, "xfconf"),
    };
    let wake = tagged(wake, name);
    let rule = rule.to_string();
    let _ = std::thread::Builder::new().name(format!("scooch-{name}")).spawn(move || {
        let Ok(connection) = bus::session() else { return };
        let Ok(messages) = zbus::blocking::MessageIterator::for_match_rule(rule.as_str(), &connection, Some(256)) else { return };
        for message in messages.flatten() {
            if wanted(&signal, &message) {
                wake();
            }
        }
    });
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
