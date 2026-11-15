use crate::area::{self, Area};
use crate::look::*;
use crate::puffs::*;
use crate::shape::{self, smoothstep, Grid, Ground};
use crate::{blur, math};
use rayon::prelude::*;

pub struct Live {
    lattice: Lattice,
    icons: Vec<Icon>,
    labels: Spots,
    raw: Vec<[f32; 3]>,
    clouds: Vec<[f32; 3]>,
    isles: Vec<[f32; 3]>,
    cores: Vec<[f32; 3]>,
    puffs: Vec<[f32; 3]>,
    piles: Vec<bool>,
    prio: Vec<f32>,
    cloud: Vec<f32>,
    land: Vec<f32>,
    solid: Vec<u8>,
    closed: Vec<u8>,
    deep: Vec<f32>,
    lit: Vec<f32>,
    px: Vec<f32>,
    u: f32,
    pub look: Vec<u8>,
}

fn field_of(px: f32, u: f32) -> f32 {
    (px / u).clamp(HOLD.0, HOLD.1)
}

impl Live {
    pub fn sites(&self) -> usize {
        self.lattice.len()
    }

    pub fn bytes(&self) -> usize {
        let floats = [&self.cloud, &self.land, &self.deep, &self.lit, &self.px].iter().map(|v| v.len()).sum::<usize>();
        floats * 4 + self.solid.len() + self.closed.len() + self.look.len() + (self.raw.len() + self.clouds.len() + self.isles.len() + self.cores.len() + self.puffs.len()) * 12 + self.piles.len() + self.prio.len() * 4
    }

    pub fn field(&self) -> Vec<f32> {
        self.px.iter().map(|v| field_of(*v, self.u)).collect()
    }

    pub fn field_in(&self, w: usize, a: Area) -> Vec<f32> {
        area::take(&self.px, w, a).into_iter().map(|v| field_of(v, self.u)).collect()
    }
}

struct Run<'a> {
    fixed: &'a Fixed,
    grid: Grid,
    u: f32,
    cell: f32,
}

impl Run<'_> {
    fn clamp_px(&self) -> (f32, f32) {
        (FLOOR * self.u, HOLD.1 * self.u)
    }

    fn shallow(&self, x: f32, y: f32) -> f32 {
        (SHALLOW.0 + SHALLOW.1 * smoothstep(0.2, 0.8, 0.5 + 0.5 * shape::noise(x / (SHALLOW.2 * self.cell), y / (SHALLOW.2 * self.cell), SHALLOW.3))) * self.cell
    }

    fn guard(&self, x: f32, y: f32) -> f32 {
        smoothstep(GUARD.0 * self.cell, GUARD.1 * self.cell, self.fixed.solid(x, y))
    }

    fn splat(&self, (puffs, rows): (&[[f32; 3]], &[Vec<u32>]), how: Splat, area: Area, out: &mut [f32]) {
        let Splat { extra, k, floor, start } = how;
        let grid = self.grid;
        let cell = grid.cell as f32;
        out.par_chunks_mut(grid.w).enumerate().skip(area[1]).take(area[3] - area[1]).for_each(|(y, row)| {
            let py = (y as f32 + 0.5) * cell;
            row[area[0]..area[2]].fill(start);
            for &n in &rows[y] {
                let p = &puffs[n as usize];
                let reach = p[2] + extra;
                let dy = py - p[1];
                if dy * dy >= reach * reach {
                    continue;
                }
                let span = (reach * reach - dy * dy).sqrt();
                let x0 = (((p[0] - span) / cell - 0.5).floor().max(0.0) as usize).max(area[0]);
                let x1 = ((((p[0] + span) / cell - 0.5).ceil() + 1.0).max(0.0) as usize).min(area[2]);
                for (x, d) in row.iter_mut().enumerate().take(x1).skip(x0) {
                    if *d <= floor {
                        continue;
                    }
                    let dx = (x as f32 + 0.5) * cell - p[0];
                    if dx * dx + dy * dy >= reach * reach {
                        continue;
                    }
                    *d = smooth_min(*d, length(dx, dy) - p[2], k);
                }
            }
        });
    }

    fn fields_into(&self, clouds: (&[[f32; 3]], &[Vec<u32>]), land: (&[[f32; 3]], &[Vec<u32>]), area: Area, cloud_out: &mut [f32], land_out: &mut [f32]) {
        let grid = self.grid;
        let (low, high) = self.clamp_px();
        let (kc, kl) = (CLOUD_K * self.cell, LAND_K * self.cell);
        let land_high = high * (SHALLOW.0 + SHALLOW.1) + COAST.0 * self.cell;
        let coast = COAST.0 * self.cell;
        self.splat(clouds, Splat { extra: high + kc, k: kc, floor: low, start: high + kc }, area, cloud_out);
        self.splat(land, Splat { extra: land_high + kl, k: kl, floor: low - coast, start: land_high + kl }, area, land_out);
        cloud_out.par_chunks_mut(grid.w).zip(land_out.par_chunks_mut(grid.w)).enumerate().skip(area[1]).take(area[3] - area[1]).for_each(|(y, (cloud_row, land_row))| {
            for x in area[0]..area[2] {
                cloud_row[x] = cloud_row[x].clamp(low, high);
                let e = land_row[x];
                if e == land_high + kl {
                    land_row[x] = high;
                    continue;
                }
                let (px, py) = grid.center(y * grid.w + x);
                let e = e + coast * shape::noise(px / (COAST.1 * self.cell), py / (COAST.1 * self.cell), COAST.2);
                let scaled = if e > 0.0 { e * self.cell / self.shallow(px, py) + (1.0 - self.guard(px, py)) * PUSHED * self.cell } else { e };
                land_row[x] = scaled.clamp(low, high);
            }
        });
    }
}

