use super::{line, Command, Reply, VERSION};
use std::io::Write;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const IDLE_EXIT: Duration = Duration::from_secs(30);
pub const LAYER_IDLE: Duration = Duration::from_secs(5);

pub type Waker = Arc<dyn Fn() + Send + Sync>;

type Out = Arc<Mutex<Box<dyn Write + Send>>>;

fn say(out: &Out, reply: &Reply) {
    let text = line(reply).unwrap_or_else(|error| line(&Reply::Failed { id: None, why: format!("an answer could not be written {error:#}") }).unwrap_or_default());
    if let Ok(mut out) = out.lock() {
        let _ = out.write_all(text.as_bytes());
        let _ = out.flush();
    }
}

pub fn read(text: &str) -> Result<Command, Reply> {
    serde_json::from_str::<Command>(text).map_err(|error| Reply::Failed { id: None, why: format!("unreadable command {error}") })
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
pub use running::{serve, Platform};

#[cfg(any(windows, target_os = "macos"))]
mod running {
    use super::{read, say, Out, Waker, IDLE_EXIT, LAYER_IDLE};
    use crate::live::{warm_failed, Command, Hints, Reply, VERSION};
    use crate::platform::Screen;
    use anyhow::Result;
    use scooch_core::{Bundle, Canvas, RgbImage};
    use scooch_live::{Glide, Gpu, Look, Picture, Still, Target};
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

    pub trait Platform: 'static {
        type Layer;
        fn composition(&self, hints: &Hints) -> Result<bool>;
        fn open(&mut self, screen: &Screen, hints: &Hints) -> Result<Self::Layer>;
        fn target(&self, layer: &Self::Layer) -> Target;
        fn pixels(&self, layer: &Self::Layer) -> (u32, u32);
        fn period(&self, _layer: &Self::Layer) -> Duration {
            Duration::from_micros(16_667)
        }
        fn refresh(&self) -> Duration {
            Duration::from_micros(16_667)
        }
        fn show(&mut self, layer: &Self::Layer) -> Result<()>;
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
        canvas: Arc<Canvas>,
        picture: Picture,
    }

    type Key = (PathBuf, [u32; 2]);

    struct Want {
        key: Option<Key>,
        canvas: Option<PathBuf>,
        waiters: Vec<Option<u32>>,
        composition: bool,
    }

    enum Decoding {
        Thread(std::thread::JoinHandle<Result<RgbImage, String>>),
        Path(PathBuf),
    }

    impl Decoding {
        fn start<P: Platform>(path: PathBuf) -> Decoding {
            let moved = path.clone();
            match std::thread::Builder::new().name("scooch-live-still".into()).spawn(move || P::decode(&moved).map_err(|error| format!("{error:#}"))) {
                Ok(handle) => Decoding::Thread(handle),
                Err(_) => Decoding::Path(path),
            }
        }

        fn finish<P: Platform>(self) -> Result<RgbImage, String> {
            match self {
                Decoding::Thread(handle) => handle.join().unwrap_or_else(|_| Err("the still could not be decoded".into())),
                Decoding::Path(path) => P::decode(&path).map_err(|error| format!("{error:#}")),
            }
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
        Timed { timed: Option<Duration> },
        Presented { id: u32, result: Result<(), String> },
        Glided { id: u32, frames: u32, slow: u32, first: u64, scale: f32, timed: Option<Duration> },
        Settled { id: u32 },
        Broke { id: u32, why: String },
        Stopped { id: u32 },
    }

    struct Live<L> {
        layer: Option<L>,
        jobs: Sender<Job>,
        asked: Instant,
        touched: Instant,
        shown: bool,
        stopped: bool,
        closing: Option<(Instant, bool)>,
    }

    struct Pending<L> {
        id: u32,
        screen: Screen,
        still: Option<Decoding>,
        asked: Instant,
        layer: Option<L>,
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
        uploads: u32,
        running: Option<Want>,
        queue: VecDeque<Want>,
        pending: Vec<Pending<P::Layer>>,
        layers: BTreeMap<u32, Live<P::Layer>>,
        last: Instant,
        leaving: bool,
    }

    #[derive(Clone)]
    enum Shown {
        Still(Arc<Still>),
        Glide(Arc<Glide>, f32),
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
        incoming: Option<Receiver<Result<RgbImage, String>>>,
        still: Option<Arc<Still>>,
        length: Duration,
        from: Vec<(Shown, f32)>,
        started: Option<Instant>,
    }

    struct Start {
        id: u32,
        gpu: Gpu,
        surface: Surface,
        still: Option<Decoding>,
        size: (u32, u32),
        period: Duration,
    }

    fn load_pair(a: &Path, b: &Path, pictures: &[Loaded]) -> Result<(Bundle, Bundle, Loaded), String> {
        let read = |path: &Path| std::fs::read(path).map_err(|error| format!("could not read {} {error}", path.display()));
        let (first, second) = (read(a)?, read(b)?);
        let mut last = "glide needs a warm with a picture first".to_string();
        for loaded in pictures {
            match Bundle::tables_from_bytes(&first, (*loaded.canvas).clone()) {
                Ok(one) => {
                    let two = Bundle::tables_from_bytes(&second, (*loaded.canvas).clone()).map_err(|error| format!("{} {error}", b.display()))?;
                    return Ok((one, two, loaded.clone()));
                }
                Err(error) => last = format!("{} {error}", a.display()),
            }
        }
        Err(last)
    }

    fn render<P: Platform>(start: Start, jobs: Receiver<Job>, events: Sender<Event>, waker: Waker) {
        let tell = |event: Event| {
            let _ = events.send(event);
            waker();
        };
        let Start { id, gpu, surface, still, size, period } = start;
        let made = still
            .ok_or_else(|| "open needs the still the system shows now".to_string())
            .and_then(Decoding::finish::<P>)
            .and_then(|image| gpu.still(&image))
            .and_then(|first| gpu.screen(surface.0, size.0, size.1).map(|screen| (first, screen)));
        let (first, mut screen) = match made {
            Ok(made) => made,
            Err(why) => {
                tell(Event::Presented { id, result: Err(why) });
                tell(Event::Stopped { id });
                return;
            }
        };
        let mut shown = vec![(Shown::Still(Arc::new(first)), 1.0f32)];
        let result = P::frame(|| screen.show(&looks(&shown)));
        let ok = result.is_ok();
        tell(Event::Presented { id, result });
        let slow_after = period.mul_f32(1.5);
        let mut motion: Option<Motion> = None;
        let mut settling: Option<Settling> = None;
        let mut stop = !ok;
        while !stop {
            let animating = motion.is_some() || settling.as_ref().is_some_and(|settle| settle.started.is_some() || settle.still.is_some() && motion.is_none());
            let job = if animating {
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
                    let built = load_pair(&a, &b, &loaded).and_then(|(a, b, loaded)| {
                        let compatible = scooch_core::compatible(&a, &b);
                        gpu.glide(&loaded.picture, &a, &b).map(|glide| (glide, compatible))
                    });
                    match built {
                        Ok((glide, compatible)) => {
                            let from = motion.take().map(|moving| vec![(Shown::Glide(moving.glide, 1.0), 1.0)]).unwrap_or_else(|| shown.clone());
                            motion = Some(Motion { glide: Arc::new(glide), compatible, from, started: Instant::now(), length: Duration::from_millis(u64::from(ms.clamp(1, LONGEST))), asked, frames: 0, slow: 0, first: None, last: None });
                        }
                        Err(why) => tell(Event::Broke { id, why }),
                    }
                }
                Some(Job::Settle { still, ms }) => {
                    let (sender, incoming) = mpsc::channel();
                    let spawned = std::thread::Builder::new().name("scooch-live-decode".into()).spawn(move || {
                        let _ = sender.send(P::decode(&still).map_err(|error| format!("{error:#}")));
                    });
                    match spawned {
                        Ok(_) => settling = Some(Settling { incoming: Some(incoming), still: None, length: Duration::from_millis(u64::from(ms.clamp(1, LONGEST))), from: Vec::new(), started: None }),
                        Err(error) => tell(Event::Broke { id, why: format!("could not start the decode thread {error}") }),
                    }
                }
                None => {}
            }
            if stop {
                break;
            }
            if let Some(settle) = settling.as_mut() {
                if let Some(incoming) = &settle.incoming {
                    match incoming.try_recv() {
                        Ok(Ok(image)) => {
                            settle.incoming = None;
                            match gpu.still(&image) {
                                Ok(still) => settle.still = Some(Arc::new(still)),
                                Err(why) => {
                                    tell(Event::Broke { id, why });
                                    settling = None;
                                }
                            }
                        }
                        Ok(Err(why)) => {
                            tell(Event::Broke { id, why });
                            settling = None;
                        }
                        Err(mpsc::TryRecvError::Empty) => {}
                        Err(mpsc::TryRecvError::Disconnected) => {
                            tell(Event::Broke { id, why: "the still could not be decoded".into() });
                            settling = None;
                        }
                    }
                }
            }
            let frame: Option<(Vec<(Shown, f32)>, bool)> = if let Some(moving) = motion.as_mut() {
                let elapsed = moving.started.elapsed();
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
                Some((layers, t >= 1.0))
            } else if let Some(settle) = settling.as_mut() {
                if let Some(still) = settle.still.clone() {
                    if settle.started.is_none() {
                        settle.from = shown.clone();
                        settle.started = Some(Instant::now());
                    }
                    let started = settle.started.unwrap_or_else(Instant::now);
                    let u = (started.elapsed().as_secs_f32() / settle.length.as_secs_f32()).min(1.0);
                    if u >= 1.0 {
                        Some((vec![(Shown::Still(still), 1.0)], true))
                    } else {
                        let mut layers = settle.from.clone();
                        layers.push((Shown::Still(still), u));
                        Some((layers, false))
                    }
                } else {
                    None
                }
            } else {
                None
            };
            let Some((layers, last)) = frame else { continue };
            let presented = P::frame(|| screen.show(&looks(&layers)));
            let now = Instant::now();
            if let Err(why) = presented {
                tell(Event::Broke { id, why });
                motion = None;
                settling = None;
                continue;
            }
            if let Some(moving) = motion.as_mut() {
                if moving.first.is_none() {
                    moving.first = Some(moving.asked.elapsed().as_millis() as u64);
                }
                if moving.last.is_some_and(|last| now - last > slow_after) {
                    moving.slow += 1;
                }
                moving.last = Some(now);
                moving.frames += 1;
                if last {
                    let scale = screen.scale;
                    let timed = gpu.time_at(screen.width, screen.height, &looks(&layers), scale).ok();
                    shown = layers;
                    tell(Event::Glided { id, frames: moving.frames, slow: moving.slow, first: moving.first.unwrap_or(0), scale, timed });
                    motion = None;
                }
            } else if last {
                for _ in 0..FLUSH {
                    if let Err(why) = P::frame(|| screen.show(&looks(&layers))) {
                        tell(Event::Broke { id, why });
                        break;
                    }
                }
                let _ = gpu.idle(COMPOSE_WAIT);
                P::composed();
                shown = layers;
                tell(Event::Settled { id });
                settling = None;
            }
        }
        drop(screen);
        tell(Event::Stopped { id });
    }

    impl<P: Platform> Helper<P> {
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
            if self.timing || self.pinned.is_some() || !self.layers.is_empty() {
                return;
            }
            let (Some((gpu, _)), Some(([width, height], picture))) = (self.gpu.as_ref().map(|(gpu, made)| (gpu.clone(), *made)), self.untimed.take()) else { return };
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new().name("scooch-live-time".into()).spawn(move || {
                let heavy = scooch_live::heavy(width.max(1), height.max(1), &picture);
                let timed = gpu.glide(&picture, &heavy, &heavy).ok().and_then(|glide| {
                    let look = [(Look::Glide(&glide, 0.5), 1.0)];
                    let _ = gpu.time(width.max(1), height.max(1), &look);
                    gpu.time(width.max(1), height.max(1), &look).ok()
                });
                let _ = events.send(Event::Timed { timed });
                waker();
            });
            self.timing = spawned.is_ok();
        }

        fn has(&self, key: &Option<Key>) -> bool {
            self.gpu.is_some() && key.as_ref().is_none_or(|key| self.pictures.iter().any(|(kept, _)| kept == key))
        }

        fn warm(&mut self, hints: Hints, (picture, canvas): (Option<PathBuf>, Option<PathBuf>), size: Option<[u32; 2]>, warm_id: Option<u32>, answer: bool) {
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
            let key = picture.map(|path| (path, size.unwrap_or([0, 0])));
            let waiters = if answer { vec![warm_id] } else { Vec::new() };
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
            let want = Want { key, canvas, waiters, composition };
            if self.running.is_none() {
                self.start(want);
            } else {
                self.queue.push_back(want);
            }
        }

        fn start(&mut self, want: Want) {
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let device = self.gpu.as_ref().map(|(gpu, _)| gpu.clone());
            let (key, cached, composition) = (want.key.clone(), want.canvas.clone(), want.composition);
            let spawned = std::thread::Builder::new().name("scooch-live-warm".into()).spawn(move || {
                let started = Instant::now();
                let reading = key.as_ref().and(cached).map(|path| std::thread::spawn(move || crate::live::canvas::read(&path).map(Arc::new).map_err(|error| format!("{error:#}"))));
                let made = device.is_none().then(|| Gpu::new(composition));
                let usable = device.clone().or_else(|| made.clone().and_then(Result::ok));
                let mut from = None;
                let loaded = key.map(|(path, [width, height])| {
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
                                std::thread::spawn(move || cached.canvas().map_err(|error| format!("{error:#}")))
                            };
                            let picture = gpu.try_picture_levels(cached.width, cached.height, &cached.levels());
                            match (checking.join().unwrap_or_else(|_| Err("the canvas check stopped".into())), picture) {
                                (Ok(canvas), Ok(picture)) => Ok((canvas, picture)),
                                (Err(why), _) | (_, Err(why)) => Err(why),
                            }
                        }
                        Some(Err(why)) => Err(why),
                        None => Err(String::new()),
                    };
                    let (canvas, picture) = match cached {
                        Ok(made) => {
                            from = Some("cache".to_string());
                            made
                        }
                        Err(why) => {
                            from = Some(if why.is_empty() { "picture".to_string() } else { format!("picture, {why}") });
                            let image = P::decode(&path).map_err(|error| format!("{error:#}"))?;
                            let (width, height) = if width == 0 || height == 0 { (image.width(), image.height()) } else { (width, height) };
                            gpu.fits(width.max(1), height.max(1))?;
                            let canvas = scooch_core::canvas(&image, width.max(1), height.max(1));
                            let picture = gpu.try_picture(&canvas)?;
                            (canvas, picture)
                        }
                    };
                    Ok(Loaded { canvas: Arc::new(canvas), picture })
                });
                let _ = events.send(Event::Warmed { gpu: made, loaded, from, ms: started.elapsed().as_millis() as u64 });
                waker();
            });
            match spawned {
                Ok(_) => self.running = Some(want),
                Err(error) => {
                    for waiter in want.waiters {
                        self.fail(None, warm_failed(waiter, &format!("could not start the thread that makes the device {error}")));
                    }
                }
            }
        }

        fn open(&mut self, mut pending: Pending<P::Layer>) {
            let id = pending.id;
            if self.layers.contains_key(&id) || self.pending.iter().any(|waiting| waiting.id == id) {
                self.fail(Some(id), format!("layer {id} is already open"));
                return;
            }
            let layer = match pending.layer.take().map(Ok).unwrap_or_else(|| self.platform.open(&pending.screen, &self.hints)) {
                Ok(layer) => layer,
                Err(error) => {
                    self.fail(Some(id), format!("{error:#}"));
                    return;
                }
            };
            let gpu = match &self.gpu {
                Some((gpu, _)) => gpu.clone(),
                None => {
                    pending.layer = Some(layer);
                    self.pending.push(pending);
                    if self.running.is_none() {
                        let hints = self.hints.clone();
                        self.warm(hints, (None, None), None, None, false);
                    }
                    return;
                }
            };
            let start = Start { id, gpu, surface: Surface(self.platform.target(&layer)), still: pending.still, size: self.platform.pixels(&layer), period: self.platform.period(&layer) };
            let (jobs, receiver) = mpsc::channel();
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new().name(format!("scooch-live-render-{id}")).spawn(move || render::<P>(start, receiver, events, waker));
            if let Err(error) = spawned {
                self.platform.close(layer);
                self.fail(Some(id), format!("could not start the render thread {error}"));
                return;
            }
            let now = Instant::now();
            self.layers.insert(id, Live { layer: Some(layer), jobs, asked: pending.asked, touched: now, shown: false, stopped: false, closing: None });
        }

        fn begin_close(&mut self, id: u32, answer: bool) {
            if let Some(index) = self.pending.iter().position(|pending| pending.id == id) {
                if let Some(layer) = self.pending.remove(index).layer {
                    self.platform.close(layer);
                }
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
            }
            if live.stopped {
                self.finish(id);
            }
        }

        fn finish(&mut self, id: u32) {
            let Some(mut live) = self.layers.remove(&id) else { return };
            if let Some(layer) = live.layer.take() {
                if live.stopped {
                    self.platform.close(layer);
                } else {
                    self.platform.abandon(layer);
                }
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
            match command {
                Command::Hello { version } if version != VERSION => {
                    self.fail(None, format!("the helper speaks version {VERSION}, not {version}"));
                    self.leaving = true;
                }
                Command::Hello { .. } => {}
                Command::Warm { hints, picture, size, warm_id, canvas, scales } => {
                    if !scales.is_empty() {
                        self.scales = scales;
                    }
                    self.warm(hints, (picture, canvas), size, warm_id, true)
                }
                Command::Open { id, screen, rect, scale, still } => {
                    let still = still.map(Decoding::start::<P>);
                    self.open(Pending { id, screen: Screen { id: screen, frame: rect, scale }, still, asked: Instant::now(), layer: None })
                }
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
                        let size = if key.1[0] > 0 && key.1[1] > 0 { key.1 } else { [loaded.canvas.width, loaded.canvas.height] };
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
            for mut pending in std::mem::take(&mut self.pending) {
                if self.gpu.is_some() {
                    self.open(pending);
                } else {
                    if let Some(layer) = pending.layer.take() {
                        self.platform.close(layer);
                    }
                    self.fail(Some(pending.id), problem.clone().unwrap_or_else(|| "no device".into()));
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

        fn event(&mut self, event: Event) {
            match event {
                Event::Command(command) => self.command(command),
                Event::HangUp => self.leaving = true,
                Event::Warmed { gpu, loaded, from, ms } => self.warmed(gpu, loaded, from, ms),
                Event::Timed { timed } => {
                    self.timing = false;
                    if let (Some(timed), None, None) = (timed, self.pinned, self.timed) {
                        self.timed = Some(timed);
                        self.scale = scooch_live::choose(timed, self.platform.refresh());
                    }
                }
                Event::Presented { id, result } => {
                    let Some(live) = self.layers.get(&id) else { return };
                    if live.closing.is_some() {
                        return;
                    }
                    let shown = match (result, live.layer.as_ref()) {
                        (Ok(()), Some(layer)) => self.platform.show(layer).map_err(|error| format!("{error:#}")),
                        (Ok(()), None) => Err("the layer window is gone".to_string()),
                        (Err(why), _) => Err(why),
                    };
                    match shown {
                        Ok(()) => {
                            if let Some(live) = self.layers.get_mut(&id) {
                                live.shown = true;
                                live.touched = Instant::now();
                                let ms = live.asked.elapsed().as_millis() as u64;
                                self.tell(Reply::Shown { id, ms });
                            }
                        }
                        Err(why) => {
                            self.fail(Some(id), why);
                            self.begin_close(id, false);
                        }
                    }
                }
                Event::Glided { id, frames, slow, first, scale, timed } => {
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
                Event::Settled { id } => {
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.touched = Instant::now();
                    }
                    self.tell(Reply::Settled { id });
                }
                Event::Broke { id, why } => {
                    self.fail(Some(id), why);
                    self.begin_close(id, false);
                }
                Event::Stopped { id } => {
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.stopped = true;
                        if live.closing.is_some() {
                            self.finish(id);
                        }
                    }
                }
            }
        }

        fn sweep(&mut self) {
            if self.last.elapsed() > IDLE_EXIT {
                self.leaving = true;
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
                        self.fail(Some(id), format!("layer {id} closed after {} s without a command", LAYER_IDLE.as_secs()));
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
        let mut helper = Helper { platform, out, events, waker, hints: Hints::default(), scale: 1.0, pinned: None, timed: None, scales: BTreeMap::new(), untimed: None, timing: false, gpu: None, pictures: Vec::new(), uploads: 0, running: None, queue: VecDeque::new(), pending: Vec::new(), layers: BTreeMap::new(), last: Instant::now(), leaving: false };
        loop {
            helper.platform.wait(TICK);
            while let Ok(event) = inbox.try_recv() {
                helper.event(event);
            }
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
        }

        #[cfg(target_os = "macos")]
        struct Drawn {
            metal: objc2::rc::Retained<objc2_quartz_core::CAMetalLayer>,
            size: (u32, u32),
        }

        #[cfg(windows)]
        struct Drawn {
            window: isize,
            size: (u32, u32),
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

            fn composition(&self, _hints: &Hints) -> Result<bool> {
                Ok(false)
            }

            fn open(&mut self, screen: &Screen, _hints: &Hints) -> Result<Drawn> {
                let size = (screen.frame.w as u32, screen.frame.h as u32);
                #[cfg(target_os = "macos")]
                return Ok(Drawn { metal: objc2_quartz_core::CAMetalLayer::new(), size });
                #[cfg(windows)]
                return Ok(Drawn { window: window(size.0, size.1)?, size });
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
                Ok(())
            }

            #[cfg(windows)]
            fn close(&mut self, layer: Drawn) {
                self.closed += 1;
                let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::DestroyWindow(windows::Win32::Foundation::HWND(layer.window as *mut std::ffi::c_void)) };
            }

            #[cfg(target_os = "macos")]
            fn close(&mut self, layer: Drawn) {
                self.closed += 1;
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

            fn waker(&self) -> Waker {
                Arc::new(|| {})
            }

            fn decode(path: &Path) -> Result<RgbImage> {
                Ok(scooch_core::open(path)?)
            }
        }

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
            let mut helper = Helper { platform: Fake::default(), out: Arc::new(Mutex::new(Box::new(out.clone()))), events, waker: Arc::new(|| {}), hints: Hints::default(), scale: 1.0, pinned: Some(1.0), timed: None, scales: BTreeMap::new(), untimed: None, timing: false, gpu: None, pictures: Vec::new(), uploads: 0, running: None, queue: VecDeque::new(), pending: Vec::new(), layers: BTreeMap::new(), last: Instant::now(), leaving: false };
            let before = scooch_live::devices_made();
            let screen = |made: &Made| (format!("screen-{}", made.size[0]), Rect { x: 0.0, y: 0.0, w: made.size[0] as f32, h: made.size[1] as f32 });
            for (index, made) in [&first, &second].into_iter().enumerate() {
                let id = index as u32 + 1;
                let (name, rect) = screen(made);
                helper.event(Event::Command(Command::Warm { hints: Hints { present: None, scale: Some(1.0) }, picture: Some(made.picture.clone()), size: Some(made.size), warm_id: Some(id), canvas: Some(made.canvas.clone()), scales: BTreeMap::new() }));
                helper.event(Event::Command(Command::Open { id, screen: name, rect, scale: 1.0, still: Some(made.still.clone()) }));
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
            let _ = std::fs::remove_dir_all(&folder);
        }

        fn closed_after_stop(stopped: bool) -> (usize, usize, Vec<Reply>) {
            let (events, _inbox) = mpsc::channel();
            let out = Captured::default();
            let mut helper = Helper { platform: Fake::default(), out: Arc::new(Mutex::new(Box::new(out.clone()))), events, waker: Arc::new(|| {}), hints: Hints::default(), scale: 1.0, pinned: Some(1.0), timed: None, scales: BTreeMap::new(), untimed: None, timing: false, gpu: None, pictures: Vec::new(), uploads: 0, running: None, queue: VecDeque::new(), pending: Vec::new(), layers: BTreeMap::new(), last: Instant::now(), leaving: false };
            let screen = Screen { id: "stuck".into(), frame: Rect { x: 0.0, y: 0.0, w: 64.0, h: 48.0 }, scale: 1.0 };
            let layer = helper.platform.open(&screen, &Hints::default()).unwrap();
            let (jobs, _receiver) = mpsc::channel();
            helper.layers.insert(1, Live { layer: Some(layer), jobs, asked: Instant::now(), touched: Instant::now(), shown: true, stopped, closing: None });
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

        #[test]
        fn a_line_that_cannot_be_read_is_answered() {
            assert!(matches!(read(""), Err(Reply::Failed { id: None, .. })));
            assert!(matches!(read("{\"kind\":\"warm\""), Err(Reply::Failed { id: None, .. })));
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
