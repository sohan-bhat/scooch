//! The Scooch engine for web pages, built for WebAssembly with wasm-bindgen.
//!
//! A page draws a screen with `frame.glsl` from a bundle's tables. This crate gives the page those
//! tables for a screen and its icons, with the same session the desktop uses, and draws nothing.
//!
//! ```text
//! Engine.create(picture: RGB bytes, width, height, analysis: bytes) -> Engine
//! engine.screen(width, height)
//! engine.drop(icons: Float32Array of x, y, w, h ...) -> Tables
//! engine.drag(icons: Float32Array, held: index, budget_ms) -> Tables
//! engine.free()
//! ```
//!
//! `picture` is the canvas the export wrote, decoded to RGB, and `analysis` is the export's
//! `analysis.bin`. `drop` solves with the previous placement and is the full answer. `drag` is the
//! quick answer while an icon is held, within `budget_ms`, and a `drop` follows it. `held` names the
//! held icon and is kept for later use.
//!
//! [`Tables`] holds one answer. Every array is a fresh copy out of the engine's memory, ready to
//! upload as is, and matches the texture table in the `scooch-core` docs:
//!
//! | Call | Type | Length | Texture |
//! |---|---|---|---|
//! | `map()` | `Float32Array` | `mapWidth * 2`, row 0 `xmap`, row 1 `ymap`, padded with the last value | `map_a`, `map_b`, R32F, `mapWidth` x 2 |
//! | `field()` | `Uint16Array` | `gridWidth * gridHeight` half floats | `field_a`, `field_b`, R16F |
//! | `look()` | `Uint8Array` | `gridWidth * gridHeight * 4` | `look_a`, `look_b`, RGBA8 unorm |
//! | `pad()` | `Uint8Array` | `gridWidth * gridHeight * 4` | `pad_a`, `pad_b`, RGBA8 sRGB |
//! | `paints()` | `Uint8Array` | `paintWidth * paintHeight * 4 * 8`, layer after layer | `paints_a`, `paints_b`, RGBA8 sRGB array of 8 layers |
//! | `ramps()` | `Uint8Array` | `512 * 2 * 4` | `ramps_a`, `ramps_b`, RGBA8 unorm, 512 x 2 |
//! | `constants()` | `Float32Array` | 28 | the uniform block `Frame` of `frame.wgsl`, in order |
//! | `canvas()` | `Uint8Array` | `canvasWidth * canvasHeight * 3`, RGB | `canvas`, only needed when `canvasFits` is false |
//!
//! The 28 constants are `size` (2), `grid` (2), `paint` (2), `s`, `unit`, `ramp_low`, `ramp_high`,
//! `reach`, `grain`, `grain_scale`, `grain_floor`, `grain_lod`, `core_lift`, `core_drop`, `volume_lit`,
//! `volume_dark`, `spare`, `shadow` (2), `rim` (2), `belly` (2) and `unused` (2). `s` is 0, `spare` and
//! `unused` are 0, and `grain_lod` is `log2(4 * canvasWidth / width)`. In `frame.glsl` the same values
//! go to the uniforms of the same names, with `paint_grid`, `grain_amount`, `shadow_at`, `rim_at` and
//! `belly_at` for `paint`, `grain`, `shadow`, `rim` and `belly`.
//!
//! The sizes are `width`, `height`, `canvasWidth`, `canvasHeight`, `gridWidth`, `gridHeight`,
//! `paintWidth`, `paintHeight` and `mapWidth`, which is `max(width, height)`. `canvasFits` is true when
//! the canvas is the picture the page passed to `create`, so the page's canvas texture still matches.
//! `mirrored` tells whether the placement is mirrored.
//!
//! The WebAssembly build turns on scooch-core's `portable` feature, so exp, ln, pow, cbrt and atan2
//! come from the `libm` crate instead of the system's math library. Its tables are byte equal to a
//! native build with the same feature, and every call repeats to the bit, but they may differ from a
//! desktop build's tables and stills in the last bit.