fn rows(puffs: &[[f32; 3]], extra: f32, grid: &Grid) -> Vec<Vec<u32>> {
    let cell = grid.cell as f32;
    let mut out = vec![Vec::new(); grid.h];
    for (k, p) in puffs.iter().enumerate() {
        if p[2] <= 0.0 {
            continue;
        }
        let reach = p[2] + extra;
        let y0 = ((p[1] - reach) / cell - 1.5).floor().max(0.0) as usize;
        let y1 = (((p[1] + reach) / cell + 0.5).ceil().max(0.0) as usize).min(grid.h);
        for row in out.iter_mut().take(y1).skip(y0) {
            row.push(k as u32);
        }
    }
    out
}

fn label(icon: &Icon, grow: f32) -> [f32; 4] {
    let r = &icon.r;
    [r[0] - grow, r[1] + LABEL_TOP * (r[3] - r[1]) - grow, r[2] + grow, r[3] + grow]
}

fn labels_of(icons: &[Icon], grid: &Grid, cell: f32) -> Spots {
    let rects: Vec<[f32; 4]> = icons.iter().map(|icon| label(icon, LABEL_FALL * cell)).collect();
    Spots::new(&rects, 2.0 * cell, (grid.w * grid.cell) as f32, (grid.h * grid.cell) as f32)
}

fn label_hold(icons: &[Icon], labels: &Spots, x: f32, y: f32, cell: f32) -> f32 {
    let fall = LABEL_FALL * cell;
    let mut most = 0.0f32;
    for &k in labels.at(x, y) {
        let r = label(&icons[k as usize], 0.0);
        let (dx, dy) = ((r[0] - x).max(x - r[2]).max(0.0), (r[1] - y).max(y - r[3]).max(0.0));
        if dx >= fall || dy >= fall {
            continue;
        }
        most = most.max(smoothstep(fall, 0.0, length(dx, dy)));
    }
    most
}

fn bounds(p: &[f32; 3], grow: f32) -> [f32; 4] {
    [p[0] - p[2] - grow, p[1] - p[2] - grow, p[0] + p[2] + grow, p[1] + p[2] + grow]
}

fn cells_of(grid: &Grid, b: [f32; 4]) -> Area {
    shape::cells_near(grid, &b, 0.0)
}

