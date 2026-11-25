use super::{line, Command, Reply, VERSION};
use std::io::Write;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const IDLE_EXIT: Duration = Duration::from_secs(30);
pub const LAYER_IDLE: Duration = Duration::from_secs(5);

pub type Waker = Arc<dyn Fn() + Send + Sync>;
pub type Commit = Box<dyn Fn() + Send>;

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn say(out: &Out, reply: &Reply) {
    let text = line(reply).unwrap_or_else(|error| line(&Reply::Failed { id: None, why: format!("an answer could not be written {error:#}") }).unwrap_or_default());
    if let Ok(mut out) = out.lock() {
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
}

pub fn read(text: &str) -> Result<Command, Box<Reply>> {
    serde_json::from_str::<Command>(text).map_err(|error| Box::new(Reply::Failed { id: None, why: format!("unreadable command {error}") }))
}

pub fn refuse(why: &str) -> ExitCode {
    let out: Out = Arc::new(Mutex::new(Box::new(std::io::stdout())));
    say(&out, &Reply::Hello { version: VERSION, pid: std::process::id() });
    say(&out, &Reply::Failed { id: None, why: why.to_string() });
    ExitCode::from(1)
}

pub fn main() -> ExitCode {
    #[cfg(windows)]
    return crate::platform::windows::layer::serve();
    #[cfg(target_os = "macos")]
    return crate::platform::macos::host::serve();
    #[cfg(not(any(windows, target_os = "macos")))]
    refuse("this system has no live layer yet, so Scooch keeps the plain swap")
}

#[cfg(any(windows, target_os = "macos"))]
pub use running::{serve, Build, Platform};

#[cfg(any(windows, target_os = "macos"))]
mod running {
    use super::{read, say, Commit, Out, Waker, IDLE_EXIT, LAYER_IDLE};
    use crate::live::{warm_failed, Command, Hints, Reply, Stills, VERSION};
    use crate::platform::Screen;
    use anyhow::Result;
    use scooch_core::{Bundle, RgbImage};
    use scooch_live::{Glide, Gpu, Look, Picture, Side, Still, Target, Took};
    use std::collections::{BTreeMap, VecDeque};
    use std::io::BufRead;
    use std::path::{Path, PathBuf};
    use std::process::ExitCode;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const TICK: Duration = Duration::from_millis(50);
    const STOP_WAIT: Duration = Duration::from_secs(2);
    const LONGEST: u32 = 10_000;
    const LEAD_IN: Duration = Duration::from_millis(80);
    const SETTLE: u32 = 100;
    const FLUSH: usize = 2;
    const COMPOSE_WAIT: Duration = Duration::from_secs(1);
    const POLL: Duration = Duration::from_millis(10);
    const REST: Duration = Duration::from_secs(1 << 20);
    const STILLS_KEPT: usize = 2;
    const PRIMED: u32 = 1 << 31;
    const TIMED_FRAMES: usize = 5;
    const WARM_LIMIT: Duration = Duration::from_secs(60);
    const STUCK: &str = "the GPU device or the picture did not come up within 60 s, so this helper makes no more";

    pub enum Build<M, L> {
        Partly(M, &'static str),
        Whole(L, &'static str),
    }

    pub trait Platform: 'static {
        type Layer;
        type Making;
        fn composition(&self, hints: &Hints) -> Result<bool>;
        fn make(&mut self, screen: &Screen, hints: &Hints, making: Option<Self::Making>) -> Result<Build<Self::Making, Self::Layer>>;
        fn present(&mut self, layer: &Self::Layer) -> Result<()>;
        fn visible(&mut self, _layer: &Self::Layer, _waited: Duration) -> Result<bool> {
            Ok(true)
        }
        fn target(&self, layer: &Self::Layer) -> Target;
        fn pixels(&self, layer: &Self::Layer) -> (u32, u32);
        fn period(&self, _layer: &Self::Layer) -> Duration {
            Duration::from_micros(16_667)
        }
        fn refresh(&self) -> Duration {
            Duration::from_micros(16_667)
        }
        fn show(&mut self, layer: &Self::Layer) -> Result<()>;
        fn revealed(&mut self) -> Vec<(&'static str, u64)> {
            Vec::new()
        }
        fn committer(&self) -> Option<Commit> {
            None
        }
        fn screen_gpu(&self, _layer: &Self::Layer) -> Option<String> {
            None
        }
        fn hide(&mut self, _layer: &Self::Layer) {}
        fn room(&mut self, _screen: &Screen, _layer: &Self::Layer) -> Result<bool> {
            Ok(true)
        }
        fn close(&mut self, layer: Self::Layer);
        fn abandon(&mut self, layer: Self::Layer) {
            std::mem::forget(layer);
        }
        fn wait(&mut self, timeout: Duration);
        fn waker(&self) -> Waker;
        fn decode(path: &Path) -> Result<RgbImage>
        where
            Self: Sized;
        fn frame<T>(work: impl FnOnce() -> T) -> T
        where
            Self: Sized,
        {
            work()
        }
        fn composed()
        where
            Self: Sized,
        {
        }
        fn prepare(&mut self) {}
        fn rest(&mut self) {}
        fn begin(&mut self) {}
    }

    pub fn ease(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    struct Surface(Target);

    unsafe impl Send for Surface {}

    const KEPT: usize = 4;

    #[derive(Clone)]
    struct Loaded {
        print: u64,
        picture: Picture,
    }

    type Key = (PathBuf, [u32; 2], u64);

    struct Want {
        key: Option<Key>,
        canvas: Option<PathBuf>,
        waiters: Vec<Option<u32>>,
        composition: bool,
        begun: Option<Instant>,
    }

    enum Image {
        Rgb(RgbImage),
        Rgba(crate::live::still::Raw),
    }

    type Fetched = Result<(Image, &'static str), String>;

    enum Source {
        Kept(Arc<Still>),
        Loading(std::thread::JoinHandle<Fetched>),
        Here(Stills, fn(&Path) -> Result<RgbImage>),
        Missing,
    }

    fn fetch(stills: &Stills, decode: fn(&Path) -> Result<RgbImage>) -> Fetched {
        let Some(still) = stills.still.as_deref() else { return Err("open needs the still the system shows now".into()) };
        if let Some(raw) = stills.raw.as_deref().and_then(|raw| crate::live::still::read(raw).ok()) {
            return Ok((Image::Rgba(raw), "raw"));
        }
        decode(still).map(|image| (Image::Rgb(image), "decoded")).map_err(|error| format!("the still {} could not be read {error:#}", still.display()))
    }

    impl Source {
        fn start(stills: &Stills, kept: Option<Arc<Still>>, decode: fn(&Path) -> Result<RgbImage>) -> Source {
            if let Some(still) = kept {
                return Source::Kept(still);
            }
            if stills.still.is_none() {
                return Source::Missing;
            }
            let wanted = stills.clone();
            match std::thread::Builder::new().name("scooch-live-still".into()).spawn(move || fetch(&wanted, decode)) {
                Ok(handle) => Source::Loading(handle),
                Err(_) => Source::Here(stills.clone(), decode),
            }
        }

        fn finish(self, gpu: &Gpu) -> Result<(Arc<Still>, &'static str), String> {
            let fetched = match self {
                Source::Kept(still) => return Ok((still, "kept")),
                Source::Missing => return Err("open needs the still the system shows now".into()),
                Source::Loading(handle) => handle.join().unwrap_or_else(|_| Err("the still could not be read".into())),
                Source::Here(stills, decode) => fetch(&stills, decode),
            };
            let (image, from) = fetched?;
            let still = match image {
                Image::Rgb(image) => gpu.still(&image)?,
                Image::Rgba(raw) => gpu.still_rgba(raw.width, raw.height, raw.rgba())?,
            };
            Ok((Arc::new(still), from))
        }
    }

    enum Job {
        Glide { a: PathBuf, b: PathBuf, ms: u32, loaded: Vec<Loaded>, asked: Instant, scale: f32 },
        Settle { still: PathBuf, ms: u32 },
        Stop,
    }

    enum Event {
        Command(Command),
        HangUp,
        Warmed { gpu: Option<Result<Gpu, String>>, loaded: Option<Result<Loaded, String>>, from: Option<String>, ms: u64 },
        Device { gpu: Gpu, composition: bool },
        Timed { timed: Option<Duration> },
        Presented { id: u32, result: Result<(), String>, first: u64, still: &'static str },
        Glided { id: u32, frames: u32, slow: u32, first: u64, scale: f32, timed: Option<Duration> },
        Settled { id: u32 },
        Drew { id: u32, frames: String },
        Broke { id: u32, why: String },
        Stopped { id: u32 },
        Kept { path: PathBuf, still: Arc<Still> },
    }

    #[derive(Default)]
    struct Split {
        window: u64,
        make: u64,
        present: u64,
        found: bool,
        steps: Vec<(&'static str, u64)>,
    }

    struct Primed {
        screen: Screen,
        hints: Hints,
        still: Option<PathBuf>,
    }

    struct Live<L> {
        layer: Option<L>,
        jobs: Sender<Job>,
        split: Split,
        asked: Instant,
        touched: Instant,
        shown: bool,
        stopped: bool,
        closing: Option<(Instant, bool)>,
        told: Option<String>,
        primed: Option<Primed>,
        drawn: Option<(u64, &'static str)>,
    }

    fn spent(since: Instant) -> u64 {
        since.elapsed().as_millis() as u64
    }

    enum Win<P: Platform> {
        Making(Option<P::Making>),
        Made(P::Layer),
        Presenting(P::Layer, Instant),
        Shown(P::Layer),
    }

    struct Pane<P: Platform> {
        screen: Screen,
        hints: Hints,
        stills: Stills,
        win: Option<Win<P>>,
        steps: Vec<(&'static str, u64)>,
        make: u64,
        present: u64,
        asked: Instant,
    }

    impl<P: Platform> Pane<P> {
        fn new(screen: Screen, hints: Hints, stills: Stills) -> Self {
            Self { screen, hints, stills, win: Some(Win::Making(None)), steps: Vec::new(), make: 0, present: 0, asked: Instant::now() }
        }

        fn fits(&self, screen: &Screen, hints: &Hints) -> bool {
            self.screen == *screen && self.hints == *hints
        }

        fn primes(&self) -> bool {
            self.stills.still.is_some()
        }

        fn made(&self) -> bool {
            matches!(self.win, Some(Win::Made(_)))
        }

        fn shown(&self) -> bool {
            matches!(self.win, Some(Win::Shown(_)))
        }

        fn pause(&self, present: bool) -> Option<Duration> {
            match &self.win {
                Some(Win::Making(_)) => Some(Duration::ZERO),
                Some(Win::Made(_)) if present => Some(Duration::ZERO),
                Some(Win::Presenting(..)) => Some(POLL),
                _ => None,
            }
        }

        fn into_layer(self) -> Option<P::Layer> {
            match self.win {
                Some(Win::Made(layer) | Win::Presenting(layer, _) | Win::Shown(layer)) => Some(layer),
                Some(Win::Making(_)) | None => None,
            }
        }

        fn step(&mut self, platform: &mut P, present: bool) -> Result<bool> {
            let started = Instant::now();
            let (win, worked) = match self.win.take() {
                Some(Win::Making(making)) => {
                    let made = platform.make(&self.screen, &self.hints, making)?;
                    let took = spent(started);
                    self.make += took;
                    match made {
                        Build::Partly(next, name) => {
                            self.steps.push((name, took));
                            (Win::Making(Some(next)), true)
                        }
                        Build::Whole(layer, name) => {
                            self.steps.push((name, took));
                            (Win::Made(layer), true)
                        }
                    }
                }
                Some(Win::Made(layer)) if present => {
                    if let Err(error) = platform.present(&layer) {
                        platform.close(layer);
                        return Err(error);
                    }
                    let took = spent(started);
                    self.present += took;
                    self.steps.push(("order", took));
                    (self.watch(platform, layer, Instant::now())?, true)
                }
                Some(Win::Presenting(layer, since)) => (self.watch(platform, layer, since)?, false),
                Some(other) => (other, false),
                None => return Ok(false),
            };
            self.win = Some(win);
            Ok(worked)
        }

        fn watch(&mut self, platform: &mut P, layer: P::Layer, since: Instant) -> Result<Win<P>> {
            match platform.visible(&layer, since.elapsed()) {
                Ok(true) => {
                    let waited = spent(since);
                    self.present += waited;
                    self.steps.push(("visible", waited));
                    Ok(Win::Shown(layer))
                }
                Ok(false) => Ok(Win::Presenting(layer, since)),
                Err(error) => {
                    platform.close(layer);
                    Err(error)
                }
            }
        }
    }

    struct Pending<P: Platform> {
        id: u32,
        stills: Stills,
        asked: Instant,
        pane: Pane<P>,
        found: bool,
        window: Option<u64>,
    }

    struct Helper<P: Platform> {
        platform: P,
        out: Out,
        events: Sender<Event>,
        waker: Waker,
        hints: Hints,
        scale: f32,
        pinned: Option<f32>,
        timed: Option<Duration>,
        scales: BTreeMap<String, f32>,
        untimed: Option<([u32; 2], Picture)>,
        timing: bool,
        gpu: Option<(Gpu, bool)>,
        pictures: Vec<(Key, Loaded)>,
        stills: Vec<(PathBuf, Arc<Still>)>,
        uploads: u32,
        running: Option<Want>,
        queue: VecDeque<Want>,
        pending: Vec<Pending<P>>,
        early: Option<Pane<P>>,
        primed: Option<u32>,
        aliases: BTreeMap<u32, u32>,
        next_primed: u32,
        layers: BTreeMap<u32, Live<P::Layer>>,
        last: Instant,
        parked: bool,
        stuck: bool,
        holding: bool,
        leaving: bool,
    }

    #[derive(Clone)]
    enum Shown {
        Still(Arc<Still>),
        Glide(Arc<Glide>, f32),
    }

    fn described(layers: &[(Shown, f32)], step: &str, took: Took, whole: Duration) -> String {
        let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64());
        let ms = |time: Duration| time.as_secs_f64() * 1000.0;
        let parts: Vec<String> = layers
            .iter()
            .map(|(shown, k)| match shown {
                Shown::Still(_) => format!("S@{k:.2}"),
                Shown::Glide(_, s) => format!("G{s:.3}@{k:.2}"),
            })
            .collect();
        format!("{at:.3} {step} {} a{:.1} d{:.1} p{:.1} f{:.1}", parts.join(" "), ms(took.acquire), ms(took.draw), ms(took.present), ms(whole))
    }

    fn looks(layers: &[(Shown, f32)]) -> Vec<(Look<'_>, f32)> {
        layers
            .iter()
            .map(|(shown, k)| {
                let look = match shown {
                    Shown::Still(still) => Look::Still(still),
                    Shown::Glide(glide, s) => Look::Glide(glide, *s),
                };
                (look, *k)
            })
            .collect()
    }

    struct Motion {
        glide: Arc<Glide>,
        compatible: bool,
        from: Vec<(Shown, f32)>,
        started: Instant,
        length: Duration,
        asked: Instant,
        frames: u32,
        slow: u32,
        first: Option<u64>,
        last: Option<Instant>,
    }

    struct Settling {
        still: Option<Arc<Still>>,
        length: Duration,
        from: Vec<(Shown, f32)>,
        started: Option<Instant>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Step {
        Moving,
        Glided,
        Fading,
        Settled,
    }

    impl Step {
        fn word(self) -> &'static str {
            match self {
                Step::Moving => "move",
                Step::Glided => "glided",
                Step::Fading => "fade",
                Step::Settled => "settled",
            }
        }
    }

    struct Ended {
        frames: u32,
        slow: u32,
        first: u64,
    }

    fn span(ms: u32) -> Duration {
        Duration::from_millis(u64::from(ms.clamp(1, LONGEST)))
    }

    struct Reel {
        shown: Vec<(Shown, f32)>,
        motion: Option<Motion>,
        settling: Option<Settling>,
    }

    impl Reel {
        fn new(first: Arc<Still>) -> Reel {
            Reel { shown: vec![(Shown::Still(first), 1.0)], motion: None, settling: None }
        }

        fn animating(&self) -> bool {
            self.motion.is_some() || self.settling.as_ref().is_some_and(|settle| settle.started.is_some() || settle.still.is_some())
        }

        fn glide(&mut self, glide: Glide, ms: u32, asked: Instant, now: Instant) {
            let compatible = glide.compatible();
            let from = self.motion.take().map(|moving| vec![(Shown::Glide(moving.glide, 1.0), 1.0)]).unwrap_or_else(|| self.shown.clone());
            self.motion = Some(Motion { glide: Arc::new(glide), compatible, from, started: now, length: span(ms), asked, frames: 0, slow: 0, first: None, last: None });
        }

        fn settle(&mut self, ms: u32) {
            self.settling = Some(Settling { still: None, length: span(ms), from: Vec::new(), started: None });
        }

        fn ready(&mut self, still: Arc<Still>) {
            if let Some(settle) = self.settling.as_mut() {
                settle.still = Some(still);
            }
        }

        fn unsettle(&mut self) {
            self.settling = None;
        }

        fn halt(&mut self) {
            self.motion = None;
            self.settling = None;
        }

        fn next(&mut self, now: Instant) -> Option<(Vec<(Shown, f32)>, Step)> {
            if let Some(moving) = self.motion.as_ref() {
                let elapsed = now.saturating_duration_since(moving.started);
                let t = (elapsed.as_secs_f32() / moving.length.as_secs_f32()).min(1.0);
                let s = ease(t);
                let mut layers = moving.from.clone();
                if moving.compatible {
                    let lead = (elapsed.as_secs_f32() / LEAD_IN.as_secs_f32()).min(1.0);
                    if lead >= 1.0 {
                        layers.clear();
                    }
                    layers.push((Shown::Glide(moving.glide.clone(), s), if layers.is_empty() { 1.0 } else { lead }));
                } else if t >= 1.0 {
                    layers = vec![(Shown::Glide(moving.glide.clone(), 1.0), 1.0)];
                } else {
                    layers.push((Shown::Glide(moving.glide.clone(), 1.0), s));
                }
                return Some((layers, if t >= 1.0 { Step::Glided } else { Step::Moving }));
            }
            let settle = self.settling.as_mut()?;
            let still = settle.still.clone()?;
            if settle.started.is_none() {
                settle.from = self.shown.clone();
                settle.started = Some(now);
            }
            let started = settle.started.unwrap_or(now);
            let u = (now.saturating_duration_since(started).as_secs_f32() / settle.length.as_secs_f32()).min(1.0);
            if u >= 1.0 {
                return Some((vec![(Shown::Still(still), 1.0)], Step::Settled));
            }
            let mut layers = settle.from.clone();
            layers.push((Shown::Still(still), ease(u)));
            Some((layers, Step::Fading))
        }

        fn presented(&mut self, layers: Vec<(Shown, f32)>, step: Step, now: Instant, slow_after: Duration) -> Option<Ended> {
            self.shown = layers;
            match step {
                Step::Moving | Step::Glided => {
                    let moving = self.motion.as_mut()?;
                    if moving.first.is_none() {
                        moving.first = Some(now.saturating_duration_since(moving.asked).as_millis() as u64);
                    }
                    if moving.last.is_some_and(|last| now.saturating_duration_since(last) > slow_after) {
                        moving.slow += 1;
                    }
                    moving.last = Some(now);
                    moving.frames += 1;
                    if step == Step::Moving {
                        return None;
                    }
                    let moving = self.motion.take()?;
                    Some(Ended { frames: moving.frames, slow: moving.slow, first: moving.first.unwrap_or(0) })
                }
                Step::Fading => None,
                Step::Settled => {
                    self.settling = None;
                    None
                }
            }
        }
    }

    struct Start {
        id: u32,
        gpu: Gpu,
        surface: Surface,
        source: Source,
        stills: Stills,
        size: (u32, u32),
        period: Duration,
        commit: Option<Commit>,
    }

    type Prepared = (PathBuf, (u32, u32, u64), Side);

    fn side_of(gpu: &Gpu, path: &Path) -> Result<((u32, u32, u64), Side), String> {
        let bytes = std::fs::read(path).map_err(|error| format!("could not read {} {error}", path.display()))?;
        let key = crate::live::canvas::tables_canvas(&bytes).ok_or_else(|| format!("{} {}", path.display(), scooch_core::TablesError::Short))?;
        let bundle = Bundle::tables_for(&bytes, key.0, key.1, key.2).map_err(|error| format!("{} {error}", path.display()))?;
        Ok((key, gpu.side(&bundle)?))
    }

    fn build_glide(gpu: &Gpu, (a, b): (&Path, &Path), loaded: &[Loaded], prepared: Option<Prepared>) -> Result<Glide, String> {
        let (key, first) = match prepared.filter(|(path, _, _)| path == a) {
            Some((_, key, side)) => (key, side),
            None => side_of(gpu, a)?,
        };
        let picture = loaded.iter().find(|loaded| (loaded.picture.width, loaded.picture.height, loaded.print) == key).ok_or("glide needs a warm with a picture first")?;
        let bytes = std::fs::read(b).map_err(|error| format!("could not read {} {error}", b.display()))?;
        let bundle = Bundle::tables_for(&bytes, key.0, key.1, key.2).map_err(|error| format!("{} {error}", b.display()))?;
        let second = gpu.side(&bundle)?;
        gpu.glide_between(&picture.picture, &first, &second)
    }

    fn render<P: Platform>(start: Start, jobs: Receiver<Job>, events: Sender<Event>, waker: Waker) {
        let tell = |event: Event| {
            let _ = events.send(event);
            waker();
        };
        let Start { id, gpu, surface, source, stills, size, period, commit } = start;
        let begun = Instant::now();
        let made = source.finish(&gpu).and_then(|(first, from)| gpu.screen(surface.0, size.0, size.1).map(|screen| (first, from, screen)));
        let (first, from, mut screen) = match made {
            Ok(made) => made,
            Err(why) => {
                tell(Event::Presented { id, result: Err(why), first: 0, still: "" });
                tell(Event::Stopped { id });
                return;
            }
        };
        if let (Some(path), false) = (stills.still.clone(), from == "kept") {
            tell(Event::Kept { path, still: first.clone() });
        }
        let mut reel = Reel::new(first);
        let started = Instant::now();
        let result = P::frame(|| screen.show(&looks(&reel.shown)));
        let whole = started.elapsed();
        let ok = result.is_ok();
        let mut drew = if ok { vec![described(&reel.shown, "first", screen.took, whole)] } else { Vec::new() };
        if let (true, Some(commit)) = (ok, commit.as_ref()) {
            commit();
        }
        tell(Event::Presented { id, result, first: begun.elapsed().as_millis() as u64, still: from });
        let mut prepared: Option<Prepared> = stills.tables.as_deref().filter(|_| ok).and_then(|path| side_of(&gpu, path).ok().map(|(key, side)| (path.to_path_buf(), key, side)));
        let slow_after = period.mul_f32(1.5);
        let mut decoding: Option<(Receiver<Result<RgbImage, String>>, PathBuf)> = None;
        let mut stop = !ok;
        while !stop {
            let job = if reel.animating() {
                jobs.try_recv().ok()
            } else {
                match jobs.recv_timeout(Duration::from_millis(20)) {
                    Ok(job) => Some(job),
                    Err(mpsc::RecvTimeoutError::Timeout) => None,
                    Err(mpsc::RecvTimeoutError::Disconnected) => Some(Job::Stop),
                }
            };
            match job {
                Some(Job::Stop) => stop = true,
                Some(Job::Glide { a, b, ms, loaded, asked, scale }) => {
                    screen.scale = scale;
                    match build_glide(&gpu, (&a, &b), &loaded, prepared.take()) {
                        Ok(glide) => reel.glide(glide, ms, asked, Instant::now()),
                        Err(why) => tell(Event::Broke { id, why }),
                    }
                }
                Some(Job::Settle { still, ms }) => {
                    let (sender, incoming) = mpsc::channel();
                    let path = still.clone();
                    let spawned = std::thread::Builder::new().name("scooch-live-decode".into()).spawn(move || {
                        let _ = sender.send(P::decode(&still).map_err(|error| format!("{error:#}")));
                    });
                    match spawned {
                        Ok(_) => {
                            decoding = Some((incoming, path));
                            reel.settle(ms);
                        }
                        Err(error) => tell(Event::Broke { id, why: format!("could not start the decode thread {error}") }),
                    }
                }
                None => {}
            }
            if stop {
                break;
            }
            if let Some((incoming, path)) = decoding.as_ref() {
                let decoded = match incoming.try_recv() {
                    Ok(Ok(image)) => Some(gpu.still(&image).map(Arc::new)),
                    Ok(Err(why)) => Some(Err(why)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some(Err("the still could not be decoded".to_string())),
                };
                match decoded {
                    Some(Ok(still)) => {
                        tell(Event::Kept { path: path.clone(), still: still.clone() });
                        reel.ready(still);
                        decoding = None;
                    }
                    Some(Err(why)) => {
                        tell(Event::Broke { id, why });
                        reel.unsettle();
                        decoding = None;
                    }
                    None => {}
                }
            }
            let Some((layers, step)) = reel.next(Instant::now()) else { continue };
            let started = Instant::now();
            if let Err(why) = P::frame(|| screen.show(&looks(&layers))) {
                tell(Event::Broke { id, why });
                reel.halt();
                continue;
            }
            let now = Instant::now();
            drew.push(described(&layers, step.word(), screen.took, now.saturating_duration_since(started)));
            match step {
                Step::Glided => {
                    let scale = screen.scale;
                    let timed = gpu.time_at(screen.width, screen.height, &looks(&layers), scale).ok();
                    if let Some(ended) = reel.presented(layers, step, now, slow_after) {
                        tell(Event::Glided { id, frames: ended.frames, slow: ended.slow, first: ended.first, scale, timed });
                    }
                }
                Step::Settled => {
                    for _ in 0..FLUSH {
                        let started = Instant::now();
                        if let Err(why) = P::frame(|| screen.show(&looks(&layers))) {
                            tell(Event::Broke { id, why });
                            break;
                        }
                        drew.push(described(&layers, "flush", screen.took, started.elapsed()));
                    }
                    let _ = gpu.idle(COMPOSE_WAIT);
                    P::composed();
                    reel.presented(layers, step, now, slow_after);
                    tell(Event::Drew { id, frames: std::mem::take(&mut drew).join(", ") });
                    tell(Event::Settled { id });
                }
                Step::Moving | Step::Fading => {
                    reel.presented(layers, step, now, slow_after);
                }
            }
        }
        drop(screen);
        if !drew.is_empty() {
            tell(Event::Drew { id, frames: drew.join(", ") });
        }
        tell(Event::Stopped { id });
    }

    impl<P: Platform> Helper<P> {
        fn new(platform: P, out: Out, events: Sender<Event>, waker: Waker) -> Self {
            Helper {
                platform,
                out,
                events,
                waker,
                hints: Hints::default(),
                scale: 1.0,
                pinned: None,
                timed: None,
                scales: BTreeMap::new(),
                untimed: None,
                timing: false,
                gpu: None,
                pictures: Vec::new(),
                stills: Vec::new(),
                uploads: 0,
                running: None,
                queue: VecDeque::new(),
                pending: Vec::new(),
                early: None,
                primed: None,
                aliases: BTreeMap::new(),
                next_primed: PRIMED,
                layers: BTreeMap::new(),
                last: Instant::now(),
                parked: false,
                stuck: false,
                holding: true,
                leaving: false,
            }
        }

        fn tell(&self, reply: Reply) {
            say(&self.out, &reply);
        }

        fn fail(&self, id: Option<u32>, why: impl Into<String>) {
            self.tell(Reply::Failed { id, why: why.into() });
        }

        fn ready(&self, warm_id: Option<u32>, ms: u64, from: Option<String>) {
            let adapter = self.gpu.as_ref().map(|(gpu, _)| gpu.name.clone()).unwrap_or_default();
            let kind = self.gpu.as_ref().map_or("", |(gpu, _)| gpu.kind).to_string();
            self.tell(Reply::Ready { adapter, ms, uploads: self.uploads, kind, warm_id, from, scale: Some(self.scale) });
        }

        fn first_scale(&self, gpu: &Gpu) -> f32 {
            self.scales.get(&gpu.name).copied().map(|scale| scale.clamp(0.25, 1.0)).unwrap_or(if gpu.kind == "discrete" { 1.0 } else { 0.6 })
        }

        fn time_later(&mut self) {
            if self.timing || self.pinned.is_some() || !self.layers.is_empty() || self.early.is_some() || !self.pending.is_empty() {
                return;
            }
            let (Some((gpu, _)), Some(([width, height], picture))) = (self.gpu.as_ref().map(|(gpu, made)| (gpu.clone(), *made)), self.untimed.take()) else { return };
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new().name("scooch-live-time".into()).spawn(move || {
                let heavy = scooch_live::heavy(width.max(1), height.max(1), &picture);
                let timed = gpu.glide(&picture, &heavy, &heavy).ok().and_then(|glide| {
                    let look = [(Look::Glide(&glide, 0.5), 1.0)];
                    let (mut each, _) = gpu.burst_at(width.max(1), height.max(1), &look, 1.0, TIMED_FRAMES).ok()?;
                    each.sort();
                    each.get(each.len() / 2).copied()
                });
                let _ = events.send(Event::Timed { timed });
                waker();
            });
            self.timing = spawned.is_ok();
        }

        fn has(&self, key: &Option<Key>) -> bool {
            self.gpu.is_some() && key.as_ref().is_none_or(|key| self.pictures.iter().any(|(kept, _)| kept == key))
        }

        fn warm(&mut self, hints: Hints, (picture, canvas, stamp): (Option<PathBuf>, Option<PathBuf>, u64), size: Option<[u32; 2]>, warm_id: Option<u32>, answer: bool) {
            let composition = match self.platform.composition(&hints) {
                Ok(composition) => composition,
                Err(error) => {
                    if answer {
                        self.fail(None, warm_failed(warm_id, &format!("{error:#}")));
                    }
                    return;
                }
            };
            if self.gpu.as_ref().is_some_and(|(_, made)| *made != composition) {
                if answer {
                    self.fail(None, warm_failed(warm_id, "this helper keeps one way to present for its whole life"));
                }
                return;
            }
            let was = self.pinned;
            self.pinned = hints.scale.map(|scale| scale.clamp(0.25, 1.0));
            match (self.pinned, was) {
                (Some(scale), _) => self.scale = scale,
                (None, Some(_)) => self.scale = self.timed.map_or(1.0, |timed| scooch_live::choose(timed, self.platform.refresh())),
                (None, None) => {}
            }
            self.hints = hints;
            let key = picture.map(|path| (path, size.unwrap_or([0, 0]), stamp));
            let waiters = if answer { vec![warm_id] } else { Vec::new() };
            if self.stuck {
                for waiter in waiters {
                    self.fail(None, warm_failed(waiter, STUCK));
                }
                return;
            }
            if self.has(&key) {
                for waiter in waiters {
                    self.ready(waiter, 0, Some("kept".into()));
                }
                return;
            }
            if let Some(running) = self.running.as_mut().filter(|running| running.composition == composition && (running.key == key || key.is_none())) {
                running.waiters.extend(waiters);
                return;
            }
            if let Some(queued) = self.queue.iter_mut().find(|queued| queued.composition == composition && queued.key == key) {
                queued.waiters.extend(waiters);
                return;
            }
            let want = Want { key, canvas, waiters, composition, begun: None };
            if self.running.is_none() {
                self.start(want);
            } else {
                self.queue.push_back(want);
            }
        }

        fn start(&mut self, want: Want) {
            let (events, waker, out) = (self.events.clone(), self.waker.clone(), self.out.clone());
            let warm_id = want.waiters.iter().flatten().next().copied();
            let device = self.gpu.as_ref().map(|(gpu, _)| gpu.clone());
            let (key, cached, composition) = (want.key.clone(), want.canvas.clone(), want.composition);
            let spawned = std::thread::Builder::new().name("scooch-live-warm".into()).spawn(move || {
                let started = Instant::now();
                let reading = key.as_ref().and(cached).map(|path| std::thread::spawn(move || crate::live::canvas::read(&path).map(Arc::new).map_err(|error| format!("{error:#}"))));
                let made = device.is_none().then(|| Gpu::new(composition));
                if let Some(Ok(gpu)) = &made {
                    say(&out, &Reply::Device { ms: started.elapsed().as_millis() as u64 });
                    if events.send(Event::Device { gpu: gpu.clone(), composition }).is_ok() {
                        waker();
                    }
                }
                let usable = device.clone().or_else(|| made.clone().and_then(Result::ok));
                let mut from = None;
                let loaded = key.map(|(path, [width, height], _)| {
                    let gpu = usable.ok_or_else(|| "no device".to_string())?;
                    gpu.ready()?;
                    if width > 0 && height > 0 {
                        gpu.fits(width, height)?;
                    }
                    let read = reading.map(|reading| reading.join().unwrap_or_else(|_| Err("the canvas reader stopped".into())));
                    let cached = match read {
                        Some(Ok(cached)) => {
                            let checking = {
                                let cached = cached.clone();
                                std::thread::spawn(move || cached.canvas().map(|_| cached.print()).map_err(|error| format!("{error:#}")))
                            };
                            let picture = gpu.try_picture_levels(cached.width, cached.height, &cached.levels());
                            match (checking.join().unwrap_or_else(|_| Err("the canvas check stopped".into())), picture) {
                                (Ok(print), Ok(picture)) => Ok(Loaded { print, picture }),
                                (Err(why), _) | (_, Err(why)) => Err(why),
                            }
                        }
                        Some(Err(why)) => Err(why),
                        None => Err(String::new()),
                    };
                    match cached {
                        Ok(loaded) => {
                            from = Some("cache".to_string());
                            Ok(loaded)
                        }
                        Err(why) => {
                            from = Some(if why.is_empty() { "picture".to_string() } else { format!("picture, {why}") });
                            let image = P::decode(&path).map_err(|error| format!("{error:#}"))?;
                            let (width, height) = if width == 0 || height == 0 { (image.width(), image.height()) } else { (width, height) };
                            gpu.fits(width.max(1), height.max(1))?;
                            let canvas = scooch_core::canvas(&image, width.max(1), height.max(1));
                            let print = scooch_core::fingerprint(canvas.width, canvas.height, &canvas.pixels);
                            Ok(Loaded { print, picture: gpu.try_picture(&canvas)? })
                        }
                    }
                });
                if let Some(Ok(_)) = &loaded {
                    say(&out, &Reply::Canvas { warm_id, ms: started.elapsed().as_millis() as u64 });
                }
                let _ = events.send(Event::Warmed { gpu: made, loaded, from, ms: started.elapsed().as_millis() as u64 });
                waker();
            });
            match spawned {
                Ok(_) => {
                    self.running = Some(Want { begun: Some(Instant::now()), ..want });
                    self.platform.begin();
                }
                Err(error) => {
                    for waiter in want.waiters {
                        self.fail(None, warm_failed(waiter, &format!("could not start the thread that makes the device {error}")));
                    }
                }
            }
        }

        fn drop_pane(&mut self, pane: Pane<P>) {
            if let Some(layer) = pane.into_layer() {
                self.platform.close(layer);
            }
        }

        fn kept_still(&self, stills: &Stills) -> Option<Arc<Still>> {
            let wanted = stills.still.as_ref()?;
            self.stills.iter().find(|(path, _)| path == wanted).map(|(_, still)| still.clone())
        }

        fn primed_for(&self, screen: &Screen, stills: &Stills) -> bool {
            self.primed.and_then(|id| self.layers.get(&id)).and_then(|live| live.primed.as_ref()).is_some_and(|primed| primed.screen == *screen && primed.hints == self.hints && primed.still == stills.still)
        }

        fn drop_primed(&mut self) {
            if let Some(id) = self.primed.take() {
                self.begin_close(id, false);
            }
        }

        fn unprime(&mut self, screen: String) {
            if self.primed.and_then(|id| self.layers.get(&id)).and_then(|live| live.primed.as_ref()).is_some_and(|primed| primed.screen.id == screen) {
                self.drop_primed();
            }
            if self.early.as_ref().is_some_and(|early| early.screen.id == screen) {
                if let Some(early) = self.early.take() {
                    self.drop_pane(early);
                }
            }
            self.tell(Reply::Unprimed { screen });
        }

        fn make_early(&mut self, screen: Screen, stills: Stills) {
            if self.primed_for(&screen, &stills) {
                if let Some(live) = self.primed.and_then(|id| self.layers.get_mut(&id)) {
                    live.asked = Instant::now();
                }
                return;
            }
            if let Some(early) = self.early.as_mut().filter(|early| early.fits(&screen, &self.hints) && early.stills == stills) {
                early.asked = Instant::now();
                return;
            }
            self.drop_primed();
            if let Some(stale) = self.early.take() {
                self.drop_pane(stale);
            }
            if !self.leaving {
                self.early = Some(Pane::new(screen, self.hints.clone(), stills));
            }
        }

        fn open(&mut self, id: u32, screen: Screen, stills: Stills) {
            if self.layers.contains_key(&id) || self.pending.iter().any(|waiting| waiting.id == id) {
                self.fail(Some(id), format!("layer {id} is already open"));
                return;
            }
            if self.primed_for(&screen, &stills) {
                let layer = self.primed.and_then(|primed| self.layers.get(&primed)).and_then(|live| live.layer.as_ref());
                match layer.map_or(Ok(false), |layer| self.platform.room(&screen, layer)) {
                    Ok(true) => {
                        self.adopt(id);
                        return;
                    }
                    Ok(false) => {}
                    Err(error) => {
                        self.drop_primed();
                        self.fail(Some(id), format!("{error:#}"));
                        return;
                    }
                }
            }
            self.drop_primed();
            let pane = match self.early.take() {
                Some(early) if early.fits(&screen, &self.hints) => early,
                other => {
                    if let Some(stale) = other {
                        self.drop_pane(stale);
                    }
                    Pane::new(screen, self.hints.clone(), stills.clone())
                }
            };
            let found = pane.made() || pane.shown();
            self.pending.push(Pending { id, stills, asked: Instant::now(), pane, found, window: None });
            if self.gpu.is_none() && self.running.is_none() {
                let hints = self.hints.clone();
                self.warm(hints, (None, None, 0), None, None, false);
            }
        }

        fn adopt(&mut self, id: u32) {
            let Some(primed) = self.primed.take() else { return };
            let Some(mut live) = self.layers.remove(&primed) else { return };
            self.aliases.insert(primed, id);
            live.primed = None;
            live.asked = Instant::now();
            live.touched = live.asked;
            let drawn = live.drawn;
            self.layers.insert(id, live);
            if let Some((first, still)) = drawn {
                self.reveal(id, Ok(()), first, still);
            }
        }

        fn pause(&self) -> Duration {
            let pending = self.pending.iter().filter_map(|pending| pending.pane.pause(true));
            let early = self.early.iter().filter_map(|early| early.pause(early.primes() && self.gpu.is_some()));
            let busy = !self.layers.is_empty() || self.early.is_some() || !self.pending.is_empty() || self.timing || self.running.is_some() || !self.queue.is_empty();
            let idle = if busy || !self.parked {
                TICK
            } else if self.loaded() {
                IDLE_EXIT.saturating_sub(self.last.elapsed()) + POLL
            } else {
                REST
            };
            pending.chain(early).min().unwrap_or(idle)
        }

        fn loaded(&self) -> bool {
            !self.pictures.is_empty() || !self.stills.is_empty() || self.untimed.is_some() || self.holding
        }

        fn advance(&mut self) {
            if self.leaving {
                return;
            }
            let mut worked = false;
            let mut index = 0;
            while index < self.pending.len() {
                let pending = &mut self.pending[index];
                if worked && pending.pane.pause(true) == Some(Duration::ZERO) {
                    index += 1;
                    continue;
                }
                match pending.pane.step(&mut self.platform, true) {
                    Ok(did) => {
                        worked |= did;
                        if pending.window.is_none() && pending.pane.shown() {
                            pending.window = Some(spent(pending.asked));
                        }
                        index += 1;
                    }
                    Err(error) => {
                        let id = pending.id;
                        self.pending.remove(index);
                        self.fail(Some(id), format!("{error:#}"));
                    }
                }
            }
            if !worked {
                let present = self.gpu.is_some();
                if let Some(early) = self.early.as_mut() {
                    let prime = early.primes() && present;
                    if early.step(&mut self.platform, prime).is_err() {
                        self.early = None;
                    }
                }
            }
            self.launch_shown();
        }

        fn launch_shown(&mut self) {
            let Some(gpu) = self.gpu.as_ref().map(|(gpu, _)| gpu.clone()) else { return };
            let mut index = 0;
            while index < self.pending.len() {
                if self.pending[index].pane.shown() {
                    let pending = self.pending.remove(index);
                    self.launch(pending, gpu.clone(), None);
                } else {
                    index += 1;
                }
            }
            if self.early.as_ref().is_some_and(|early| early.primes() && early.shown()) {
                if let Some(early) = self.early.take() {
                    let id = self.next_primed;
                    self.next_primed = self.next_primed.wrapping_add(1).max(PRIMED);
                    let primed = Primed { screen: early.screen.clone(), hints: early.hints.clone(), still: early.stills.still.clone() };
                    let pending = Pending { id, stills: early.stills.clone(), asked: early.asked, window: Some(spent(early.asked)), found: true, pane: early };
                    self.launch(pending, gpu, Some(primed));
                    if self.layers.contains_key(&id) {
                        self.primed = Some(id);
                    }
                }
            }
        }

        fn launch(&mut self, pending: Pending<P>, gpu: Gpu, primed: Option<Primed>) {
            let Pending { id, stills, asked, mut pane, found, window } = pending;
            let split = Split { window: window.unwrap_or_else(|| spent(asked)), make: pane.make, present: pane.present, found, steps: std::mem::take(&mut pane.steps) };
            let Some(layer) = pane.into_layer() else {
                if primed.is_none() {
                    self.fail(Some(id), "the layer window is gone");
                }
                return;
            };
            let source = Source::start(&stills, self.kept_still(&stills), P::decode);
            let start = Start { id, gpu, surface: Surface(self.platform.target(&layer)), source, stills, size: self.platform.pixels(&layer), period: self.platform.period(&layer), commit: self.platform.committer() };
            let (jobs, receiver) = mpsc::channel();
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new().name(format!("scooch-live-render-{id}")).spawn(move || render::<P>(start, receiver, events, waker));
            if let Err(error) = spawned {
                self.platform.close(layer);
                if primed.is_none() {
                    self.fail(Some(id), format!("could not start the render thread {error}"));
                }
                return;
            }
            let now = Instant::now();
            self.layers.insert(id, Live { layer: Some(layer), jobs, split, asked, touched: now, shown: false, stopped: false, closing: None, told: None, primed, drawn: None });
        }

        fn begin_close(&mut self, id: u32, answer: bool) {
            if let Some(index) = self.pending.iter().position(|pending| pending.id == id) {
                let pending = self.pending.remove(index);
                self.drop_pane(pending.pane);
                if answer {
                    self.tell(Reply::Closed { id });
                }
                return;
            }
            let Some(live) = self.layers.get_mut(&id) else {
                if answer {
                    self.fail(Some(id), format!("layer {id} is not open"));
                }
                return;
            };
            if live.closing.is_none() {
                live.closing = Some((Instant::now(), answer));
                let _ = live.jobs.send(Job::Stop);
                if let Some(layer) = live.layer.as_ref() {
                    self.platform.hide(layer);
                }
            }
            if live.stopped {
                self.finish(id);
            }
        }

        fn finish(&mut self, id: u32) {
            let Some(mut live) = self.layers.remove(&id) else { return };
            self.aliases.retain(|_, real| *real != id);
            if self.primed == Some(id) {
                self.primed = None;
            }
            if let Some(layer) = live.layer.take() {
                if live.stopped {
                    self.platform.close(layer);
                } else {
                    self.platform.abandon(layer);
                }
            }
            if let Some(why) = live.told.take() {
                self.fail(Some(id), why);
            }
            if live.closing.is_some_and(|(_, answer)| answer) {
                self.tell(Reply::Closed { id });
            }
        }

        fn send_job(&mut self, id: u32, job: Job, busy: Duration) {
            match self.layers.get_mut(&id) {
                Some(live) if live.shown && live.closing.is_none() => {
                    live.touched = Instant::now() + busy;
                    let _ = live.jobs.send(job);
                }
                Some(_) => self.fail(Some(id), format!("layer {id} is not on screen yet")),
                None => self.fail(Some(id), format!("layer {id} is not open")),
            }
        }

        fn command(&mut self, command: Command) {
            self.last = Instant::now();
            if !self.holding {
                self.platform.prepare();
                self.holding = true;
            }
            match command {
                Command::Hello { version } if version != VERSION => {
                    self.fail(None, format!("the helper speaks version {VERSION}, not {version}"));
                    self.leaving = true;
                }
                Command::Hello { .. } => {}
                Command::Park { warm_id } => {
                    self.parked = true;
                    let hints = self.hints.clone();
                    self.warm(hints, (None, None, 0), None, warm_id, true);
                }
                Command::Warm { hints, picture, size, warm_id, canvas, key, scales } => {
                    if !scales.is_empty() {
                        self.scales = scales;
                    }
                    self.warm(hints, (picture, canvas, key.unwrap_or(0)), size, warm_id, true)
                }
                Command::Window { screen, rect, scale, stills } => self.make_early(Screen { id: screen, frame: rect, scale }, stills),
                Command::Open { id, screen, rect, scale, stills } => self.open(id, Screen { id: screen, frame: rect, scale }, stills),
                Command::Glide { id, a, b, ms } => {
                    let loaded = self.pictures.iter().rev().map(|(_, loaded)| loaded.clone()).collect();
                    let job = Job::Glide { a, b, ms, loaded, asked: Instant::now(), scale: self.scale };
                    self.send_job(id, job, Duration::from_millis(u64::from(ms.min(LONGEST))));
                }
                Command::Settle { id, still, ms } => {
                    let ms = ms.unwrap_or(SETTLE);
                    self.send_job(id, Job::Settle { still, ms }, Duration::from_millis(u64::from(ms.min(LONGEST))));
                }
                Command::Close { id } => self.begin_close(id, true),
                Command::Unprime { screen } => self.unprime(screen),
                Command::Quit => self.leaving = true,
            }
        }

        fn warmed(&mut self, gpu: Option<Result<Gpu, String>>, loaded: Option<Result<Loaded, String>>, from: Option<String>, ms: u64) {
            let want = self.running.take();
            let mut problem = None;
            match gpu {
                Some(Ok(gpu)) if self.gpu.is_none() => {
                    if self.pinned.is_none() && self.timed.is_none() {
                        self.scale = self.first_scale(&gpu);
                    }
                    self.gpu = Some((gpu, want.as_ref().is_some_and(|want| want.composition)));
                }
                Some(Ok(_)) => {}
                Some(Err(why)) => problem = Some(format!("no device {why}")),
                None => {}
            }
            match (loaded, want.as_ref().and_then(|want| want.key.clone())) {
                (Some(Ok(loaded)), Some(key)) => {
                    if self.timed.is_none() && self.pinned.is_none() {
                        let size = if key.1[0] > 0 && key.1[1] > 0 { key.1 } else { [loaded.picture.width, loaded.picture.height] };
                        self.untimed = Some((size, loaded.picture.clone()));
                    }
                    self.pictures.retain(|(kept, _)| *kept != key);
                    self.pictures.push((key, loaded));
                    if self.pictures.len() > KEPT {
                        self.pictures.remove(0);
                    }
                    self.uploads += 1;
                }
                (Some(Err(why)), _) => problem = Some(format!("the picture could not be loaded {why}")),
                _ => {}
            }
            for waiter in want.map(|want| want.waiters).unwrap_or_default() {
                match &problem {
                    Some(why) => self.fail(None, warm_failed(waiter, why)),
                    None => self.ready(waiter, ms, from.clone()),
                }
            }
            if self.gpu.is_some() {
                self.launch_shown();
            } else {
                for pending in std::mem::take(&mut self.pending) {
                    let id = pending.id;
                    self.drop_pane(pending.pane);
                    self.fail(Some(id), problem.clone().unwrap_or_else(|| "no device".into()));
                }
            }
            while let Some(next) = self.queue.pop_front() {
                if self.gpu.as_ref().is_some_and(|(_, made)| *made != next.composition) {
                    for waiter in next.waiters {
                        self.fail(None, warm_failed(waiter, "this helper keeps one way to present for its whole life"));
                    }
                    continue;
                }
                if self.has(&next.key) {
                    for waiter in next.waiters {
                        self.ready(waiter, 0, Some("kept".into()));
                    }
                    continue;
                }
                self.start(next);
                break;
            }
        }

        fn reveal(&mut self, id: u32, result: Result<(), String>, first: u64, still: &'static str) {
            let revealing = Instant::now();
            let shown = match (result, self.layers.get(&id).and_then(|live| live.layer.as_ref())) {
                (Ok(()), Some(layer)) => self.platform.show(layer).map_err(|error| format!("{error:#}")),
                (Ok(()), None) => Err("the layer window is gone".to_string()),
                (Err(why), _) => Err(why),
            };
            let revealing_steps: Vec<(String, u64)> = self.platform.revealed().into_iter().map(|(name, took)| (String::from(name), took)).collect();
            match shown {
                Ok(()) => {
                    let screen_gpu = self.layers.get(&id).and_then(|live| live.layer.as_ref()).and_then(|layer| self.platform.screen_gpu(layer));
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.shown = true;
                        live.touched = Instant::now();
                        let ms = live.asked.elapsed().as_millis() as u64;
                        let split = &live.split;
                        let steps = split.steps.iter().map(|(name, took)| (String::from(*name), *took)).collect();
                        let drawn = Reply::Drawn { id, window: split.window, first, reveal: spent(revealing), make: split.make, present: split.present, found: split.found, steps, revealing: revealing_steps, still: Some(still.to_string()), screen_gpu };
                        self.tell(drawn);
                        self.tell(Reply::Shown { id, ms });
                    }
                }
                Err(why) => {
                    self.fail(Some(id), why);
                    self.begin_close(id, false);
                }
            }
        }

        fn event(&mut self, event: Event) {
            match event {
                Event::Command(command) => self.command(command),
                Event::HangUp => self.leaving = true,
                Event::Warmed { gpu, loaded, from, ms } => self.warmed(gpu, loaded, from, ms),
                Event::Device { gpu, composition } => {
                    if self.gpu.is_none() {
                        if self.pinned.is_none() && self.timed.is_none() {
                            self.scale = self.first_scale(&gpu);
                        }
                        self.gpu = Some((gpu, composition));
                        self.launch_shown();
                    }
                }
                Event::Timed { timed } => {
                    self.timing = false;
                    if let (Some(timed), None, None) = (timed, self.pinned, self.timed) {
                        self.timed = Some(timed);
                        self.scale = scooch_live::choose(timed, self.platform.refresh());
                    }
                }
                Event::Kept { path, still } => {
                    self.stills.retain(|(kept, _)| *kept != path);
                    self.stills.push((path, still));
                    if self.stills.len() > STILLS_KEPT {
                        self.stills.remove(0);
                    }
                }
                Event::Presented { id, result, first, still } => {
                    let id = self.aliases.get(&id).copied().unwrap_or(id);
                    let Some(live) = self.layers.get_mut(&id) else { return };
                    if live.closing.is_some() {
                        return;
                    }
                    if let Some(primed) = live.primed.as_ref() {
                        match result {
                            Ok(()) => {
                                live.drawn = Some((first, still));
                                let primed = Reply::Primed { screen: primed.screen.id.clone(), window: live.split.window, first, still: still.to_string() };
                                self.tell(primed);
                            }
                            Err(_) => self.drop_primed(),
                        }
                        return;
                    }
                    self.reveal(id, result, first, still);
                }
                Event::Glided { id, frames, slow, first, scale, timed } => {
                    let id = self.aliases.get(&id).copied().unwrap_or(id);
                    if self.pinned.is_none() {
                        if let Some(timed) = timed {
                            self.untimed = None;
                            self.timed = Some(timed);
                            self.scale = scooch_live::step(scale, frames, slow, timed, self.platform.refresh());
                        }
                    }
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.touched = Instant::now();
                    }
                    self.tell(Reply::Glided { id, frames, slow_frames: slow, first_frame_ms: first, uploads: self.uploads, scale });
                }
                Event::Drew { id, frames } => {
                    let id = self.aliases.get(&id).copied().unwrap_or(id);
                    self.tell(Reply::Drew { id, frames });
                }
                Event::Settled { id } => {
                    let id = self.aliases.get(&id).copied().unwrap_or(id);
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.touched = Instant::now();
                    }
                    self.tell(Reply::Settled { id });
                }
                Event::Broke { id, why } => {
                    let id = self.aliases.get(&id).copied().unwrap_or(id);
                    if self.primed == Some(id) {
                        self.drop_primed();
                        return;
                    }
                    self.fail(Some(id), why);
                    self.begin_close(id, false);
                }
                Event::Stopped { id } => {
                    let id = self.aliases.get(&id).copied().unwrap_or(id);
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.stopped = true;
                        if live.closing.is_some() {
                            self.finish(id);
                        }
                    }
                }
            }
        }

        fn unload(&mut self) {
            if !self.loaded() || !self.layers.is_empty() || !self.pending.is_empty() || self.running.is_some() {
                return;
            }
            self.pictures.clear();
            self.stills.clear();
            self.untimed = None;
            if let Some(early) = self.early.take() {
                self.drop_pane(early);
            }
            if self.holding {
                self.platform.rest();
                self.holding = false;
            }
        }

        fn sweep(&mut self) {
            if self.running.as_ref().is_some_and(|running| running.begun.is_some_and(|begun| begun.elapsed() > WARM_LIMIT)) {
                self.stuck = true;
                let hung = self.running.take().into_iter().chain(std::mem::take(&mut self.queue));
                for waiter in hung.flat_map(|want| want.waiters) {
                    self.fail(None, warm_failed(waiter, STUCK));
                }
            }
            if self.last.elapsed() > IDLE_EXIT {
                if self.parked {
                    self.unload();
                } else {
                    self.leaving = true;
                }
            }
            if self.leaving || self.early.as_ref().is_some_and(|early| early.asked.elapsed() > LAYER_IDLE) {
                if let Some(early) = self.early.take() {
                    self.drop_pane(early);
                }
            }
            if self.leaving {
                for pending in std::mem::take(&mut self.pending) {
                    self.drop_pane(pending.pane);
                }
            }
            if self.primed.and_then(|id| self.layers.get(&id)).is_some_and(|live| live.asked.elapsed() > LAYER_IDLE) {
                self.drop_primed();
            }
            if self.untimed.is_some() && !self.leaving {
                self.time_later();
            }
            let ids: Vec<u32> = self.layers.keys().copied().collect();
            for id in ids {
                let Some((closing, shown, touched)) = self.layers.get(&id).map(|live| (live.closing, live.shown, live.touched)) else { continue };
                match closing {
                    Some((since, _)) if since.elapsed() > STOP_WAIT => self.finish(id),
                    Some(_) => {}
                    None if self.leaving => self.begin_close(id, false),
                    None if shown && touched.elapsed() > LAYER_IDLE => {
                        if let Some(live) = self.layers.get_mut(&id) {
                            live.told = Some(format!("layer {id} closed after {} s without a command", LAYER_IDLE.as_secs()));
                        }
                        self.begin_close(id, false);
                    }
                    None => {}
                }
            }
        }
    }

    fn slow_hello() {
        if let Some(ms) = std::env::var(crate::live::SLOW_HELLO).ok().and_then(|value| value.parse::<u64>().ok()) {
            std::thread::sleep(Duration::from_millis(ms.min(10_000)));
        }
    }

    pub fn serve<P: Platform>(mut platform: P) -> ExitCode {
        let out: Out = Arc::new(Mutex::new(Box::new(std::io::stdout())));
        slow_hello();
        say(&out, &Reply::Hello { version: VERSION, pid: std::process::id() });
        platform.prepare();
        let (events, inbox) = mpsc::channel();
        let waker = platform.waker();
        let (reader, wake) = (events.clone(), waker.clone());
        let reply = out.clone();
        let started = std::thread::Builder::new().name("scooch-live-protocol".into()).spawn(move || {
            for text in std::io::stdin().lock().lines() {
                let Ok(text) = text else { break };
                match read(&text) {
                    Ok(command) => {
                        if reader.send(Event::Command(command)).is_err() {
                            return;
                        }
                    }
                    Err(answer) => say(&reply, &answer),
                }
                wake();
            }
            let _ = reader.send(Event::HangUp);
            wake();
        });
        if started.is_err() {
            say(&out, &Reply::Failed { id: None, why: "could not start the protocol thread".into() });
            return ExitCode::from(1);
        }
        let mut helper = Helper::new(platform, out, events, waker);
        loop {
            let pause = helper.pause();
            helper.platform.wait(pause);
            while let Ok(event) = inbox.try_recv() {
                helper.event(event);
            }
            helper.advance();
            helper.sweep();
            if helper.leaving && helper.layers.is_empty() {
                break;
            }
        }
        ExitCode::SUCCESS
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use crate::live::line;
        use scooch_core::Rect;
        use std::io::Write;

        static DEVICE: Mutex<()> = Mutex::new(());

        fn testing(platform: Fake, out: Out, events: Sender<Event>) -> Helper<Fake> {
            Helper { pinned: Some(1.0), ..Helper::new(platform, out, events, Arc::new(|| {})) }
        }

        #[derive(Clone, Default)]
        struct Captured(Arc<Mutex<Vec<u8>>>);

        impl Write for Captured {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }

        impl Captured {
            fn replies(&self) -> Vec<Reply> {
                let bytes = self.0.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
                String::from_utf8_lossy(&bytes).lines().filter_map(|text| serde_json::from_str(text).ok()).collect()
            }
        }

        #[derive(Default)]
        struct Fake {
            closed: usize,
            abandoned: usize,
            made: usize,
            steps: u32,
            presented: Vec<String>,
            gone: Vec<String>,
            during: Option<Box<dyn FnOnce() + Send>>,
            watched: Option<Captured>,
            ready_when_made: Option<bool>,
            reveal: crate::live::Reveal,
            log: Arc<Mutex<Vec<String>>>,
            failed_at_close: Option<bool>,
            rested: usize,
            covered: bool,
        }

        #[cfg(target_os = "macos")]
        struct Drawn {
            metal: objc2::rc::Retained<objc2_quartz_core::CAMetalLayer>,
            size: (u32, u32),
            screen: String,
        }

        #[cfg(windows)]
        struct Drawn {
            window: isize,
            size: (u32, u32),
            screen: String,
        }

        #[cfg(windows)]
        fn window(width: u32, height: u32) -> Result<isize> {
            use windows::core::{w, PCWSTR};
            use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
            use windows::Win32::System::LibraryLoader::GetModuleHandleW;
            use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DefWindowProcW, RegisterClassExW, WINDOW_EX_STYLE, WNDCLASSEXW, WS_POPUP};
            unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
                unsafe { DefWindowProcW(window, message, wparam, lparam) }
            }
            let instance = unsafe { GetModuleHandleW(PCWSTR::null()) }?;
            let class = WNDCLASSEXW { cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: instance.into(), lpszClassName: w!("ScoochFakeLayer"), ..Default::default() };
            unsafe { RegisterClassExW(&class) };
            let made = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("ScoochFakeLayer"), w!("fake"), WS_POPUP, 0, 0, width as i32, height as i32, None, None, Some(instance.into()), None) }?;
            Ok(made.0 as isize)
        }

        impl Platform for Fake {
            type Layer = Drawn;
            type Making = u32;

            fn composition(&self, _hints: &Hints) -> Result<bool> {
                Ok(false)
            }

            fn make(&mut self, screen: &Screen, _hints: &Hints, making: Option<u32>) -> Result<Build<u32, Drawn>> {
                if let Some(during) = self.during.take() {
                    during();
                }
                let done = making.unwrap_or(0) + 1;
                if done < self.steps {
                    return Ok(Build::Partly(done, "step"));
                }
                self.made += 1;
                self.ready_when_made = self.watched.as_ref().map(|out| out.replies().iter().any(|reply| matches!(reply, Reply::Ready { .. })));
                let size = (screen.frame.w as u32, screen.frame.h as u32);
                #[cfg(target_os = "macos")]
                return Ok(Build::Whole(Drawn { metal: objc2_quartz_core::CAMetalLayer::new(), size, screen: screen.id.clone() }, "window"));
                #[cfg(windows)]
                return Ok(Build::Whole(Drawn { window: window(size.0, size.1)?, size, screen: screen.id.clone() }, "window"));
            }

            fn hide(&mut self, layer: &Drawn) {
                self.log.lock().unwrap().push(format!("hide {}", layer.screen));
            }

            fn room(&mut self, _screen: &Screen, _layer: &Drawn) -> Result<bool> {
                if self.covered {
                    anyhow::bail!("Windows cover the whole desktop on this screen, so Scooch keeps the plain swap.");
                }
                Ok(true)
            }

            fn present(&mut self, layer: &Drawn) -> Result<()> {
                self.presented.push(layer.screen.clone());
                Ok(())
            }

            fn target(&self, layer: &Drawn) -> Target {
                #[cfg(target_os = "macos")]
                return Target::Layer(objc2::rc::Retained::as_ptr(&layer.metal) as *mut std::ffi::c_void);
                #[cfg(windows)]
                return Target::Window { window: layer.window, instance: 0, composition: false };
            }

            fn pixels(&self, layer: &Drawn) -> (u32, u32) {
                layer.size
            }

            fn show(&mut self, _layer: &Drawn) -> Result<()> {
                let mut log = self.log.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                if self.reveal == crate::live::Reveal::Spin {
                    log.push("spin".into());
                }
                log.push("show".into());
                Ok(())
            }

            fn committer(&self) -> Option<Commit> {
                let log = self.log.clone();
                (self.reveal == crate::live::Reveal::Transaction).then(|| Box::new(move || log.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push("commit".into())) as Commit)
            }

            #[cfg(windows)]
            fn close(&mut self, layer: Drawn) {
                self.failed_at_close = self.watched.as_ref().map(|out| out.replies().iter().any(|reply| matches!(reply, Reply::Failed { .. })));
                self.closed += 1;
                self.gone.push(layer.screen);
                let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(windows::Win32::Foundation::HWND(layer.window as *mut std::ffi::c_void)) };
            }

            #[cfg(target_os = "macos")]
            fn close(&mut self, layer: Drawn) {
                self.failed_at_close = self.watched.as_ref().map(|out| out.replies().iter().any(|reply| matches!(reply, Reply::Failed { .. })));
                self.closed += 1;
                self.gone.push(layer.screen);
                drop(layer.metal);
            }

            fn abandon(&mut self, layer: Drawn) {
                self.abandoned += 1;
                #[cfg(target_os = "macos")]
                std::mem::forget(layer.metal);
                #[cfg(windows)]
                let _ = layer.window;
            }

            fn wait(&mut self, timeout: Duration) {
                std::thread::sleep(timeout.min(Duration::from_millis(5)));
            }

            fn rest(&mut self) {
                self.rested += 1;
            }

            fn waker(&self) -> Waker {
                Arc::new(|| {})
            }

            fn decode(path: &Path) -> Result<RgbImage> {
                DECODED.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).push(path.to_path_buf());
                Ok(scooch_core::open(path)?)
            }
        }

        static DECODED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

        struct Made {
            picture: PathBuf,
            size: [u32; 2],
            still: PathBuf,
            tables: (PathBuf, PathBuf),
            canvas: PathBuf,
        }

        fn made(folder: &Path, name: &str, image: &RgbImage, size: [u32; 2]) -> Made {
            let picture = folder.join(format!("{name}.png"));
            std::fs::write(&picture, scooch_core::encode_png(image).unwrap()).unwrap();
            let image = scooch_core::open(&picture).unwrap();
            let analysis = scooch_core::analyze(&image);
            let options = scooch_core::Options::default();
            let icon = Rect { x: 4.0, y: 4.0, w: size[0] as f32 / 6.0, h: size[1] as f32 / 5.0 };
            let first = scooch_core::Scene { width: size[0], height: size[1], icons: vec![icon] };
            let second = scooch_core::Scene { width: size[0], height: size[1], icons: vec![Rect { x: size[0] as f32 * 0.5, y: size[1] as f32 * 0.4, ..icon }] };
            let a = scooch_core::solve(&analysis, &first, &options, None);
            let b = scooch_core::solve(&analysis, &second, &options, Some(&a));
            let still = folder.join(format!("{name}-still.png"));
            std::fs::write(&still, scooch_core::encode_png(&scooch_core::render_with(&image, &analysis, &a, &first, &options)).unwrap()).unwrap();
            let (one, two) = (folder.join(format!("{name}-a.tables")), folder.join(format!("{name}-b.tables")));
            let bundle = scooch_core::bundle(&image, &analysis, &a, &first, &options);
            std::fs::write(&one, bundle.tables_to_bytes()).unwrap();
            std::fs::write(&two, scooch_core::bundle(&image, &analysis, &b, &second, &options).tables_to_bytes()).unwrap();
            let canvas = folder.join(format!("{name}.canvas"));
            let print = crate::live::canvas::fingerprint(bundle.canvas_width, bundle.canvas_height, &bundle.canvas);
            let pixels = scooch_core::Canvas { width: bundle.canvas_width, height: bundle.canvas_height, pixels: bundle.canvas.clone() };
            crate::live::canvas::write(&canvas, bundle.canvas_width, bundle.canvas_height, &scooch_live::levels(&pixels), print).unwrap();
            Made { picture, size, still, tables: (one, two), canvas }
        }

        fn pump(helper: &mut Helper<Fake>, inbox: &Receiver<Event>, out: &Captured, done: impl Fn(&[Reply]) -> bool) -> Vec<Reply> {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(90) {
                while let Ok(event) = inbox.try_recv() {
                    helper.event(event);
                }
                helper.advance();
                let replies = out.replies();
                if done(&replies) || replies.iter().any(|reply| matches!(reply, Reply::Failed { .. })) {
                    return replies;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            out.replies()
        }

        #[test]
        fn two_screens_of_different_sizes_warm_and_glide_on_one_device() {
            let _device = DEVICE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let folder = std::env::temp_dir().join(format!("scooch-helper-{}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            let paint = |width: u32, height: u32, step: u32| RgbImage::from_raw(width, height, (0..height).flat_map(|y| (0..width).flat_map(move |x| [((x * step + y) % 256) as u8, ((y * 2) % 256) as u8, ((x + y * step) % 256) as u8])).collect()).unwrap();
            let (one, two) = (paint(320, 200, 3), paint(240, 240, 5));
            let (first, second) = (made(&folder, "one", &one, [160, 100]), made(&folder, "two", &two, [120, 96]));
            let mut damaged = std::fs::read(&first.canvas).unwrap();
            damaged[crate::live::canvas::HEAD] ^= 1;
            std::fs::write(&first.canvas, damaged).unwrap();
            let (events, inbox) = mpsc::channel();
            let out = Captured::default();
            let mut helper = testing(Fake::default(), Arc::new(Mutex::new(Box::new(out.clone()))), events);
            let before = scooch_live::devices_made();
            let screen = |made: &Made| (format!("screen-{}", made.size[0]), Rect { x: 0.0, y: 0.0, w: made.size[0] as f32, h: made.size[1] as f32 });
            for (index, made) in [&first, &second].into_iter().enumerate() {
                let id = index as u32 + 1;
                let (name, rect) = screen(made);
                helper.event(Event::Command(Command::Warm { hints: Hints { present: None, scale: Some(1.0) }, picture: Some(made.picture.clone()), size: Some(made.size), warm_id: Some(id), canvas: Some(made.canvas.clone()), key: None, scales: BTreeMap::new() }));
                helper.event(Event::Command(Command::Open { id, screen: name, rect, scale: 1.0, stills: Stills::of(&made.still) }));
            }
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().filter(|reply| matches!(reply, Reply::Shown { .. } | Reply::Ready { .. })).count() >= 4);
            if replies.iter().any(|reply| matches!(reply, Reply::Failed { why, .. } if why.contains("no device"))) {
                assert!(std::env::var_os("SCOOCH_NEED_ADAPTER").is_none(), "no adapter, and this runner must have one");
                println!("skipped: no adapter");
                return;
            }
            let readies: Vec<(Option<u32>, String)> = replies.iter().filter_map(|reply| match reply {
                Reply::Ready { warm_id, from, .. } => Some((*warm_id, from.clone().unwrap_or_default())),
                _ => None,
            }).collect();
            assert_eq!(readies.iter().map(|(id, _)| *id).collect::<Vec<_>>(), vec![Some(1), Some(2)], "{replies:?}");
            assert!(readies[0].1.starts_with("picture, ") && readies[0].1.contains("fingerprint"), "{readies:?}");
            assert_eq!(readies[1].1, "cache", "{readies:?}");
            for (index, made) in [&first, &second].into_iter().enumerate() {
                helper.event(Event::Command(Command::Glide { id: index as u32 + 1, a: made.tables.0.clone(), b: made.tables.1.clone(), ms: 120 }));
            }
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().filter(|reply| matches!(reply, Reply::Glided { .. })).count() >= 2);
            assert!(!replies.iter().any(|reply| matches!(reply, Reply::Failed { .. })), "{replies:?}");
            for id in [1, 2] {
                helper.event(Event::Command(Command::Close { id }));
            }
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().filter(|reply| matches!(reply, Reply::Closed { .. })).count() >= 2);
            assert_eq!(replies.iter().filter(|reply| matches!(reply, Reply::Closed { .. })).count(), 2, "{replies:?}");
            assert_eq!(scooch_live::devices_made() - before, 1, "the helper made more than one device");
            assert_eq!(helper.pictures.len(), 2);
            for (warm_id, key) in [(3, None), (4, Some(9))] {
                helper.event(Event::Command(Command::Warm { hints: Hints { present: None, scale: Some(1.0) }, picture: Some(second.picture.clone()), size: Some(second.size), warm_id: Some(warm_id), canvas: Some(second.canvas.clone()), key, scales: BTreeMap::new() }));
            }
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Ready { warm_id: Some(4), .. })));
            let from = |wanted: u32| replies.iter().find_map(|reply| match reply {
                Reply::Ready { warm_id: Some(warm_id), from, .. } if *warm_id == wanted => from.clone(),
                _ => None,
            });
            assert_eq!(from(3).as_deref(), Some("kept"), "{replies:?}");
            assert_ne!(from(4).as_deref(), Some("kept"), "a picture rewritten with a new key was answered from the old load {replies:?}");
            let _ = std::fs::remove_dir_all(&folder);
        }

        fn opening(reveal: crate::live::Reveal, decodable: bool) -> Option<(Vec<Reply>, Vec<String>, usize)> {
            let _device = DEVICE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let folder = std::env::temp_dir().join(format!("scooch-commit-{}-{reveal:?}-{decodable}", std::process::id()));
            std::fs::create_dir_all(&folder).unwrap();
            let still = folder.join("still.png");
            if decodable {
                std::fs::write(&still, scooch_core::encode_png(&RgbImage::from_pixel(64, 48, [90, 120, 200].into())).unwrap()).unwrap();
            }
            let (events, inbox) = mpsc::channel();
            let out = Captured::default();
            let log = Arc::new(Mutex::new(Vec::new()));
            let platform = Fake { reveal, log: log.clone(), ..Fake::default() };
            let mut helper = testing(platform, Arc::new(Mutex::new(Box::new(out.clone()))), events);
            helper.event(Event::Command(Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: Some(1), canvas: None, key: None, scales: BTreeMap::new() }));
            helper.event(Event::Command(Command::Open { id: 1, screen: "main".into(), rect: Rect { x: 0.0, y: 0.0, w: 64.0, h: 48.0 }, scale: 1.0, stills: Stills::of(&still) }));
            let started = Instant::now();
            let mut replies = Vec::new();
            while started.elapsed() < Duration::from_secs(90) {
                while let Ok(event) = inbox.try_recv() {
                    helper.event(event);
                }
                helper.advance();
                replies = out.replies();
                let shown = replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 1, .. }));
                let failed = replies.iter().any(|reply| matches!(reply, Reply::Failed { .. }));
                if shown || (failed && helper.layers.is_empty() && helper.pending.is_empty()) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(5));
            }
            let _ = std::fs::remove_dir_all(&folder);
            if replies.iter().any(|reply| matches!(reply, Reply::Failed { why, .. } if why.contains("no device"))) {
                assert!(std::env::var_os("SCOOCH_NEED_ADAPTER").is_none(), "no adapter, and this runner must have one");
                println!("skipped: no adapter");
                return None;
            }
            let seen = log.lock().unwrap_or_else(|poisoned| poisoned.into_inner()).clone();
            Some((replies, seen, helper.platform.closed))
        }

        #[test]
        fn the_default_commits_the_first_frames_transaction_before_presented_and_shows_with_no_spin() {
            let Some((replies, seen, _)) = opening(crate::live::Reveal::from(None), true) else { return };
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 1, .. })), "{replies:?}");
            assert_eq!(seen, vec!["commit".to_string(), "show".to_string()]);
        }

        #[test]
        fn the_spin_switch_gives_the_old_order_show_after_presented_with_no_commit() {
            let Some((replies, seen, _)) = opening(crate::live::Reveal::from(Some("1")), true) else { return };
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 1, .. })), "{replies:?}");
            assert_eq!(seen, vec!["spin".to_string(), "show".to_string()]);
        }

        #[test]
        fn the_drawn_reply_goes_out_before_shown_so_the_daemon_holds_it_at_shown() {
            let Some((replies, _, _)) = opening(crate::live::Reveal::from(None), true) else { return };
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Drawn { id: 1, still: Some(still), .. } if still == "decoded")), "a still with no raw file was not decoded {replies:?}");
            let drawn = replies.iter().position(|reply| matches!(reply, Reply::Drawn { id: 1, .. }));
            let shown = replies.iter().position(|reply| matches!(reply, Reply::Shown { id: 1, .. }));
            assert!(drawn.zip(shown).is_some_and(|(drawn, shown)| drawn < shown), "{replies:?}");
        }

        #[test]
        fn a_first_frame_that_fails_closes_its_layer_with_no_commit_and_no_show() {
            let Some((replies, seen, closed)) = opening(crate::live::Reveal::from(None), false) else { return };
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Failed { id: Some(1), .. })) && !replies.iter().any(|reply| matches!(reply, Reply::Shown { .. })), "{replies:?}");
            assert!(seen.iter().all(|event| event.starts_with("hide ")), "{seen:?}");
            assert_eq!(closed, 1, "{replies:?}");
        }

        #[test]
        fn a_layer_closed_by_the_five_second_rule_is_reported_only_after_its_window_is_gone() {
            let (events, _inbox) = mpsc::channel();
            let out = Captured::default();
            let platform = Fake { watched: Some(out.clone()), ..Fake::default() };
            let mut helper = testing(platform, Arc::new(Mutex::new(Box::new(out.clone()))), events);
            let screen = Screen { id: "idle".into(), frame: Rect { x: 0.0, y: 0.0, w: 64.0, h: 48.0 }, scale: 1.0 };
            let Ok(Build::Whole(layer, _)) = helper.platform.make(&screen, &Hints::default(), None) else { panic!("the fake made no layer") };
            let (jobs, _receiver) = mpsc::channel();
            let idle = Instant::now().checked_sub(LAYER_IDLE + Duration::from_millis(50)).unwrap_or_else(Instant::now);
            helper.layers.insert(7, Live { layer: Some(layer), jobs, split: Split::default(), asked: idle, touched: idle, shown: true, stopped: true, closing: None, told: None, primed: None, drawn: None });
            helper.sweep();
            let replies = out.replies();
            assert_eq!(helper.platform.closed, 1, "{replies:?}");
            assert_eq!(helper.platform.failed_at_close, Some(false), "{replies:?}");
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Failed { id: Some(7), why } if why.starts_with("layer 7 closed after"))), "{replies:?}");
        }

        fn closed_after_stop(stopped: bool) -> (usize, usize, Vec<Reply>) {
            let (events, _inbox) = mpsc::channel();
            let out = Captured::default();
            let mut helper = testing(Fake::default(), Arc::new(Mutex::new(Box::new(out.clone()))), events);
            let screen = Screen { id: "stuck".into(), frame: Rect { x: 0.0, y: 0.0, w: 64.0, h: 48.0 }, scale: 1.0 };
            let Ok(Build::Whole(layer, _)) = helper.platform.make(&screen, &Hints::default(), None) else { panic!("the fake made no layer") };
            let (jobs, _receiver) = mpsc::channel();
            helper.layers.insert(1, Live { layer: Some(layer), jobs, split: Split::default(), asked: Instant::now(), touched: Instant::now(), shown: true, stopped, closing: None, told: None, primed: None, drawn: None });
            helper.begin_close(1, true);
            if let Some(live) = helper.layers.get_mut(&1) {
                live.closing = Instant::now().checked_sub(STOP_WAIT + Duration::from_millis(50)).map(|since| (since, true));
            }
            helper.sweep();
            (helper.platform.closed, helper.platform.abandoned, out.replies())
        }

        #[test]
        fn a_stuck_render_thread_leaves_its_layer_abandoned_not_closed() {
            let (closed, abandoned, replies) = closed_after_stop(false);
            assert_eq!((closed, abandoned), (0, 1));
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Closed { id: 1 })), "{replies:?}");
            let (closed, abandoned, replies) = closed_after_stop(true);
            assert_eq!((closed, abandoned), (1, 0));
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Closed { id: 1 })), "{replies:?}");
        }

        fn waiting(platform: Fake) -> (Helper<Fake>, Receiver<Event>, Captured) {
            let (events, inbox) = mpsc::channel();
            let out = Captured::default();
            let running = Some(Want { key: None, canvas: None, waiters: Vec::new(), composition: false, begun: None });
            let helper = Helper { running, ..testing(platform, Arc::new(Mutex::new(Box::new(out.clone()))), events) };
            (helper, inbox, out)
        }

        fn turn(helper: &mut Helper<Fake>, inbox: &Receiver<Event>, times: usize) {
            for _ in 0..times {
                while let Ok(event) = inbox.try_recv() {
                    helper.event(event);
                }
                helper.advance();
            }
        }

        fn screen(name: &str, width: f32) -> Screen {
            Screen { id: name.into(), frame: Rect { x: 0.0, y: 0.0, w: width, h: 48.0 }, scale: 1.0 }
        }

        fn window_for(screen: &Screen) -> Event {
            Event::Command(Command::Window { screen: screen.id.clone(), rect: screen.frame, scale: screen.scale, stills: Stills::default() })
        }

        fn open_on(id: u32, screen: &Screen) -> Event {
            Event::Command(Command::Open { id, screen: screen.id.clone(), rect: screen.frame, scale: screen.scale, stills: Stills::default() })
        }

        #[test]
        fn an_early_window_is_made_once_and_the_open_on_its_screen_presents_it() {
            let (mut helper, inbox, out) = waiting(Fake::default());
            let one = screen("one", 64.0);
            helper.event(window_for(&one));
            turn(&mut helper, &inbox, 3);
            assert_eq!(helper.platform.made, 1);
            assert!(helper.platform.presented.is_empty(), "the early window was ordered in before the open");
            helper.event(open_on(1, &one));
            turn(&mut helper, &inbox, 3);
            assert_eq!(helper.platform.made, 1, "the open made a second window");
            assert_eq!(helper.platform.presented, vec!["one".to_string()]);
            assert!(helper.early.is_none() && helper.pending.len() == 1 && helper.pending[0].found && helper.pending[0].pane.shown());
            helper.event(Event::Command(Command::Close { id: 1 }));
            assert_eq!((helper.platform.made, helper.platform.closed), (1, 1));
            assert!(out.replies().iter().any(|reply| matches!(reply, Reply::Closed { id: 1 })), "{:?}", out.replies());
        }

        #[test]
        fn an_open_on_another_screen_makes_its_own_window_and_closes_the_early_one() {
            let (mut helper, inbox, _) = waiting(Fake::default());
            let (one, two) = (screen("one", 64.0), screen("two", 80.0));
            helper.event(window_for(&one));
            turn(&mut helper, &inbox, 3);
            helper.event(open_on(1, &two));
            turn(&mut helper, &inbox, 3);
            assert_eq!(helper.platform.gone, vec!["one".to_string()], "the early window was kept");
            assert_eq!(helper.platform.made, 2);
            assert_eq!(helper.platform.presented, vec!["two".to_string()]);
            assert!(!helper.pending[0].found);
            let wider = Screen { frame: Rect { w: 96.0, ..one.frame }, ..one.clone() };
            helper.event(window_for(&one));
            turn(&mut helper, &inbox, 3);
            helper.event(open_on(2, &wider));
            turn(&mut helper, &inbox, 3);
            assert_eq!(helper.platform.gone, vec!["one".to_string(), "one".to_string()], "an early window of another size was kept");
            assert_eq!(helper.platform.made, 4);
            assert_eq!(helper.platform.presented, vec!["two".to_string(), "one".to_string()]);
        }

        #[test]
        fn an_early_helper_sent_nothing_quits_with_no_window_left() {
            let (mut helper, inbox, _) = waiting(Fake::default());
            helper.event(window_for(&screen("one", 64.0)));
            turn(&mut helper, &inbox, 3);
            assert_eq!(helper.platform.made, 1);
            helper.event(Event::Command(Command::Quit));
            helper.sweep();
            assert!(helper.leaving && helper.layers.is_empty() && helper.early.is_none(), "the helper would not quit");
            assert_eq!(helper.platform.closed, 1, "the early window was left behind");
            assert!(helper.platform.presented.is_empty(), "the early window was ordered in");
        }

        #[test]
        fn ready_is_said_before_the_window_is_done_when_the_canvas_is_done_first() {
            let (mut helper, inbox, out) = waiting(Fake { steps: 3, ..Fake::default() });
            helper.running = Some(Want { key: None, canvas: None, waiters: vec![Some(1)], composition: false, begun: None });
            helper.platform.watched = Some(out.clone());
            let events = helper.events.clone();
            helper.platform.during = Some(Box::new(move || {
                let _ = events.send(Event::Warmed { gpu: None, loaded: None, from: Some("cache".into()), ms: 1 });
            }));
            helper.event(window_for(&screen("one", 64.0)));
            turn(&mut helper, &inbox, 5);
            assert_eq!(helper.platform.made, 1);
            assert!(out.replies().iter().any(|reply| matches!(reply, Reply::Ready { warm_id: Some(1), .. })), "{:?}", out.replies());
            assert_eq!(helper.platform.ready_when_made, Some(true), "the window held Ready back until it was done");
        }

        #[test]
        fn a_device_that_never_comes_up_fails_its_warm_after_a_minute_and_the_helper_stops_polling() {
            let (events, _inbox) = mpsc::channel();
            let out = Captured::default();
            let mut helper = testing(Fake::default(), Arc::new(Mutex::new(Box::new(out.clone()))), events);
            helper.parked = true;
            helper.holding = false;
            helper.running = Some(Want { key: None, canvas: None, waiters: vec![Some(5)], composition: false, begun: Some(Instant::now() - WARM_LIMIT + Duration::from_secs(5)) });
            helper.sweep();
            assert!(helper.running.is_some() && helper.pause() == TICK, "a warm inside its minute was given up");
            helper.running.as_mut().unwrap().begun = Some(Instant::now() - WARM_LIMIT - Duration::from_secs(1));
            helper.sweep();
            assert!(out.replies().iter().any(|reply| matches!(reply, Reply::Failed { id: None, why } if why.starts_with("warm 5: "))), "{:?}", out.replies());
            assert!(helper.running.is_none());
            assert_eq!(helper.pause(), REST, "a helper whose device never came up kept polling");
            helper.event(Event::Command(Command::Warm { hints: Hints::default(), picture: None, size: None, warm_id: Some(6), canvas: None, key: None, scales: BTreeMap::new() }));
            assert!(out.replies().iter().any(|reply| matches!(reply, Reply::Failed { id: None, why } if why.starts_with("warm 6: "))), "{:?}", out.replies());
            assert!(helper.running.is_none(), "a stuck helper started another device");
        }

        #[test]
        fn a_parked_helper_rests_with_no_timer_and_lets_its_picture_go_thirty_seconds_after_its_last_command() {
            let (events, _inbox) = mpsc::channel();
            let out = Captured::default();
            let mut helper = testing(Fake::default(), Arc::new(Mutex::new(Box::new(out.clone()))), events.clone());
            helper.parked = true;
            helper.last = Instant::now() - IDLE_EXIT + Duration::from_secs(1);
            let waiting = helper.pause();
            assert!(waiting > Duration::from_millis(500) && waiting <= Duration::from_secs(1) + POLL, "{waiting:?}");
            helper.last = Instant::now() - IDLE_EXIT - Duration::from_secs(1);
            helper.sweep();
            assert!(!helper.leaving && !helper.holding && helper.platform.rested == 1);
            assert_eq!(helper.pause(), REST);
            helper.sweep();
            assert_eq!(helper.platform.rested, 1, "the rest was taken twice");
            helper.event(Event::Command(Command::Hello { version: VERSION }));
            assert!(helper.holding && helper.pause() <= IDLE_EXIT + POLL && helper.pause() > IDLE_EXIT - Duration::from_secs(1), "a command did not bring the timer for the letting go back {:?}", helper.pause());
            let mut passing = testing(Fake::default(), Arc::new(Mutex::new(Box::new(out.clone()))), events);
            passing.last = Instant::now() - IDLE_EXIT - Duration::from_secs(1);
            passing.sweep();
            assert!(passing.leaving, "a helper that was never parked stayed past thirty seconds");
        }

        fn still_files(folder: &Path, raw: bool) -> Stills {
            std::fs::create_dir_all(folder).unwrap();
            let image = RgbImage::from_fn(64, 48, |x, y| [(x * 4) as u8, (y * 5) as u8, 90].into());
            let still = folder.join("still.png");
            std::fs::write(&still, scooch_core::encode_png(&image).unwrap()).unwrap();
            let kept = folder.join("still.png.still");
            if raw {
                crate::live::still::write_rgb(&kept, &image).unwrap();
            }
            Stills { still: Some(still), raw: raw.then_some(kept), tables: None }
        }

        #[test]
        fn a_primed_window_draws_its_first_frame_hidden_and_the_open_only_shows_it() {
            let _device = DEVICE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let folder = std::env::temp_dir().join(format!("scooch-primed-{}", std::process::id()));
            let stills = still_files(&folder, true);
            let (events, inbox) = mpsc::channel();
            let out = Captured::default();
            let log = Arc::new(Mutex::new(Vec::new()));
            let mut helper = testing(Fake { log: log.clone(), ..Fake::default() }, Arc::new(Mutex::new(Box::new(out.clone()))), events);
            let screen = || (String::from("main"), Rect { x: 0.0, y: 0.0, w: 64.0, h: 48.0 });
            helper.event(Event::Command(Command::Park { warm_id: Some(1) }));
            helper.event(Event::Command(Command::Window { screen: screen().0, rect: screen().1, scale: 1.0, stills: stills.clone() }));
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Primed { .. })));
            if replies.iter().any(|reply| matches!(reply, Reply::Failed { why, .. } if why.contains("no device"))) {
                assert!(std::env::var_os("SCOOCH_NEED_ADAPTER").is_none(), "no adapter, and this runner must have one");
                println!("skipped: no adapter");
                return;
            }
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Primed { still, .. } if still == "raw")), "{replies:?}");
            assert_eq!(log.lock().unwrap().clone(), vec!["commit".to_string()], "the primed window was shown");
            assert_eq!(helper.platform.presented, vec!["main".to_string()]);
            helper.event(Event::Command(Command::Open { id: 1, screen: screen().0, rect: screen().1, scale: 1.0, stills: stills.clone() }));
            let replies = out.replies();
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Drawn { id: 1, found: true, still: Some(still), .. } if still == "raw")), "the open did not show the primed window at once {replies:?}");
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 1, .. })), "{replies:?}");
            assert_eq!(log.lock().unwrap().clone(), vec!["commit".to_string(), "show".to_string()]);
            assert_eq!(helper.platform.made, 1, "the open made a second window");
            helper.event(Event::Command(Command::Close { id: 1 }));

            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Closed { id: 1 })));
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Closed { id: 1 })), "{replies:?}");
            helper.event(Event::Command(Command::Open { id: 2, screen: screen().0, rect: screen().1, scale: 1.0, stills: Stills { raw: None, ..stills.clone() } }));
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 2, .. })));
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Drawn { id: 2, still: Some(still), .. } if still == "kept")), "the still drawn for the first open was not kept {replies:?}");
            helper.event(Event::Command(Command::Close { id: 2 }));
            let _ = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Closed { id: 2 })));
            let other = folder.join("other.png");
            std::fs::write(&other, scooch_core::encode_png(&RgbImage::from_fn(64, 48, |x, _| [x as u8, 30, 200].into())).unwrap()).unwrap();
            helper.event(Event::Command(Command::Open { id: 3, screen: screen().0, rect: screen().1, scale: 1.0, stills: Stills { still: Some(other.clone()), raw: None, tables: None } }));
            let replies = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 3, .. })));
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Drawn { id: 3, still: Some(still), .. } if still == "decoded")), "{replies:?}");
            assert!(DECODED.lock().unwrap().contains(&other), "a still with no raw copy was not decoded the way the system decodes it");
            helper.event(Event::Command(Command::Close { id: 3 }));
            let _ = pump(&mut helper, &inbox, &out, |replies| replies.iter().any(|reply| matches!(reply, Reply::Closed { id: 3 })));
            let primes = out.replies().iter().filter(|reply| matches!(reply, Reply::Primed { .. })).count();
            helper.event(Event::Command(Command::Window { screen: screen().0, rect: screen().1, scale: 1.0, stills: stills.clone() }));
            let _ = pump(&mut helper, &inbox, &out, |replies| replies.iter().filter(|reply| matches!(reply, Reply::Primed { .. })).count() > primes);
            helper.platform.covered = true;
            helper.event(Event::Command(Command::Open { id: 9, screen: screen().0, rect: screen().1, scale: 1.0, stills: stills.clone() }));
            let replies = out.replies();
            assert!(replies.iter().any(|reply| matches!(reply, Reply::Failed { id: Some(9), why } if why.contains("cover the whole desktop"))), "a primed window was shown over a covered desktop {replies:?}");
            assert!(!replies.iter().any(|reply| matches!(reply, Reply::Shown { id: 9, .. })), "{replies:?}");
            helper.platform.covered = false;
            let primes = out.replies().iter().filter(|reply| matches!(reply, Reply::Primed { .. })).count();
            helper.event(Event::Command(Command::Window { screen: screen().0, rect: screen().1, scale: 1.0, stills: stills.clone() }));
            let started = Instant::now();
            while out.replies().iter().filter(|reply| matches!(reply, Reply::Primed { .. })).count() <= primes && started.elapsed() < Duration::from_secs(30) {
                while let Ok(event) = inbox.try_recv() {
                    helper.event(event);
                }
                helper.advance();
                std::thread::sleep(Duration::from_millis(5));
            }
            log.lock().unwrap().clear();
            helper.event(Event::Command(Command::Unprime { screen: "elsewhere".into() }));
            assert!(helper.primed.is_some(), "an unprime for another screen dropped this one {:?}", out.replies());
            assert!(log.lock().unwrap().is_empty(), "an unprime for another screen hid a window {:?}", log.lock().unwrap());
            helper.event(Event::Command(Command::Unprime { screen: screen().0 }));
            assert!(helper.primed.is_none(), "the primed window was kept after its screen was cleared");
            assert_eq!(log.lock().unwrap().clone(), vec!["hide main".to_string()], "the primed window was not hidden at once");
            let replies = out.replies();
            let unprimed = replies.iter().rposition(|reply| matches!(reply, Reply::Unprimed { screen } if screen == "main"));
            assert!(unprimed.is_some(), "{replies:?}");
            let _ = std::fs::remove_dir_all(&folder);
        }

        const REEL: [u32; 2] = [240, 150];

        fn warm(shift: u32) -> RgbImage {
            let [width, height] = REEL;
            RgbImage::from_fn(width, height, |x, y| {
                let (dx, dy) = (x as f32 - width as f32 * 0.62, y as f32 - height as f32 * 0.45);
                if (dx * dx + dy * dy).sqrt() < height as f32 * 0.22 {
                    [150, 70 + ((x + shift) % 40) as u8, 60].into()
                } else {
                    [230 - (y * 40 / height) as u8, 170 + (x * 50 / width) as u8, 90 + ((x + y + shift) % 30) as u8].into()
                }
            })
        }

        struct Pair {
            a: scooch_core::Bundle,
            b: scooch_core::Bundle,
            crossed: scooch_core::Bundle,
            stills: [RgbImage; 3],
            image: RgbImage,
        }

        fn pair() -> Pair {
            let [width, height] = REEL;
            let image = warm(0);
            let analysis = scooch_core::analyze(&image);
            let options = scooch_core::Options::default();
            let icon = |x: f32, y: f32| Rect { x, y, w: 30.0, h: 34.0 };
            let first = scooch_core::Scene { width, height, icons: vec![icon(4.0, 4.0), icon(4.0, 60.0), icon(200.0, 110.0)] };
            let second = scooch_core::Scene { width, height, icons: vec![icon(4.0, 4.0), icon(140.0, 52.0), icon(200.0, 110.0)] };
            let a = scooch_core::solve(&analysis, &first, &options, None);
            let b = scooch_core::solve(&analysis, &second, &options, Some(&a));
            let mut flipped = a.clone();
            flipped.fx.iter_mut().for_each(|value| *value = 1.0 - *value);
            flipped.mirrored = !a.mirrored;
            let render = |placement: &scooch_core::Placement, scene: &scooch_core::Scene| scooch_core::render_with(&image, &analysis, placement, scene, &options);
            let stills = [render(&a, &first), render(&b, &second), render(&flipped, &first)];
            let bundle = |placement: &scooch_core::Placement, scene: &scooch_core::Scene| scooch_core::bundle(&image, &analysis, placement, scene, &options);
            Pair { a: bundle(&a, &first), b: bundle(&b, &second), crossed: bundle(&flipped, &first), stills, image }
        }

        fn luma(pixel: &[u8]) -> f32 {
            0.2126 * f32::from(pixel[0]) + 0.7152 * f32::from(pixel[1]) + 0.0722 * f32::from(pixel[2])
        }

        fn mean_luma(image: &RgbImage) -> f32 {
            image.as_raw().as_chunks::<3>().0.iter().map(|pixel| luma(pixel)).sum::<f32>() / (image.width() * image.height()).max(1) as f32
        }

        fn darker_than_its_pictures(frame: &RgbImage, pictures: &[RgbImage]) -> Option<String> {
            let dark = frame.as_raw().as_chunks::<3>().0.iter().filter(|pixel| pixel.iter().all(|channel| *channel <= 24)).count();
            let (mean, floor) = (mean_luma(frame), pictures.iter().map(mean_luma).fold(f32::MAX, f32::min) - 8.0);
            (dark > 0 || mean < floor).then(|| format!("{dark} near black pixels, mean lightness {mean:.1} against {floor:.1} below the darker picture"))
        }

        enum Cue {
            Glide(usize, f32),
            Settle(usize, u32),
            Ready,
            Failed,
            Broken,
        }

        fn play(gpu: &Gpu, pair: &Pair, name: &str, cues: &[(u64, Cue)]) -> Vec<String> {
            let [width, height] = REEL;
            let picture = gpu.picture(&scooch_core::canvas(&pair.image, width, height));
            let stills: Vec<Arc<Still>> = pair.stills.iter().map(|image| Arc::new(gpu.still(image).expect("still"))).collect();
            let glides = [(&pair.a, &pair.b), (&pair.a, &pair.crossed), (&pair.b, &pair.a)];
            let start = Instant::now();
            let mut reel = Reel::new(stills[0].clone());
            let mut scale = 1.0f32;
            let mut waiting: Option<usize> = None;
            let mut found = Vec::new();
            let mut check = |layers: &[(Shown, f32)], scale: f32, at: u64, step: &str| {
                let (frame, cover) = gpu.snapshot_cover_at(width, height, &looks(layers), scale).expect("snapshot");
                if let Some(why) = darker_than_its_pictures(&frame, &pair.stills) {
                    found.push(format!("{name} at {at} ms, {step} frame of {} layers at scale {scale}: {why}", layers.len()));
                }
                if cover < u8::MAX {
                    found.push(format!("{name} at {at} ms, {step} frame of {} layers at scale {scale}: a pixel covers only {cover} of 255, so the see through layer would show what lies under it", layers.len()));
                }
            };
            let last = cues.iter().map(|(at, _)| *at).max().unwrap_or(0) + 1200;
            let mut at = 0u64;
            while at <= last {
                let now = start + Duration::from_millis(at);
                for (_, cue) in cues.iter().filter(|(when, _)| *when == at) {
                    match cue {
                        Cue::Glide(index, chosen) => {
                            scale = *chosen;
                            let (a, b) = glides[*index];
                            reel.glide(gpu.glide(&picture, a, b).expect("glide"), 450, now, now);
                        }
                        Cue::Settle(index, ms) => {
                            waiting = Some(*index);
                            reel.settle(*ms);
                        }
                        Cue::Ready => {
                            if let Some(index) = waiting.take() {
                                reel.ready(stills[index].clone());
                            }
                        }
                        Cue::Failed => {
                            waiting = None;
                            reel.unsettle();
                        }
                        Cue::Broken => reel.halt(),
                    }
                }
                if let Some((layers, step)) = reel.next(now) {
                    check(&layers, scale, at, &format!("{step:?}"));
                    if step == Step::Glided {
                        let _ = gpu.time_at(width, height, &looks(&layers), scale);
                    }
                    if step == Step::Settled {
                        for _ in 0..FLUSH {
                            check(&layers, scale, at, "flush");
                        }
                    }
                    reel.presented(layers, step, now, Duration::from_millis(25));
                }
                at += 16;
            }
            found
        }

        #[test]
        fn no_frame_of_any_sequence_is_darker_than_both_of_its_pictures() {
            let _device = DEVICE.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            let gpu = match Gpu::new(false) {
                Ok(gpu) => gpu,
                Err(why) => {
                    assert!(std::env::var_os("SCOOCH_NEED_ADAPTER").is_none(), "no adapter, and this runner must have one");
                    println!("skipped: no adapter {why}");
                    return;
                }
            };
            let pair = pair();
            assert!(scooch_core::compatible(&pair.a, &pair.b) && !scooch_core::compatible(&pair.a, &pair.crossed), "the pair does not hold one glide and one cross");
            let sequences: Vec<(&str, Vec<(u64, Cue)>)> = vec![
                ("glide then settle", vec![(0, Cue::Glide(0, 1.0)), (96, Cue::Settle(1, SETTLE)), (560, Cue::Ready)]),
                ("cross then settle", vec![(0, Cue::Glide(1, 1.0)), (96, Cue::Settle(2, SETTLE)), (560, Cue::Ready)]),
                ("glide then settle back", vec![(0, Cue::Glide(0, 1.0)), (480, Cue::Settle(0, SETTLE)), (496, Cue::Ready)]),
                ("settle ready before the glide ends", vec![(0, Cue::Glide(0, 0.6)), (64, Cue::Settle(1, SETTLE)), (160, Cue::Ready)]),
                ("failed decode then settle back", vec![(0, Cue::Glide(0, 0.6)), (96, Cue::Settle(1, SETTLE)), (512, Cue::Failed), (608, Cue::Settle(0, SETTLE)), (640, Cue::Ready)]),
                ("scale change mid glide", vec![(0, Cue::Glide(0, 0.6)), (208, Cue::Glide(2, 1.0)), (304, Cue::Settle(0, SETTLE)), (800, Cue::Ready)]),
                ("new picture fades in", vec![(0, Cue::Settle(1, 450)), (32, Cue::Ready)]),
                ("present broken mid glide then settle back", vec![(0, Cue::Glide(0, 0.5)), (160, Cue::Broken), (320, Cue::Settle(0, SETTLE)), (336, Cue::Ready)]),
                ("glide during the fade", vec![(0, Cue::Settle(1, 450)), (16, Cue::Ready), (208, Cue::Glide(2, 0.75)), (320, Cue::Settle(0, SETTLE)), (720, Cue::Ready)]),
            ];
            let mut found = Vec::new();
            for (name, cues) in &sequences {
                found.extend(play(&gpu, &pair, name, cues));
            }
            assert!(found.is_empty(), "{} frames were darker than both of their pictures {found:#?}", found.len());
        }

        #[test]
        fn a_line_that_cannot_be_read_is_answered() {
            assert!(read("").is_err_and(|answer| matches!(*answer, Reply::Failed { id: None, .. })));
            assert!(read("{\"kind\":\"warm\"").is_err_and(|answer| matches!(*answer, Reply::Failed { id: None, .. })));
            assert!(matches!(read(line(&Command::Quit).unwrap().trim()), Ok(Command::Quit)));
        }

        #[test]
        fn the_ease_starts_and_ends_at_rest() {
            assert_eq!(ease(0.0), 0.0);
            assert_eq!(ease(1.0), 1.0);
            assert!((ease(0.5) - 0.5).abs() < 1e-6);
            assert!(ease(0.01) < 0.001 && ease(0.99) > 0.999);
            let mut last = 0.0;
            for step in 0..=100 {
                let now = ease(step as f32 / 100.0);
                assert!(now >= last);
                last = now;
            }
        }
    }
}
