use super::{layer, picture};
use crate::live::helper::{self, Build, Platform, Waker};
use crate::live::Hints;
use crate::platform::Screen;
use anyhow::Result;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use scooch_core::RgbImage;
use scooch_live::Target;
use std::path::Path;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

const PERIOD: Duration = Duration::from_micros(16_667);
const SETTLE_IN: Duration = Duration::from_millis(34);
const COMPOSITIONS: u32 = 2;

pub struct Host {
    marker: MainThreadMarker,
    busy: Option<super::Busy>,
    app: bool,
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
        objc2::rc::autoreleasepool(|_| layer.order());
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
        layer::spin(SETTLE_IN.as_secs_f64());
        layer.reveal();
        Ok(())
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
    helper::serve(Host { marker, busy: None, app: false })
}
