use crate::look::*;
use crate::math;
use crate::shape::{self, smoothstep, Ground};
use rayon::prelude::*;

#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Icon {
    pub(crate) r: [f32; 4],
    pub(crate) sx: f32,
    pub(crate) sy: f32,
    pub(crate) reach: [f32; 4],
}

pub(crate) fn icons(g: &Ground, cell: f32) -> Vec<Icon> {
    g.icons
        .iter()
        .enumerate()
        .map(|(k, r)| {
            let sx = SPREAD.0 * cell * (SCALE.0 + SCALE.1 * unit_hash(k as i32, 7, 41));
            let sy = SPREAD.1 * cell * (0.85 + 0.3 * unit_hash(k as i32, 8, 41));
            let wide = SUPPORT * sx * (STRETCH.0 + STRETCH.1) * (1.0 + FLARE.0);
            Icon { r: *r, sx, sy, reach: [r[0] - wide, r[1] - SUPPORT * sy, r[2] + wide, r[3] + SUPPORT * BELOW * sy] }
        })
        .collect()
}

pub(crate) struct Shape<'a> {
    pub(crate) fixed: &'a Fixed,
    pub(crate) icons: Vec<Icon>,
    pub(crate) cell: f32,
    pub(crate) floor: f32,
}

impl Shape<'_> {
    pub(crate) fn stretch(&self, x: f32, y: f32) -> f32 {
        smoothstep(0.2, 0.8, 0.5 + 0.5 * shape::noise(x / (STRETCH.2 * self.cell), y / (STRETCH.2 * self.cell), STRETCH.3))
    }

    pub(crate) fn cover(&self, x: f32, y: f32) -> f32 {
        let reach = STRETCH.0 + STRETCH.1 * self.stretch(x, y);
        let mut keep = 1.0f32;
        for icon in &self.icons {
            if x <= icon.reach[0] || x >= icon.reach[2] || y <= icon.reach[1] || y >= icon.reach[3] {
                continue;
            }
            let r = &icon.r;
            let (dx, dy) = ((r[0] - x).max(x - r[2]).max(0.0), (r[1] - y).max(y - r[3]).max(0.0));
            let flare = 1.0 + FLARE.0 * smoothstep(0.0, FLARE.1 * self.cell, y - r[3]);
            let sy = if y > r[3] { BELOW * icon.sy } else { icon.sy };
            let (qx, qy) = (dx / (icon.sx * reach * flare), dy / sy);
            let q = qx * qx + qy * qy;
            if q >= SUPPORT * SUPPORT {
                continue;
            }
            let g = ((math::exp(-0.5 * q) - self.floor) / (1.0 - self.floor)).clamp(0.0, 1.0);
            keep *= 1.0 - g;
        }
        1.0 - keep
    }

    pub(crate) fn cloud(&self, i: i32, j: i32, x: f32, y: f32) -> [f32; 3] {
        let none = [x, y, 0.0];
        if unit_hash(i, j, 13) >= BIG {
            return none;
        }
        let f = self.fixed;
        let c = self.cover(x, y);
        if c <= 0.01 {
            return none;
        }
        let airy = smoothstep(0.3, 0.7, f.at(&f.sky, x, y)) * smoothstep(0.05, 0.4, f.at(&f.belong, x, y));
        if airy <= 0.0 {
            return none;
        }
        let cell = self.cell;
        let swell = 1.0 + SWELL.0 * shape::noise(x / (SWELL.1 * cell), y / (SWELL.1 * cell), SWELL.2);
        let swell = swell + (swell.max(1.0) - swell) * smoothstep(FIRM.0, FIRM.1, c);
        let vg = self.cover(x, y + TILT * cell) - self.cover(x, y - TILT * cell);
        let (top, base) = (smoothstep(0.0, 0.2, vg), smoothstep(0.0, 0.2, -vg));
        let mut r = ((PUFF.0 + PUFF.1 * unit_hash(i, j, 14)) * (1.0 + RISE * top) * cell * smoothstep(0.06, 0.3, c) * swell * airy * (0.6 + 0.4 * self.stretch(x, y))).min(MOST * cell);
        r *= smoothstep(FADE.0 * cell, FADE.1 * cell, r);
        if r <= 0.0 {
            return none;
        }
        let clear = f.subject(x, y) - CLEAR * cell;
        r = (r * smoothstep(ROOM.0, ROOM.1, clear / r)).min(clear.max(0.0));
        if r <= 0.0 {
            return none;
        }
        let wide = (r * (1.0 + WIDE * base)).min(clear.max(0.0)).min(MOST * cell);
        [x, y - (wide - r).max(0.0), wide.max(r)]
    }

    pub(crate) fn isle(&self, i: i32, j: i32, x: f32, y: f32, bare: &[(Icon, f32)]) -> [f32; 3] {
        let none = [x, y, 0.0];
        if (i + 2 * j).rem_euclid(3) != 0 {
            return none;
        }
        let f = self.fixed;
        let c = self.cover(x, y);
        if c <= ISLE_COVER.0 {
            return none;
        }
        let cell = self.cell;
        let gate = smoothstep(0.05, 0.4, f.at(&f.belong, x, y));
        let swell = 1.0 + SWELL.0 * shape::noise(x / (SWELL.1 * cell), y / (SWELL.1 * cell), SWELL.2);
        let natural = ((ISLE.0 + ISLE.1 * unit_hash(i, j, 15)) * cell * smoothstep(ISLE_COVER.0, ISLE_COVER.1, c) * (1.0 - f.at(&f.sky, x, y)) * gate * swell).min(ISLE_MOST * cell);
        if natural <= 0.0 {
            return none;
        }
        let clear = f.subject(x, y) - CLEAR * cell;
        let (mut lx, mut ly) = (x, y);
        if natural > clear {
            let s = f.grid.cell as f32;
            let (gx, gy) = (f.subject(x + s, y) - f.subject(x - s, y), f.subject(x, y + s) - f.subject(x, y - s));
            let m = length(gx, gy);
            if m > 1e-3 {
                let push = (natural - clear).min(PUSH * cell);
                (lx, ly) = (x + gx / m * push, y + gy / m * push);
            }
        }
        let mut r = natural.min(f.subject(lx, ly) - CLEAR * cell).max(0.0);
        r *= smoothstep(ISLE_FADE.0 * cell, ISLE_FADE.1 * cell, r);
        for (icon, weight) in bare {
            let q = &icon.r;
            let away = length((q[0] - lx).max(lx - q[2]).max(0.0), (q[1] - ly).max(ly - q[3]).max(0.0));
            r *= 1.0 - weight * (1.0 - smoothstep(BARE.0 * cell, BARE.1 * cell, away));
        }
        [lx, ly, r]
    }

    pub(crate) fn cores(&self) -> Cores {
        let f = self.fixed;
        let cell = self.cell;
        let mut out = Vec::with_capacity(self.icons.len() * 4);
        let mut air = Vec::with_capacity(self.icons.len() * 4);
        let mut bare = Vec::new();
        for icon in &self.icons {
            let r = &icon.r;
            let (cx, cy) = (0.5 * (r[0] + r[2]), 0.5 * (r[1] + r[3]));
            let (qx, qy) = (0.25 * (r[2] - r[0]), 0.25 * (r[3] - r[1]));
            let water = smoothstep(0.6, 0.4, f.at(&f.sky, cx, cy)) * smoothstep(CORE_GATE.0, CORE_GATE.1, f.at(&f.belong, cx, cy));
            let spots = [(cx - qx, cy - qy), (cx + qx, cy - qy), (cx - qx, cy + qy), (cx + qx, cy + qy)];
            let room = spots.iter().map(|(x, y)| f.solid(*x, *y) - CORE * cell).fold(f32::MAX, f32::min);
            let fit = smoothstep(FIT.0 * cell, FIT.1 * cell, room);
            let sky = smoothstep(0.3, 0.7, f.at(&f.sky, cx, cy)) * smoothstep(0.05, 0.4, f.at(&f.belong, cx, cy));
            for (x, y) in spots {
                out.push([x, y, CORE * cell * water * fit]);
                let r = CORE * cell * sky;
                let clear = f.subject(x, y) - CLEAR * cell;
                let r = if r > 0.0 { (r * smoothstep(ROOM.0, ROOM.1, clear / r)).min(clear.max(0.0)) } else { 0.0 };
                air.push([x, y, r * smoothstep(FADE.0 * cell, FADE.1 * cell, r)]);
            }
            if water * (1.0 - fit) > 0.0 {
                bare.push((*icon, water * (1.0 - fit)));
            }
        }
        Cores { land: out, air, bare }
    }
}

