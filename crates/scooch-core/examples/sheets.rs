#[path = "../tests/support/layouts.rs"]
mod layouts;

use image::{imageops, Rgb, RgbImage};
use scooch_core::{Analysis, Options, Rect, Scene};
use std::path::PathBuf;
use std::time::Instant;

const GAP: u32 = 12;
const PAPER: Rgb<u8> = Rgb([28, 28, 30]);

fn thumb(image: &RgbImage, rects: &[Rect], box_w: u32, box_h: u32) -> RgbImage {
    let scale = (box_w as f32 / image.width() as f32).min(box_h as f32 / image.height() as f32);
    let (w, h) = (((image.width() as f32 * scale).round() as u32).max(1), ((image.height() as f32 * scale).round() as u32).max(1));
    let mut small = imageops::resize(image, w, h, imageops::FilterType::Triangle);
    for r in rects {
        outline(&mut small, r.x * scale, r.y * scale, r.w * scale, r.h * scale);
    }
    small
}

fn outline(image: &mut RgbImage, x: f32, y: f32, w: f32, h: f32) {
    let (x0, y0) = (x.round().max(0.0) as i64, y.round().max(0.0) as i64);
    let (x1, y1) = (((x + w).round() as i64).min(image.width() as i64 - 1), ((y + h).round() as i64).min(image.height() as i64 - 1));
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let mut put = |px: i64, py: i64, color: Rgb<u8>| {
        if px >= 0 && py >= 0 && (px as u32) < image.width() && (py as u32) < image.height() {
            image.put_pixel(px as u32, py as u32, color);
        }
    };
    for (inset, color) in [(0i64, Rgb([255, 40, 200])), (1, Rgb([0, 0, 0]))] {
        for px in x0 + inset..=x1 - inset {
            put(px, y0 + inset, color);
            put(px, y1 - inset, color);
        }
        for py in y0 + inset..=y1 - inset {
            put(x0 + inset, py, color);
            put(x1 - inset, py, color);
        }
    }
}

fn gray_to_rgb(gray: &image::GrayImage) -> RgbImage {
    RgbImage::from_fn(gray.width(), gray.height(), |x, y| {
        let v = gray.get_pixel(x, y).0[0];
        Rgb([v, v, v])
    })
}

struct Row {
    before: RgbImage,
    after: RgbImage,
}

fn compose(header: Vec<RgbImage>, rows: &[Row], cell_w: u32) -> RgbImage {
    let header_h = header.iter().map(|i| i.height()).max().unwrap_or(0);
    let row_h: Vec<u32> = rows.iter().map(|r| r.before.height().max(r.after.height())).collect();
    let width = 2 * cell_w + 3 * GAP;
    let height = GAP + header_h + GAP + row_h.iter().map(|h| h + GAP).sum::<u32>();
    let mut sheet = RgbImage::from_pixel(width, height, PAPER);
    let mut x = GAP;
    for part in &header {
        imageops::replace(&mut sheet, part, x as i64, GAP as i64);
        x += part.width() + GAP;
    }
    let mut y = GAP + header_h + GAP;
    for (row, h) in rows.iter().zip(&row_h) {
        imageops::replace(&mut sheet, &row.before, GAP as i64, y as i64);
        imageops::replace(&mut sheet, &row.after, (2 * GAP + cell_w) as i64, y as i64);
        y += h + GAP;
    }
    sheet
}

fn row(name: &str, photo: &RgbImage, analysis: &Analysis, layout: &str, (w, h): (u32, u32), cell_w: u32, cell_h: u32) -> Row {
    let icons = layouts::layout(layout, analysis, w, h);
    let scene = Scene { width: w, height: h, icons: icons.clone() };
    let options = Options::default();
    let started = Instant::now();
    let placement = scooch_core::solve(analysis, &scene, &options, None);
    let solved = started.elapsed();
    let fill = scooch_core::fill(analysis.source_width, analysis.source_height, &scene);
    let started = Instant::now();
    let after = scooch_core::render(photo, &placement, &scene, &options);
    let rendered = started.elapsed();
    let before = scooch_core::render(photo, &fill, &Scene { icons: Vec::new(), ..scene.clone() }, &options);
    let hidden_before = scooch_core::hidden_share(analysis, &scene, &fill);
    let hidden_after = scooch_core::hidden_share(analysis, &scene, &placement);
    println!(
        "{name:16} {w}x{h} {layout:9} zoom {:.2} mirror {} crop [{:.3} {:.3} {:.3} {:.3}] shift [{:+.3} {:+.3}] hidden {:.3} -> {:.3} solve {:.1} ms render {:.0} ms",
        placement.zoom,
        placement.mirrored,
        placement.crop[0],
        placement.crop[1],
        placement.crop[2],
        placement.crop[3],
        placement.shift[0],
        placement.shift[1],
        hidden_before,
        hidden_after,
        solved.as_secs_f64() * 1000.0,
        rendered.as_secs_f64() * 1000.0
    );
    if let Some(dir) = std::env::var_os("SCOOCH_FULL") {
        let mut marked = after.clone();
        for r in &icons {
            outline(&mut marked, r.x, r.y, r.w, r.h);
        }
        let path = PathBuf::from(dir).join(format!("{name}-{w}x{h}-{layout}.png"));
        marked.save(path).expect("save full");
    }
    Row { before: thumb(&before, &icons, cell_w, cell_h), after: thumb(&after, &icons, cell_w, cell_h) }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "sheets".into()));
    let only: Vec<String> = args.collect();
    std::fs::create_dir_all(&out).expect("out folder");
    let cell_w = 640;
    let cell_h = 400;
    for path in layouts::photos() {
        let name = path.file_stem().unwrap().to_string_lossy().to_string();
        if !only.is_empty() && !only.contains(&name) {
            continue;
        }
        let photo = scooch_core::open(&path).expect("photo");
        let analysis = scooch_core::analyze(&photo);
        let header = vec![
            thumb(&photo, &[], 300, 200),
            thumb(&gray_to_rgb(&scooch_core::saliency_image(&analysis)), &[], 300, 200),
            thumb(&gray_to_rgb(&scooch_core::busy_image(&analysis)), &[], 300, 200),
        ];
        println!("{name} flat {} subject {:?}", analysis.flat, analysis.subject);
        if let Some(dir) = std::env::var_os("SCOOCH_FULL") {
            scooch_core::saliency_image(&analysis).save(PathBuf::from(&dir).join(format!("{name}-saliency.png"))).expect("save");
            scooch_core::busy_image(&analysis).save(PathBuf::from(&dir).join(format!("{name}-busy.png"))).expect("save");
        }
        let main: Vec<Row> = layouts::NAMES.iter().map(|layout| row(&name, &photo, &analysis, layout, (2880, 1800), cell_w, cell_h)).collect();
        compose(header.clone(), &main, cell_w).save(out.join(format!("{name}-a.png"))).expect("save");
        let others = [((1920, 1080), "mac"), ((1920, 1080), "windows"), ((3840, 2160), "cluster"), ((5120, 2880), "mac"), ((1080, 1920), "mac"), ((1080, 1920), "cluster")];
        let extra: Vec<Row> = others.iter().map(|(screen, layout)| row(&name, &photo, &analysis, layout, *screen, cell_w, cell_h)).collect();
        compose(header, &extra, cell_w).save(out.join(format!("{name}-b.png"))).expect("save");
    }
}
