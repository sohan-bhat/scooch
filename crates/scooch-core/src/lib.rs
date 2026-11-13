mod analysis;
mod blur;
mod bundle;
mod color;
mod look;
mod pads;
mod paint;
mod render;
mod sat;
mod scenery;
mod shape;
mod solver;
mod table;
mod warp;

pub use bundle::{Bundle, Constants};
pub use image::{GrayImage, RgbImage, RgbaImage};
pub use paint::Materials;
pub use shape::FIELD_STEP as SCENERY_FIELD_STEP;
use serde::{Deserialize, Serialize};

pub const TABLE: usize = 257;

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
    #[default]
    Auto,
    Islands,
    Clouds,
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
        Self { strength: 1.0, max_zoom: 1.3, mirror: false, pads: 0.7, scenery: Scenery::Auto }
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

pub fn solve_near(analysis: &Analysis, scene: &Scene, options: &Options, previous: &Placement, budget: std::time::Duration) -> Placement {
    solver::solve_near(analysis, scene, options, previous, budget)
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

pub fn frame(a: &Bundle, b: &Bundle, s: f32) -> RgbImage {
    bundle::frame(a, b, s)
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

pub fn open(path: &std::path::Path) -> image::ImageResult<RgbImage> {
    use image::ImageDecoder;
    let mut decoder = image::ImageReader::open(path)?.with_guessed_format()?.into_decoder()?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut picture = image::DynamicImage::from_decoder(decoder)?;
    picture.apply_orientation(orientation);
    Ok(picture.to_rgb8())
}

pub fn encode_jpeg(image: &RgbImage, quality: u8) -> image::ImageResult<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, quality).encode_image(image)?;
    Ok(out)
}

pub fn encode_png(image: &RgbImage) -> image::ImageResult<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::ImageEncoder;
    let mut out = Vec::new();
    PngEncoder::new_with_quality(&mut out, CompressionType::Fast, FilterType::Adaptive).write_image(image, image.width(), image.height(), image::ExtendedColorType::Rgb8)?;
    Ok(out)
}
