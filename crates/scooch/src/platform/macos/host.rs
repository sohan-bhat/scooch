use super::{layer, picture};
use crate::live::helper::{self, Platform, Waker};
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

pub struct Host {
    marker: MainThreadMarker,
}

impl Platform for Host {
    type Layer = layer::Layer;

    fn composition(&self, _hints: &Hints) -> Result<bool> {
        Ok(false)
    }

    fn open(&mut self, screen: &Screen, _hints: &Hints) -> Result<layer::Layer> {
        layer::open(self.marker, screen)
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

    fn wait(&mut self, timeout: Duration) {
        layer::wait(timeout);
    }

    fn waker(&self) -> Waker {
        Arc::new(layer::nudge)
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
    NSApplication::sharedApplication(marker).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    helper::serve(Host { marker })
}