pub(crate) struct Cores {
    pub(crate) land: Vec<[f32; 3]>,
    pub(crate) air: Vec<[f32; 3]>,
    pub(crate) bare: Vec<(Icon, f32)>,
}

pub(crate) struct Splat {
    pub(crate) extra: f32,
    pub(crate) k: f32,
    pub(crate) floor: f32,
    pub(crate) start: f32,
}

pub(crate) struct Lattice {
    pub(crate) spacing: f32,
    pub(crate) cols: i32,
    pub(crate) rows: i32,
}

impl Lattice {
    pub(crate) fn new(width: usize, height: usize, cell: f32) -> Self {
        let spacing = SPACING * cell;
        Self { spacing, cols: (width as f32 / spacing).ceil() as i32 + 6, rows: (height as f32 / spacing).ceil() as i32 + 6 }
    }

    pub(crate) fn len(&self) -> usize {
        (self.cols * self.rows) as usize
    }

    pub(crate) fn ij(&self, k: usize) -> (i32, i32) {
        (k as i32 % self.cols - 3, k as i32 / self.cols - 3)
    }

    pub(crate) fn site(&self, k: usize) -> (f32, f32) {
        let (i, j) = self.ij(k);
        ((i as f32 + 0.5 + JITTER * (unit_hash(i, j, 11) - 0.5)) * self.spacing, (j as f32 + 0.5 + JITTER * (unit_hash(i, j, 12) - 0.5)) * self.spacing)
    }