use scooch_core::{Analysis, Bundle, Options, Placement, Rect, RgbImage, Scene, Scenery, Session};
use std::sync::Arc;
use std::time::Duration;
use wasm_bindgen::prelude::*;

pub const CONSTANTS: usize = 28;

pub struct Core {
    picture: RgbImage,
    analysis: Analysis,
    session: Session,
    width: u32,
    height: u32,
    placement: Option<Placement>,
    options: Options,
}

pub struct Answer {
    pub bundle: Arc<Bundle>,
    pub fits: bool,
}

impl Core {
    pub fn new(picture: &[u8], width: u32, height: u32, analysis: &[u8]) -> Result<Core, String> {
        let picture = RgbImage::from_raw(width, height, picture.to_vec()).ok_or_else(|| format!("the picture is not {width} x {height} RGB"))?;
        let analysis = Analysis::from_bytes(analysis).map_err(|e| e.to_string())?;
        Ok(Core { picture, analysis, session: Session::new(), width: 0, height: 0, placement: None, options: Options { scenery: Scenery::Auto, ..Options::default() } })
    }

    pub fn screen(&mut self, width: u32, height: u32) {
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.placement = None;
            self.session.forget();
        }
    }

    fn scene(&self, icons: &[f32]) -> Scene {
        let icons = icons.as_chunks::<4>().0.iter().map(|r| Rect { x: r[0], y: r[1], w: r[2], h: r[3] }).collect();
        Scene { width: self.width, height: self.height, icons }
    }

    pub fn drop_icons(&mut self, icons: &[f32]) -> Answer {
        let scene = self.scene(icons);
        let placement = scooch_core::solve(&self.analysis, &scene, &self.options, self.placement.as_ref());
        self.answer(&scene, placement)
    }

    pub fn drag(&mut self, icons: &[f32], budget: Duration, elapsed: &dyn Fn() -> Duration) -> Answer {
        let scene = self.scene(icons);
        let placement = match &self.placement {
            Some(previous) => scooch_core::solve_near_timed(&self.analysis, &scene, &self.options, previous, budget, elapsed),
            None => scooch_core::solve(&self.analysis, &scene, &self.options, None),
        };
        self.answer(&scene, placement)
    }

    fn answer(&mut self, scene: &Scene, placement: Placement) -> Answer {
        let bundle = self.session.tables_shared(&self.picture, &self.analysis, &placement, scene, &self.options);
        self.placement = Some(placement);
        let fits = (bundle.canvas_width, bundle.canvas_height) == self.picture.dimensions();
        Answer { bundle, fits }
    }
}

pub fn map(b: &Bundle) -> Vec<f32> {
    let long = b.width.max(b.height) as usize;
    let mut out = vec![0f32; long * 2];
    for (k, row) in [&b.xmap, &b.ymap].iter().enumerate() {
        if let Some(last) = row.last() {
            for (i, v) in out[k * long..(k + 1) * long].iter_mut().enumerate() {
                *v = *row.get(i).unwrap_or(last) as f32;
            }
        }
    }
    out
}

pub fn field(b: &Bundle) -> Vec<u16> {
    b.field.iter().map(|v| scooch_core::half_bits(*v)).collect()
}

pub fn constants(b: &Bundle) -> Vec<f32> {
    let c = &b.constants;
    let lod = if b.width > 0 { (4.0 * b.canvas_width as f32 / b.width as f32).log2() } else { 0.0 };
    vec![
        b.width as f32,
        b.height as f32,
        b.grid_width as f32,
        b.grid_height as f32,
        b.paint_width as f32,
        b.paint_height as f32,
        0.0,
        c.unit,
        c.ramp_low,
        c.ramp_high,
        c.reach,
        c.grain,
        c.grain_scale,
        c.grain_floor,
        lod,
        c.core_lift,
        c.core_drop,
        c.volume_lit,
        c.volume_dark,
        0.0,
        c.shadow[0],
        c.shadow[1],
        c.rim[0],
        c.rim[1],
        c.belly[0],
        c.belly[1],
        0.0,
        0.0,
    ]
}

