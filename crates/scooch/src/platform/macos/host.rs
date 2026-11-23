use super::{layer, picture};
use crate::live::helper::{self, Build, Commit, Platform, Waker};
use crate::live::{Hints, Reveal};
use crate::platform::Screen;
use anyhow::Result;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use scooch_core::RgbImage;
use scooch_live::Target;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Duration, Instant};

const PERIOD: Duration = Duration::from_micros(16_667);
const SETTLE_IN: Duration = Duration::from_millis(34);
const COMPOSITIONS: u32 = 2;

pub struct Host {
    marker: MainThreadMarker,
    busy: Option<super::Busy>,
    app: bool,
    reveal: Reveal,
    revealed: Vec<(&'static str, u64)>,
}

impl Platform for Host {
    type Layer = layer::Layer;
    type Making = layer::Making;

    fn composition(&self, _hints: &Hints) -> Result<bool> {
        Ok(false)
    }

    fn make(&mut self, screen: &Screen, _hints: &Hints, making: Option<layer::Making>) -> Result<Build<layer::Making, layer::Layer>> {
        let making = match making {
            Some(making) => making,
            None if !self.app => {
                self.begin();
                return Ok(Build::Partly(layer::Making::Start, "app"));
            }
            None => layer::Making::Start,
        };
        layer::step(self.marker, screen, making)
    }

    fn present(&mut self, layer: &layer::Layer) -> Result<()> {
        objc2::rc::autoreleasepool(|_| {
            if self.reveal == Reveal::Transaction {
                layer.transact(true);
            }
            layer.order()
        });
        Ok(())
    }

    fn visible(&mut self, layer: &layer::Layer, waited: Duration) -> Result<bool> {
        objc2::rc::autoreleasepool(|_| layer.waited(waited))
    }

    fn begin(&mut self) {
        if !self.app {
            objc2::rc::autoreleasepool(|_| {
                NSApplication::sharedApplication(self.marker).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            });
            self.app = true;
        }
    }

    fn target(&self, layer: &layer::Layer) -> Target {
        Target::Layer(layer.surface())
    }

    fn pixels(&self, layer: &layer::Layer) -> (u32, u32) {
        layer.pixels
    }

    fn period(&self, layer: &layer::Layer) -> Duration {
        layer::period(layer.display).unwrap_or(PERIOD)
    }

    fn refresh(&self) -> Duration {
        layer::period(layer::main_display()).unwrap_or(PERIOD)
    }

    fn show(&mut self, layer: &layer::Layer) -> Result<()> {
        let started = Instant::now();
        match self.reveal {
            Reveal::Transaction => {
                layer.transact(false);
                layer.reveal();
                self.revealed = vec![("unhide", started.elapsed().as_millis() as u64)];
            }
            Reveal::Spin => {
                layer::spin(SETTLE_IN.as_secs_f64());
                let spun = started.elapsed();
                layer.reveal();
                self.revealed = vec![("spin", spun.as_millis() as u64), ("unhide", started.elapsed().saturating_sub(spun).as_millis() as u64)];
            }
        }
        Ok(())
    }

    fn revealed(&mut self) -> Vec<(&'static str, u64)> {
        std::mem::take(&mut self.revealed)
    }

    fn screen_gpu(&self, layer: &layer::Layer) -> Option<String> {
        objc2::rc::autoreleasepool(|_| layer::display_gpu(layer.display))
    }

    fn committer(&self) -> Option<Commit> {
        (self.reveal == Reveal::Transaction).then(|| Box::new(layer::commit) as Commit)
    }

    fn close(&mut self, layer: layer::Layer) {
        layer.close();
    }

    fn abandon(&mut self, layer: layer::Layer) {
        layer.abandon();
    }

    fn wait(&mut self, timeout: Duration) {
        objc2::rc::autoreleasepool(|_| layer::wait(timeout));
    }

    fn waker(&self) -> Waker {
        Arc::new(layer::nudge)
    }

    fn composed() {
        std::thread::sleep(layer::period(layer::main_display()).unwrap_or(PERIOD) * COMPOSITIONS);
    }

    fn prepare(&mut self) {
        self.busy = Some(super::busy());
    }

    fn rest(&mut self) {
        self.busy = None;
    }

    fn decode(path: &Path) -> Result<RgbImage> {
        objc2::rc::autoreleasepool(|_| picture::decode(path))
    }

    fn frame<T>(work: impl FnOnce() -> T) -> T {
        layer::present(work)
    }
}

pub fn serve() -> ExitCode {
    let Some(marker) = MainThreadMarker::new() else {
        return helper::refuse("the helper has to run on the main thread");
    };
    helper::serve(Host { marker, busy: None, app: false, reveal: Reveal::chosen(), revealed: Vec::new() })
}
