use crate::cache::digest;
use crate::platform::Screen;
use scooch_core::{Options, Placement, Rect, Scene};
use std::path::Path;
use std::time::UNIX_EPOCH;

pub fn scene(screen: &Screen, icons: &[Rect]) -> Scene {
    let width = (screen.frame.w * screen.scale).round().max(1.0) as u32;
    let height = (screen.frame.h * screen.scale).round().max(1.0) as u32;
    let mut placed: Vec<Rect> = icons.iter().filter(|icon| touches(&screen.frame, icon)).map(|icon| local(screen, icon)).collect();
    placed.sort_by(|a, b| (a.x, a.y, a.w, a.h).partial_cmp(&(b.x, b.y, b.w, b.h)).unwrap_or(std::cmp::Ordering::Equal));
    placed.dedup();
    Scene { width, height, icons: placed }
}

pub fn touches(frame: &Rect, icon: &Rect) -> bool {
    icon.x < frame.x + frame.w && icon.x + icon.w > frame.x && icon.y < frame.y + frame.h && icon.y + icon.h > frame.y
}

fn local(screen: &Screen, icon: &Rect) -> Rect {
    Rect { x: (icon.x - screen.frame.x) * screen.scale, y: (icon.y - screen.frame.y) * screen.scale, w: icon.w * screen.scale, h: icon.h * screen.scale }
}

pub fn fingerprint(source: &Path, scene: &Scene, options: &Options) -> String {
    let mut text = format!("{}|{}x{}|", source.display(), scene.width, scene.height);
    if let Ok(details) = std::fs::metadata(source) {
        let changed = details.modified().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map_or(0, |since| since.as_nanos());
        text.push_str(&format!("{}@{changed}|", details.len()));
    }
    text.push_str(&format!("{}/{}/{}/{}|", options.strength, options.max_zoom, options.mirror, options.pads));
    if options.scenery != scooch_core::Scenery::Off {
        text.push_str(&format!("scenery {:?}|", options.scenery));
    }
    for icon in &scene.icons {
        text.push_str(&format!("{},{},{},{};", icon.x.round(), icon.y.round(), icon.w.round(), icon.h.round()));
    }
    format!("{:016x}", digest(text.as_bytes()))
}

pub fn is_identity(placement: &Placement) -> bool {
    let near = |a: f32, b: f32| (a - b).abs() <= 1e-4;
    let [x0, y0, w, h] = placement.crop;
    near(placement.zoom, 1.0) && near(placement.shift[0], 0.0) && near(placement.shift[1], 0.0) && !placement.mirrored && near(x0 + w / 2.0, 0.5) && near(y0 + h / 2.0, 0.5)
}