#[wasm_bindgen]
pub struct Engine {
    core: Core,
}

#[wasm_bindgen]
impl Engine {
    pub fn create(picture: &[u8], width: u32, height: u32, analysis: &[u8]) -> Result<Engine, JsError> {
        Core::new(picture, width, height, analysis).map(|core| Engine { core }).map_err(|e| JsError::new(&e))
    }

    pub fn screen(&mut self, width: u32, height: u32) {
        self.core.screen(width, height);
    }

    #[wasm_bindgen(js_name = drop)]
    pub fn drop_icons(&mut self, icons: &[f32]) -> Tables {
        Tables { answer: self.core.drop_icons(icons) }
    }

    pub fn drag(&mut self, icons: &[f32], held: u32, budget_ms: f64) -> Tables {
        let _ = held;
        let started = js_sys::Date::now();
        let elapsed = || Duration::from_secs_f64(((js_sys::Date::now() - started) / 1000.0).max(0.0));
        Tables { answer: self.core.drag(icons, Duration::from_secs_f64(budget_ms.max(0.0) / 1000.0), &elapsed) }
    }
}

#[wasm_bindgen]
pub struct Tables {
    answer: Answer,
}

#[wasm_bindgen]
impl Tables {
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.answer.bundle.width
    }

    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.answer.bundle.height
    }

    #[wasm_bindgen(getter, js_name = canvasWidth)]
    pub fn canvas_width(&self) -> u32 {
        self.answer.bundle.canvas_width
    }

    #[wasm_bindgen(getter, js_name = canvasHeight)]
    pub fn canvas_height(&self) -> u32 {
        self.answer.bundle.canvas_height
    }

    #[wasm_bindgen(getter, js_name = gridWidth)]
    pub fn grid_width(&self) -> u32 {
        self.answer.bundle.grid_width
    }

    #[wasm_bindgen(getter, js_name = gridHeight)]
    pub fn grid_height(&self) -> u32 {
        self.answer.bundle.grid_height
    }

    #[wasm_bindgen(getter, js_name = paintWidth)]
    pub fn paint_width(&self) -> u32 {
        self.answer.bundle.paint_width
    }

    #[wasm_bindgen(getter, js_name = paintHeight)]
    pub fn paint_height(&self) -> u32 {
        self.answer.bundle.paint_height
    }

    #[wasm_bindgen(getter, js_name = mapWidth)]
    pub fn map_width(&self) -> u32 {
        self.answer.bundle.width.max(self.answer.bundle.height)
    }

    #[wasm_bindgen(getter, js_name = canvasFits)]
    pub fn canvas_fits(&self) -> bool {
        self.answer.fits
    }

    #[wasm_bindgen(getter)]
    pub fn mirrored(&self) -> bool {
        self.answer.bundle.mirrored()
    }

    pub fn map(&self) -> Vec<f32> {
        map(&self.answer.bundle)
    }

    pub fn field(&self) -> Vec<u16> {
        field(&self.answer.bundle)
    }

    pub fn look(&self) -> Vec<u8> {
        self.answer.bundle.look.clone()
    }

    pub fn pad(&self) -> Vec<u8> {
        self.answer.bundle.pad.clone()
    }

    pub fn paints(&self) -> Vec<u8> {
        self.answer.bundle.paints.clone()
    }

    pub fn ramps(&self) -> Vec<u8> {
        self.answer.bundle.ramps.clone()
    }

    pub fn constants(&self) -> Vec<f32> {
        constants(&self.answer.bundle)
    }

    pub fn canvas(&self) -> Vec<u8> {
        self.answer.bundle.canvas.clone()
    }
}
