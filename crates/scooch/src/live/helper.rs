use super::{line, Reply, VERSION};
use std::io::Write;
use std::process::ExitCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const IDLE_EXIT: Duration = Duration::from_secs(30);
pub const LAYER_IDLE: Duration = Duration::from_secs(5);

pub type Waker = Arc<dyn Fn() + Send + Sync>;

type Out = Arc<Mutex<std::io::Stdout>>;

fn say(out: &Out, reply: &Reply) {
    if let Ok(mut out) = out.lock() {
        let _ = out.write_all(line(reply).as_bytes());
        let _ = out.flush();
    }
}

pub fn refuse(why: &str) -> ExitCode {
    let out: Out = Arc::new(Mutex::new(std::io::stdout()));
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
    use super::{say, Out, Waker, IDLE_EXIT, LAYER_IDLE};
    use crate::live::{Command, Hints, Reply, VERSION};
    use crate::platform::Screen;
    use anyhow::Result;
    use scooch_core::{Bundle, Canvas, RgbImage};
    use scooch_live::{Glide, Gpu, Look, Picture, Still, Target};
    use std::collections::BTreeMap;
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

    pub trait Platform: 'static {
        type Layer;
        fn composition(&self, hints: &Hints) -> Result<bool>;
        fn open(&mut self, screen: &Screen, hints: &Hints) -> Result<Self::Layer>;
        fn target(&self, layer: &Self::Layer) -> Target;
        fn pixels(&self, layer: &Self::Layer) -> (u32, u32);
        fn period(&self, _layer: &Self::Layer) -> Duration {
            Duration::from_micros(16_667)
        }
        fn show(&mut self, layer: &Self::Layer) -> Result<()>;
        fn close(&mut self, layer: Self::Layer);
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
    }

    pub fn ease(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    struct Surface(Target);

    unsafe impl Send for Surface {}

    #[derive(Clone)]
    struct Loaded {
        canvas: Arc<Canvas>,
        picture: Picture,
    }

    enum Job {
        Glide { a: PathBuf, b: PathBuf, ms: u32, loaded: Option<Loaded>, asked: Instant },
        Settle { still: PathBuf, ms: u32 },
        Stop,
    }

    enum Event {
        Command(Command),
        HangUp,
        Warmed { gpu: Option<Result<Gpu, String>>, composition: bool, loaded: Option<Result<Loaded, String>>, ms: u64 },
        Presented { id: u32, result: Result<(), String> },
        Glided { id: u32, frames: u32, slow: u32, first: u64 },
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

    struct Pending {
        id: u32,
        screen: Screen,
        still: Option<PathBuf>,
        asked: Instant,
    }

    struct Helper<P: Platform> {
        platform: P,
        out: Out,
        events: Sender<Event>,
        waker: Waker,
        hints: Hints,
        gpu: Option<(Gpu, bool)>,
        loaded: Option<Loaded>,
        uploads: u32,
        warming: bool,
        waiting: usize,
        pending: Vec<Pending>,
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
        still: Option<PathBuf>,
        size: (u32, u32),
        period: Duration,
    }

    fn load_tables(path: &Path, canvas: &Canvas) -> Result<Bundle, String> {
        let bytes = std::fs::read(path).map_err(|error| format!("could not read {} {error}", path.display()))?;
        Bundle::tables_from_bytes(&bytes, canvas.clone()).map_err(|error| format!("{} {error}", path.display()))
    }

    fn render<P: Platform>(start: Start, jobs: Receiver<Job>, events: Sender<Event>, waker: Waker) {
        let tell = |event: Event| {
            let _ = events.send(event);
            waker();
        };
        let Start { id, gpu, surface, still, size, period } = start;
        let made = still
            .ok_or_else(|| "open needs the still the system shows now".to_string())
            .and_then(|path| P::decode(&path).map_err(|error| format!("{error:#}")))
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
                Some(Job::Glide { a, b, ms, loaded, asked }) => {
                    let built = loaded.ok_or_else(|| "glide needs a warm with a picture first".to_string()).and_then(|loaded| {
                        let (a, b) = (load_tables(&a, &loaded.canvas)?, load_tables(&b, &loaded.canvas)?);
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
            let frame: Option<Vec<(Shown, f32)>> = if let Some(moving) = motion.as_mut() {
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
                } else {
                    layers.push((Shown::Glide(moving.glide.clone(), 1.0), s));
                }
                Some(layers)
            } else if let Some(settle) = settling.as_mut() {
                if let Some(still) = settle.still.clone() {
                    if settle.started.is_none() {
                        settle.from = shown.clone();
                        settle.started = Some(Instant::now());
                    }
                    let started = settle.started.unwrap_or_else(Instant::now);
                    let u = (started.elapsed().as_secs_f32() / settle.length.as_secs_f32()).min(1.0);
                    let mut layers = settle.from.clone();
                    layers.push((Shown::Still(still), u));
                    Some(layers)
                } else {
                    None
                }
            } else {
                None
            };
            let Some(layers) = frame else { continue };
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
                if moving.started.elapsed() >= moving.length {
                    shown = vec![(Shown::Glide(moving.glide.clone(), 1.0), 1.0)];
                    tell(Event::Glided { id, frames: moving.frames, slow: moving.slow, first: moving.first.unwrap_or(0) });
                    motion = None;
                }
            } else if let Some(settle) = settling.as_ref() {
                if let (Some(still), Some(started)) = (&settle.still, settle.started) {
                    if started.elapsed() >= settle.length {
                        shown = vec![(Shown::Still(still.clone()), 1.0)];
                        tell(Event::Settled { id });
                        settling = None;
                    }
                }
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

        fn warm(&mut self, hints: Hints, picture: Option<PathBuf>, size: Option<[u32; 2]>, answer: bool) {
            let composition = match self.platform.composition(&hints) {
                Ok(composition) => composition,
                Err(error) => {
                    self.fail(None, format!("{error:#}"));
                    return;
                }
            };
            self.hints = hints;
            let device = self.gpu.as_ref().filter(|(_, made)| *made == composition).map(|(gpu, _)| gpu.clone());
            if device.is_some() && picture.is_none() {
                if answer {
                    let adapter = device.map(|gpu| gpu.name).unwrap_or_default();
                    self.tell(Reply::Ready { adapter, ms: 0, uploads: self.uploads });
                }
                return;
            }
            if answer {
                self.waiting += 1;
            }
            if self.warming && picture.is_none() {
                return;
            }
            self.warming = true;
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new().name("scooch-live-warm".into()).spawn(move || {
                let started = Instant::now();
                let made = device.is_none().then(|| Gpu::new(composition));
                let usable = device.clone().or_else(|| made.clone().and_then(Result::ok));
                let loaded = picture.map(|path| {
                    let gpu = usable.ok_or_else(|| "no device".to_string())?;
                    gpu.ready()?;
                    let image = P::decode(&path).map_err(|error| format!("{error:#}"))?;
                    let [width, height] = size.unwrap_or([image.width(), image.height()]);
                    let canvas = scooch_core::canvas(&image, width.max(1), height.max(1));
                    let picture = gpu.picture(&canvas);
                    Ok(Loaded { canvas: Arc::new(canvas), picture })
                });
                let _ = events.send(Event::Warmed { gpu: made, composition, loaded, ms: started.elapsed().as_millis() as u64 });
                waker();
            });
            if spawned.is_err() {
                self.warming = false;
                self.fail(None, "could not start the thread that makes the device");
            }
        }

        fn open(&mut self, pending: Pending) {
            let id = pending.id;
            if self.layers.contains_key(&id) {
                self.fail(Some(id), format!("layer {id} is already open"));
                return;
            }
            let wanted = self.platform.composition(&self.hints).unwrap_or(false);
            let gpu = match &self.gpu {
                Some((gpu, made)) if *made == wanted => gpu.clone(),
                _ => {
                    self.pending.push(pending);
                    if !self.warming {
                        let hints = self.hints.clone();
                        self.warm(hints, None, None, false);
                    }
                    return;
                }
            };
            let layer = match self.platform.open(&pending.screen, &self.hints) {
                Ok(layer) => layer,
                Err(error) => {
                    self.fail(Some(id), format!("{error:#}"));
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
                self.platform.close(layer);
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
                Command::Warm { hints, picture, size } => self.warm(hints, picture, size, true),
                Command::Open { id, screen, rect, scale, still } => self.open(Pending { id, screen: Screen { id: screen, frame: rect, scale }, still, asked: Instant::now() }),
                Command::Glide { id, a, b, ms } => {
                    let job = Job::Glide { a, b, ms, loaded: self.loaded.clone(), asked: Instant::now() };
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

        fn warmed(&mut self, gpu: Option<Result<Gpu, String>>, composition: bool, loaded: Option<Result<Loaded, String>>, ms: u64) {
            self.warming = false;
            let mut problem = None;
            match gpu {
                Some(Ok(gpu)) => {
                    self.gpu = Some((gpu, composition));
                    self.loaded = None;
                }
                Some(Err(why)) => problem = Some(format!("no device {why}")),
                None => {}
            }
            match loaded {
                Some(Ok(loaded)) => {
                    self.loaded = Some(loaded);
                    self.uploads += 1;
                }
                Some(Err(why)) => problem = Some(format!("the picture could not be loaded {why}")),
                None => {}
            }
            let wanted = self.platform.composition(&self.hints).unwrap_or(composition);
            if problem.is_none() && self.gpu.as_ref().is_some_and(|(_, made)| *made != wanted) {
                let hints = self.hints.clone();
                self.warm(hints, None, None, false);
                return;
            }
            if self.waiting > 0 {
                self.waiting = 0;
                match &problem {
                    Some(why) => self.fail(None, why.clone()),
                    None => {
                        let adapter = self.gpu.as_ref().map(|(gpu, _)| gpu.name.clone()).unwrap_or_default();
                        self.tell(Reply::Ready { adapter, ms, uploads: self.uploads });
                    }
                }
            }
            for pending in std::mem::take(&mut self.pending) {
                if self.gpu.is_some() {
                    self.open(pending);
                } else {
                    self.fail(Some(pending.id), problem.clone().unwrap_or_else(|| "no device".into()));
                }
            }
        }

        fn event(&mut self, event: Event) {
            match event {
                Event::Command(command) => self.command(command),
                Event::HangUp => self.leaving = true,
                Event::Warmed { gpu, composition, loaded, ms } => self.warmed(gpu, composition, loaded, ms),
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
                Event::Glided { id, frames, slow, first } => {
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.touched = Instant::now();
                    }
                    self.tell(Reply::Glided { id, frames, slow_frames: slow, first_frame_ms: first, uploads: self.uploads });
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

    pub fn serve<P: Platform>(platform: P) -> ExitCode {
        let out: Out = Arc::new(Mutex::new(std::io::stdout()));
        say(&out, &Reply::Hello { version: VERSION, pid: std::process::id() });
        let (events, inbox) = mpsc::channel();
        let waker = platform.waker();
        let (reader, wake) = (events.clone(), waker.clone());
        let reply = out.clone();
        let started = std::thread::Builder::new().name("scooch-live-protocol".into()).spawn(move || {
            for text in std::io::stdin().lock().lines() {
                let Ok(text) = text else { break };
                if text.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Command>(&text) {
                    Ok(command) => {
                        if reader.send(Event::Command(command)).is_err() {
                            return;
                        }
                    }
                    Err(error) => say(&reply, &Reply::Failed { id: None, why: format!("unreadable command {error}") }),
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
        let mut helper = Helper { platform, out, events, waker, hints: Hints::default(), gpu: None, loaded: None, uploads: 0, warming: false, waiting: 0, pending: Vec::new(), layers: BTreeMap::new(), last: Instant::now(), leaving: false };
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
        use super::ease;

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
