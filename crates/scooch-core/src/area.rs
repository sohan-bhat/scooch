pub use crate::blur::Area;

pub fn grow(a: Area, r: usize, w: usize, h: usize) -> Area {
    [a[0].saturating_sub(r), a[1].saturating_sub(r), (a[2] + r).min(w), (a[3] + r).min(h)]
}

pub fn merge(mut areas: Vec<Area>) -> Vec<Area> {
    areas.retain(|a| a[0] < a[2] && a[1] < a[3]);
    let mut i = 0;
    while i < areas.len() {
        let mut j = i + 1;
        let mut grew = false;
        while j < areas.len() {
            let (a, b) = (areas[i], areas[j]);
            if a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3] {
                areas[i] = [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])];
                areas.swap_remove(j);
                grew = true;
            } else {
                j += 1;
            }
        }
        if !grew {
            i += 1;
        }
    }
    areas
}

pub fn take<T: Copy>(values: &[T], w: usize, a: Area) -> Vec<T> {
    let mut out = Vec::with_capacity((a[2] - a[0]) * (a[3] - a[1]));
    for y in a[1]..a[3] {
        out.extend_from_slice(&values[y * w + a[0]..y * w + a[2]]);
    }
    out
}

pub fn moved<T: Copy>(before: &[T], values: &[T], w: usize, a: Area, same: impl Fn(&T, &T) -> bool) -> Option<Area> {
    let span = a[2] - a[0];
    let mut out: Option<Area> = None;
    for y in a[1]..a[3] {
        let (old, new) = (&before[(y - a[1]) * span..(y - a[1] + 1) * span], &values[y * w + a[0]..y * w + a[2]]);
        let first = old.iter().zip(new).position(|(p, q)| !same(p, q));
        if let Some(first) = first {
            let last = old.iter().zip(new).rposition(|(p, q)| !same(p, q)).unwrap_or(first);
            let (x0, x1) = (a[0] + first, a[0] + last + 1);
            out = Some(out.map_or([x0, y, x1, y + 1], |o| [o[0].min(x0), o[1], o[2].max(x1), y + 1]));
        }
    }
    out
}

pub fn mark<T: Copy>(before: &[T], values: &[T], w: usize, a: Area, same: impl Fn(&T, &T) -> bool, marks: &mut [bool]) -> Option<Area> {
    let span = a[2] - a[0];
    let mut out: Option<Area> = None;
    for y in a[1]..a[3] {
        for x in a[0]..a[2] {
            if !same(&before[(y - a[1]) * span + x - a[0]], &values[y * w + x]) {
                marks[y * w + x] = true;
                out = Some(out.map_or([x, y, x + 1, y + 1], |o| [o[0].min(x), o[1].min(y), o[2].max(x + 1), o[3].max(y + 1)]));
            }
        }
    }
    out
}

pub fn spread(marks: &[bool], w: usize, h: usize, r: usize, boxes: &[Area], out: &mut [bool]) {
    for b in boxes {
        let c = grow(*b, r, w, h);
        let (cw, ch) = (c[2] - c[0], c[3] - c[1]);
        if cw == 0 || ch == 0 {
            continue;
        }
        let wide = widen(&take(marks, w, c), cw, ch, r);
        for y in c[1]..c[3] {
            for (x, on) in (c[0]..c[2]).zip(&wide[(y - c[1]) * cw..(y - c[1] + 1) * cw]) {
                if *on {
                    out[y * w + x] = true;
                }
            }
        }
    }
}

pub fn bits(p: &f32, q: &f32) -> bool {
    p.to_bits() == q.to_bits()
}

pub fn equal<T: PartialEq>(p: &T, q: &T) -> bool {
    p == q
}

pub fn widen(cells: &[bool], w: usize, h: usize, r: usize) -> Vec<bool> {
    if r == 0 || w == 0 || h == 0 {
        return cells.to_vec();
    }
    let near = |line: &mut dyn Iterator<Item = bool>, out: &mut Vec<bool>| {
        let seen: Vec<bool> = line.collect();
        let n = seen.len();
        let mut last = usize::MAX;
        out.clear();
        out.resize(n, false);
        for i in 0..n {
            if seen[i] {
                last = i;
            }
            out[i] = last != usize::MAX && i - last <= r;
        }
        let mut next = usize::MAX;
        for i in (0..n).rev() {
            if seen[i] {
                next = i;
            }
            out[i] = out[i] || (next != usize::MAX && next - i <= r);
        }
    };
    let mut across = vec![false; w * h];
    let mut line = Vec::new();
    for y in 0..h {
        near(&mut cells[y * w..(y + 1) * w].iter().copied(), &mut line);
        across[y * w..(y + 1) * w].copy_from_slice(&line);
    }
    let mut out = vec![false; w * h];
    for x in 0..w {
        near(&mut (0..h).map(|y| across[y * w + x]), &mut line);
        for (y, v) in line.iter().enumerate() {
            out[y * w + x] = *v;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn widening_marks_the_box_around_each_cell() {
        let (w, h) = (9, 7);
        let mut cells = vec![false; w * h];
        cells[3 * w + 4] = true;
        cells[6 * w] = true;
        let out = widen(&cells, w, h, 2);
        for y in 0..h {
            for x in 0..w {
                let near = (x.abs_diff(4) <= 2 && y.abs_diff(3) <= 2) || (x <= 2 && y >= 4);
                assert_eq!(out[y * w + x], near, "{x} {y}");
            }
        }
    }

    #[test]
    fn merging_joins_touching_areas() {
        let out = merge(vec![[0, 0, 2, 2], [5, 5, 6, 6], [2, 1, 4, 3], [1, 1, 1, 3]]);
        assert_eq!(out.len(), 2);
        assert!(out.contains(&[0, 0, 4, 3]) && out.contains(&[5, 5, 6, 6]));
    }

    #[test]
    fn moved_finds_the_box_of_changes() {
        let w = 6;
        let before = vec![1.0f32; 24];
        let mut after = before.clone();
        after[w + 2] = 2.0;
        after[3 * w + 4] = -0.0;
        let a = [1, 0, 6, 4];
        assert_eq!(moved(&take(&before, w, a), &after, w, a, bits), Some([2, 1, 5, 4]));
        assert_eq!(moved(&take(&before, w, a), &before, w, a, bits), None);
    }
}
