//! Saliency, placement solver and renderer for Scooch.
//!
//! A [`Bundle`] is one state of one screen as plain tables. [`frame`] blends two bundles and is the
//! reference for `shaders/frame.wgsl` and `shaders/frame.glsl`, which read the same tables. A pixel
//! position `p` is the pixel center in output pixels from the top left, and every grid is sampled at
//! `p / cell / grid size` in normalized coordinates, so grid texel `i` sits at output pixel `cell * (i + 0.5)`.
//!
//! | Texture | Size | Format | Filter | Wrap |
//! |---|---|---|---|---|
//! | `canvas` | `canvas_width` x `canvas_height`, long side at most twice the output's and at most 8192 | RGBA8 sRGB with mipmaps, alpha 1 | linear, mipmap linear | clamp to edge |
//! | `map_a`, `map_b` | `max(width, height)` x 2, row 0 `xmap`, row 1 `ymap`, padded with the last value | R32F, canvas coordinate from 0 to 1 | none, read with texel fetch | clamp to edge |
//! | `field_a`, `field_b` | `grid_width` x `grid_height`, a quarter of the output | R16F, distance to the scenery's edge in icon units, negative inside; past an island it is stretched so the shallows end at 1.1 | linear | clamp to edge |
//! | `look_a`, `look_b` | `grid_width` x `grid_height` | RGBA8 unorm: kind (1 cloud, 0 island), mark (on clouds the piled puff arcs, on islands the foam strength), broad light, facing with 0.5 for none | linear | clamp to edge |
//! | `pad_a`, `pad_b` | `grid_width` x `grid_height` | RGBA8 sRGB: calm color, alpha is the pad weight | linear | clamp to edge |
//! | `paints_a`, `paints_b` | `paint_width` x `paint_height` x 8 layers, a sixteenth of the output | RGBA8 sRGB array: shallows gain over 4 (alpha land opacity), foam, dry sand (alpha land opacity), wet sand, cloud shade (alpha cloud opacity), cloud light, cloud underside, unused | linear | clamp to edge |
//! | `ramps_a`, `ramps_b` | 512 x 2 | RGBA8 unorm: row 0 shallows, foam line, sand, wet sand; row 1 cloud body, lit band, underside, unused | linear | clamp to edge |
//!
//! | Uniform | Meaning |
//! |---|---|
//! | `size` | output width and height in pixels |
//! | `grid`, `paint` (`paint_grid` in GLSL) | `grid_width`, `grid_height` and `paint_width`, `paint_height` |
//! | `s` | eased time from bundle A to bundle B |
//! | `unit` | icon height in output pixels, the unit of the field |
//! | `ramp_low`, `ramp_high` | field range the ramps cover, in units |
//! | `reach` | field value past which nothing is painted |
//! | `grain` (`grain_amount` in GLSL) | strength of the sand grain, a hash of the pixel |
//! | `core_lift`, `core_drop` | span and floor of the cloud mark: a cloud's tone is scaled by `1 + mark * core_lift - core_drop` |
//! | `volume_lit`, `volume_dark` | how far broad light lifts a cloud toward its light tone, and how far the underside darkens it |
//! | `grain_scale` | how far a glide shrinks what only bundle A covers and grows what only bundle B covers, in units |
//! | `grain_floor` | the field range over which a glide hands the look from A to B |
//! | `shadow` (`shadow_at` in GLSL) | how deep an overlap of the two bundles a glide eats mid way, in units, and the change between them, the largest at the pixel and at four points 0.3 units away on the diagonals, below which it eats nothing |
//! | `spare` | 0 dissolves sky scenery, which never passes both end states, and shrinks and grows land, which passes them by at most `shadow` and only near a change, 1 dissolves all of it, kept for comparison |
//! | `rim`, `belly`, `grain_lod` | unused, kept so the block keeps its layout |

mod analysis;
mod area;
mod blur;
mod bundle;
mod color;
mod look;
mod math;
mod pack;
mod pads;
mod paint;
mod puffs;
mod render;
mod sat;
mod scenery;
mod session;
mod shape;
mod solver;
mod surface;
mod table;
mod warp;

