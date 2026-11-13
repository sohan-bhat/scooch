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
    #[cfg(not(windows))]
    refuse("this system has no live layer yet, so Scooch keeps the plain swap")
}

#[cfg(any(windows, target_os = "macos"))]
pub use running::{serve, Platform};

#[cfg(any(windows, target_os = "macos"))]
mod running {
    use super::{say, Out, Waker, IDLE_EXIT, LAYER_IDLE};
    use crate::live::{Command, Hints, Reply, TestPattern, VERSION};
    use crate::platform::Screen;
    use anyhow::Result;
    use scooch_live::{Gpu, Pattern, Renderer, Target};
    use std::collections::BTreeMap;
    use std::io::BufRead;
    use std::path::PathBuf;
    use std::process::ExitCode;
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};

    const TICK: Duration = Duration::from_millis(50);
    const STOP_WAIT: Duration = Duration::from_secs(2);
    const SLOW: Duration = Duration::from_millis(25);
    const PLAY_LIMIT: u32 = 10_000;

    pub trait Platform: 'static {
        type Layer;
        fn composition(&self, hints: &Hints) -> Result<bool>;
        fn open(&mut self, screen: &Screen, hints: &Hints) -> Result<Self::Layer>;
        fn target(&self, layer: &Self::Layer) -> Target;
        fn pixels(&self, layer: &Self::Layer) -> (u32, u32);
        fn show(&mut self, layer: &Self::Layer) -> Result<()>;
        fn close(&mut self, layer: Self::Layer);
        fn wait(&mut self, timeout: Duration);
        fn waker(&self) -> Waker;
        fn frame<T>(work: impl FnOnce() -> T) -> T
        where
            Self: Sized,
        {
            work()
        }
    }

    struct Surface(Target);

    unsafe impl Send for Surface {}

    enum Job {
        Play { pattern: TestPattern, ms: u32 },
        Stop,
    }

    enum Event {
        Command(Command),
        HangUp,
        Warmed { gpu: Result<Gpu, String>, composition: bool, ms: u64 },
        Presented { id: u32, result: Result<(), String> },
        Played { id: u32, frames: u32, slow: u32, failed: Option<String> },
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
        picture: Option<PathBuf>,
        asked: Instant,
    }

    struct Helper<P: Platform> {
        platform: P,
        out: Out,
        events: Sender<Event>,
        waker: Waker,
        hints: Hints,
        gpu: Option<(Gpu, bool)>,
        warming: Option<bool>,
        waiting: usize,
        pending: Vec<Pending>,
        layers: BTreeMap<u32, Live<P::Layer>>,
        last: Instant,
        leaving: bool,
    }

    fn picture(path: Option<&PathBuf>, size: (u32, u32)) -> Result<Pattern, String> {
        let (width, height) = size;
        let Some(path) = path else {
            return Ok(Pattern { width, height, rgba: [128u8, 128, 128, 255].repeat(width as usize * height as usize) });
        };
        let image = scooch_core::open(path).map_err(|error| format!("could not read {} {error}", path.display()))?;
        let (from_width, from_height) = (image.width().max(1), image.height().max(1));
        let raw = image.as_raw();
        let mut rgba = Vec::with_capacity(width as usize * height as usize * 4);
        for y in 0..height {
            let source_y = (u64::from(y) * u64::from(from_height) / u64::from(height.max(1))) as usize;
            for x in 0..width {
                let source_x = (u64::from(x) * u64::from(from_width) / u64::from(width.max(1))) as usize;
                let at = (source_y * from_width as usize + source_x) * 3;
                rgba.extend_from_slice(&[raw[at], raw[at + 1], raw[at + 2], 255]);
            }
        }
        Ok(Pattern { width, height, rgba })
    }

    fn play<P: Platform>(renderer: &mut Renderer, pattern: TestPattern, ms: u32, jobs: &Receiver<Job>) -> (u32, u32, Option<String>, bool) {
        let cycle = Duration::from_millis(u64::from(pattern.cycle_ms.max(1)));
        let length = Duration::from_millis(u64::from(ms.min(PLAY_LIMIT)));
        let started = Instant::now();
        let (mut frames, mut slow, mut last) = (0u32, 0u32, started);
        while started.elapsed() < length {
            if let Ok(Job::Stop) = jobs.try_recv() {
                return (frames, slow, None, true);
            }
            let bar = (started.elapsed().as_secs_f32() % cycle.as_secs_f32()) / cycle.as_secs_f32();
            if let Err(why) = P::frame(|| renderer.frame(Some(bar))) {
                return (frames, slow, Some(why), false);
            }
            let now = Instant::now();
            if frames > 0 && now - last > SLOW {
                slow += 1;
            }
            frames += 1;
            last = now;
        }
        (frames, slow, None, false)
    }

    struct Start {
        id: u32,
        gpu: Gpu,
        surface: Surface,
        picture: Option<PathBuf>,
        size: (u32, u32),
    }

    fn render<P: Platform>(start: Start, jobs: Receiver<Job>, events: Sender<Event>, waker: Waker) {
        let tell = |event: Event| {
            let _ = events.send(event);
            waker();
        };
        let Start { id, gpu, surface, picture: path, size } = start;
        let made = picture(path.as_ref(), size).and_then(|pattern| P::frame(|| Renderer::on(&gpu, surface.0, &pattern)));
        let mut renderer = match made {
            Ok(renderer) => renderer,
            Err(why) => {
                tell(Event::Presented { id, result: Err(why) });
                tell(Event::Stopped { id });
                return;
            }
        };
        let first = P::frame(|| renderer.frame(None));
        let presented = first.is_ok();
        tell(Event::Presented { id, result: first });
        if presented {
            while let Ok(job) = jobs.recv() {
                match job {
                    Job::Play { pattern, ms } => {
                        let (frames, slow, failed, stop) = play::<P>(&mut renderer, pattern, ms, &jobs);
                        tell(Event::Played { id, frames, slow, failed });
                        if stop {
                            break;
                        }
                    }
                    Job::Stop => break,
                }
            }
        }
        drop(renderer);
        tell(Event::Stopped { id });
    }

    impl<P: Platform> Helper<P> {
        fn tell(&self, reply: Reply) {
            say(&self.out, &reply);
        }

        fn fail(&self, id: Option<u32>, why: impl Into<String>) {
            self.tell(Reply::Failed { id, why: why.into() });
        }

        fn warm(&mut self, hints: Hints, answer: bool) {
            let composition = match self.platform.composition(&hints) {
                Ok(composition) => composition,
                Err(error) => {
                    self.fail(None, format!("{error:#}"));
                    return;
                }
            };
            self.hints = hints;
            if let Some((gpu, made)) = &self.gpu {
                if *made == composition {
                    if answer {
                        let adapter = gpu.name.clone();
                        self.tell(Reply::Ready { adapter, ms: 0 });
                    }
                    return;
                }
            }
            if answer {
                self.waiting += 1;
            }
            if self.warming.is_some() {
                return;
            }
            self.warming = Some(composition);
            let (events, waker) = (self.events.clone(), self.waker.clone());
            let spawned = std::thread::Builder::new().name("scooch-live-warm".into()).spawn(move || {
                let started = Instant::now();
                let gpu = Gpu::new(composition);
                let _ = events.send(Event::Warmed { gpu, composition, ms: started.elapsed().as_millis() as u64 });
                waker();
            });
            if spawned.is_err() {
                self.warming = None;
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
                    let hints = self.hints.clone();
                    self.warm(hints, false);
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
            let start = Start { id, gpu, surface: Surface(self.platform.target(&layer)), picture: pending.picture, size: self.platform.pixels(&layer) };
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

        fn command(&mut self, command: Command) {
            self.last = Instant::now();
            match command {
                Command::Hello { version } if version != VERSION => {
                    self.fail(None, format!("the helper speaks version {VERSION}, not {version}"));
                    self.leaving = true;
                }
                Command::Hello { .. } => {}
                Command::Warm { hints } => self.warm(hints, true),
                Command::Open { id, screen, rect, scale, picture } => self.open(Pending { id, screen: Screen { id: screen, frame: rect, scale }, picture, asked: Instant::now() }),
                Command::Play { id, pattern, ms } => match self.layers.get_mut(&id) {
                    Some(live) if live.shown && live.closing.is_none() => {
                        live.touched = Instant::now() + Duration::from_millis(u64::from(ms.min(PLAY_LIMIT)));
                        let _ = live.jobs.send(Job::Play { pattern, ms });
                    }
                    Some(_) => self.fail(Some(id), format!("layer {id} is not on screen yet")),
                    None => self.fail(Some(id), format!("layer {id} is not open")),
                },
                Command::Close { id } => self.begin_close(id, true),
                Command::Quit => self.leaving = true,
            }
        }

        fn event(&mut self, event: Event) {
            match event {
                Event::Command(command) => self.command(command),
                Event::HangUp => self.leaving = true,
                Event::Warmed { gpu, composition, ms } => {
                    self.warming = None;
                    match gpu {
                        Ok(gpu) => {
                            let adapter = gpu.name.clone();
                            self.gpu = Some((gpu, composition));
                            if self.platform.composition(&self.hints).is_ok_and(|wanted| wanted != composition) {
                                let hints = self.hints.clone();
                                self.warm(hints, false);
                                return;
                            }
                            if self.waiting > 0 {
                                self.waiting = 0;
                                self.tell(Reply::Ready { adapter, ms });
                            }
                            for pending in std::mem::take(&mut self.pending) {
                                self.open(pending);
                            }
                        }
                        Err(why) => {
                            if self.waiting > 0 {
                                self.waiting = 0;
                                self.fail(None, format!("no device {why}"));
                            }
                            for pending in std::mem::take(&mut self.pending) {
                                self.fail(Some(pending.id), format!("no device {why}"));
                            }
                        }
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
                Event::Played { id, frames, slow, failed } => {
                    if let Some(live) = self.layers.get_mut(&id) {
                        live.touched = Instant::now();
                    }
                    match failed {
                        Some(why) => {
                            self.fail(Some(id), why);
                            self.begin_close(id, false);
                        }
                        None => self.tell(Reply::Played { id, frames, slow_frames: slow }),
                    }
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
        let mut helper = Helper { platform, out, events, waker, hints: Hints::default(), gpu: None, warming: None, waiting: 0, pending: Vec::new(), layers: BTreeMap::new(), last: Instant::now(), leaving: false };
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
}
