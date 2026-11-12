use crate::platform::{Desktop, Format, Screen, Snapshot};
use crate::state::{parse, same, write_atomic, Entry, State, Store};
use crate::{cache, poke, scene};
use anyhow::{Context, Result};
use scooch_core::{Options, Placement, Rect, RgbImage, Scene};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const QUIET: Duration = Duration::from_millis(250);
pub const LIMIT: Duration = Duration::from_secs(2);
pub const QUALITY: u8 = 95;

#[derive(Default)]
struct Pending {
    first: Option<Instant>,
    last: Option<Instant>,
}

#[derive(Default)]
pub struct Signal {
    pending: Mutex<Pending>,
    ready: Condvar,
}

impl Signal {
    pub fn wake(&self) {
        let now = Instant::now();
        let mut pending = self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        pending.first.get_or_insert(now);
        pending.last = Some(now);
        self.ready.notify_one();
    }

    pub fn settle(&self, quiet: Duration, limit: Duration) {
        let mut pending = self.pending.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        while pending.last.is_none() {
            pending = self.ready.wait(pending).unwrap_or_else(|poisoned| poisoned.into_inner());
        }
        while let (Some(first), Some(last)) = (pending.first, pending.last) {
            let due = (last + quiet).min(first + limit);
            let now = Instant::now();
            if now >= due {
                break;
            }
            pending = self.ready.wait_timeout(pending, due - now).unwrap_or_else(|poisoned| poisoned.into_inner()).0;
        }
        *pending = Pending::default();
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Outcome {
    Unchanged,
    Reapplied,
    Plain,
    Kept,
    Rendered,
    Skipped,
}

#[derive(Debug, Default)]
pub struct Report {
    pub outcomes: Vec<(String, Outcome)>,
    pub problem: Option<String>,
    pub retry: bool,
}

pub struct Daemon {
    pub desktop: Arc<dyn Desktop>,
    pub store: Store,
}

pub fn adoptable(entry: &Entry, current: &Path, ours: bool) -> bool {
    !ours && current.is_file() && !same(current, &entry.source)
}

pub const NOT_A_FILE: &str = "Scooch only moves picture files, so pick a photo as your wallpaper to use it again.";

pub fn trace() -> bool {
    std::env::var_os("SCOOCH_TRACE").is_some_and(|value| !value.is_empty() && value != "0")
}

pub fn stamp() -> String {
    let since = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    format!("{}.{:03}", since.as_secs(), since.subsec_millis())
}

impl Daemon {
    pub fn new(desktop: Arc<dyn Desktop>, store: Store) -> Self {
        Self { desktop, store }
    }

    pub fn tick(&self) -> Report {
        let before = self.store.read();
        let mut state = parse(before.as_deref());
        let mut report = Report::default();
        match self.desktop.snapshot() {
            Ok(snapshot) => self.pass(&mut state, &snapshot, &mut report),
            Err(error) => report.problem = Some(error.to_string()),
        }
        state.problem = report.problem.clone();
        if state == parse(before.as_deref()) {
            return report;
        }
        let now = self.store.read();
        if now != before {
            state = merge(&state, &parse(before.as_deref()), &parse(now.as_deref()));
            report.retry = true;
        }
        if let Err(error) = self.store.save(&state) {
            report.problem.get_or_insert(error.to_string());
        }
        report
    }

    fn pass(&self, state: &mut State, snapshot: &Snapshot, report: &mut Report) {
        let options = state.options;
        for screen in &snapshot.screens {
            let outcome = self.screen(state, screen, &snapshot.icons, &options);
            match outcome {
                Ok(outcome) => report.outcomes.push((screen.id.clone(), outcome)),
                Err(error) => {
                    report.outcomes.push((screen.id.clone(), Outcome::Skipped));
                    report.problem.get_or_insert(error.to_string());
                }
            }
        }
    }

    fn screen(&self, state: &mut State, screen: &Screen, icons: &[Rect], options: &Options) -> Result<Outcome> {
        let current = self.desktop.wallpaper(screen)?;
        let ours = current.as_deref().is_some_and(|path| self.store.owns(path));
        let usable = current.clone().filter(|path| path.is_file());
        if !state.screens.contains_key(&screen.id) {
            if ours {
                return Ok(Outcome::Skipped);
            }
            let Some(picture) = usable.as_deref() else { anyhow::bail!(NOT_A_FILE) };
            state.screens.insert(screen.id.clone(), Entry::new(picture));
        }
        let entry = state.screens.get_mut(&screen.id).context("the screen entry vanished")?;
        if let Some(picture) = usable.as_deref() {
            if adoptable(entry, picture, ours) {
                (entry.source, entry.original, entry.placement) = (picture.to_path_buf(), picture.to_path_buf(), None);
            }
        }
        if usable.is_none() && !ours {
            anyhow::bail!(NOT_A_FILE);
        }
        let scene = scene::scene(screen, icons);
        entry.icons = scene.icons.len();
        let fingerprint = scene::fingerprint(&entry.source, &scene, options);
        let showing = current.as_deref().is_some_and(|path| same(path, entry.shown()));
        if entry.fingerprint.as_deref() == Some(fingerprint.as_str()) {
            if showing && entry.shown().is_file() {
                return Ok(Outcome::Unchanged);
            }
            if entry.shown().is_file() {
                let shown = entry.shown().to_path_buf();
                self.show(screen, &shown)?;
                return Ok(Outcome::Reapplied);
            }
        }
        let outcome = self.place(entry, screen, &scene, options, current.as_deref())?;
        entry.fingerprint = Some(fingerprint);
        Ok(outcome)
    }

    fn place(&self, entry: &mut Entry, screen: &Screen, scene: &Scene, options: &Options, current: Option<&Path>) -> Result<Outcome> {
        let source = entry.source.clone();
        let mut image: Option<RgbImage> = None;
        let analysis = cache::analysis(&self.store, &source, || {
            let decoded = self.decode(&source)?;
            let made = scooch_core::analyze(&decoded);
            image = Some(decoded);
            Ok(made)
        })?;
        let placement = scooch_core::solve(&analysis, scene, options, entry.placement.as_ref());
        drop(analysis);
        let mut picture = None;
        if scene::is_identity(&placement) {
            if !scene.icons.is_empty() && options.pads > 0.0 {
                let decoded = match image.take() {
                    Some(decoded) => decoded,
                    None => self.decode(&source)?,
                };
                picture = padded(&decoded, &placement, scene, options);
            }
            if picture.is_none() {
                if !current.is_some_and(|path| same(path, &source)) {
                    self.show(screen, &source)?;
                }
                entry.output = None;
                entry.placement = Some(placement);
                self.store.sweep(&screen.id, None);
                return Ok(Outcome::Plain);
            }
        }
        let output = entry.output.clone().filter(|path| path.is_file());
        let showing_output = matches!((current, &output), (Some(current), Some(output)) if same(current, output));
        if picture.is_none() && options.pads <= 0.0 && showing_output && entry.placement.as_ref() == Some(&placement) {
            return Ok(Outcome::Kept);
        }
        let picture = match picture {
            Some(picture) => picture,
            None => {
                let decoded = match image.take() {
                    Some(decoded) => decoded,
                    None => self.decode(&source)?,
                };
                scooch_core::render(&decoded, &placement, scene, options)
            }
        };
        drop(image);
        let format = self.desktop.format();
        let bytes = encode(&picture, format)?;
        drop(picture);
        entry.placement = Some(placement);
        if let (true, Some(output)) = (showing_output, &output) {
            if std::fs::read(output).is_ok_and(|old| old == bytes) {
                return Ok(Outcome::Kept);
            }
        }
        let (counter, path) = self.fresh_name(&screen.id, entry.counter, format, current);
        std::fs::create_dir_all(self.store.out()).with_context(|| format!("Scooch could not make {}, so check the disk.", self.store.out().display()))?;
        write_atomic(&path, &bytes)?;
        drop(bytes);
        if let Err(error) = self.show(screen, &path) {
            let _ = std::fs::remove_file(&path);
            return Err(error);
        }
        self.store.sweep(&screen.id, Some(&path));
        entry.counter = counter;
        entry.output = Some(path);
        Ok(Outcome::Rendered)
    }

    fn fresh_name(&self, screen: &str, counter: u64, format: Format, current: Option<&Path>) -> (u64, PathBuf) {
        let mut next = counter + 1;
        loop {
            let path = self.store.output(screen, next, format.extension());
            if !path.exists() && !current.is_some_and(|current| same(current, &path)) {
                return (next, path);
            }
            next += 1;
        }
    }

    fn decode(&self, source: &Path) -> Result<RgbImage> {
        self.desktop.decode(source).with_context(|| format!("Scooch could not read {} as a picture, so pick another wallpaper.", source.display()))
    }

    fn show(&self, screen: &Screen, path: &Path) -> Result<()> {
        self.desktop.set_wallpaper(screen, path)?;
        if trace() {
            eprintln!("{} set {}", stamp(), path.display());
        }
        Ok(())
    }
}

pub fn merge(mine: &State, before: &State, fresh: &State) -> State {
    let mut merged = fresh.clone();
    merged.problem = mine.problem.clone();
    for (id, ours) in &mine.screens {
        let untouched = |theirs: &Entry| before.screens.get(id).is_none_or(|was| was.source == theirs.source && was.original == theirs.original);
        match merged.screens.get_mut(id) {
            Some(theirs) if untouched(theirs) => *theirs = ours.clone(),
            Some(theirs) => {
                (theirs.output, theirs.icons) = (ours.output.clone(), ours.icons);
                theirs.counter = theirs.counter.max(ours.counter);
            }
            None => {
                merged.screens.insert(id.clone(), ours.clone());
            }
        }
    }
    merged
}

fn padded(image: &RgbImage, placement: &Placement, scene: &Scene, options: &Options) -> Option<RgbImage> {
    let with = scooch_core::render(image, placement, scene, options);
    let without = scooch_core::render(image, placement, scene, &Options { pads: 0.0, ..*options });
    (with != without).then_some(with)
}

pub fn encode(picture: &RgbImage, format: Format) -> Result<Vec<u8>> {
    Ok(match format {
        Format::Jpeg => scooch_core::encode_jpeg(picture, QUALITY)?,
        Format::Png => scooch_core::encode_png(picture)?,
    })
}

pub fn watch(desktop: Arc<dyn Desktop>, store: Store) -> Result<()> {
    let signal = Arc::new(Signal::default());
    let daemon = Daemon::new(desktop.clone(), store);
    let worker = signal.clone();
    let tracing = trace();
    std::thread::Builder::new().name("scooch-tick".into()).spawn(move || {
        let mut said: Option<String> = None;
        loop {
            worker.settle(QUIET, LIMIT);
            let started = Instant::now();
            let report = daemon.tick();
            if tracing {
                eprintln!("{} tick {} ms {:?}", stamp(), started.elapsed().as_millis(), report.outcomes);
            }
            if report.problem != said {
                if let Some(problem) = &report.problem {
                    eprintln!("{problem}");
                }
                said = report.problem;
            }
            if report.retry {
                worker.wake();
            }
        }
    })?;
    signal.wake();
    let poked = signal.clone();
    poke::listen(move || poked.wake())?;
    desktop.run(Box::new(move || signal.wake()))
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::platform::Wake;
    use crate::state::scratch;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    pub struct Fake {
        pub screens: Vec<Screen>,
        pub icons: Mutex<Vec<Rect>>,
        pub shown: Mutex<HashMap<String, PathBuf>>,
        pub sets: AtomicUsize,
        pub decodes: AtomicUsize,
        pub snapshots: AtomicUsize,
        pub hook: Mutex<Option<Box<dyn FnOnce() + Send>>>,
    }

    impl Fake {
        pub fn new(screens: Vec<Screen>, picture: &Path) -> Arc<Self> {
            let shown = screens.iter().map(|screen| (screen.id.clone(), picture.to_path_buf())).collect();
            Arc::new(Self { screens, icons: Mutex::new(Vec::new()), shown: Mutex::new(shown), sets: AtomicUsize::new(0), decodes: AtomicUsize::new(0), snapshots: AtomicUsize::new(0), hook: Mutex::new(None) })
        }

        pub fn showing(&self, screen: &str) -> PathBuf {
            self.shown.lock().unwrap()[screen].clone()
        }

        pub fn show(&self, screen: &str, picture: &Path) {
            self.shown.lock().unwrap().insert(screen.into(), picture.to_path_buf());
        }

        pub fn place(&self, icons: Vec<Rect>) {
            *self.icons.lock().unwrap() = icons;
        }

        pub fn during(&self, work: impl FnOnce() + Send + 'static) {
            *self.hook.lock().unwrap() = Some(Box::new(work));
        }
    }

    impl Desktop for Fake {
        fn name(&self) -> String {
            "fake".into()
        }

        fn snapshot(&self) -> Result<Snapshot> {
            self.snapshots.fetch_add(1, Ordering::SeqCst);
            if let Some(work) = self.hook.lock().unwrap().take() {
                work();
            }
            Ok(Snapshot { screens: self.screens.clone(), icons: self.icons.lock().unwrap().clone() })
        }

        fn wallpaper(&self, screen: &Screen) -> Result<Option<PathBuf>> {
            Ok(self.shown.lock().unwrap().get(&screen.id).cloned())
        }

        fn set_wallpaper(&self, screen: &Screen, path: &Path) -> Result<()> {
            self.sets.fetch_add(1, Ordering::SeqCst);
            self.show(&screen.id, path);
            Ok(())
        }

        fn run(&self, _wake: Wake) -> Result<()> {
            Ok(())
        }

        fn autostart(&self, _exe: Option<&Path>) -> Result<()> {
            Ok(())
        }

        fn check(&self) -> Vec<(String, Result<String>)> {
            Vec::new()
        }

        fn format(&self) -> Format {
            Format::Jpeg
        }

        fn decode(&self, path: &Path) -> Result<RgbImage> {
            self.decodes.fetch_add(1, Ordering::SeqCst);
            Ok(scooch_core::open(path)?)
        }
    }

    pub fn screen(id: &str, x: f32, scale: f32) -> Screen {
        Screen { id: id.into(), frame: Rect { x, y: 0.0, w: 160.0, h: 100.0 }, scale }
    }

    pub fn picture(folder: &Path, name: &str) -> PathBuf {
        let path = folder.join(name);
        std::fs::write(&path, scooch_core::encode_png(&scene::sample(480, 100)).unwrap()).unwrap();
        path
    }

    pub fn rig(name: &str, screens: Vec<Screen>) -> (Daemon, Arc<Fake>, PathBuf) {
        let folder = scratch(name);
        let original = picture(&folder, "original.png");
        let fake = Fake::new(screens, &original);
        let daemon = Daemon::new(fake.clone(), Store::at(folder.join("data")));
        let mut state = State { on: true, ..State::default() };
        state.options.pads = 0.0;
        daemon.store.save(&state).unwrap();
        (daemon, fake, original)
    }

    fn subject() -> Rect {
        Rect { x: 60.0, y: 30.0, w: 40.0, h: 40.0 }
    }

    fn corner() -> Rect {
        Rect { x: 0.0, y: 0.0, w: 20.0, h: 20.0 }
    }

    fn outcome(report: &Report) -> Outcome {
        assert!(report.problem.is_none(), "{:?}", report.problem);
        report.outcomes[0].1
    }

    #[test]
    fn no_icons_show_the_picture_itself() {
        let (daemon, fake, original) = rig("plain", vec![screen("main", 0.0, 1.0)]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.source.as_path(), entry.original.as_path(), entry.output.as_deref()), (original.as_path(), original.as_path(), None));
        assert!(entry.fingerprint.is_some());
        assert_eq!(fake.showing("main"), original);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn an_icon_over_the_subject_renders_a_new_picture() {
        let (daemon, fake, _) = rig("render", vec![screen("main", 0.0, 2.0)]);
        fake.place(vec![subject()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        let output = daemon.store.output("main", 1, "jpg");
        assert_eq!(entry.output.as_deref(), Some(output.as_path()));
        assert_eq!((entry.counter, entry.icons), (1, 1));
        assert_eq!(fake.showing("main"), output);
        let written = scooch_core::open(&output).unwrap();
        assert_eq!(written.dimensions(), (320, 200));
    }

    #[test]
    fn nothing_changed_means_nothing_happens() {
        let (daemon, fake, _) = rig("idle", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let saved = daemon.store.read();
        assert_eq!(outcome(&daemon.tick()), Outcome::Unchanged);
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
        assert_eq!(daemon.store.read(), saved);
    }

    #[test]
    fn a_moved_icon_draws_again_and_drops_the_old_output() {
        let (daemon, fake, _) = rig("moved", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.place(vec![corner()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        assert!(!daemon.store.output("main", 1, "jpg").exists());
        fake.place(vec![subject(), corner()]);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!((entry.icons, entry.counter), (2, 2));
        let files: Vec<_> = std::fs::read_dir(daemon.store.out()).unwrap().flatten().map(|file| file.path()).collect();
        assert_eq!(files, vec![daemon.store.output("main", 2, "jpg")]);
    }

    #[test]
    fn a_new_wallpaper_from_the_user_becomes_source_and_original() {
        let (daemon, fake, original) = rig("adopt", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        let chosen = picture(original.parent().unwrap(), "chosen.png");
        fake.show("main", &chosen);
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!(entry.source, chosen);
        assert_eq!(entry.original, chosen);
        assert_eq!(fake.showing("main"), daemon.store.output("main", 2, "jpg"));
    }

    #[test]
    fn a_space_showing_the_old_picture_gets_the_output_without_drawing() {
        let (daemon, fake, original) = rig("space", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        fake.show("main", &original);
        assert_eq!(outcome(&daemon.tick()), Outcome::Reapplied);
        assert_eq!(fake.decodes.load(Ordering::SeqCst), 1);
        assert_eq!(fake.showing("main"), daemon.store.output("main", 1, "jpg"));
        assert_eq!(daemon.store.load().screens["main"].source, original);
    }

    #[test]
    fn a_picture_chosen_with_use_is_not_undone_by_adoption() {
        let (daemon, fake, original) = rig("use", vec![screen("main", 0.0, 1.0)]);
        daemon.tick();
        let wanted = picture(original.parent().unwrap(), "wanted.png");
        let mut state = daemon.store.load();
        let entry = state.screens.get_mut("main").unwrap();
        (entry.source, entry.fingerprint, entry.placement) = (wanted.clone(), None, None);
        daemon.store.save(&state).unwrap();
        fake.show("main", &wanted);
        assert_eq!(outcome(&daemon.tick()), Outcome::Plain);
        let entry = daemon.store.load().screens["main"].clone();
        assert_eq!(entry.source, wanted);
        assert_eq!(entry.original, original);
        assert_eq!(fake.showing("main"), wanted);
    }

    #[test]
    fn a_lost_output_is_drawn_again() {
        let (daemon, fake, _) = rig("lost", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        daemon.tick();
        std::fs::remove_file(daemon.store.output("main", 1, "jpg")).unwrap();
        assert_eq!(outcome(&daemon.tick()), Outcome::Rendered);
        assert_eq!(fake.showing("main"), daemon.store.output("main", 2, "jpg"));
    }

    #[test]
    fn two_screens_with_mixed_scales_get_their_own_icons_and_outputs() {
        let (daemon, fake, _) = rig("two", vec![screen("left", 0.0, 2.0), screen("right", 160.0, 1.0)]);
        let right = Rect { x: 160.0 + 60.0, ..subject() };
        fake.place(vec![subject(), right, Rect { x: 300.0, y: 80.0, w: 10.0, h: 10.0 }]);
        let report = daemon.tick();
        assert_eq!(report.outcomes.len(), 2);
        let state = daemon.store.load();
        assert_eq!((state.screens["left"].icons, state.screens["right"].icons), (1, 2));
        assert_eq!(fake.showing("left"), daemon.store.output("left", 1, "jpg"));
        assert_eq!(fake.showing("right"), daemon.store.output("right", 1, "jpg"));
        assert_eq!(scooch_core::open(&fake.showing("left")).unwrap().dimensions(), (320, 200));
        assert_eq!(scooch_core::open(&fake.showing("right")).unwrap().dimensions(), (160, 100));
    }

    #[test]
    fn a_wallpaper_that_is_not_a_file_is_left_alone() {
        let (daemon, fake, original) = rig("folder", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        fake.show("main", original.parent().unwrap());
        assert_eq!(daemon.tick().outcomes[0].1, Outcome::Skipped);
        assert!(daemon.store.load().screens.is_empty());
        fake.show("main", &original);
        daemon.tick();
        fake.show("main", original.parent().unwrap());
        let report = daemon.tick();
        assert_eq!(report.problem.as_deref(), Some(NOT_A_FILE));
        assert_eq!(fake.sets.load(Ordering::SeqCst), 1);
        assert_eq!(fake.showing("main"), original.parent().unwrap());
    }

    #[test]
    fn an_unreadable_picture_is_reported_and_kept_out_of_the_way() {
        let (daemon, fake, original) = rig("unreadable", vec![screen("main", 0.0, 1.0)]);
        let broken = original.parent().unwrap().join("broken.png");
        std::fs::write(&broken, b"not a picture").unwrap();
        fake.show("main", &broken);
        let report = daemon.tick();
        assert!(report.problem.as_deref().is_some_and(|problem| problem.ends_with('.') && !problem.contains(": ")));
        assert_eq!(daemon.store.load().problem, report.problem);
        fake.show("main", &original);
        let mut state = daemon.store.load();
        state.screens.clear();
        daemon.store.save(&state).unwrap();
        assert!(daemon.tick().problem.is_none());
        assert_eq!(daemon.store.load().problem, None);
    }

    #[test]
    fn a_state_edited_during_a_tick_is_merged_not_overwritten() {
        let (daemon, fake, _) = rig("race", vec![screen("main", 0.0, 1.0)]);
        fake.place(vec![subject()]);
        let store = daemon.store.clone();
        fake.during(move || {
            let mut edited = store.load();
            edited.options.strength = 0.5;
            store.save(&edited).unwrap();
        });
        assert!(daemon.tick().retry);
        let state = daemon.store.load();
        assert_eq!(state.options.strength, 0.5);
        assert_eq!(state.screens["main"].output.as_ref(), Some(&fake.showing("main")));
        assert!(!daemon.tick().retry);
        assert_eq!(daemon.store.load().screens["main"].output.as_ref(), Some(&fake.showing("main")));
    }

    #[test]
    fn a_picture_switched_during_a_tick_survives_the_merge() {
        let mut mine = State { on: true, ..State::default() };
        mine.screens.insert("main".into(), Entry { output: Some("/d/out/main-4.jpg".into()), counter: 4, fingerprint: Some("f".into()), icons: 2, ..Entry::new(Path::new("/p/old.jpg")) });
        let mut before = State { on: true, ..State::default() };
        before.screens.insert("main".into(), Entry { counter: 3, ..Entry::new(Path::new("/p/old.jpg")) });
        let mut fresh = before.clone();
        fresh.screens.get_mut("main").unwrap().source = "/p/new.jpg".into();
        let merged = merge(&mine, &before, &fresh);
        let entry = &merged.screens["main"];
        assert_eq!(entry.source, PathBuf::from("/p/new.jpg"));
        assert_eq!(entry.fingerprint, None);
        assert_eq!((entry.counter, entry.output.clone()), (4, Some(PathBuf::from("/d/out/main-4.jpg"))));
        let adopted = State { screens: [("main".to_string(), Entry::new(Path::new("/p/picked.jpg")))].into(), ..mine.clone() };
        let options = State { options: Options { strength: 0.5, ..Options::default() }, ..before.clone() };
        let merged = merge(&adopted, &before, &options);
        assert_eq!(merged.screens["main"].source, PathBuf::from("/p/picked.jpg"));
        assert_eq!(merged.options.strength, 0.5);
    }

    #[test]
    fn adoption_ignores_outputs_and_fresh_sources() {
        let folder = scratch("adoptable");
        let picture = folder.join("a.jpg");
        std::fs::write(&picture, b"x").unwrap();
        let mut entry = Entry::new(&folder.join("b.jpg"));
        assert!(adoptable(&entry, &picture, false));
        assert!(!adoptable(&entry, &picture, true));
        assert!(!adoptable(&entry, &folder, false));
        entry.source = picture.clone();
        assert!(!adoptable(&entry, &picture, false));
    }

    #[test]
    fn settle_turns_a_burst_into_one_tick() {
        let signal = Signal::default();
        for _ in 0..50 {
            signal.wake();
        }
        let started = Instant::now();
        signal.settle(Duration::from_millis(40), Duration::from_secs(5));
        assert!(started.elapsed() >= Duration::from_millis(30));
        assert!(signal.pending.lock().unwrap().last.is_none());
    }

    #[test]
    fn settle_waits_for_quiet() {
        let signal = Arc::new(Signal::default());
        let feeder = signal.clone();
        let started = Instant::now();
        let feeding = std::thread::spawn(move || {
            for _ in 0..5 {
                feeder.wake();
                std::thread::sleep(Duration::from_millis(30));
            }
        });
        signal.settle(Duration::from_millis(120), Duration::from_secs(5));
        feeding.join().unwrap();
        assert!(started.elapsed() >= Duration::from_millis(120 + 4 * 30 - 10), "{:?}", started.elapsed());
    }

    #[test]
    fn settle_stops_waiting_at_the_limit() {
        let signal = Arc::new(Signal::default());
        let feeder = signal.clone();
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = done.clone();
        let feeding = std::thread::spawn(move || {
            while !stop.load(Ordering::SeqCst) {
                feeder.wake();
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        let started = Instant::now();
        signal.settle(Duration::from_millis(100), Duration::from_millis(300));
        let waited = started.elapsed();
        done.store(true, Ordering::SeqCst);
        feeding.join().unwrap();
        assert!(waited >= Duration::from_millis(290) && waited < Duration::from_millis(600), "{waited:?}");
    }

    #[test]
    fn settle_sleeps_until_the_first_wake() {
        let signal = Arc::new(Signal::default());
        let waiter = signal.clone();
        let started = Instant::now();
        let waiting = std::thread::spawn(move || {
            waiter.settle(Duration::from_millis(20), Duration::from_secs(1));
            started.elapsed()
        });
        std::thread::sleep(Duration::from_millis(150));
        signal.wake();
        let waited = waiting.join().unwrap();
        assert!(waited >= Duration::from_millis(170), "{waited:?}");
    }

    #[cfg(unix)]
    #[test]
    fn a_poke_from_another_process_wakes_the_worker() {
        let signal = Arc::new(Signal::default());
        let poked = signal.clone();
        poke::listen(move || poked.wake()).unwrap();
        assert!(poke::send(std::process::id()));
        let started = Instant::now();
        signal.settle(Duration::from_millis(10), Duration::from_secs(1));
        assert!(started.elapsed() < Duration::from_secs(1));
    }
}
