pub const BINS: usize = 64;
const RIGID_FROM: f64 = 0.12;
const RIGID_AT: f64 = 0.35;
const BUSY_DAMP: f64 = 0.6;
const WIDEST_CORE: f64 = 0.85;
const LEAST_SIDE: f64 = 0.5;
const STRETCH: f64 = 0.45;
const SQUEEZE: f64 = 0.35;
const SIDE_STEPS: usize = 32;
const FLEX_FLOOR: f64 = 0.15;
const BUSY_WEIGHT: f64 = 0.5;

fn smoothstep(a: f64, b: f64, x: f64) -> f64 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn box3(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    (0..n).map(|k| (values[k.saturating_sub(1)] + values[k] + values[(k + 1).min(n - 1)]) / 3.0).collect()
}

pub fn rigidity(peak: &[f64]) -> Vec<f64> {
    peak.iter().map(|c| smoothstep(RIGID_FROM, RIGID_AT, *c)).collect()
}

pub fn flex(peak: &[f64], busy: &[f64]) -> Vec<f64> {
    let n = peak.len();
    let raw: Vec<f64> = rigidity(peak).iter().zip(busy).map(|(r, b)| (1.0 - r) * (1.0 - BUSY_DAMP * b.clamp(0.0, 1.0))).collect();
    let low: Vec<f64> = (0..n).map(|k| raw[k.saturating_sub(1)..(k + 2).min(n)].iter().copied().fold(f64::INFINITY, f64::min)).collect();
    box3(&low)
}

pub fn core(peak: &[f64]) -> Option<(usize, usize)> {
    let rigid = rigidity(peak);
    let first = rigid.iter().position(|r| *r >= 0.5)?;
    let last = rigid.iter().rposition(|r| *r >= 0.5)?;
    Some((first, last))
}

pub fn penalty(scales: &[f64], peak: &[f64], busy: &[f64]) -> f64 {
    let n = scales.len().max(1) as f64;
    scales.iter().zip(peak).zip(busy).map(|((s, c), b)| (s - 1.0).powi(2) * (FLEX_FLOOR + c + BUSY_WEIGHT * b)).sum::<f64>() / n
}

pub struct Plan {
    flex: Vec<f64>,
    core: (usize, usize),
    low: f64,
    high: f64,
    pub ahead: f64,
    pub back: f64,
}

impl Plan {
    pub fn new(peak: &[f64], busy: &[f64], strength: f64) -> Option<Self> {
        if peak.len() != BINS || busy.len() != BINS || strength.is_nan() || strength <= 0.0 {
            return None;
        }
        let (ka, kb) = core(peak)?;
        if (kb - ka + 1) as f64 > WIDEST_CORE * BINS as f64 {
            return None;
        }
        let flex = flex(peak, busy);
        let low: f64 = flex[..ka].iter().sum();
        let high: f64 = flex[kb + 1..].iter().sum();
        if low < LEAST_SIDE || high < LEAST_SIDE {
            return None;
        }
        let k = BINS as f64;
        let ahead = (STRETCH * strength * low).min(SQUEEZE * strength * high) / k;
        let back = (SQUEEZE * strength * low).min(STRETCH * strength * high) / k;
        Some(Self { flex, core: (ka, kb), low, high, ahead, back })
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
            .flex
            .iter()
            .enumerate()
            .map(|(j, f)| {
                if j < self.core.0 {
                    1.0 + d * k * f / self.low
                } else if j > self.core.1 {
                    1.0 - d * k * f / self.high
                } else {
                    1.0
                }
            })
            .collect();
        let smooth = box3(&box3(&raw));
        let sum: f64 = smooth.iter().sum();
        smooth.iter().map(|s| s * k / sum).collect()
    }
}