    pub(crate) fn within(&self, b: [f32; 4]) -> Vec<usize> {
        let lo = |v: f32| (v / self.spacing).floor() as i32 - 1;
        let hi = |v: f32| (v / self.spacing).ceil() as i32 + 1;
        let (i0, i1) = (lo(b[0]).max(-3), hi(b[2]).min(self.cols - 4));
        let (j0, j1) = (lo(b[1]).max(-3), hi(b[3]).min(self.rows - 4));
        let mut out = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                out.push(((j + 3) * self.cols + i + 3) as usize);
            }
        }
        out
    }
}

pub(crate) struct Buckets {
    pub(crate) size: f32,
    pub(crate) w: i32,
    pub(crate) h: i32,
    pub(crate) cells: Vec<Vec<u32>>,
}

impl Buckets {
    pub(crate) fn new(puffs: &[[f32; 3]], size: f32, width: usize, height: usize) -> Self {
        let (w, h) = ((width as f32 / size).ceil() as i32 + 4, (height as f32 / size).ceil() as i32 + 4);
        let mut cells = vec![Vec::new(); (w * h) as usize];
        for (k, p) in puffs.iter().enumerate() {
            if p[2] > 0.0 {
                let (bx, by) = (((p[0] / size).floor() as i32 + 2).clamp(0, w - 1), ((p[1] / size).floor() as i32 + 2).clamp(0, h - 1));
                cells[(by * w + bx) as usize].push(k as u32);
            }
        }
        Self { size, w, h, cells }
    }

    pub(crate) fn near(&self, x: f32, y: f32) -> impl Iterator<Item = u32> + '_ {
        let (bx, by) = ((x / self.size).floor() as i32 + 2, (y / self.size).floor() as i32 + 2);
        (by - 1..=by + 1).flat_map(move |j| (bx - 1..=bx + 1).map(move |i| (i, j))).filter(|(i, j)| *i >= 0 && *j >= 0 && *i < self.w && *j < self.h).flat_map(move |(i, j)| self.cells[(j * self.w + i) as usize].iter().copied())
    }
}

