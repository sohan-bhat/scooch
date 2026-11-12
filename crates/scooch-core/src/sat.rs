#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Place {
    index: usize,
    frac: f64,
    pub at: f64,
}

fn place(v: f64, size: usize) -> Place {
    if size == 0 {
        return Place { index: 0, frac: 0.0, at: 0.0 };
    }
    let at = if v.is_nan() { 0.0 } else { v.clamp(0.0, size as f64) };
    let index = (at as usize).min(size - 1);
    Place { index, frac: at - index as f64, at }
}

pub struct Sat<const N: usize> {
    width: usize,
    height: usize,
    sums: Vec<[f64; N]>,
}

impl<const N: usize> Sat<N> {
    pub fn new(planes: [&[f32]; N], width: usize, height: usize) -> Self {
        let stride = width + 1;
        let mut sums = vec![[0.0f64; N]; stride * (height + 1)];
        if planes.iter().all(|p| p.len() == width * height) {
            for y in 0..height {
                let mut run = [0.0f64; N];
                for x in 0..width {
                    for c in 0..N {
                        run[c] += planes[c][y * width + x].max(0.0) as f64;
                        sums[(y + 1) * stride + x + 1][c] = sums[y * stride + x + 1][c] + run[c];
                    }
                }
            }
        }
        Self { width, height, sums }
    }

    #[inline]
    fn at(&self, x: f64, y: f64) -> [f64; N] {
        let stride = self.width + 1;
        let xi = (x as usize).min(self.width - 1);
        let yi = (y as usize).min(self.height - 1);
        let (tx, ty) = (x - xi as f64, y - yi as f64);
        let i = yi * stride + xi;
        let (a, b, c, d) = (&self.sums[i], &self.sums[i + 1], &self.sums[i + stride], &self.sums[i + stride + 1]);
        let mut out = [0.0f64; N];
        for k in 0..N {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bottom = c[k] + (d[k] - c[k]) * tx;
            out[k] = top + (bottom - top) * ty;
        }
        out
    }

    pub fn sum(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> [f64; N] {
        if self.width == 0 || self.height == 0 {
            return [0.0; N];
        }
        let (w, h) = (self.width as f64, self.height as f64);
        let (xa, xb) = (x0.min(x1).clamp(0.0, w), x0.max(x1).clamp(0.0, w));
        let (ya, yb) = (y0.min(y1).clamp(0.0, h), y0.max(y1).clamp(0.0, h));
        if !(xb > xa && yb > ya) {
            return [0.0; N];
        }
        let (p, q, r, s) = (self.at(xb, yb), self.at(xa, yb), self.at(xb, ya), self.at(xa, ya));
        let mut out = [0.0f64; N];
        for k in 0..N {
            out[k] = (p[k] - q[k] - r[k] + s[k]).max(0.0);
        }
        out
    }

    pub fn place_x(&self, x: f64) -> Place {
        place(x, self.width)
    }

    pub fn place_y(&self, y: f64) -> Place {
        place(y, self.height)
    }

    #[inline]
    fn corner(&self, x: Place, y: Place) -> [f64; N] {
        let stride = self.width + 1;
        let i = y.index * stride + x.index;
        let (a, b, c, d) = (&self.sums[i], &self.sums[i + 1], &self.sums[i + stride], &self.sums[i + stride + 1]);
        let mut out = [0.0f64; N];
        for k in 0..N {
            let top = a[k] + (b[k] - a[k]) * x.frac;
            let bottom = c[k] + (d[k] - c[k]) * x.frac;
            out[k] = top + (bottom - top) * y.frac;
        }
        out
    }

    #[inline]
    pub fn sum_placed(&self, xa: Place, xb: Place, ya: Place, yb: Place) -> [f64; N] {
        if self.width == 0 || self.height == 0 || !(xb.at > xa.at && yb.at > ya.at) {
            return [0.0; N];
        }
        let (p, q, r, s) = (self.corner(xb, yb), self.corner(xa, yb), self.corner(xb, ya), self.corner(xa, ya));
        let mut out = [0.0f64; N];
        for k in 0..N {
            out[k] = (p[k] - q[k] - r[k] + s[k]).max(0.0);
        }
        out
    }

    pub fn total(&self) -> [f64; N] {
        self.sums.last().copied().unwrap_or([0.0; N])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whole_and_partial_cells_add_up() {
        let values: Vec<f32> = (0..12).map(|i| i as f32).collect();
        let sat = Sat::new([&values], 4, 3);
        assert_eq!(sat.total()[0], 66.0);
        assert_eq!(sat.sum(0.0, 0.0, 4.0, 3.0)[0], 66.0);
        assert_eq!(sat.sum(1.0, 1.0, 3.0, 2.0)[0], 11.0);
        assert!((sat.sum(0.5, 0.0, 1.5, 1.0)[0] - 0.5).abs() < 1e-12);
        assert!((sat.sum(1.25, 1.5, 1.75, 2.5)[0] - 0.5 * (0.5 * 5.0 + 0.5 * 9.0)).abs() < 1e-12);
        assert_eq!(sat.sum(3.0, 2.0, 1.0, 1.0)[0], 11.0);
        assert_eq!(sat.sum(-5.0, -5.0, 99.0, 99.0)[0], 66.0);
        assert_eq!(sat.sum(2.0, 1.0, 2.0, 2.0)[0], 0.0);
        assert_eq!(sat.sum(f64::NAN, 0.0, 1.0, 1.0)[0], 0.0);
    }

    #[test]
    fn placed_sums_match_plain_sums() {
        let values: Vec<f32> = (0..35).map(|i| ((i * 7) % 11) as f32).collect();
        let sat = Sat::new([&values], 7, 5);
        for (x0, y0, x1, y1) in [(0.3, 0.2, 6.1, 4.9), (-2.0, 1.0, 3.5, 9.0), (2.0, 2.0, 2.0, 3.0), (1.25, 0.5, 6.999, 5.0)] {
            let placed = sat.sum_placed(sat.place_x(x0), sat.place_x(x1), sat.place_y(y0), sat.place_y(y1));
            assert!((placed[0] - sat.sum(x0, y0, x1, y1)[0]).abs() < 1e-9);
        }
    }

    #[test]
    fn two_planes_are_summed_together() {
        let a = vec![1.0f32; 6];
        let b = vec![2.0f32; 6];
        let sat = Sat::new([&a, &b], 3, 2);
        assert_eq!(sat.sum(0.0, 0.0, 1.5, 2.0), [3.0, 6.0]);
    }

    #[test]
    fn empty_maps_sum_to_zero() {
        let sat = Sat::<1>::new([&[]], 0, 0);
        assert_eq!(sat.sum(0.0, 0.0, 1.0, 1.0), [0.0]);
    }
}