#[derive(Clone)]
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
    fn the_core_spans_the_rigid_bins() {
        assert_eq!(core(&bump()), Some((25, 34)));
        assert_eq!(core(&vec![0.0; BINS]), None);
        let mut dot = vec![0.0; BINS];
        dot[10] = 0.6;
        dot[40] = 0.9;
        assert_eq!(core(&dot), Some((10, 40)));
    }

    #[test]
    fn busy_bins_flex_less_but_stay_flexible() {
        let calm = flex(&bump(), &vec![0.0; BINS]);
        let busy = flex(&bump(), &vec![1.0; BINS]);
        assert!(calm[30] == 0.0 && calm[5] == 1.0);
        assert!((busy[5] - 0.4).abs() < 1e-9);
        assert!(calm[24] < 0.4 && calm[35] < 0.4);
    }

    #[test]
    fn every_bin_stays_inside_its_own_limit() {
        let busy: Vec<f64> = (0..BINS).map(|j| (j % 7) as f64 / 7.0).collect();
        let plan = Plan::new(&bump(), &busy, 1.0).unwrap();
        let f = flex(&bump(), &busy);
        for shift in [-plan.back, -plan.back / 3.0, plan.ahead / 2.0, plan.ahead, 9.0, -9.0] {
            let d = shift.clamp(-plan.back, plan.ahead);
            let k = BINS as f64;
            for (j, fj) in f.iter().enumerate() {
                let raw = if j < 25 { 1.0 + d * k * fj / plan.low } else if j > 34 { 1.0 - d * k * fj / plan.high } else { 1.0 };
                assert!(raw <= 1.0 + STRETCH * fj + 1e-9 && raw >= 1.0 - SQUEEZE * fj - 1e-9, "{j} {raw} {fj}");
            }
            let scales = plan.scales(shift);
            assert!((scales.iter().sum::<f64>() - k).abs() < 1e-9);
            assert!(scales.iter().all(|s| *s >= 1.0 - SQUEEZE - 1e-3 && *s <= 1.0 + STRETCH + 1e-3));
            assert!(scales[28..32].iter().all(|s| (*s - 1.0).abs() < 1e-9));
        }
        assert!(plan.scales(0.0).iter().all(|s| (*s - 1.0).abs() < 1e-12));
    }

    #[test]
    fn shifts_include_zero_and_both_ends() {
        let plan = Plan::new(&bump(), &vec![0.0; BINS], 1.0).unwrap();
        let shifts = plan.shifts();
        assert_eq!(shifts.len(), 65);
        assert_eq!(shifts[32], 0.0);
        assert_eq!((shifts[0], shifts[64]), (-plan.back, plan.ahead));
        assert!(shifts.windows(2).all(|p| p[1] > p[0]));
    }

    #[test]
    fn a_positive_shift_moves_the_core_ahead() {
        let plan = Plan::new(&bump(), &vec![0.0; BINS], 1.0).unwrap();
        let curve = Pchip::from_scales(&plan.scales(plan.ahead));
        let source = 30.0 / 64.0;
        let shown = (0..=4096).map(|i| i as f64 / 4096.0).find(|u| curve.at(*u) >= source).unwrap();
        assert!(shown > source + plan.ahead * 0.8, "{shown} {source} {}", plan.ahead);
        assert!(curve.at(0.0).abs() < 1e-12 && (curve.at(1.0) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn a_wide_core_or_a_stiff_side_is_left_alone() {
        let mut wide = vec![0.0; BINS];
        wide[2] = 1.0;
        wide[60] = 1.0;
        assert!(Plan::new(&wide, &vec![0.0; BINS], 1.0).is_none());
        let mut edge = vec![0.0; BINS];
        edge[0] = 1.0;
        assert!(Plan::new(&edge, &vec![0.0; BINS], 1.0).is_none());
        assert!(Plan::new(&vec![0.0; BINS], &vec![0.0; BINS], 1.0).is_none());
        assert!(Plan::new(&bump(), &vec![0.0; BINS], 0.0).is_none());
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
