pub const BINS: usize = 64;
const RIGID_FROM: f64 = 0.15;
const RIGID_AT: f64 = 0.5;
const BAND_SHARE: f64 = 0.9;
const WIDEST_BAND: f64 = 0.8;
const LEAST_GIVE: f64 = 2.0;
const STRETCH: f64 = 0.6;
const SQUEEZE: f64 = 0.4;
const SIDE_STEPS: usize = 32;
const FLEX_FLOOR: f64 = 0.25;

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn box3(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    (0..n).map(|k| (values[k.saturating_sub(1)] + values[k] + values[(k + 1).min(n - 1)]) / 3.0).collect()
}

pub fn elasticity(profile: &[f64]) -> Vec<f64> {
    let n = profile.len();
    let raw: Vec<f64> = profile.iter().map(|c| 1.0 - smoothstep(RIGID_FROM, RIGID_AT, *c)).collect();
    let low: Vec<f64> = (0..n).map(|k| raw[k.saturating_sub(1)..(k + 2).min(n)].iter().copied().fold(f64::INFINITY, f64::min)).collect();
    box3(&low)
}

pub fn band(profile: &[f64]) -> Option<(usize, usize)> {
    let total: f64 = profile.iter().sum();
    if !(total > 1e-12) {
        return None;
    }
    let need = total * BAND_SHARE;
    let mut best: Option<(usize, usize)> = None;
    let (mut end, mut inside) = (0usize, 0.0f64);
    for start in 0..profile.len() {
        while end < profile.len() && inside < need {
            inside += profile[end];
            end += 1;
        }
        if inside < need {
            break;
        }
        if best.map_or(true, |(a, b)| end - 1 - start < b - a) {
            best = Some((start, end - 1));
        }
        inside -= profile[start];
    }
    best
}

pub struct Plan {
    profile: Vec<f64>,
    give: Vec<f64>,
    band: (usize, usize),
    low: f64,
    high: f64,
    pub ahead: f64,
    pub back: f64,
}

impl Plan {
    pub fn new(profile: &[f64], strength: f64) -> Option<Self> {
        if profile.len() != BINS || !(strength > 0.0) {
            return None;
        }
        let give = elasticity(profile);
        let (ka, kb) = band(profile)?;
        if (kb - ka + 1) as f64 > WIDEST_BAND * BINS as f64 {
            return None;
        }
        let low: f64 = give[..ka].iter().sum();
        let high: f64 = give[kb + 1..].iter().sum();
        if low + high < LEAST_GIVE || !(low > 1e-9 && high > 1e-9) {
            return None;
        }
        let low_peak = give[..ka].iter().copied().fold(0.0f64, f64::max);
        let high_peak = give[kb + 1..].iter().copied().fold(0.0f64, f64::max);
        let reach = |limit: f64, sum: f64, peak: f64| limit * strength * sum / (BINS as f64 * peak);
        let ahead = reach(STRETCH, low, low_peak).min(reach(SQUEEZE, high, high_peak));
        let back = reach(SQUEEZE, low, low_peak).min(reach(STRETCH, high, high_peak));
        Some(Self { profile: profile.to_vec(), give, band: (ka, kb), low, high, ahead, back })
    }

    pub fn shifts(&self) -> Vec<f64> {
        let steps = SIDE_STEPS as f64;
        let mut out: Vec<f64> = (0..SIDE_STEPS).map(|i| -self.back * (SIDE_STEPS - i) as f64 / steps).collect();
        out.push(0.0);
        out.extend((1..=SIDE_STEPS).map(|i| self.ahead * i as f64 / steps));
        out
    }

    pub fn scales(&self, shift: f64) -> Vec<f64> {
        let d = shift.clamp(-self.back, self.ahead);
        let k = BINS as f64;
        let raw: Vec<f64> = self
            .give
            .iter()
            .enumerate()
            .map(|(j, e)| {
                if j < self.band.0 {
                    1.0 + d * k * e / self.low
                } else if j > self.band.1 {
                    1.0 - d * k * e / self.high
                } else {
                    1.0
                }
            })
            .collect();
        let smooth = box3(&box3(&raw));
        let sum: f64 = smooth.iter().sum();
        smooth.iter().map(|s| s * k / sum).collect()
    }

    pub fn penalty(&self, scales: &[f64]) -> f64 {
        scales.iter().zip(&self.profile).map(|(s, c)| (s - 1.0).powi(2) * (FLEX_FLOOR + c)).sum::<f64>() / BINS as f64
    }
}

pub fn penalty(scales: &[f64], profile: &[f64]) -> f64 {
    scales.iter().zip(profile).map(|(s, c)| (s - 1.0).powi(2) * (FLEX_FLOOR + c)).sum::<f64>() / scales.len().max(1) as f64
}

pub struct Pchip {
    xs: Vec<f64>,
    ys: Vec<f64>,
    ms: Vec<f64>,
}

impl Pchip {
    pub fn new(xs: Vec<f64>, ys: Vec<f64>) -> Self {
        let n = xs.len();
        let h: Vec<f64> = xs.windows(2).map(|p| p[1] - p[0]).collect();
        let delta: Vec<f64> = (0..n - 1).map(|k| (ys[k + 1] - ys[k]) / h[k]).collect();
        let mut ms = vec![0.0f64; n];
        ms[0] = delta[0];
        ms[n - 1] = delta[n - 2];
        for k in 1..n - 1 {
            let (a, b) = (delta[k - 1], delta[k]);
            if a * b > 0.0 {
                let (w1, w2) = (2.0 * h[k] + h[k - 1], h[k] + 2.0 * h[k - 1]);
                ms[k] = (w1 + w2) / (w1 / a + w2 / b);
            }
        }
        Self { xs, ys, ms }
    }

