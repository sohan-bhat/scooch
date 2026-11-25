const TOKENS: &str = include_str!("../../../site/src/styles/tokens.css");
const PAGE: &str = include_str!("../ui/src/styles/app.css");

#[cfg(any(windows, test))]
pub struct Screen {
    pub area: (i32, i32, u32, u32),
    pub work: (i32, i32, u32, u32),
    pub scale: f64,
}

fn room_in(css: &str) -> Option<(u8, u8, u8)> {
    let value = u32::from_str_radix(css.split("--color-room:").nth(1)?.trim_start().strip_prefix('#')?.get(..6)?, 16).ok()?;
    Some(((value >> 16) as u8, (value >> 8) as u8, value as u8))
}

pub fn room(light: bool) -> (u8, u8, u8) {
    let css = if light { PAGE.split("prefers-color-scheme: light").nth(1).unwrap_or_default() } else { TOKENS };
    room_in(css).unwrap_or_default()
}

#[cfg(any(windows, test))]
pub type Spot = ((i32, i32), (u32, u32), f64);

#[cfg(any(windows, test))]
pub fn place(pointer: (f64, f64), screens: &[Screen], inner: Option<(u32, u32)>, fallback: (f64, f64)) -> Option<Spot> {
    let inside = |(x, y, width, height): (i32, i32, u32, u32)| pointer.0 >= x as f64 && pointer.1 >= y as f64 && pointer.0 < x as f64 + width as f64 && pointer.1 < y as f64 + height as f64;
    let screen = screens.iter().find(|screen| inside(screen.area)).or_else(|| screens.first())?;
    let inner = inner.unwrap_or_else(|| ((fallback.0 * screen.scale).round() as u32, (fallback.1 * screen.scale).round() as u32));
    let (side, top) = ((8.0 * screen.scale).round() as i32, (31.0 * screen.scale).round() as i32);
    let (x, y, width, height) = screen.work;
    let left = x + (width as i32 - inner.0 as i32 - 2 * side) / 2;
    let up = y + (height as i32 - inner.1 as i32 - top - side) / 2;
    Some(((left.max(x), up.max(y)), inner, screen.scale))
}

#[cfg(any(windows, test))]
pub fn saved(text: &str) -> (Option<(u32, u32)>, bool) {
    let all: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    let size = all["main"]["width"].as_u64().zip(all["main"]["height"].as_u64()).filter(|&(width, height)| width > 0 && height > 0);
    (size.map(|(width, height)| (width as u32, height as u32)), all["main"]["maximized"].as_bool().unwrap_or(false))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_frame_is_the_page_background_of_each_look_from_the_one_token() {
        assert_eq!(room_in("--color-room-2: #000000; --color-room: #a1b2c3;"), Some((0xa1, 0xb2, 0xc3)));
        assert!(room(true).1 > 200 && room(false).1 < 60, "light {:?} dark {:?}", room(true), room(false));
        let (red, green, blue) = room(false);
        assert!(include_str!("../tauri.conf.json").contains(&format!("\"backgroundColor\": \"#{red:02x}{green:02x}{blue:02x}\"")));
    }

    #[test]
    fn the_frame_is_centred_on_the_pointers_work_area_with_its_title_bar_inside() {
        let one = Screen { area: (0, 0, 1920, 1080), work: (0, 0, 1920, 1032), scale: 1.0 };
        let two = Screen { area: (1920, 0, 2560, 1440), work: (1920, 0, 2560, 1392), scale: 1.5 };
        let screens = [one, two];
        assert_eq!(place((100.0, 100.0), &screens, None, (900.0, 620.0)), Some(((502, 186), (900, 620), 1.0)));
        assert_eq!(place((100.0, 100.0), &screens, Some((1200, 1032)), (900.0, 620.0)), Some(((352, 0), (1200, 1032), 1.0)));
        assert_eq!(place((100.0, 100.0), &screens, Some((2400, 1400)), (900.0, 620.0)), Some(((0, 0), (2400, 1400), 1.0)));
        assert_eq!(place((2500.0, 300.0), &screens, None, (900.0, 620.0)), Some(((2513, 201), (1350, 930), 1.5)));
        assert_eq!(place((-50.0, -50.0), &screens[..1], None, (900.0, 620.0)), Some(((502, 186), (900, 620), 1.0)));
    }

    #[test]
    fn the_saved_state_is_read_leniently() {
        let sample = "{\n  \"main\": {\n    \"width\": 1100,\n    \"height\": 700,\n    \"x\": 410,\n    \"y\": 166,\n    \"prev_x\": 0,\n    \"prev_y\": 0,\n    \"maximized\": true,\n    \"visible\": true,\n    \"decorated\": true,\n    \"fullscreen\": false\n  }\n}";
        assert_eq!(saved(sample), (Some((1100, 700)), true));
        assert_eq!(saved("{\"main\": {\"width\": 0, \"height\": 700}}"), (None, false));
        assert_eq!(saved("not json"), (None, false));
    }
}
