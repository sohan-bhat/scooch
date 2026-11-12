use scooch_core::Rect;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Insets {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Insets {
    pub fn apply(&self, rect: Rect) -> Rect {
        let w = (rect.w as f64 - self.left - self.right).max(1.0);
        let h = (rect.h as f64 - self.top - self.bottom).max(1.0);
        Rect { x: (rect.x as f64 + self.left) as f32, y: (rect.y as f64 + self.top) as f32, w: w as f32, h: h as f32 }
    }
}

pub fn logical(rect: [f64; 4], monitor: [f64; 4], frame: &Rect) -> Rect {
    let fx = if monitor[2] > 0.0 { frame.w as f64 / monitor[2] } else { 1.0 };
    let fy = if monitor[3] > 0.0 { frame.h as f64 / monitor[3] } else { 1.0 };
    Rect {
        x: (frame.x as f64 + (rect[0] - monitor[0]) * fx) as f32,
        y: (frame.y as f64 + (rect[1] - monitor[1]) * fy) as f32,
        w: (rect[2] * fx) as f32,
        h: (rect[3] * fy) as f32,
    }
}

pub fn subtract(frame: &Rect, edges: [f64; 4]) -> Rect {
    let [top, bottom, left, right] = edges;
    Rect {
        x: (frame.x as f64 + left) as f32,
        y: (frame.y as f64 + top) as f32,
        w: (frame.w as f64 - left - right).max(0.0) as f32,
        h: (frame.h as f64 - top - bottom).max(0.0) as f32,
    }
}

pub fn intersect(a: &Rect, b: &Rect) -> Option<Rect> {
    let left = a.x.max(b.x);
    let top = a.y.max(b.y);
    let right = (a.x + a.w).min(b.x + b.w);
    let bottom = (a.y + a.h).min(b.y + b.h);
    (right > left && bottom > top).then_some(Rect { x: left, y: top, w: right - left, h: bottom - top })
}

pub fn transformed(width: i32, height: i32, transform: u32) -> (i32, i32) {
    if transform % 2 == 1 {
        (height, width)
    } else {
        (width, height)
    }
}

pub fn scale(mode_width: i32, mode_height: i32, transform: u32, logical_width: i32) -> f32 {
    let (wide, _) = transformed(mode_width, mode_height, transform);
    if logical_width <= 0 || wide <= 0 {
        return 1.0;
    }
    wide as f32 / logical_width as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insets_shrink_from_each_side() {
        let inset = Insets { left: 2.0, top: 3.0, right: 4.0, bottom: 5.0 };
        assert_eq!(inset.apply(Rect { x: 10.0, y: 10.0, w: 100.0, h: 100.0 }), Rect { x: 12.0, y: 13.0, w: 94.0, h: 92.0 });
    }

    #[test]
    fn stage_rects_map_onto_the_logical_frame() {
        let frame = Rect { x: 1920.0, y: 0.0, w: 1440.0, h: 900.0 };
        let mapped = logical([3840.0 + 200.0, 100.0, 248.0, 224.0], [3840.0, 0.0, 2880.0, 1800.0], &frame);
        assert_eq!(mapped, Rect { x: 2020.0, y: 50.0, w: 124.0, h: 112.0 });
        let same = logical([10.0, 20.0, 30.0, 40.0], [0.0, 0.0, 100.0, 100.0], &Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 });
        assert_eq!(same, Rect { x: 10.0, y: 20.0, w: 30.0, h: 40.0 });
    }

    #[test]
    fn panels_come_off_their_edges() {
        let frame = Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1080.0 };
        assert_eq!(subtract(&frame, [0.0, 44.0, 0.0, 0.0]), Rect { x: 0.0, y: 0.0, w: 1920.0, h: 1036.0 });
        assert_eq!(subtract(&frame, [30.0, 0.0, 48.0, 0.0]), Rect { x: 48.0, y: 30.0, w: 1872.0, h: 1050.0 });
    }

    #[test]
    fn overlaps_are_found() {
        let a = Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 };
        assert_eq!(intersect(&a, &Rect { x: 50.0, y: 50.0, w: 100.0, h: 100.0 }), Some(Rect { x: 50.0, y: 50.0, w: 50.0, h: 50.0 }));
        assert_eq!(intersect(&a, &Rect { x: 100.0, y: 0.0, w: 10.0, h: 10.0 }), None);
    }

    #[test]
    fn scale_is_mode_width_over_logical_width_after_turning() {
        assert_eq!(scale(3840, 2160, 0, 1920), 2.0);
        assert_eq!(scale(2560, 1600, 0, 2048), 1.25);
        assert_eq!(scale(1080, 1920, 1, 1920), 1.0);
        assert_eq!(scale(2160, 3840, 3, 1920), 2.0);
        assert_eq!(scale(1920, 1080, 0, 0), 1.0);
    }
}