pub(crate) fn pile_flags(lattice: &Lattice) -> Vec<bool> {
    let n = lattice.len();
    let mut flags = vec![false; n];
    let size = PILE_CELL / SPACING;
    let mut best: std::collections::HashMap<(i32, i32), (f32, usize)> = std::collections::HashMap::new();
    for k in 0..n {
        let (i, j) = lattice.ij(k);
        if unit_hash(i, j, 13) >= BIG {
            continue;
        }
        let (ci, cj) = ((i as f32 / size).floor() as i32, (j as f32 / size).floor() as i32);
        if unit_hash(ci, cj, 17) < PILE_SKIP {
            continue;
        }
        let score = unit_hash(i, j, 16);
        let entry = best.entry((ci, cj)).or_insert((f32::MAX, usize::MAX));
        if score < entry.0 {
            *entry = (score, k);
        }
    }
    for (_, (_, k)) in best {
        if k != usize::MAX {
            flags[k] = true;
        }
    }
    flags
}

pub(crate) fn lattice_raw(shape: &Shape, lattice: &Lattice, which: &[usize], old: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let mut out = old.to_vec();
    let fresh: Vec<(usize, [f32; 3])> = which
        .par_iter()
        .map(|&k| {
            let (i, j) = lattice.ij(k);
            let (x, y) = lattice.site(k);
            (k, shape.cloud(i, j, x, y))
        })
        .collect();
    for (k, p) in fresh {
        out[k] = p;
    }
    out
}

pub(crate) fn isles(shape: &Shape, lattice: &Lattice, which: &[usize], old: &[[f32; 3]], bare: &[(Icon, f32)]) -> Vec<[f32; 3]> {
    let mut out = old.to_vec();
    let fresh: Vec<(usize, [f32; 3])> = which
        .par_iter()
        .map(|&k| {
            let (i, j) = lattice.ij(k);
            let (x, y) = lattice.site(k);
            (k, shape.isle(i, j, x, y, bare))
        })
        .collect();
    for (k, p) in fresh {
        out[k] = p;
    }
    out
}

pub(crate) fn finished(lattice: &Lattice, raw: &[[f32; 3]], which: &[usize], old: &[[f32; 3]], cell: f32) -> Vec<[f32; 3]> {
    let mut out = old.to_vec();
    let reach = 2.0 * MOST * cell + 2.0 * lattice.spacing;
    let fresh: Vec<(usize, [f32; 3])> = which
        .par_iter()
        .map(|&k| {
            let p = raw[k];
            let need = smoothstep(SMALL.0 * cell, SMALL.1 * cell, p[2]);
            if p[2] <= 0.0 || need <= 0.0 {
                return (k, p);
            }
            let others = lattice.within([p[0] - reach, p[1] - reach, p[0] + reach, p[1] + reach]);
            let spots: [(f32, f32); 13] = std::array::from_fn(|s| (p[0] + SPOTS[s].0 * p[2], p[1] + SPOTS[s].1 * p[2]));
            let mut share = 0.0f32;
            for (sx, sy) in spots {
                let mut covered = 0.0f32;
                for &q in &others {
                    if q == k {
                        continue;
                    }
                    let o = raw[q];
                    if o[2] <= 0.0 {
                        continue;
                    }
                    covered = covered.max(smoothstep(SOFT * o[2], -SOFT * o[2], length(sx - o[0], sy - o[1]) - o[2]));
                    if covered >= 1.0 {
                        break;
                    }
                }
                share += covered;
            }
            let keep = smoothstep(INSIDE.0, INSIDE.1, share / 13.0);
            (k, [p[0], p[1], p[2] * (1.0 + (keep - 1.0) * need)])
        })
        .collect();
    for (k, p) in fresh {
        out[k] = p;
    }
    out
}