pub fn live(fixed: &Fixed, g: &Ground) -> Live {
    let grid = fixed.grid;
    let u = fixed.u;
    let cell = CELL * u;
    let (width, height) = (grid.w * grid.cell, grid.h * grid.cell);
    let lattice = Lattice::new(width, height, cell);
    let n = lattice.len();
    let cells = grid.w * grid.h;
    let piles = pile_flags(&lattice);
    let prio: Vec<f32> = (0..n).map(|k| lattice.site(k).1).collect();
    let mut out = Live {
        lattice,
        icons: Vec::new(),
        labels: Spots::new(&[], 2.0 * cell, width as f32, height as f32),
        raw: vec![[0.0; 3]; n],
        clouds: vec![[0.0; 3]; n],
        isles: vec![[0.0; 3]; n],
        cores: Vec::new(),
        puffs: Vec::new(),
        piles,
        prio,
        cloud: vec![f32::NAN; cells],
        land: vec![f32::NAN; cells],
        solid: vec![2; cells],
        closed: vec![2; cells],
        deep: vec![f32::NAN; cells],
        lit: vec![f32::NAN; cells],
        px: vec![f32::NAN; cells],
        u,
        look: vec![0; cells * 4],
    };
    let all: Vec<usize> = (0..n).collect();
    let shape = Shape::new(fixed, icons(g, cell), cell);
    let Cores { land: cores, air: puffs, bare } = shape.cores();
    out.raw = lattice_raw(&shape, &out.lattice, &all, &out.raw);
    out.clouds = finished(&out.lattice, &out.raw, &all, &out.clouds, cell);
    out.isles = isles(&shape, &out.lattice, &all, &out.isles, &bare);
    out.cores = cores;
    out.puffs = puffs;
    out.labels = labels_of(&shape.icons, &grid, cell);
    out.icons = shape.icons;
    let whole = shape::whole(&grid);
    rebuild(fixed, &mut out, &[whole]);
    out
}

