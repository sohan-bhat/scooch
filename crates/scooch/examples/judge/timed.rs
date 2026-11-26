pub const TRIES: usize = 6;

pub const STALL_MS: f64 = 100.0;

pub const MOTION: &str = "fewer than 3 distinct frames showed between the two pictures";

#[derive(Clone, Debug, PartialEq)]
pub enum Verdict {
    Good,
    Again,
    Bad(Vec<String>),
}

pub fn verdict(failed: &[String], stalled: bool) -> Verdict {
    if failed.is_empty() {
        Verdict::Good
    } else if stalled && failed.iter().all(|reason| reason == MOTION) {
        Verdict::Again
    } else {
        Verdict::Bad(failed.to_vec())
    }
}

pub fn moving_frames(log: &[String]) -> Vec<(f64, f32)> {
    log.iter()
        .filter_map(|line| line.split(" glide drew ").nth(1))
        .filter_map(|record| record.split_once(' ').map(|(_, frames)| frames))
        .flat_map(|frames| frames.split(", "))
        .filter_map(|frame| {
            let mut words = frame.split_whitespace();
            let at = words.next()?.parse::<f64>().ok()?;
            let step = words.next()?;
            let s = words.filter_map(|word| word.strip_prefix('G')).filter_map(|word| word.split('@').next()?.parse::<f32>().ok()).next_back()?;
            matches!(step, "move" | "glided").then_some((at, s))
        })
        .collect()
}

pub fn stalled(log: &[String]) -> Option<(usize, f64)> {
    let frames = moving_frames(log);
    let mut seen: Vec<i32> = frames.iter().map(|(_, s)| (s * 20.0).round() as i32).collect();
    seen.dedup();
    let gap = frames.windows(2).map(|pair| (pair[1].0 - pair[0].0) * 1000.0).fold(0.0, f64::max);
    (!frames.is_empty() && seen.len() < 3 && gap > STALL_MS).then_some((seen.len(), gap.round()))
}

pub fn left_over(windows: usize, primed_after_close: bool) -> bool {
    windows > usize::from(primed_after_close)
}

pub fn never_came(stalls: &[(usize, f64)]) -> String {
    let said: Vec<String> = stalls.iter().map(|(distinct, gap)| format!("{distinct} distinct with a {gap:.0} ms gap")).collect();
    format!("the footing for the timed glide never came, in each of the {} tries the helper presented fewer than 3 distinct moving frames with a gap over {STALL_MS} ms between them, {said:?}, so no try was judged", stalls.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(frames: &str) -> Vec<String> {
        vec![format!("1795638386.521 glide drew 3 layer drawables unknown latency 2 paced by the clock at 15.62 ms, 1795638383.621 first S@1.00 a0.0, {frames}")]
    }

    #[test]
    fn a_timed_glide_that_stalled_is_tried_again_and_any_other_fault_fails_at_once_by_name() {
        let stall = record("10.000 move S@1.00 G0.000@0.00, 10.001 move S@1.00 G0.000@0.01, 10.225 move S@1.00 G0.000@0.01, 10.410 glided G1.000@1.00");
        assert_eq!(stalled(&stall), Some((2, 224.0)));
        let warp = record("10.000 move S@1.00 G0.000@0.00, 10.001 move S@1.00 G0.000@0.01, 10.225 move S@1.00 G0.000@0.01, 10.410 move G0.505@1.00, 10.600 move G0.997@1.00, 10.775 glided G1.000@1.00");
        assert_eq!(stalled(&warp), None, "a glide that presented three distinct frames was called stalled");
        let steady = record("10.000 move G0.000@1.00, 10.017 move G0.300@1.00, 10.033 move G0.600@1.00, 10.050 glided G1.000@1.00");
        assert_eq!(stalled(&steady), None);
        assert_eq!(stalled(&[]), None);
        assert_eq!(verdict(&[], false), Verdict::Good);
        assert_eq!(verdict(&[MOTION.to_string()], true), Verdict::Again);
        assert_eq!(verdict(&[MOTION.to_string()], false), Verdict::Bad(vec![MOTION.to_string()]), "too few frames on screen while the helper presented enough passed as a stall");
        let wrong = vec![MOTION.to_string(), "the picture after the close differs from the new one".to_string()];
        assert_eq!(verdict(&wrong, true), Verdict::Bad(wrong.clone()), "a wrong picture in a stalled try was tried again");
        assert_eq!(verdict(&["icons were covered".to_string()], true), Verdict::Bad(vec!["icons were covered".to_string()]));
        assert_eq!(never_came(&[(2, 224.0), (1, 310.0)]), "the footing for the timed glide never came, in each of the 2 tries the helper presented fewer than 3 distinct moving frames with a gap over 100 ms between them, [\"2 distinct with a 224 ms gap\", \"1 distinct with a 310 ms gap\"], so no try was judged");
    }

    #[test]
    fn a_window_primed_by_a_wake_after_the_close_is_not_a_layer_left_over() {
        assert!(!left_over(0, false));
        assert!(left_over(1, false), "the glide's own window left after the close passed");
        assert!(!left_over(1, true), "the window a wake primed after the close was counted as the glide's");
        assert!(left_over(2, true), "a second window beside the primed one passed");
    }
}
