#![allow(dead_code)]

use image::{imageops, Rgb, RgbImage};
use scooch_core::Rect;

fn rounded(px: f32, py: f32, x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (ex, ey) = ((x1 - x0) / 2.0 - radius, (y1 - y0) / 2.0 - radius);
    let (qx, qy) = ((px - cx).abs() - ex, (py - cy).abs() - ey);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - radius
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn over(image: &mut RgbImage, x: u32, y: u32, color: [f32; 3], alpha: f32) {
    if alpha <= 0.0 {
        return;
    }
    let p = image.get_pixel_mut(x, y);
    for (slot, target) in p.0.iter_mut().zip(color) {
        *slot = (*slot as f32 + (target - *slot as f32) * alpha.min(1.0)).round().clamp(0.0, 255.0) as u8;
    }
}

pub fn draw_icons(image: &mut RgbImage, rects: &[Rect]) {
    let (w, h) = (image.width() as f32, image.height() as f32);
    for r in rects {
        if !(r.x.is_finite() && r.y.is_finite() && r.w > 0.0 && r.h > 0.0) {
            continue;
        }
        let side = (r.w * 0.84).min(r.h * 0.66);
        let (sx0, sy0) = (r.x + (r.w - side) / 2.0, r.y + 0.03 * r.h);
        let (sx1, sy1) = (sx0 + side, sy0 + side);
        let radius = 0.22 * side;
        let (bw, bh) = (0.62 * r.w, 0.075 * r.h);
        let (bx0, by0) = (r.x + (r.w - bw) / 2.0, r.y + 0.80 * r.h - bh / 2.0);
        let (bx1, by1) = (bx0 + bw, by0 + bh);
        let soft = (0.05 * r.h).max(1.5);
        let x_range = ((r.x - soft).floor().max(0.0) as u32)..((r.x + r.w + soft).ceil().min(w) as u32);
        let y_range = ((r.y - soft).floor().max(0.0) as u32)..((r.y + r.h + soft).ceil().min(h) as u32);
        for y in y_range.clone() {
            for x in x_range.clone() {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let drop = rounded(px, py - 0.03 * side, sx0, sy0, sx1, sy1, radius);
                over(image, x, y, [0.0, 0.0, 0.0], 0.30 * smoothstep(soft, -soft * 0.3, drop));
                let body = rounded(px, py, sx0, sy0, sx1, sy1, radius);
                let shade = 236.0 - 22.0 * ((py - sy0) / side).clamp(0.0, 1.0);
                over(image, x, y, [shade, shade + 2.0, shade + 6.0], smoothstep(0.6, -0.6, body));
                over(image, x, y, [150.0, 156.0, 166.0], smoothstep(0.6, -0.6, body.abs() - 0.6) * 0.6);
                let halo = rounded(px, py - 1.0, bx0, by0, bx1, by1, bh / 2.0);
                over(image, x, y, [0.0, 0.0, 0.0], 0.55 * smoothstep(soft * 0.8, -soft * 0.2, halo));
                let text = rounded(px, py, bx0, by0, bx1, by1, bh / 2.0);
                over(image, x, y, [255.0, 255.0, 255.0], smoothstep(0.6, -0.6, text));
            }
        }
    }
}

pub fn fit(image: &RgbImage, box_w: u32, box_h: u32) -> RgbImage {
    let scale = (box_w as f32 / image.width() as f32).min(box_h as f32 / image.height() as f32);
    let (w, h) = (((image.width() as f32 * scale).round() as u32).max(1), ((image.height() as f32 * scale).round() as u32).max(1));
    imageops::resize(image, w, h, imageops::FilterType::Triangle)
}

pub fn grid(tiles: &[RgbImage], columns: usize, gap: u32) -> RgbImage {
    let cell_w = tiles.iter().map(|t| t.width()).max().unwrap_or(1);
    let cell_h = tiles.iter().map(|t| t.height()).max().unwrap_or(1);
    let rows = tiles.len().div_ceil(columns) as u32;
    let mut sheet = RgbImage::from_pixel(columns as u32 * (cell_w + gap) + gap, rows * (cell_h + gap) + gap, Rgb([28, 28, 30]));
    for (i, t) in tiles.iter().enumerate() {
        let (c, r) = ((i % columns) as u32, (i / columns) as u32);
        imageops::replace(&mut sheet, t, (gap + c * (cell_w + gap)) as i64, (gap + r * (cell_h + gap)) as i64);
    }
    sheet
}
