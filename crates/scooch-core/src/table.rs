use crate::{Placement, TABLE};

pub const STEPS: usize = TABLE - 1;

pub fn cover(image_aspect: f64, screen_aspect: f64, zoom: f64) -> (f64, f64) {
    if !(image_aspect > 0.0 && screen_aspect > 0.0 && image_aspect.is_finite() && screen_aspect.is_finite()) {
        return (1.0 / zoom, 1.0 / zoom);
    }
    if image_aspect >= screen_aspect {
        ((screen_aspect / image_aspect / zoom).min(1.0), 1.0 / zoom)
    } else {
        (1.0 / zoom, (image_aspect / screen_aspect / zoom).min(1.0))
    }
}

pub fn aspect(width: u32, height: u32) -> f64 {
    if width == 0 || height == 0 {
        return 1.0;
    }
    width as f64 / height as f64
}

pub fn line(from: f64, to: f64) -> Vec<f32> {
    let mut out: Vec<f32> = (0..TABLE).map(|k| (from + (to - from) * k as f64 / STEPS as f64) as f32).collect();
    out[0] = from as f32;
    out[STEPS] = to as f32;
    out
}

pub fn fill(image_width: u32, image_height: u32, screen_width: u32, screen_height: u32) -> Placement {
    let (cw, ch) = cover(aspect(image_width, image_height), aspect(screen_width, screen_height), 1.0);
    let (x0, y0) = ((1.0 - cw) / 2.0, (1.0 - ch) / 2.0);
    Placement { fx: line(x0, x0 + cw), fy: line(y0, y0 + ch), zoom: 1.0, mirrored: false, crop: [x0 as f32, y0 as f32, cw as f32, ch as f32], shift: [0.0, 0.0] }
}

pub fn usable(table: &[f32]) -> bool {
    if table.len() != TABLE || table.iter().any(|v| !v.is_finite() || *v < 0.0 || *v > 1.0) {
        return false;
    }
    let rising = table[STEPS] > table[0];
    table.windows(2).all(|pair| if rising { pair[1] > pair[0] } else { pair[1] < pair[0] })
}

pub struct Curve {
    values: Vec<f64>,
    tangents: Vec<f64>,
}

impl Curve {
    pub fn new(table: &[f32]) -> Self {
        let values: Vec<f64> = table.iter().map(|v| *v as f64).collect();
        let n = values.len();
        let secant: Vec<f64> = values.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let mut tangents = vec![0.0f64; n];
        tangents[0] = secant[0];
        tangents[n - 1] = secant[n - 2];
        for k in 1..n - 1 {
            let (a, b) = (secant[k - 1], secant[k]);
            tangents[k] = if a * b > 0.0 { 2.0 * a * b / (a + b) } else { 0.0 };
        }
        Self { values, tangents }
    }

    pub fn at(&self, t: f64) -> f64 {
        let last = self.values.len() - 1;
        let p = t.clamp(0.0, 1.0) * last as f64;
        let i = (p as usize).min(last - 1);
        let s = p - i as f64;
        let (a, b) = (self.values[i], self.values[i + 1]);
        let (ma, mb) = (self.tangents[i], self.tangents[i + 1]);
        let (s2, s3) = (s * s, s * s * s);
        a * (2.0 * s3 - 3.0 * s2 + 1.0) + ma * (s3 - 2.0 * s2 + s) + b * (-2.0 * s3 + 3.0 * s2) + mb * (s3 - s2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matching_image_fills_the_whole_table() {
        let placement = fill(2880, 1800, 2880, 1800);
        assert_eq!(placement.fx.len(), TABLE);
        for k in 0..TABLE {
            assert_eq!(placement.fx[k], k as f32 / 256.0);
            assert_eq!(placement.fy[k], k as f32 / 256.0);
        }
        assert_eq!(placement.crop, [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn a_wide_image_is_cropped_at_the_sides() {
        let placement = fill(4000, 1000, 1000, 1000);
        assert!((placement.fx[0] - 0.375).abs() < 1e-6 && (placement.fx[256] - 0.625).abs() < 1e-6);
        assert_eq!((placement.fy[0], placement.fy[256]), (0.0, 1.0));
    }

    #[test]
    fn a_tall_image_is_cropped_top_and_bottom() {
        let (cw, ch) = cover(0.5, 2.0, 1.0);
        assert_eq!(cw, 1.0);
        assert!((ch - 0.25).abs() < 1e-12);
        let (cw, ch) = cover(1.5, 1.6, 1.25);
        assert!((cw - 0.8).abs() < 1e-12 && (ch - 1.5 / 1.6 / 1.25).abs() < 1e-12);
    }

    #[test]
    fn the_curve_follows_a_straight_table() {
        let table = line(0.2, 0.9);
        let curve = Curve::new(&table);
        for i in 0..=1000 {
            let t = i as f64 / 1000.0;
            assert!((curve.at(t) - (0.2 + 0.7 * t)).abs() < 1e-6);
        }
    }

    #[test]
    fn the_curve_never_turns_back() {
        let table: Vec<f32> = (0..TABLE).map(|k| ((k as f32 / 256.0).powi(3) * 0.5 + if k > 128 { 0.3 } else { 0.0 }).min(1.0)).collect();
        let curve = Curve::new(&table);
        let mut last = curve.at(0.0);
        for i in 1..=5000 {
            let now = curve.at(i as f64 / 5000.0);
            assert!(now >= last - 1e-12);
            last = now;
        }
    }

    #[test]
    fn broken_tables_are_refused() {
        assert!(usable(&line(0.0, 1.0)) && usable(&line(1.0, 0.0)));
        assert!(!usable(&line(0.5, 0.5)));
        assert!(!usable(&[0.0, 1.0]));
        let mut bent = line(0.0, 1.0);
        bent[100] = 0.9;
        assert!(!usable(&bent));
        let mut nan = line(0.0, 1.0);
        nan[5] = f32::NAN;
        assert!(!usable(&nan));
    }
}