fn rebuild(fixed: &Fixed, live: &mut Live, areas: &[Area]) -> (Vec<Area>, Vec<Area>) {
    let grid = fixed.grid;
    let (w, h) = (grid.w, grid.h);
    let u = fixed.u;
    let cell = CELL * u;
    let run = Run { fixed, grid, u, cell };
    let (width, height) = (grid.w * grid.cell, grid.h * grid.cell);
    let mut land_puffs: Vec<[f32; 3]> = live.isles.clone();
    land_puffs.extend_from_slice(&live.cores);
    let mut cloud_puffs: Vec<[f32; 3]> = live.clouds.clone();
    cloud_puffs.extend_from_slice(&live.puffs);
    let high = run.clamp_px().1;
    let cloud_rows = rows(&cloud_puffs, high + CLOUD_K * cell, &grid);
    let land_rows = rows(&land_puffs, high * (SHALLOW.0 + SHALLOW.1) + COAST.0 * cell + LAND_K * cell, &grid);
    let mut changed = Vec::new();
    for a in area::merge(areas.to_vec()) {
        let (before_c, before_l) = (area::take(&live.cloud, w, a), area::take(&live.land, w, a));
        run.fields_into((&cloud_puffs, &cloud_rows), (&land_puffs, &land_rows), a, &mut live.cloud, &mut live.land);
        changed.extend(area::moved(&before_c, &live.cloud, w, a, area::bits));
        changed.extend(area::moved(&before_l, &live.land, w, a, area::bits));
    }
    let changed = area::merge(changed);
    let step = grid.cell as f32;
    let close = CLOSE * cell;
    let r_close = (close / step).ceil() as usize + 1;
    let mut solid_changed = Vec::new();
    for a in &changed {
        let before = area::take(&live.solid, w, *a);
        for y in a[1]..a[3] {
            for x in a[0]..a[2] {
                let i = y * w + x;
                live.solid[i] = (live.cloud[i].min(live.land[i]) < 0.0) as u8;
            }
        }
        solid_changed.extend(area::moved(&before, &live.solid, w, *a, area::equal));
    }
    let solid_changed = area::merge(solid_changed);
    let mut closed_changed = Vec::new();
    for a in area::merge(solid_changed.iter().map(|a| area::grow(*a, 2 * r_close, w, h)).collect()) {
        let wide = area::grow(a, r_close, w, h);
        let near = chamfer(&grid, wide, close, &|i| live.solid[i] > 0);
        let ww = wide[2] - wide[0];
        let filled = chamfer(&grid, a, close, &|i| {
            let (x, y) = (i % w, i / w);
            x < wide[0] || x >= wide[2] || y < wide[1] || y >= wide[3] || near[(y - wide[1]) * ww + x - wide[0]] >= close
        });
        let before = area::take(&live.closed, w, a);
        let aw = a[2] - a[0];
        for y in a[1]..a[3] {
            for x in a[0]..a[2] {
                live.closed[y * w + x] = (filled[(y - a[1]) * aw + x - a[0]] >= close) as u8;
            }
        }
        closed_changed.extend(area::moved(&before, &live.closed, w, a, area::equal));
    }
    let closed_changed = area::merge(closed_changed);
    let cap = DEPTH * cell;
    let r_depth = (cap / step).ceil() as usize + 1;
    let r_blur = blur::reach(DEPTH_BLUR);
    let mut scratch = vec![0.0f32; w * h];
    let mut deep_changed = Vec::new();
    for a in area::merge(closed_changed.iter().map(|a| area::grow(*a, r_depth + r_blur, w, h)).collect()) {
        let crop = area::grow(a, r_blur, w, h);
        let fresh = chamfer(&grid, crop, cap, &|i| live.closed[i] == 0);
        put(&mut scratch, w, crop, &fresh);
        let before = area::take(&live.deep, w, a);
        blur::fixed_into(&scratch, &mut live.deep, w, h, DEPTH_BLUR, a);
        deep_changed.extend(area::moved(&before, &live.deep, w, a, area::bits));
    }
    let mut px_areas = changed.clone();
    px_areas.extend(closed_changed.iter().copied());
    px_areas.extend(deep_changed);
    let mut px_changed = Vec::new();
    let mut field_changed = Vec::new();
    for a in area::merge(px_areas) {
        let before = area::take(&live.px, w, a);
        for y in a[1]..a[3] {
            for x in a[0]..a[2] {
                let i = y * w + x;
                let least = live.cloud[i].min(live.land[i]);
                let filled = live.closed[i] > 0 && (least < 0.0 || live.deep[i] > HOLE * cell);
                live.px[i] = if filled { -(live.deep[i] - step).max(-least) } else { least };
            }
        }
        px_changed.extend(area::moved(&before, &live.px, w, a, area::bits));
        field_changed.extend(area::moved(&before, &live.px, w, a, |p, q| field_of(*p, u).to_bits() == field_of(*q, u).to_bits()));
    }
    let px_changed = area::merge(px_changed);
    let march = TOWARD * cell;
    let r_march = (march / step).ceil() as usize + 1;
    let r_lit = blur::reach(TOWARD_BLUR);
    let mut lit_changed = Vec::new();
    for a in area::merge(closed_changed.iter().map(|a| area::grow(*a, r_march + r_lit, w, h)).collect()) {
        let crop = area::grow(a, r_lit, w, h);
        let cw = crop[2] - crop[0];
        let fresh: Vec<f32> = (0..cw * (crop[3] - crop[1]))
            .into_par_iter()
            .map(|k| {
                let i = (crop[1] + k / cw) * w + crop[0] + k % cw;
                if live.closed[i] == 0 {
                    return 0.0;
                }
                let (x, y) = grid.center(i);
                let l = fixed.light(x, y);
                let mut t = 0.0;
                while t < march {
                    t += 2.0 * step;
                    let (sx, sy) = (x + l[0] * t, y + l[1] * t);
                    if sx < 0.0 || sy < 0.0 || sx >= width as f32 || sy >= height as f32 {
                        break;
                    }
                    let c = ((sy / step) as usize).min(h - 1) * w + ((sx / step) as usize).min(w - 1);
                    if live.closed[c] == 0 {
                        break;
                    }
                }
                t.min(march)
            })
            .collect();
        put(&mut scratch, w, crop, &fresh);
        let before = area::take(&live.lit, w, a);
        blur::fixed_into(&scratch, &mut live.lit, w, h, TOWARD_BLUR, a);
        lit_changed.extend(area::moved(&before, &live.lit, w, a, area::bits));
    }
    let mut look_areas = changed;
    look_areas.extend(px_changed.iter().map(|a| area::grow(*a, 1, w, h)));
    look_areas.extend(lit_changed);
    look_areas.extend(areas.iter().copied());
    let look_areas = area::merge(look_areas);
    let pile_buckets = Buckets::new(&live.clouds.iter().zip(&live.piles).map(|(p, on)| if *on { *p } else { [p[0], p[1], 0.0] }).collect::<Vec<[f32; 3]>>(), (1.0 + TUCK) * MOST * cell + grid.cell as f32, width, height);
    let mut look_changed = Vec::new();
    for a in look_areas {
        let before = area::take(live.look.as_chunks::<4>().0, w, a);
        let aw = a[2] - a[0];
        let view: &Live = live;
        let fresh: Vec<[u8; 4]> = (0..aw * (a[3] - a[1]))
            .into_par_iter()
            .map(|k| {
                let i = (a[1] + k / aw) * w + a[0] + k % aw;
                look_cell(&run, view, &pile_buckets, i)
            })
            .collect();
        let cells = live.look.as_chunks_mut::<4>().0;
        for y in a[1]..a[3] {
            cells[y * w + a[0]..y * w + a[2]].copy_from_slice(&fresh[(y - a[1]) * aw..(y - a[1] + 1) * aw]);
        }
        look_changed.extend(area::moved(&before, live.look.as_chunks::<4>().0, w, a, area::equal));
    }
    (area::merge(field_changed), area::merge(look_changed))
}

