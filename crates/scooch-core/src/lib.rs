pub use image::RgbImage;
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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Options {
    pub strength: f32,
    pub max_zoom: f32,
    pub mirror: bool,
    pub pads: f32,
}

impl Default for Options {
    fn default() -> Self {
        Self { strength: 1.0, max_zoom: 1.3, mirror: false, pads: 0.7 }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Analysis {
    pub width: u32,
    pub height: u32,
    pub saliency: Vec<f32>,
    pub busy: Vec<f32>,
    pub subject: Rect,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub fx: Vec<f32>,
    pub fy: Vec<f32>,
    pub zoom: f32,
    pub mirrored: bool,
}

pub fn analyze(image: &RgbImage) -> Analysis {
    let (w, h) = image.dimensions();
    let scale = 320.0 / w.max(h) as f32;
    let (aw, ah) = (((w as f32 * scale) as u32).max(1), ((h as f32 * scale) as u32).max(1));
    let n = (aw * ah) as usize;
    Analysis {
        width: aw,
        height: ah,
        saliency: vec![0.0; n],
        busy: vec![0.0; n],
        subject: Rect { x: 0.25, y: 0.25, w: 0.5, h: 0.5 },
    }
}

pub fn solve(analysis: &Analysis, scene: &Scene, _options: &Options, _previous: Option<&Placement>) -> Placement {
    let image_aspect = analysis.width as f32 / analysis.height as f32;
    let screen_aspect = scene.width as f32 / scene.height as f32;
    let (cw, ch) = if image_aspect > screen_aspect { (screen_aspect / image_aspect, 1.0) } else { (1.0, image_aspect / screen_aspect) };
    let table = |span: f32| (0..TABLE).map(|k| (1.0 - span) / 2.0 + span * k as f32 / (TABLE - 1) as f32).collect();
    Placement { fx: table(cw), fy: table(ch), zoom: 1.0, mirrored: false }
}

pub fn render(image: &RgbImage, placement: &Placement, scene: &Scene, _options: &Options) -> RgbImage {
    let (sw, sh) = image.dimensions();
    let lookup = |table: &[f32], t: f32| {
        let p = t.clamp(0.0, 1.0) * (TABLE - 1) as f32;
        let i = (p as usize).min(TABLE - 2);
        table[i] + (table[i + 1] - table[i]) * (p - i as f32)
    };
    RgbImage::from_fn(scene.width, scene.height, |x, y| {
        let u = lookup(&placement.fx, (x as f32 + 0.5) / scene.width as f32);
        let v = lookup(&placement.fy, (y as f32 + 0.5) / scene.height as f32);
        let sx = ((u * sw as f32) as u32).min(sw - 1);
        let sy = ((v * sh as f32) as u32).min(sh - 1);
        *image.get_pixel(sx, sy)
    })
}

pub fn open(path: &std::path::Path) -> image::ImageResult<RgbImage> {
    Ok(image::ImageReader::open(path)?.with_guessed_format()?.decode()?.to_rgb8())
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