#[cfg(test)]
pub fn sample(width: u32, height: u32) -> scooch_core::RgbImage {
    let mut bytes = Vec::with_capacity((width * height * 3) as usize);
    for y in 0..height {
        for x in 0..width {
            let dx = x as f32 / width as f32 - 0.5;
            let dy = y as f32 / height as f32 - 0.5;
            let lit = (dx * dx * 9.0 + dy * dy) < 0.02;
            bytes.extend_from_slice(if lit { &[250, 240, 220] } else { &[12, 18, 30] });
        }
    }
    scooch_core::RgbImage::from_raw(width, height, bytes).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retina() -> Screen {
        Screen { id: "main".into(), frame: Rect { x: 0.0, y: 0.0, w: 1440.0, h: 900.0 }, scale: 2.0 }
    }

    fn side() -> Screen {
        Screen { id: "side".into(), frame: Rect { x: 1440.0, y: -180.0, w: 1920.0, h: 1080.0 }, scale: 1.0 }
    }

    #[test]
    fn points_become_pixels() {
        let made = scene(&retina(), &[Rect { x: 1311.5, y: 8.0, w: 127.0, h: 109.0 }]);
        assert_eq!((made.width, made.height), (2880, 1800));
        assert_eq!(made.icons, vec![Rect { x: 2623.0, y: 16.0, w: 254.0, h: 218.0 }]);
    }

    #[test]
    fn two_screens_with_mixed_scales_each_get_their_own_pixels() {
        let icons = [Rect { x: 100.0, y: 100.0, w: 64.0, h: 100.0 }, Rect { x: 1500.0, y: -100.0, w: 64.0, h: 100.0 }, Rect { x: 1400.0, y: 10.0, w: 80.0, h: 100.0 }];
        let left = scene(&retina(), &icons);
        let right = scene(&side(), &icons);
        assert_eq!((right.width, right.height), (1920, 1080));
        assert_eq!(left.icons, vec![Rect { x: 200.0, y: 200.0, w: 128.0, h: 200.0 }, Rect { x: 2800.0, y: 20.0, w: 160.0, h: 200.0 }]);
        assert_eq!(right.icons, vec![Rect { x: -40.0, y: 190.0, w: 80.0, h: 100.0 }, Rect { x: 60.0, y: 80.0, w: 64.0, h: 100.0 }]);
    }

    #[test]
    fn icons_on_another_screen_are_left_out() {
        let far = Rect { x: -500.0, y: 0.0, w: 100.0, h: 100.0 };
        let edge = Rect { x: 1440.0, y: 0.0, w: 50.0, h: 50.0 };
        assert!(scene(&retina(), &[far, edge]).icons.is_empty());
    }

    #[test]
    fn the_order_icons_arrive_in_does_not_matter() {
        let a = Rect { x: 10.0, y: 10.0, w: 64.0, h: 100.0 };
        let b = Rect { x: 200.0, y: 10.0, w: 64.0, h: 100.0 };
        assert_eq!(scene(&retina(), &[a, b]), scene(&retina(), &[b, a]));
    }

    #[test]
    fn fingerprint_follows_every_input() {
        let screen = retina();
        let icon = Rect { x: 10.0, y: 10.0, w: 64.0, h: 100.0 };
        let moved = Rect { x: 11.0, ..icon };
        let options = Options::default();
        let a = Path::new("/nowhere/a.jpg");
        let base = fingerprint(a, &scene(&screen, &[icon]), &options);
        assert_eq!(base, fingerprint(a, &scene(&screen, &[icon]), &options));
        assert_ne!(base, fingerprint(Path::new("/nowhere/b.jpg"), &scene(&screen, &[icon]), &options));
        assert_ne!(base, fingerprint(a, &scene(&screen, &[moved]), &options));
        assert_ne!(base, fingerprint(a, &scene(&screen, &[]), &options));
        assert_ne!(base, fingerprint(a, &scene(&screen, &[icon]), &Options { pads: 0.1, ..options }));
        assert_ne!(base, fingerprint(a, &scene(&screen, &[icon]), &Options { scenery: scooch_core::Scenery::Auto, ..options }));
        assert_ne!(base, fingerprint(a, &scene(&Screen { scale: 1.0, ..retina() }, &[icon]), &options));
        assert_eq!(base.len(), 16);
    }

    #[test]
    fn fingerprint_ignores_movement_under_half_a_pixel() {
        let screen = retina();
        let icon = Rect { x: 10.0, y: 10.0, w: 64.0, h: 100.0 };
        let nudged = Rect { x: 10.1, ..icon };
        let a = Path::new("/nowhere/a.jpg");
        assert_eq!(fingerprint(a, &scene(&screen, &[icon]), &Options::default()), fingerprint(a, &scene(&screen, &[nudged]), &Options::default()));
    }

    #[test]
    fn fingerprint_notices_an_edited_picture() {
        let folder = crate::state::scratch("fingerprint");
        let picture = folder.join("a.jpg");
        std::fs::write(&picture, b"one").unwrap();
        let before = fingerprint(&picture, &scene(&retina(), &[]), &Options::default());
        std::fs::write(&picture, b"longer").unwrap();
        assert_ne!(before, fingerprint(&picture, &scene(&retina(), &[]), &Options::default()));
    }

    fn placement(crop: [f32; 4], zoom: f32, shift: [f32; 2], mirrored: bool) -> Placement {
        let image = sample(64, 40);
        let blank = Scene { width: 64, height: 40, icons: Vec::new() };
        let mut made = scooch_core::solve(&scooch_core::analyze(&image), &blank, &Options::default(), None);
        (made.crop, made.zoom, made.shift, made.mirrored) = (crop, zoom, shift, mirrored);
        made
    }

    #[test]
    fn no_icons_solve_to_the_identity() {
        let image = sample(96, 40);
        let blank = Scene { width: 160, height: 100, icons: Vec::new() };
        assert!(is_identity(&scooch_core::solve(&scooch_core::analyze(&image), &blank, &Options::default(), None)));
    }

    #[test]
    fn identity_means_a_centered_fill() {
        assert!(is_identity(&placement([0.0, 0.0, 1.0, 1.0], 1.0, [0.0, 0.0], false)));
        assert!(is_identity(&placement([0.1, 0.0, 0.8, 1.0], 1.0, [0.0, 0.0], false)));
        assert!(!is_identity(&placement([0.0, 0.0, 0.8, 1.0], 1.0, [0.0, 0.0], false)));
        assert!(!is_identity(&placement([0.1, 0.1, 0.8, 0.8], 1.25, [0.0, 0.0], false)));
        assert!(!is_identity(&placement([0.0, 0.0, 1.0, 1.0], 1.0, [0.05, 0.0], false)));
        assert!(!is_identity(&placement([0.0, 0.0, 1.0, 1.0], 1.0, [0.0, 0.0], true)));
    }
}