pub use bundle::{Bundle, Constants, RAMP};
pub use pack::{half_bits, Canvas, TablesError, TABLES_VERSION};
pub use session::{Map, Session};
#[doc(hidden)]
pub use puffs::SITES_MOST;
pub use image::{GrayImage, RgbImage, RgbaImage};
pub use paint::Materials;
pub use shape::FIELD_STEP as SCENERY_FIELD_STEP;
use serde::{Deserialize, Serialize};

pub const TABLE: usize = 257;

#[doc(hidden)]
pub const ISLAND_WATER: [u8; 3] = [20, 110, 120];

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scene {
    pub width: u32,
    pub height: u32,
    pub icons: Vec<Rect>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scenery {
    Auto,
    Islands,
    Clouds,
    #[default]
    Off,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    pub strength: f32,
    pub max_zoom: f32,
    pub mirror: bool,
    pub pads: f32,
    pub scenery: Scenery,
}

fn one() -> f32 {
    1.0
}

impl Default for Options {
    fn default() -> Self {
        Self { strength: 1.0, max_zoom: 1.3, mirror: false, pads: 0.7, scenery: Scenery::Off }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    pub saliency: Vec<f32>,
    pub busy: Vec<f32>,
    pub subject: Rect,
    #[serde(default)]
    pub flat: bool,
    #[serde(default)]
    pub source_width: u32,
    #[serde(default)]
    pub source_height: u32,
    #[serde(default)]
    pub palette: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    #[serde(default)]
    pub fx: Vec<f32>,
    #[serde(default)]
    pub fy: Vec<f32>,
    #[serde(default = "one")]
    pub zoom: f32,
    #[serde(default)]
    pub mirrored: bool,
    #[serde(default)]
    pub crop: [f32; 4],
    #[serde(default)]
    pub shift: [f32; 2],
}

pub fn analyze(image: &RgbImage) -> Analysis {
    analysis::analyze(image)
}

pub fn solve(analysis: &Analysis, scene: &Scene, options: &Options, previous: Option<&Placement>) -> Placement {
    solver::solve(analysis, scene, options, previous)
}

/// The version of the rules that decide where scenery belongs. It is part of `belonging_stamp`, so an answer kept from a build with other rules never matches. A rule change that moves any answer raises it.
pub const BELONGING_VERSION: u32 = 2;

/// The layout of the file `Map::to_bytes` writes. A change to that layout raises it.
pub const BELONGING_MAP_FORMAT: u32 = 1;

/// Each belonging version with the digest of the answers it gives over the test set. A test checks the last entry against the rules as they are.
#[doc(hidden)]
pub const BELONGING_ANSWERS: &[(u32, u64)] = &[(1, 0x44fb_d8cd_2af2_5f5f), (2, 0xfc36_2b43_6890_42ea)];

/// A hash of everything the belonging map depends on apart from the picture's pixels: the belonging version, the analysis, the screen size, the icon unit and the scenery setting. It is the key a `Session` compares before it reuses a map, so a caller that keeps the answer of `Session::belongs` beside its own picture key and this stamp knows when the answer still holds.
pub fn belonging_stamp(analysis: &Analysis, scene: &Scene, options: &Options) -> u64 {
    look::stamp(look::print(analysis), scene, options)
}

/// A quick placement near the previous one, only for while an icon is held. It searches nearby pans only and can hide more of the subject than `solve`, so the caller follows it with a full `solve` when the icon is dropped.
pub fn solve_near(analysis: &Analysis, scene: &Scene, options: &Options, previous: &Placement, budget: std::time::Duration) -> Placement {
    let started = std::time::Instant::now();
    solver::solve_near(analysis, scene, options, previous, budget, &|| started.elapsed())
}

/// `solve_near` with the caller's clock, for targets without `std::time::Instant`. `elapsed` returns the time since the call began.
pub fn solve_near_timed(analysis: &Analysis, scene: &Scene, options: &Options, previous: &Placement, budget: std::time::Duration, elapsed: &dyn Fn() -> std::time::Duration) -> Placement {
    solver::solve_near(analysis, scene, options, previous, budget, elapsed)
}

pub fn render(image: &RgbImage, placement: &Placement, scene: &Scene, options: &Options) -> RgbImage {
    render::render(image, None, placement, scene, options)
}

pub fn render_with(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> RgbImage {
    render::render(image, Some(analysis), placement, scene, options)
}

pub fn render_painted(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options, materials: &Materials) -> RgbImage {
    let (mut picture, layer) = render::layers(image, Some(analysis), placement, scene, options, Some(materials));
    paint::composite(&mut picture, &layer);
    picture
}

pub fn bundle(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> Bundle {
    bundle::make(image, analysis, placement, scene, options, &paint::materials(analysis))
}

#[doc(hidden)]
pub fn regions(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> (Vec<[u8; 4]>, u32, u32) {
    bundle::regions(image, analysis, placement, scene, options, &paint::materials(analysis))
}

pub fn still(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> (Bundle, RgbImage) {
    render::still(image, analysis, placement, scene, options)
}

pub fn scenery_active(analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> bool {
    look::active(analysis, placement, scene, options)
}

pub fn canvas(image: &RgbImage, width: u32, height: u32) -> Canvas {
    let made = bundle::canvas(image, width, height);
    Canvas { width: made.width(), height: made.height(), pixels: made.into_owned().into_raw() }
}

pub fn frame(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    bundle::frame(a, b, s)
}

#[doc(hidden)]
pub fn solid_cells(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> (Vec<bool>, u32, u32) {
    bundle::solid_cells(image, analysis, placement, scene, options, &paint::materials(analysis))
}

#[doc(hidden)]
pub fn belonging_map(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> (Vec<[f32; 2]>, u32, u32) {
    bundle::belonging_map(image, analysis, placement, scene, options, &paint::materials(analysis))
}

#[doc(hidden)]
pub fn frame_through(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    bundle::frame_through(a, b, s)
}

pub fn frame_dissolve(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    bundle::frame_with(a, b, s, bundle::Glide::Dissolve)
}

pub fn frame_dissolve_layers(a: &Bundle, b: &Bundle, s: f32) -> Layers {
    let (picture, scenery) = bundle::layers_with(a, b, s, bundle::Glide::Dissolve);
    Layers { picture, scenery }
}

pub fn frame_layers(a: &Bundle, b: &Bundle, s: f32) -> Layers {
    let (picture, scenery) = bundle::layers(a, b, s);
    Layers { picture, scenery }
}

pub fn compatible(a: &Bundle, b: &Bundle) -> bool {
    bundle::compatible(a, b)
}

pub struct Layers {
    pub picture: RgbImage,
    pub scenery: RgbaImage,
}

pub fn render_layers(image: &RgbImage, analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> Layers {
    let (picture, scenery) = render::layers(image, Some(analysis), placement, scene, options, None);
    Layers { picture, scenery }
}

pub fn composite(picture: &mut RgbImage, scenery: &RgbaImage) {
    paint::composite(picture, scenery)
}

pub fn materials(analysis: &Analysis) -> Materials {
    paint::materials(analysis)
}

pub fn scenery_field(scene: &Scene, width: u32, height: u32) -> GrayImage {
    shape::field_image(scene, width, height)
}

pub fn pads_active(analysis: &Analysis, placement: &Placement, scene: &Scene, options: &Options) -> bool {
    pads::active(analysis, placement, scene, options)
}

pub fn home(analysis: &Analysis, scene: &Scene) -> Placement {
    solver::home(analysis, scene)
}

pub fn fill(image_width: u32, image_height: u32, scene: &Scene) -> Placement {
    table::fill(image_width, image_height, scene.width, scene.height)
}

pub fn hidden_share(analysis: &Analysis, scene: &Scene, placement: &Placement) -> f32 {
    solver::hidden_share(analysis, scene, placement)
}

pub fn saliency_image(analysis: &Analysis) -> GrayImage {
    analysis::gray(analysis.width, analysis.height, &analysis.saliency)
}

pub fn busy_image(analysis: &Analysis) -> GrayImage {
    analysis::gray(analysis.width, analysis.height, &analysis.busy)
}

#[cfg(feature = "codecs")]
pub fn open(path: &std::path::Path) -> image::ImageResult<RgbImage> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder)?;
    picture.apply_orientation(orientation);
    Ok(picture.to_rgb8())
}

#[cfg(feature = "codecs")]
pub fn encode_jpeg(image: &RgbImage, quality: u8) -> image::ImageResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(image)?;
    Ok(out)
}

#[cfg(feature = "codecs")]
pub fn encode_png(image: &RgbImage) -> image::ImageResult<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive).write_image(image, image.width(), image.height(), image::ExtendedColorType::Rgb8)?;
    Ok(out)
}