    pub fn from_scales(scales: &[f64]) -> Self {
        let k = scales.len() as f64;
        let mut edges = Vec::with_capacity(scales.len() + 1);
        let mut run = 0.0f64;
        edges.push(0.0);
        for s in scales {
            run += s / k;
            edges.push(run);
        }
        let total = run;
        for e in edges.iter_mut() {
            *e /= total;
        }
        let last = edges.len() - 1;
        edges[last] = 1.0;
        let ys = (0..=scales.len()).map(|j| j as f64 / k).collect();
        Self::new(edges, ys)
    }

    pub fn at(&self, x: f64) -> f64 {
        let n = self.xs.len();
        let x = x.clamp(self.xs[0], self.xs[n - 1]);
        let i = match self.xs.binary_search_by(|v| v.total_cmp(&x)) {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let h = self.xs[i + 1] - self.xs[i];
        let s = (x - self.xs[i]) / h;
        let (s2, s3) = (s * s, s * s * s);
        self.ys[i] * (2.0 * s3 - 3.0 * s2 + 1.0) + h * self.ms[i] * (s3 - 2.0 * s2 + s) + self.ys[i + 1] * (-2.0 * s3 + 3.0 * s2) + h * self.ms[i + 1] * (s3 - s2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bump() -> Vec<f64> {
        (0..BINS).map(|j| if (25..35).contains(&j) { 1.0 } else { 0.0 }).collect()
    }

    #[test]
    fn the_band_wraps_the_importance() {
        assert_eq!(band(&bump()), Some((25, 33)));
        assert_eq!(band(&vec![0.0; BINS]), None);
        let mut one = vec![0.0; BINS];
        one[10] = 2.0;
        assert_eq!(band(&one), Some((10, 10)));
    }

    #[test]
    fn rigid_bins_and_their_neighbours_do_not_give() {
        let e = elasticity(&bump());
        assert!(e[30] == 0.0 && e[24] < 0.4 && e[35] < 0.4);
        assert!(e[10] == 1.0 && e[50] == 1.0);
    }

    #[test]
    fn scales_stay_inside_the_limits_and_keep_the_length() {
        let plan = Plan::new(&bump(), 1.0).unwrap();
        assert!(plan.ahead > 0.02 && plan.back > 0.02);
        for shift in [-plan.back, -plan.back / 3.0, 0.0, plan.ahead / 2.0, plan.ahead, 9.0, -9.0] {
            let scales = plan.scales(shift);
            let length: f64 = scales.iter().sum();
            assert!((length - BINS as f64).abs() < 1e-9);
            assert!(scales.iter().all(|s| *s >= 1.0 - SQUEEZE - 1e-3 && *s <= 1.0 + STRETCH + 1e-3), "{scales:?}");
            assert!(scales[28..32].iter().all(|s| (*s - 1.0).abs() < 1e-9));
        }
        assert!(plan.scales(0.0).iter().all(|s| (*s - 1.0).abs() < 1e-12));
    }

    #[test]
    fn shifts_include_zero_and_both_ends() {
        let plan = Plan::new(&bump(), 1.0).unwrap();
        let shifts = plan.shifts();
        assert_eq!(shifts.len(), 65);
        assert_eq!(shifts[32], 0.0);
        assert_eq!((shifts[0], shifts[64]), (-plan.back, plan.ahead));
        assert!(shifts.windows(2).all(|p| p[1] > p[0]));
    }

    #[test]
    fn a_positive_shift_moves_the_band_ahead() {
        let plan = Plan::new(&bump(), 1.0).unwrap();
        let curve = Pchip::from_scales(&plan.scales(plan.ahead));
        let source = 30.0 / 64.0;
        let shown = (0..=4096).map(|i| i as f64 / 4096.0).find(|u| curve.at(*u) >= source).unwrap();
        assert!(shown > source + plan.ahead * 0.8, "{shown} {source} {}", plan.ahead);
        assert!((curve.at(0.0)).abs() < 1e-12 && (curve.at(1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_wide_or_rigid_axis_is_left_alone() {
        let mut rigid = bump();
        for c in rigid.iter_mut().take(25) {
            *c = 0.6;
        }
        assert!(Plan::new(&rigid, 1.0).is_none());
        assert!(Plan::new(&vec![1.0; BINS], 1.0).is_none());
        assert!(Plan::new(&vec![0.0; BINS], 1.0).is_none());
        assert!(Plan::new(&bump(), 0.0).is_none());
    }

    #[test]
    fn pchip_is_monotone_and_exact_at_knots() {
        let xs = vec![0.0, 0.1, 0.5, 0.55, 1.0];
        let ys = vec![0.0, 0.3, 0.4, 0.9, 1.0];
        let curve = Pchip::new(xs.clone(), ys.clone());
        for (x, y) in xs.iter().zip(&ys) {
            assert!((curve.at(*x) - y).abs() < 1e-12);
        }
        let mut last = -1.0;
        for i in 0..=10000 {
            let v = curve.at(i as f64 / 10000.0);
            assert!(v >= last);
            last = v;
        }
    }
}