fn look_cell(run: &Run, live: &Live, piles: &Buckets, i: usize) -> [u8; 4] {
    let grid = run.grid;
    let cell = run.cell;
    let fixed = run.fixed;
    let (px, py) = grid.center(i);
    let kind = smoothstep(-KIND * cell, KIND * cell, live.land[i] - live.cloud[i]);
    if live.px[i] >= HOLD.1 * run.u {
        return [byte(smoothstep(0.3, 0.7, fixed.at(&fixed.sky, px, py))), 0, 0, 128];
    }
    let light = fixed.light(px, py);
    let (gx, gy) = gradient(&live.px, &grid, i);
    let g = length(gx, gy);
    let facing = if g > 1e-6 { (gx * light[0] + gy * light[1]) / g } else { 0.0 };
    let facing = facing.signum() * math::powf(facing.abs(), FACING);
    let blend = fixed.blend(px, py);
    let middle = 4.0 * blend * (1.0 - blend);
    let broad = 1.0 - smoothstep(BROAD.0 * cell, BROAD.1 * cell, live.lit[i]);
    let hold = label_hold(&live.icons, &live.labels, px, py, cell);
    let broad = (broad + (1.0 - broad) * MIDDLE_LIFT * middle) * (1.0 - hold);
    let (mut arc, mut shade) = (0.0f32, 0.0f32);
    if live.cloud[i] <= grid.cell as f32 {
        let mut front = f32::MIN;
        for k in piles.near(px, py) {
            let p = &live.clouds[k as usize];
            let (dx, dy) = (px - p[0], py - p[1]);
            let d = length(dx, dy);
            let prio = live.prio[k as usize];
            if d < p[2] && prio > front {
                front = prio;
                let toward = if d > 1e-3 { (dx * light[0] + dy * light[1]) / d } else { 0.0 };
                arc = smoothstep(ARC.0 * p[2], ARC.1 * p[2], d - p[2]) * math::powf(toward.max(0.0), ARC_POWER);
            }
        }
        for k in piles.near(px, py) {
            let p = &live.clouds[k as usize];
            let (dx, dy) = (px - p[0], py - p[1]);
            let d = length(dx, dy);
            let prio = live.prio[k as usize];
            if d >= p[2] && prio > front && d > 1e-3 {
                let away = -(dx * light[0] + dy * light[1]) / d;
                shade = shade.max(smoothstep(TUCK * p[2], 0.0, d - p[2]) * smoothstep(AWAY.0, AWAY.1, away));
            }
        }
    }
    let pile = (LIFT * arc * (1.0 - hold) - DIM * shade) * (1.0 - CALM * middle);
    let pile = (pile + DIM) / (LIFT + DIM);
    let foam = if kind < 1.0 {
        let along = 0.5 + 0.5 * shape::noise(px / (FOAM_ALONG.0 * cell), py / (FOAM_ALONG.0 * cell), FOAM_ALONG.3);
        let strength = 0.5 + 0.5 * shape::noise(px / (FOAM_STRENGTH.0 * cell), py / (FOAM_STRENGTH.0 * cell), FOAM_STRENGTH.3);
        smoothstep(FOAM_ALONG.1, FOAM_ALONG.2, along) * (FOAM_STRENGTH.1 + FOAM_STRENGTH.2 * strength) * run.guard(px, py)
    } else {
        0.0
    };
    let facing = if facing > 0.0 { facing * (1.0 - hold) } else { facing };
    [byte(kind), byte(kind * pile + (1.0 - kind) * foam), byte(kind * broad + (1.0 - kind) * hold), byte(0.5 + 0.5 * facing)]
}

