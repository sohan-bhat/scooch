pub const UNEXPLAINED_FIT: f32 = 30.0;

pub const BLACK_SHARE: f32 = 0.9;

pub fn slide_faults(captures: &[(f64, Option<f32>, f32)]) -> Vec<String> {
    captures
        .iter()
        .filter_map(|(at, gap, dark)| {
            if *dark >= BLACK_SHARE {
                Some(format!("a capture during the slide showed a black desktop at {at:.0} ms after the drop"))
            } else {
                gap.filter(|gap| *gap > UNEXPLAINED_FIT).map(|gap| format!("a capture during the slide showed neither picture nor a frame between them at {at:.0} ms after the drop, {gap:.1} from the nearest"))
            }
        })
        .collect()
}