pub fn relive(fixed: &Fixed, live: &mut Live, g: &Ground, moved: &[[f32; 4]]) -> Option<(Vec<Area>, Vec<Area>)> {
    let cell = CELL * fixed.u;
    let fresh_icons = icons(g, cell);
    if fresh_icons.len() != live.icons.len() {
        return None;
    }
    let shape = Shape::new(fixed, fresh_icons, cell);
    let old_icons = std::mem::take(&mut live.icons);
    let rects = |icons: &[Icon]| icons.iter().map(|icon| icon.r).collect::<Vec<[f32; 4]>>();
    let (old_rects, new_rects) = (rects(&old_icons), rects(&shape.icons));
    let (gone, came) = (only_in(&old_rects, &new_rects), only_in(&new_rects, &old_rects));
    let changed: Vec<&Icon> = old_icons.iter().filter(|icon| gone.contains(&bits(&icon.r))).chain(shape.icons.iter().filter(|icon| came.contains(&bits(&icon.r)))).collect();
    let mut boxes: Vec<[f32; 4]> = changed.iter().map(|icon| icon.reach).collect();
    for r in moved {
        let pad = 2.0 * cell;
        boxes.push([r[0] - pad, r[1] - pad, r[2] + pad, r[3] + pad]);
    }
    let tilt = TILT * cell;
    let mut raw_points: Vec<usize> = boxes.iter().flat_map(|b| live.lattice.within([b[0], b[1] - tilt, b[2], b[3] + tilt])).collect();
    raw_points.sort_unstable();
    raw_points.dedup();
    let raw = lattice_raw(&shape, &live.lattice, &raw_points, &live.raw);
    let share = 2.0 * MOST * cell + 2.0 * live.lattice.spacing;
    let mut finish_points: Vec<usize> = boxes.iter().flat_map(|b| live.lattice.within([b[0] - share, b[1] - tilt - share, b[2] + share, b[3] + tilt + share])).collect();
    finish_points.sort_unstable();
    finish_points.dedup();
    let clouds = finished(&live.lattice, &raw, &finish_points, &live.clouds, cell);
    let Cores { land: cores, air: puffs, bare } = shape.cores();
    let marks = |list: &[(Icon, f32)]| list.iter().map(|(icon, weight)| [icon.r[0], icon.r[1], icon.r[2], icon.r[3], *weight]).collect::<Vec<[f32; 5]>>();
    let (new_bare, old_bare) = (marks(&bare), marks(&Shape::new(fixed, old_icons.clone(), cell).cores().bare));
    let (bare_gone, bare_came) = (only_in(&old_bare, &new_bare), only_in(&new_bare, &old_bare));
    let pad = BARE.1 * cell + PUSH * cell;
    let bare_boxes: Vec<[f32; 4]> = old_bare.iter().filter(|m| bare_gone.contains(&bits(&m[..]))).chain(new_bare.iter().filter(|m| bare_came.contains(&bits(&m[..])))).map(|m| [m[0] - pad, m[1] - pad, m[2] + pad, m[3] + pad]).collect();
    let mut isle_points: Vec<usize> = boxes.iter().chain(&bare_boxes).flat_map(|b| live.lattice.within([b[0] - PUSH * cell, b[1] - PUSH * cell, b[2] + PUSH * cell, b[3] + PUSH * cell])).collect();
    isle_points.sort_unstable();
    isle_points.dedup();
    let isles_now = isles(&shape, &live.lattice, &isle_points, &live.isles, &bare);
    let grid = fixed.grid;
    let high = HOLD.1 * fixed.u;
    let cloud_grow = high + CLOUD_K * cell + grid.cell as f32;
    let land_grow = high * (SHALLOW.0 + SHALLOW.1) + COAST.0 * cell + LAND_K * cell + grid.cell as f32;
    let mut areas = Vec::new();
    let fall = LABEL_FALL * cell + grid.cell as f32;
    for icon in &changed {
        areas.push(cells_of(&grid, label(icon, fall)));
    }
    for (k, (a, b)) in live.clouds.iter().zip(&clouds).enumerate() {
        if a != b {
            if a[2] > 0.0 {
                areas.push(cells_of(&grid, bounds(a, cloud_grow)));
            }
            if b[2] > 0.0 {
                areas.push(cells_of(&grid, bounds(b, cloud_grow)));
            }
            if live.piles[k] {
                areas.push(cells_of(&grid, bounds(a, TUCK * MOST * cell + grid.cell as f32)));
                areas.push(cells_of(&grid, bounds(b, TUCK * MOST * cell + grid.cell as f32)));
            }
        }
    }
    for p in moved_puffs(&live.puffs, &puffs) {
        areas.push(cells_of(&grid, bounds(&p, cloud_grow)));
    }
    for p in moved_puffs(&live.cores, &cores) {
        areas.push(cells_of(&grid, bounds(&p, land_grow)));
    }
    for (a, b) in live.isles.iter().zip(&isles_now) {
        if a != b {
            if a[2] > 0.0 {
                areas.push(cells_of(&grid, bounds(a, land_grow)));
            }
            if b[2] > 0.0 {
                areas.push(cells_of(&grid, bounds(b, land_grow)));
            }
        }
    }
    live.raw = raw;
    live.clouds = clouds;
    live.isles = isles_now;
    live.cores = cores;
    live.puffs = puffs;
    if !changed.is_empty() {
        live.labels = labels_of(&shape.icons, &grid, cell);
    }
    live.icons = shape.icons;
    let areas: Vec<Area> = areas.into_iter().filter(|a| a[0] < a[2] && a[1] < a[3]).collect();
    if areas.is_empty() {
        return Some((Vec::new(), Vec::new()));
    }
    Some(rebuild(fixed, live, &areas))
}

fn bits(v: &[f32]) -> Key {
    std::array::from_fn(|k| v.get(k).map_or(0, |x| x.to_bits()))
}

struct Keys(Vec<Key>);

impl Keys {
    fn contains(&self, k: &Key) -> bool {
        self.0.binary_search(k).is_ok()
    }
}

fn only_in<const N: usize>(a: &[[f32; N]], b: &[[f32; N]]) -> Keys {
    let mut there: Vec<Key> = b.iter().map(|v| bits(v)).collect();
    there.sort_unstable();
    let mut out: Vec<Key> = a.iter().map(|v| bits(v)).filter(|k| there.binary_search(k).is_err()).collect();
    out.sort_unstable();
    Keys(out)
}

fn moved_puffs(old: &[[f32; 3]], new: &[[f32; 3]]) -> Vec<[f32; 3]> {
    let (gone, came) = (only_in(old, new), only_in(new, old));
    old.iter().filter(|p| gone.contains(&bits(&p[..]))).chain(new.iter().filter(|p| came.contains(&bits(&p[..])))).filter(|p| p[2] > 0.0).copied().collect()
}
