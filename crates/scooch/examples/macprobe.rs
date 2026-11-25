#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macprobe runs on macOS only, so start it on a Mac runner.");
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    probe::main()
}

#[cfg(any(target_os = "macos", test))]
const SPARE_WAKEUPS: u64 = 6;

#[cfg(target_os = "macos")]
const BARE_PRIMES: usize = 2;

#[cfg(any(target_os = "macos", test))]
fn woke_more(woke: (u64, u64), baseline: Option<(u64, u64)>, events: usize) -> bool {
    events > 0 || baseline.is_some_and(|(idle, interrupt)| woke.0 > idle + SPARE_WAKEUPS || woke.1 > interrupt + SPARE_WAKEUPS)
}

#[cfg(any(target_os = "macos", test))]
const NORMAL_SKIPS: [&str; 7] = ["hidden", "no room", "too large", "helper cold", "nothing new", "helper budget", "system picture"];

#[cfg(any(target_os = "macos", test))]
const PARKED_FAULTS: [&str; 2] = ["helper cold", "helper budget"];

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
enum Try {
    Glided,
    Skipped(String),
    Fault(String),
}

#[cfg(any(target_os = "macos", test))]
fn glide_try(log: &[String], parked: bool) -> Try {
    if log.iter().any(|line| line.contains(" glide slid") || line.contains(" shown and gliding") || line.contains(" glide glided ")) {
        return Try::Glided;
    }
    let Some(rest) = log.iter().find_map(|line| line.split_once(" glide skipped, ").map(|(_, rest)| rest)) else { return Try::Glided };
    let reason = rest.split(", ").next().unwrap_or(rest).trim().to_string();
    if NORMAL_SKIPS.contains(&reason.as_str()) && !(parked && PARKED_FAULTS.contains(&reason.as_str())) {
        Try::Skipped(reason)
    } else {
        Try::Fault(reason)
    }
}

#[cfg(any(target_os = "macos", test))]
fn parked_in(log: &[String]) -> Option<f64> {
    let launched = log.iter().rposition(|line| line.contains(" glide helper launched to park"));
    let (at, line) = log.iter().enumerate().rev().find(|(_, line)| line.contains(" glide helper parked on "))?;
    if launched.is_some_and(|launched| launched > at) {
        return None;
    }
    line.split(" glide helper parked on ").nth(1).and_then(|rest| rest.split(" ms after its launch").next()).and_then(|head| head.rsplit(' ').next()).and_then(|ms| ms.parse().ok())
}

#[cfg(any(target_os = "macos", test))]
fn none_glided(tries: &[Try]) -> Option<String> {
    let reasons: Vec<&str> = tries.iter().filter_map(|tried| if let Try::Skipped(reason) = tried { Some(reason.as_str()) } else { None }).collect();
    (!tries.is_empty() && reasons.len() == tries.len()).then(|| format!("no try glided, the daemon skipped all {} as {}", tries.len(), reasons.join(", ")))
}

#[cfg(any(target_os = "macos", test))]
const GATHER: [&str; 3] = ["wake", "prime", "look"];

#[cfg(any(target_os = "macos", test))]
const WOKE_SLACK: f64 = 500.0;

#[cfg(any(target_os = "macos", test))]
const GLIDE_PIECES: [&str; 9] = ["prime", "early", "cold", "map", "read", "ask", "bundle", "open", "clear"];

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, Default, PartialEq)]
struct Drag {
    began: Option<f64>,
    laps: Vec<(String, f64)>,
    thread: Option<f64>,
    daemon: Option<f64>,
    writers: Option<(u32, u32)>,
    skip: Option<String>,
    glided: bool,
    parked: bool,
}

#[cfg(any(target_os = "macos", test))]
fn after_key(line: &str, key: &str) -> Option<f64> {
    line.split(key).nth(1).and_then(|rest| rest.split([',', ' ', ')', '"']).next()).and_then(|value| value.parse::<f64>().ok())
}

#[cfg(any(target_os = "macos", test))]
fn drag_of(line: &str) -> Drag {
    let ticked = line.split("tick laps Some(\"").nth(1).and_then(|rest| rest.split("\")").next()).unwrap_or_default();
    let began = ticked.split("began Some(").nth(1).and_then(|rest| rest.split(')').next()).and_then(|value| value.parse::<f64>().ok());
    let listed = ticked.split(" ms after the drop, took ").nth(1).and_then(|rest| rest.split(", sessions hold").next()).unwrap_or_default();
    let laps = listed.split(", ").skip(1).filter_map(|lap| lap.rsplit_once(' ').and_then(|(name, value)| Some((name.to_string(), value.parse::<f64>().ok()?)))).collect();
    let writers = after_key(ticked, ", writers ").zip(after_key(ticked, " at the wake and ")).map(|(wake, set)| (wake as u32, set as u32));
    let skip = line.split("skip Some(\"").nth(1).and_then(|rest| rest.split('"').next()).map(str::to_string);
    Drag { began, laps, thread: after_key(ticked, "cpu to the set "), daemon: after_key(ticked, " ms on the tick thread and "), writers, skip, glided: line.contains(", glided true,"), parked: line.contains(", helper parked,") }
}

#[cfg(any(target_os = "macos", test))]
impl Drag {
    fn lap(&self, name: &str) -> f64 {
        self.laps.iter().filter(|(lap, _)| lap == name).map(|(_, value)| value).sum()
    }

    fn to_set(&self) -> Option<f64> {
        let set = self.laps.iter().position(|(name, _)| name == "set")?;
        Some(self.began? + self.laps[..=set].iter().filter(|(name, _)| !GATHER.contains(&name.as_str())).map(|(_, value)| value).sum::<f64>())
    }

    fn wake_to_set(&self) -> Option<f64> {
        let set = self.laps.iter().position(|(name, _)| name == "set")?;
        Some(self.laps[..=set].iter().map(|(_, value)| value).sum())
    }

    fn glide(&self) -> Option<f64> {
        let set = self.laps.iter().position(|(name, _)| name == "set")?;
        Some(self.laps[..=set].iter().filter(|(name, _)| GLIDE_PIECES.contains(&name.as_str())).map(|(_, value)| value).sum())
    }

    fn woke(&self) -> Option<f64> {
        Some(self.began? - self.laps.iter().filter(|(name, _)| GATHER.contains(&name.as_str())).map(|(_, value)| value).sum::<f64>())
    }

    fn quiet(&self) -> bool {
        self.woke().is_some_and(|woke| woke >= -WOKE_SLACK) && self.writers == Some((0, 0)) && self.daemon.is_some() && self.to_set().is_some()
    }
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
struct Paired {
    footing: Option<String>,
    wall: Option<f64>,
    cpu: Option<f64>,
    daemon: Option<f64>,
    glide: Option<f64>,
}

#[cfg(any(target_os = "macos", test))]
fn pair_of(slow: &Drag, off: &Drag) -> Paired {
    let footing = if !slow.quiet() || !off.quiet() {
        Some(format!("the daemon woke more than {WOKE_SLACK} ms before its drop or had a writer beside it, slow woke {:?} writers {:?}, off woke {:?} writers {:?}", slow.woke(), slow.writers, off.woke(), off.writers))
    } else if slow.skip.as_deref() != Some("helper cold") || slow.parked || slow.glided {
        Some(format!("the slow drag did not meet a cold helper, skip {:?} parked {}", slow.skip, slow.parked))
    } else if off.skip.as_deref() != Some("setting off") {
        Some(format!("the off drag was not judged with the glide off, skip {:?}", off.skip))
    } else {
        None
    };
    let wall = slow.to_set().zip(off.to_set()).map(|(slow, off)| slow - off);
    let cpu = slow.thread.zip(off.thread).map(|(slow, off)| slow - off);
    let daemon = slow.daemon.zip(off.daemon).map(|(slow, off)| slow - off);
    let glide = slow.glide().zip(off.glide()).map(|(slow, off)| slow - off);
    Paired { footing, wall, cpu, daemon, glide }
}

#[cfg(any(target_os = "macos", test))]
fn median(values: &[f64]) -> Option<f64> {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    match sorted.len() {
        0 => None,
        count if count % 2 == 1 => Some(sorted[count / 2]),
        count => Some((sorted[count / 2 - 1] + sorted[count / 2]) / 2.0),
    }
}

#[cfg(any(target_os = "macos", test))]
const UNEXPLAINED_FIT: f32 = 30.0;

#[cfg(any(target_os = "macos", test))]
const BLACK_SHARE: f32 = 0.9;

#[cfg(any(target_os = "macos", test))]
fn glide_progress(window: &[(f32, f32)]) -> bool {
    window.windows(2).all(|pair| pair[1].0 >= pair[0].0 - 0.1)
}

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
struct Drawn {
    at: f64,
    step: String,
    s: Option<f32>,
}

#[cfg(any(target_os = "macos", test))]
fn drew_frames(log: &[String]) -> Vec<Drawn> {
    log.iter()
        .filter_map(|line| line.split(" glide drew ").nth(1))
        .filter_map(|record| record.split_once(' ').map(|(_, frames)| frames))
        .flat_map(|frames| frames.split(", "))
        .filter_map(|frame| {
            let mut words = frame.split_whitespace();
            let at = words.next()?.parse::<f64>().ok()?;
            let step = words.next()?.to_string();
            let s = words.filter_map(|layer| layer.strip_prefix('G')).filter_map(|layer| layer.split('@').next()?.parse::<f32>().ok()).next_back();
            Some(Drawn { at, step, s })
        })
        .collect()
}

#[cfg(any(target_os = "macos", test))]
const RECORD_REACH: (f64, f64) = (0.1, 0.02);

#[cfg(any(target_os = "macos", test))]
const RECORD_SLACK: f32 = 0.15;

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
struct Recorded {
    steady: bool,
    moving: usize,
    agreed: usize,
    disagreed: Vec<String>,
}

#[cfg(any(target_os = "macos", test))]
fn judge_record(frames: &[Drawn], captures: &[(f64, f64, f32)], dropped: f64) -> Recorded {
    let moves: Vec<&Drawn> = frames.iter().filter(|frame| matches!(frame.step.as_str(), "move" | "glided") && frame.s.is_some()).collect();
    let values: Vec<f32> = moves.iter().filter_map(|frame| frame.s).collect();
    let moving = values.iter().filter(|s| **s > 0.04 && **s < 0.96).count();
    let steady = !values.is_empty() && values.windows(2).all(|pair| pair[1] >= pair[0] - 0.001) && values.last().is_some_and(|s| *s >= 0.999) && moving >= 3;
    let (mut agreed, mut disagreed) = (0, Vec::new());
    for (start, end, fitted) in captures.iter().copied().filter(|(_, _, fitted)| *fitted > 0.04 && *fitted < 0.96) {
        let near: Vec<f32> = moves.iter().filter(|frame| frame.at >= start - RECORD_REACH.0 && frame.at <= end + RECORD_REACH.1).filter_map(|frame| frame.s).collect();
        let near = if near.is_empty() { moves.iter().rev().find(|frame| frame.at < start).and_then(|frame| frame.s).into_iter().collect() } else { near };
        let (low, high) = near.iter().fold((f32::MAX, f32::MIN), |(low, high), s| (low.min(*s), high.max(*s)));
        if !near.is_empty() && fitted >= low - RECORD_SLACK && fitted <= high + RECORD_SLACK {
            agreed += 1;
        } else {
            disagreed.push(format!("a capture {:.0} ms after the drop fit progress {fitted:.2} where the helper presented {:?}", (start - dropped) * 1000.0, near));
        }
    }
    Recorded { steady, moving, agreed, disagreed }
}

#[cfg(any(target_os = "macos", test))]
fn slide_faults(captures: &[(f64, Option<f32>, f32)]) -> Vec<String> {
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

#[cfg(any(target_os = "macos", test))]
const SLOW_LIMIT: f64 = 60.0;

#[cfg(any(target_os = "macos", test))]
const FOOTED_PAIRS: usize = 5;

#[cfg(any(target_os = "macos", test))]
#[derive(Clone, Debug, PartialEq)]
enum Slow {
    Judged { cpu: f64, glide: f64, daemon: Option<f64>, wall: Option<f64>, footed: usize },
    Unfooted(String),
}

#[cfg(any(target_os = "macos", test))]
fn judge_pairs(slow: &[String], off: &[String]) -> (Vec<Paired>, Slow) {
    let (slow, off): (Vec<Drag>, Vec<Drag>) = (slow.iter().map(|line| drag_of(line)).collect(), off.iter().map(|line| drag_of(line)).collect());
    let paired: Vec<Paired> = slow.iter().zip(&off).map(|(slow, off)| pair_of(slow, off)).collect();
    let footed: Vec<&Paired> = paired.iter().filter(|pair| pair.footing.is_none() && pair.cpu.is_some() && pair.glide.is_some()).collect();
    let cpus: Vec<f64> = footed.iter().filter_map(|pair| pair.cpu).collect();
    let glides: Vec<f64> = footed.iter().filter_map(|pair| pair.glide).collect();
    let walls: Vec<f64> = footed.iter().filter_map(|pair| pair.wall).collect();
    let daemons: Vec<f64> = footed.iter().filter_map(|pair| pair.daemon).collect();
    let judged = match (median(&cpus), median(&glides)) {
        (Some(cpu), Some(glide)) if footed.len() >= FOOTED_PAIRS => Slow::Judged { cpu, glide, daemon: median(&daemons), wall: median(&walls), footed: footed.len() },
        _ => Slow::Unfooted(format!("{} of {} pairs had footing where {FOOTED_PAIRS} are needed, the first miss {}", footed.len(), paired.len(), paired.iter().find_map(|pair| pair.footing.clone()).unwrap_or_else(|| "had no cpu or pieces to the set".to_string()))),
    };
    (paired, judged)
}

#[cfg(any(target_os = "macos", test))]
fn chosen_pairs(text: &str, tries: usize) -> (Vec<String>, Vec<String>) {
    let mut found: Vec<(usize, usize, String, String)> = Vec::new();
    for line in text.lines() {
        let Some(rest) = line.split("- try ").nth(1) else { continue };
        let Some((attempt, rest)) = rest.split_once(' ') else { continue };
        let Some((arm, rest)) = rest.split_once(' ') else { continue };
        let (Ok(attempt), Some(Ok(index))) = (attempt.parse::<usize>(), rest.split(':').next().map(str::parse::<usize>)) else { continue };
        if arm == "slow" {
            found.push((index, attempt, line.to_string(), String::new()));
        } else if let Some(pair) = found.iter_mut().rev().find(|(at, tried, _, off)| *at == index && *tried == attempt && off.is_empty() && arm == "off") {
            pair.3 = line.to_string();
        }
    }
    let mut indexes: Vec<usize> = found.iter().map(|(index, _, _, _)| *index).collect();
    indexes.dedup();
    let mut chosen = (Vec::new(), Vec::new());
    for index in indexes {
        let tried: Vec<&(usize, usize, String, String)> = found.iter().filter(|(at, attempt, _, off)| *at == index && *attempt <= tries && !off.is_empty()).collect();
        let pick = tried.iter().find(|(_, _, slow, off)| pair_of(&drag_of(slow), &drag_of(off)).footing.is_none()).or(tried.last());
        if let Some((_, _, slow, off)) = pick {
            chosen.0.push(slow.clone());
            chosen.1.push(off.clone());
        }
    }
    chosen
}

#[cfg(any(target_os = "macos", test))]
impl Slow {
    fn held_back(&self) -> Option<bool> {
        match self {
            Slow::Judged { cpu, glide, .. } => Some(*cpu > SLOW_LIMIT || *glide > SLOW_LIMIT),
            Slow::Unfooted(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{glide_try, none_glided, woke_more, Try};

    fn log(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|line| line.to_string()).collect()
    }

    #[test]
    fn a_skip_the_daemon_counts_as_normal_is_tried_again_and_any_other_fails() {
        let hidden = log(&["1795266235.131 glide open after snapshot 2 on /out/a.jpg", "1795266237.229 glide skipped, hidden, macOS never reported the live layer as visible, so Scooch keeps the plain swap.", "1795266237.777 tick 2663 ms"]);
        assert_eq!(glide_try(&hidden, false), Try::Skipped("hidden".into()));
        assert_eq!(glide_try(&log(&["1 glide skipped, helper cold"]), false), Try::Skipped("helper cold".into()));
        assert_eq!(glide_try(&log(&["1 glide skipped, helper cold"]), true), Try::Fault("helper cold".into()));
        assert_eq!(glide_try(&log(&["1 glide skipped, helper budget, the layer was not shown within 150 ms"]), true), Try::Fault("helper budget".into()));
        assert_eq!(glide_try(&log(&["1 glide skipped, hidden, macOS never reported the live layer as visible"]), true), Try::Skipped("hidden".into()));
        assert_eq!(glide_try(&log(&["1 glide slid, on main, times from the drop, helper parked"]), true), Try::Glided);
        assert_eq!(glide_try(&log(&["1 glide skipped, system picture, the system's own picture is on screen"]), true), Try::Skipped("system picture".into()));
        assert_eq!(super::parked_in(&log(&["1795 glide helper parked on Apple M1 of kind integrated 412 ms after its launch"])), Some(412.0));
        assert_eq!(super::parked_in(&log(&["1795 glide helper launched to park in 3 ms"])), None);
        assert_eq!(super::parked_in(&log(&["1 glide helper launched to park in 3 ms", "2 glide helper parked on Apple M1 of kind integrated 412 ms after its launch", "3 glide helper launched to park in 2 ms"])), None);
        assert_eq!(super::parked_in(&log(&["1 glide helper parked on GPU of kind integrated 90 ms after its launch", "2 glide helper launched to park in 2 ms", "3 glide helper parked on GPU of kind integrated 80 ms after its launch"])), Some(80.0));
        assert_eq!(glide_try(&log(&["1 glide skipped, old tables, the tables on screen come from another version"]), false), Try::Fault("old tables".into()));
        assert_eq!(glide_try(&log(&["1 glide skipped, setting off"]), false), Try::Fault("setting off".into()));
        assert_eq!(glide_try(&log(&["1 glide open 2", "2 glide glided 2 with 17 frames", "3 glide settled after 40 ms"]), false), Try::Glided);
        let settle_budget = log(&[
            "1795266265.157 glide open after snapshot 5 on /out/a-50.jpg",
            "1795266265.270 glide open 5 shown and gliding after 29 ms, opened after the snapshot",
            "1795266265.458 glide settle 5 on /out/a-51.jpg",
            "1795266266.174 glide glided 5 with 15 frames, 3 slow, at scale 0.5, first frame 33 ms after the glide was sent",
            "1795266267.553 glide skipped, settle budget, the helper did not settle within 2000 ms",
            "1795266267.553 glide gave up the settle after 2019 ms",
            "1795266267.553 glide closed 5 300 ms after its wallpaper was set",
        ]);
        assert_eq!(glide_try(&settle_budget, true), Try::Glided);
        assert_eq!(glide_try(&settle_budget[1..2], true), Try::Glided);
        assert_eq!(glide_try(&settle_budget[3..5], true), Try::Glided);
    }

    #[test]
    fn the_check_fails_only_when_no_try_glided_and_names_every_reason() {
        let three = [Try::Skipped("hidden".into()), Try::Skipped("no room".into()), Try::Skipped("hidden".into())];
        assert_eq!(none_glided(&three).as_deref(), Some("no try glided, the daemon skipped all 3 as hidden, no room, hidden"));
        assert_eq!(none_glided(&[Try::Skipped("hidden".into()), Try::Glided]), None);
        assert_eq!(none_glided(&[]), None);
    }

    fn drag_line(label: &str, skip: &str, began: f64, laps: &str, cpu: (u32, u32), writers: (u32, u32), parked: bool) -> String {
        format!(
            "  - {label} 1: 3 s after the move before it, the daemon quiet after 1200 ms true, glided false, skip Some(\"{skip}\"), waited none, set Some(186) ms by the probe, launch took 0 ms, helper 812 age 2400 ms at the drop, helper cpu 0 ms from the drop to the set, 1 helpers at the drop, writer threads at the drop [], tick laps Some(\"began Some({began}) ms after the drop, took 72 ms, {laps}, sessions hold 11 MB, cpu to the set {} ms on the tick thread and {} ms in the daemon, writers {} at the wake and {} at the set\"), the daemon's line Some(\"skipped, {skip}, on main, times from the drop, {}, hello none\")",
            cpu.0,
            cpu.1,
            writers.0,
            writers.1,
            if parked { "helper parked" } else { "helper not parked" }
        )
    }

    #[test]
    fn a_drag_line_gives_its_laps_cpu_writers_and_the_time_from_the_drop_to_the_set() {
        let slow = super::drag_of(&drag_line("quiet slow", "helper cold", 30.0, "wake 0, prime 2, look 27, state 1, block 0, snapshot 0, screens 0, wallpaper 1, early 0, analysis 2, solve 9, flip 0, decode 1, read 0, ask 0, clear 0, render 22, encode 6, same 1, write 2, check 3, set 7, later 8", (40, 61), (0, 0), false));
        assert_eq!(slow.began, Some(30.0));
        assert_eq!(slow.lap("render"), 22.0);
        assert_eq!(slow.to_set(), Some(30.0 + 1.0 + 1.0 + 2.0 + 9.0 + 1.0 + 22.0 + 6.0 + 1.0 + 2.0 + 3.0 + 7.0));
        assert_eq!(slow.wake_to_set(), Some(29.0 + 1.0 + 1.0 + 2.0 + 9.0 + 1.0 + 22.0 + 6.0 + 1.0 + 2.0 + 3.0 + 7.0));
        assert_eq!((slow.thread, slow.daemon, slow.writers), (Some(40.0), Some(61.0), Some((0, 0))));
        assert_eq!((slow.skip.as_deref(), slow.parked, slow.glided), (Some("helper cold"), false, false));
        let off = super::drag_of(&drag_line("quiet off", "setting off", 28.0, "wake 0, prime 0, look 28, state 1, block 0, snapshot 0, screens 0, wallpaper 1, analysis 1, solve 6, flip 0, decode 1, render 19, encode 6, same 1, write 2, check 3, set 8", (35, 50), (0, 0), false));
        let pair = super::pair_of(&slow, &off);
        assert_eq!(pair.footing, None);
        assert_eq!((pair.cpu, pair.daemon), (Some(5.0), Some(11.0)));
        assert_eq!(pair.wall, Some(slow.to_set().unwrap() - off.to_set().unwrap()));
        let writing = super::drag_of(&drag_line("quiet off", "setting off", 28.0, "look 28, set 8", (35, 50), (1, 0), false));
        assert!(super::pair_of(&slow, &writing).footing.is_some());
        let early = super::drag_of(&drag_line("quiet off", "setting off", -700.0, "snapshot 730, set 8", (35, 50), (0, 0), false));
        assert!(super::pair_of(&slow, &early).footing.is_some());
        let settled = super::drag_of(&drag_line("quiet off", "setting off", -61.0, "wake 0, prime 0, look 293, snapshot 79, set 8", (35, 50), (0, 0), false));
        assert_eq!(settled.woke(), Some(-354.0));
        assert!(super::pair_of(&slow, &settled).footing.is_none(), "a Desktop wake inside the drag leaves the cpu from the wake to the set fair");
        let woken = super::drag_of(&drag_line("quiet off", "setting off", 100.0, "wake 0, prime 0, look 650, set 8", (35, 50), (0, 0), false));
        assert_eq!(woken.woke(), Some(-550.0));
        assert!(super::pair_of(&slow, &woken).footing.is_some());
        let parked = super::drag_of(&drag_line("quiet slow", "helper cold", 30.0, "look 28, set 8", (35, 50), (0, 0), true));
        assert!(super::pair_of(&parked, &off).footing.is_some());
        assert_eq!(super::median(&[5.0, 1.0, 3.0]), Some(3.0));
        assert_eq!(super::median(&[4.0, 1.0, 3.0, 2.0]), Some(2.5));
        assert_eq!(super::median(&[]), None);
    }

    fn pair_lines(pairs: &[(u32, u32, f64)], footing: &[bool]) -> (Vec<String>, Vec<String>) {
        let slow = pairs.iter().zip(footing).map(|((cpu, _, glide), footed)| drag_line("try 1 slow", "helper cold", 900.0, &format!("wake 0, prime {glide}, look 30, state 1, render 400, set 2"), (*cpu, 700), (if *footed { 0 } else { 1 }, 0), false)).collect();
        let off = pairs.iter().map(|(_, cpu, _)| drag_line("try 1 off", "setting off", 30.0, "wake 0, prime 0, look 30, state 1, render 200, set 2", (*cpu, 500), (0, 0), false)).collect();
        (slow, off)
    }

    #[test]
    fn the_slow_pairs_are_judged_by_the_median_tick_cpu_and_glide_pieces_of_the_pairs_with_footing_and_fail_only_past_the_limit() {
        let (slow, off) = pair_lines(&[(600, 560, 4.0), (900, 560, 2.0), (570, 560, 8.0), (580, 560, 1.0), (560, 590, 300.0), (500, 560, 3.0), (565, 560, 2.0)], &[true; 7]);
        let (paired, judged) = super::judge_pairs(&slow, &off);
        assert_eq!(paired.len(), 7);
        assert_eq!(judged, super::Slow::Judged { cpu: 10.0, glide: 3.0, daemon: Some(200.0), wall: Some(1070.0), footed: 7 });
        assert_eq!(judged.held_back(), Some(false), "the whole daemon's cpu and the wall clock are reported and never judged");
        let (slow, off) = pair_lines(&[(700, 560, 4.0), (650, 560, 2.0), (690, 560, 8.0), (580, 560, 1.0), (660, 590, 3.0), (500, 560, 3.0), (565, 560, 2.0)], &[true; 7]);
        assert_eq!(super::judge_pairs(&slow, &off).1.held_back(), Some(true));
        let (slow, off) = pair_lines(&[(560, 560, 70.0), (560, 560, 90.0), (560, 560, 61.0), (560, 560, 1.0), (560, 560, 80.0), (560, 560, 3.0), (560, 560, 2.0)], &[true; 7]);
        assert_eq!(super::judge_pairs(&slow, &off).1.held_back(), Some(true));
        let (slow, off) = pair_lines(&[(621, 560, 61.0), (621, 560, 61.0), (621, 560, 61.0), (620, 560, 60.0), (620, 560, 60.0), (620, 560, 60.0), (620, 560, 60.0)], &[true; 7]);
        assert_eq!(super::judge_pairs(&slow, &off).1.held_back(), Some(false));
        let (slow, off) = pair_lines(&[(700, 560, 4.0), (650, 560, 2.0), (690, 560, 8.0), (580, 560, 1.0), (660, 590, 3.0), (500, 560, 3.0), (565, 560, 2.0)], &[true, false, true, false, false, true, true]);
        let (_, judged) = super::judge_pairs(&slow, &off);
        assert!(matches!(&judged, super::Slow::Unfooted(why) if why.starts_with("4 of 7 pairs had footing where 5 are needed")), "{judged:?}");
        assert_eq!(judged.held_back(), None);
    }

    #[test]
    fn each_pair_counts_its_first_try_with_footing_or_else_its_last() {
        let line = |label: &str, index: usize, skip: &str, began: f64, writers: u32| drag_line(label, skip, began, &format!("look {began}, state 1, set 0"), (40, 60), (writers, 0), false).replacen(" 1: ", &format!(" {index}: "), 1);
        let text = [
            line("try 1 slow", 1, "helper cold", 50.0, 1),
            line("try 1 off", 1, "setting off", 20.0, 0),
            "  - pair 1 try 1 had no footing, so it is dragged again".to_string(),
            line("try 2 slow", 1, "helper cold", 45.0, 0),
            line("try 2 off", 1, "setting off", 25.0, 0),
            line("try 1 slow", 2, "helper cold", 30.0, 0),
            line("try 1 off", 2, "setting off", 31.0, 1),
            line("try 2 slow", 2, "helper cold", 32.0, 0),
            line("try 2 off", 2, "setting off", 33.0, 1),
            line("try 1 slow", 3, "helper cold", 30.0, 0),
            line("try 1 off", 3, "setting off", 28.0, 0),
        ]
        .join("\n");
        let (slow, off) = super::chosen_pairs(&text, 3);
        assert_eq!((slow.len(), off.len()), (3, 3));
        let walls: Vec<Option<f64>> = slow.iter().zip(&off).map(|(slow, off)| super::pair_of(&super::drag_of(slow), &super::drag_of(off)).wall).collect();
        assert_eq!(walls, vec![Some(20.0), Some(-1.0), Some(2.0)]);
        assert!(super::pair_of(&super::drag_of(&slow[1]), &super::drag_of(&off[1])).footing.is_some());
        assert_eq!(super::chosen_pairs(&text, 1).0.len(), 3);
        assert_eq!(super::drag_of(&super::chosen_pairs(&text, 1).0[0]).began, Some(50.0));
    }

    #[test]
    fn a_black_capture_or_one_no_frame_explains_fails_the_slide_in_its_own_words() {
        let black = [(1080.0, Some(9.75), 0.0), (1140.0, Some(9.75), 0.0), (1161.0, Some(98.71), 0.97), (1223.0, Some(10.02), 0.0)];
        assert_eq!(super::slide_faults(&black), vec!["a capture during the slide showed a black desktop at 1161 ms after the drop".to_string()]);
        let unread = [(951.0, None, 0.95)];
        assert_eq!(super::slide_faults(&unread), vec!["a capture during the slide showed a black desktop at 951 ms after the drop".to_string()]);
        let odd = [(400.0, Some(41.0), 0.1)];
        assert_eq!(super::slide_faults(&odd), vec!["a capture during the slide showed neither picture nor a frame between them at 400 ms after the drop, 41.0 from the nearest".to_string()]);
        let clean = [(100.0, Some(7.36), 0.0), (650.0, Some(10.55), 0.0), (1300.0, Some(7.39), 0.02)];
        assert!(super::slide_faults(&clean).is_empty());
    }

    #[test]
    fn the_helper_record_judges_a_glide_that_captures_cannot_follow() {
        let log = vec![
            "1795551563.014 glide drew 3 1795551562.300 first S@1.00, 1795551562.320 move S@1.00 G0.000@0.25, 1795551562.400 move G0.030@1.00, 1795551562.500 move G0.300@1.00, 1795551562.600 move G0.700@1.00, 1795551562.700 move G0.900@1.00, 1795551562.760 glided G1.000@1.00, 1795551562.900 fade G1.000@1.00 S@0.40, 1795551563.000 settled S@1.00, 1795551563.010 flush S@1.00".to_string(),
            "1795551563.020 glide settled after 700 ms".to_string(),
        ];
        let frames = super::drew_frames(&log);
        assert_eq!(frames.len(), 10);
        assert_eq!(frames[3], super::Drawn { at: 1795551562.5, step: "move".into(), s: Some(0.3) });
        assert_eq!(frames[7].s, Some(1.0));
        let dropped = 1795551562.0;
        let agree = super::judge_record(&frames, &[(1795551562.45, 1795551562.55, 0.28), (1795551562.62, 1795551562.71, 0.80)], dropped);
        assert_eq!(agree, super::Recorded { steady: true, moving: 3, agreed: 2, disagreed: Vec::new() });
        let off = super::judge_record(&frames, &[(1795551562.45, 1795551562.55, 0.75)], dropped);
        assert_eq!((off.agreed, off.disagreed.len()), (0, 1));
        let back = super::drew_frames(&["1 glide drew 3 10.0 move G0.200@1.00, 10.1 move G0.500@1.00, 10.2 move G0.400@1.00, 10.3 move G0.600@1.00, 10.4 glided G1.000@1.00".to_string()]);
        assert!(!super::judge_record(&back, &[], 9.0).steady);
        let short = super::drew_frames(&["1 glide drew 3 10.0 move G0.500@1.00, 10.4 glided G1.000@1.00".to_string()]);
        assert!(!super::judge_record(&short, &[], 9.0).steady);
    }

    #[test]
    fn the_glide_progress_counts_every_capture() {
        let fell = [(0.25, 7.8), (0.85, 7.61), (0.30, 7.7), (1.0, 7.39)];
        assert!(!super::glide_progress(&fell));
        let steady = [(0.25, 7.8), (0.85, 7.61), (0.80, 7.7), (1.0, 7.39)];
        assert!(super::glide_progress(&steady));
        assert!(!super::glide_progress(&[(0.4, 98.71), (0.1, 10.0)]));
    }

    #[test]
    fn one_wakeup_against_a_baseline_of_none_is_noise() {
        assert!(!woke_more((1, 1), Some((0, 14)), 0));
    }

    #[test]
    fn a_wakeup_every_second_is_more_than_the_baseline() {
        assert!(woke_more((61, 61), Some((0, 14)), 0));
    }

    #[test]
    fn any_return_of_the_watch_fails_however_few_the_wakeups() {
        assert!(woke_more((0, 0), Some((5, 14)), 3));
        assert!(woke_more((0, 0), None, 1));
    }

    #[test]
    fn six_more_wakeups_pass_and_seven_fail_in_either_count() {
        assert!(!woke_more((6, 20), Some((0, 14)), 0));
        assert!(woke_more((7, 14), Some((0, 14)), 0));
        assert!(woke_more((0, 21), Some((0, 14)), 0));
    }
}

#[cfg(target_os = "macos")]
mod probe {
    use anyhow::{Context, Result};
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy, NSRunningApplication, NSWorkspace};
    use scooch::platform::macos::layer;
    use scooch::platform::macos::{finder, launchd, picture, spaces, store};
    use scooch_live::{Gpu, Look as Layered, Pattern, Renderer, Target};
    use scooch::platform::{self, Screen};
    use scooch_core::{Rect, RgbImage};
    use std::ffi::c_void;
    use std::fs;
    use std::io::Write as _;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command, ExitCode, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex, OnceLock};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    const PROBES: [(&str, bool); 4] = [("scooch-probe-a.txt", false), ("scooch-probe-twentyeight.txt", false), ("scooch probe with spaces that wraps.txt", false), ("scooch-probe-folder", true)];
    const RUNNER_ONLY: &str = "macprobe changes the wallpaper, Desktop files and the login item, so it runs those checks only on a GitHub Actions runner where GITHUB_ACTIONS is true.";
    const GRAY: [u8; 3] = [118, 118, 118];
    const LIMIT: Duration = Duration::from_secs(20);
    const STOP_AFTER: Duration = Duration::from_secs(300);
    const BUDGET: Duration = Duration::from_secs(68 * 60);
    static PAST: AtomicBool = AtomicBool::new(false);

    static START: OnceLock<Instant> = OnceLock::new();

    fn now() -> u128 {
        START.get_or_init(Instant::now).elapsed().as_millis()
    }

    fn epoch() -> f64 {
        SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |since| since.as_secs_f64())
    }

    #[repr(C)]
    #[derive(Clone, Copy, Debug, Default)]
    struct Usage {
        uuid: [u8; 16],
        user: u64,
        system: u64,
        idle_wakeups: u64,
        interrupt_wakeups: u64,
        pageins: u64,
        wired: u64,
        resident: u64,
        footprint: u64,
        started: u64,
        exited: u64,
        child_user: u64,
        child_system: u64,
        child_idle: u64,
        child_interrupts: u64,
        child_pageins: u64,
        child_elapsed: u64,
        read: u64,
        written: u64,
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct Timebase {
        numer: u32,
        denom: u32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Point {
        x: f64,
        y: f64,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct ThreadInfo {
        user: u64,
        system: u64,
        cpu: i32,
        policy: i32,
        run_state: i32,
        flags: i32,
        sleep_time: i32,
        current_priority: i32,
        priority: i32,
        max_priority: i32,
        name: [u8; 64],
    }

    extern "C" {
        fn proc_pidinfo(pid: i32, flavor: i32, argument: u64, buffer: *mut c_void, size: i32) -> i32;
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut Usage) -> i32;
        fn mach_timebase_info(info: *mut Timebase) -> i32;
        fn mach_absolute_time() -> u64;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventCreateMouseEvent(source: *const c_void, kind: u32, point: Point, button: u32) -> *mut c_void;
        fn CGEventPost(tap: u32, event: *mut c_void);
        fn CGPreflightPostEventAccess() -> bool;
        fn CGPreflightScreenCaptureAccess() -> bool;
        fn CGWindowListCopyWindowInfo(option: u32, relative: u32) -> *const c_void;
        fn CGDisplayCreateImage(display: u32) -> *const c_void;
        fn CGWindowListCreateImage(bounds: Frame, option: u32, window: u32, image: u32) -> *const c_void;
        fn CGDisplayCreateImageForRect(display: u32, rect: Frame) -> *const c_void;
        fn CGImageGetWidth(image: *const c_void) -> usize;
        fn CGImageGetHeight(image: *const c_void) -> usize;
        fn CGImageGetBytesPerRow(image: *const c_void) -> usize;
        fn CGImageGetBitsPerPixel(image: *const c_void) -> usize;
        fn CGImageGetDataProvider(image: *const c_void) -> *const c_void;
        fn CGDataProviderCopyData(provider: *const c_void) -> *const c_void;
        static kCGDisplayShowDuplicateLowResolutionModes: *const c_void;
        fn CGDisplayCopyAllDisplayModes(display: u32, options: *const c_void) -> *const c_void;
        fn CGDisplayCopyDisplayMode(display: u32) -> *const c_void;
        fn CGDisplayModeGetWidth(mode: *const c_void) -> usize;
        fn CGDisplayModeGetHeight(mode: *const c_void) -> usize;
        fn CGDisplayModeGetPixelWidth(mode: *const c_void) -> usize;
        fn CGDisplayModeGetPixelHeight(mode: *const c_void) -> usize;
        fn CGDisplayModeIsUsableForDesktopGUI(mode: *const c_void) -> bool;
        fn CGDisplayModeRetain(mode: *const c_void) -> *const c_void;
        fn CGDisplayModeRelease(mode: *const c_void);
        fn CGBeginDisplayConfiguration(config: *mut *mut c_void) -> i32;
        fn CGConfigureDisplayWithDisplayMode(config: *mut c_void, display: u32, mode: *const c_void, options: *const c_void) -> i32;
        fn CGCompleteDisplayConfiguration(config: *mut c_void, option: u32) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(object: *const c_void);
        fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
        fn CFDataGetLength(data: *const c_void) -> isize;
        fn CFArrayGetCount(array: *const c_void) -> isize;
        fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
        static kCFBooleanTrue: *const c_void;
        fn CFDictionaryCreate(allocator: *const c_void, keys: *const *const c_void, values: *const *const c_void, count: isize, key_callbacks: *const c_void, value_callbacks: *const c_void) -> *const c_void;
        static kCFTypeDictionaryKeyCallBacks: c_void;
        static kCFTypeDictionaryValueCallBacks: c_void;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGDisplayStreamCreateWithDispatchQueue(display: u32, width: usize, height: usize, format: i32, properties: *const c_void, queue: *mut c_void, handler: &block2::Block<dyn Fn(i32, u64, *mut c_void, *const c_void)>) -> *mut c_void;
        fn CGDisplayStreamStart(stream: *mut c_void) -> i32;
        fn CGDisplayStreamStop(stream: *mut c_void) -> i32;
    }

    #[link(name = "IOSurface", kind = "framework")]
    extern "C" {
        fn IOSurfaceLock(surface: *mut c_void, options: u32, seed: *mut u32) -> i32;
        fn IOSurfaceUnlock(surface: *mut c_void, options: u32, seed: *mut u32) -> i32;
        fn IOSurfaceGetBaseAddress(surface: *mut c_void) -> *mut c_void;
        fn IOSurfaceGetBytesPerRow(surface: *mut c_void) -> usize;
        fn IOSurfaceGetWidth(surface: *mut c_void) -> usize;
        fn IOSurfaceGetHeight(surface: *mut c_void) -> usize;
    }

    extern "C" {
        fn dispatch_queue_create(label: *const std::ffi::c_char, attributes: *const c_void) -> *mut c_void;
        fn dispatch_release(object: *mut c_void);
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        static kCFRunLoopDefaultMode: *const c_void;
        fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, after_source: u8) -> i32;
    }

    extern "C" {
        static mach_task_self_: u32;
        fn mach_port_names(task: u32, names: *mut *mut u32, count: *mut u32, types: *mut *mut u32, types_count: *mut u32) -> i32;
        fn vm_deallocate(task: u32, address: usize, size: usize) -> i32;
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Frame {
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    }

    struct Ran {
        code: Option<i32>,
        out: String,
        err: String,
        took: Duration,
    }

    impl Ran {
        fn text(&self) -> String {
            let mut text = self.out.trim().to_string();
            if !self.err.trim().is_empty() {
                if !text.is_empty() {
                    text.push(' ');
                }
                text.push_str(&format!("[stderr {}]", self.err.trim()));
            }
            text
        }

        fn ok(&self) -> bool {
            self.code == Some(0)
        }
    }

    struct Probe {
        out: PathBuf,
        report: Vec<String>,
        log: fs::File,
        home: PathBuf,
        bin: PathBuf,
        desktop: PathBuf,
        runner: bool,
        failed: Vec<String>,
        made: Vec<PathBuf>,
        original: Vec<(Screen, Option<PathBuf>)>,
        subject: Option<PathBuf>,
        kind_before: Option<String>,
        allowed: bool,
        runs: u32,
        limits: Vec<String>,
        stand: Option<Child>,
        gpu: Option<Gpu>,
        finder: bool,
        motion: Option<Option<String>>,
        scanned: usize,
        struck: bool,
        quiet: Option<(u64, u64)>,
        shared: Arc<Mutex<Vec<String>>>,
        last: Arc<Mutex<String>>,
    }

    struct Watch {
        done: Arc<AtomicBool>,
        fired: Arc<Mutex<Option<String>>>,
        limit: Duration,
    }

    impl Watch {
        fn start(p: &Probe, title: &str, limit: Duration) -> Watch {
            PAST.store(false, Ordering::SeqCst);
            let (done, fired) = (Arc::new(AtomicBool::new(false)), Arc::new(Mutex::new(None)));
            let (ended, mark, shared, last, out, home, title) = (done.clone(), fired.clone(), p.shared.clone(), p.last.clone(), p.out.clone(), p.home.clone(), title.to_string());
            let left = BUDGET.saturating_sub(START.get().map_or(Duration::ZERO, Instant::elapsed));
            let limit = limit.min(left.saturating_sub(STOP_AFTER));
            let wait = move |span: Duration| {
                let started = Instant::now();
                while started.elapsed() < span {
                    if ended.load(Ordering::SeqCst) {
                        return true;
                    }
                    std::thread::sleep(Duration::from_millis(250));
                }
                ended.load(Ordering::SeqCst)
            };
            std::thread::spawn(move || {
                if wait(limit) {
                    return;
                }
                PAST.store(true, Ordering::SeqCst);
                let step = last.lock().map(|step| step.clone()).unwrap_or_default();
                println!("- section {title} passed its deadline of {} s, its last step was `{step}`", limit.as_secs());
                if let Ok(mut mark) = mark.lock() {
                    *mark = Some(step.clone());
                }
                if wait(STOP_AFTER) {
                    return;
                }
                let step = last.lock().map(|step| step.clone()).unwrap_or_default();
                let mut lines = shared.lock().map(|lines| lines.clone()).unwrap_or_default();
                let stopped = format!("{title} passed its deadline of {} s and did not return in {} s more, its last step was `{step}`", limit.as_secs(), STOP_AFTER.as_secs());
                let earlier: Vec<String> = lines.iter().filter_map(|line| line.strip_prefix("- **missing** ").or_else(|| line.strip_prefix("- **check failed** ")).map(str::to_string)).collect();
                lines.extend([format!("- **check failed** {stopped}, so the probe stopped here"), String::new(), "## Result".to_string(), String::new()]);
                lines.extend(earlier.into_iter().chain([stopped]).map(|failure| format!("- **failed** {failure}")));
                let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
                if let Some(pid) = fs::read_to_string(home.join("watch.pid")).ok().and_then(|pid| pid.trim().parse::<i32>().ok()) {
                    unsafe { libc::kill(pid, libc::SIGTERM) };
                }
                let _ = std::io::stdout().flush();
                unsafe { libc::_exit(1) };
            });
            Watch { done, fired, limit }
        }

        fn finish(self) -> Option<(String, Duration)> {
            self.done.store(true, Ordering::SeqCst);
            PAST.store(false, Ordering::SeqCst);
            self.fired.lock().ok().and_then(|fired| fired.clone()).map(|step| (step, self.limit))
        }
    }

    fn deadline(title: &str) -> Duration {
        let longest: u64 = match title {
            "1 Session facts" => 4_566,
            "2 Permission" => 2_134,
            "3 Icons" => 9_499,
            "3b Live layer" => 47_610,
            "3c Helper colour and scale" => 5_578,
            "3d Reveal captures" => 50_000,
            "4 First run" => 101_164,
            "5 Move to new wallpaper" => 62_364,
            "5b Scenery" => 157_529,
            "5c Glide" => 292_101,
            "5g Many slides" => 1_200_000,
            "5d Cold drags" => 1_421_507,
            "5e Bare wakes" => 600_083,
            "5f Finder after the drop" => 60_000,
            "6 Idle" => 71_080,
            "7 Off" => 2_687,
            "8 Second cycle" => 277_539,
            "9 Spaces" => 91_112,
            "9b Program moved" => 300_000,
            "9c Stop" => 300_000,
            "9d Consent waiter" => 540_000,
            "10 Cleanup" => 308,
            _ => 600_000,
        };
        Duration::from_millis((longest * 3).max(30_000))
    }

    impl Probe {
        fn line(&mut self, text: impl Into<String>) {
            let text = text.into();
            println!("{text}");
            let _ = writeln!(self.log, "{:>8} {text}", now());
            if let Ok(mut shared) = self.shared.lock() {
                shared.push(text.clone());
            }
            if let (false, Ok(mut last)) = (text.is_empty(), self.last.lock()) {
                last.clone_from(&text);
            }
            self.report.push(text);
        }

        fn raw(&mut self, text: impl AsRef<str>) {
            let _ = writeln!(self.log, "{:>8} {}", now(), text.as_ref());
            if let Ok(mut last) = self.last.lock() {
                *last = text.as_ref().lines().next().unwrap_or_default().chars().take(200).collect();
            }
        }

        fn limit(&mut self, text: impl Into<String>) {
            let text = text.into();
            self.line(format!("- **runner limit** {text}"));
            if !self.limits.contains(&text) {
                self.limits.push(text);
            }
        }

        fn fail(&mut self, text: impl Into<String>) {
            let text = text.into();
            self.line(format!("- **missing** {text}"));
            self.failed.push(text);
        }

        fn save(&self) {
            let _ = fs::write(self.out.join("report.md"), self.report.join("\n") + "\n");
        }

        fn section(&mut self, title: &str, check: fn(&mut Probe) -> Result<()>) {
            self.line(String::new());
            self.line(format!("## {title}"));
            self.line(String::new());
            let started = Instant::now();
            let watch = Watch::start(self, title, deadline(title));
            if let Err(error) = check(self) {
                self.line(format!("- **check failed** {error:#}"));
                self.failed.push(format!("{title} stopped with {error:#}"));
            }
            if let Some((step, limit)) = watch.finish() {
                self.line(format!("- **check failed** {title} passed its deadline of {} s, its last step was `{step}`", limit.as_secs()));
                self.failed.push(format!("{title} passed its deadline of {} s", limit.as_secs()));
            }
            stutters(self);
            self.line(format!("- took {} ms", started.elapsed().as_millis()));
            self.save();
        }

        fn run(&mut self, program: &Path, arguments: &[&str], timeout: Duration) -> Ran {
            self.runs += 1;
            let stdout = self.out.join(format!("run-{}.out", self.runs));
            let stderr = self.out.join(format!("run-{}.err", self.runs));
            let started = Instant::now();
            let spawned = match (fs::File::create(&stdout), fs::File::create(&stderr)) {
                (Ok(out), Ok(err)) => Command::new(program).args(arguments).env("SCOOCH_HOME", &self.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(out).stderr(err).spawn(),
                _ => Err(std::io::Error::other("could not make the output files")),
            };
            let mut child = match spawned {
                Ok(child) => child,
                Err(error) => return Ran { code: None, out: String::new(), err: format!("could not start {}: {error}", program.display()), took: started.elapsed() },
            };
            let code = loop {
                match child.try_wait() {
                    Ok(Some(status)) => break status.code(),
                    Ok(None) if started.elapsed() < timeout => std::thread::sleep(Duration::from_millis(20)),
                    _ => {
                        let _ = child.kill();
                        let _ = child.wait();
                        break None;
                    }
                }
            };
            let ran = Ran { code, out: fs::read_to_string(&stdout).unwrap_or_default(), err: fs::read_to_string(&stderr).unwrap_or_default(), took: started.elapsed() };
            let _ = fs::remove_file(stdout);
            let _ = fs::remove_file(stderr);
            self.raw(format!("ran {} {:?} code {:?} in {} ms\n{}\n{}", program.display(), arguments, ran.code, ran.took.as_millis(), ran.out, ran.err));
            ran
        }

        fn tool(&mut self, program: &str, arguments: &[&str]) -> Ran {
            self.run(Path::new(program), arguments, LIMIT)
        }

        fn osa(&mut self, script: &str) -> Ran {
            self.run(Path::new("/usr/bin/osascript"), &["-e", script], LIMIT)
        }

        fn scooch(&mut self, arguments: &[&str], timeout: Duration) -> Ran {
            let bin = self.bin.clone();
            self.run(&bin, arguments, timeout)
        }
    }

    pub fn main() -> ExitCode {
        START.get_or_init(Instant::now);
        if std::env::args().nth(1).as_deref() == Some("pace") {
            return pace(PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "macpace".into())));
        }
        if std::env::args().nth(1).as_deref() == Some("helper") {
            return helper();
        }
        if std::env::args().nth(1).as_deref() == Some("cold-start") {
            return cold_start(PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "maccold".into())));
        }
        if std::env::args().nth(1).as_deref() == Some("cold-split") {
            return cold_split(PathBuf::from(std::env::args().nth(2).unwrap_or_default()), PathBuf::from(std::env::args().nth(3).unwrap_or_default()));
        }
        if std::env::args().nth(1).as_deref() == Some("slow-check") {
            return slow_check(std::env::args().skip(2).collect());
        }
        if std::env::args().nth(1).as_deref() == Some("black-frames") {
            return black_frames(PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "macblack".into())));
        }
        if std::env::args().nth(1).as_deref() == Some("gpu-bench") {
            return gpu_bench(PathBuf::from(std::env::args().nth(2).unwrap_or_else(|| "macbench".into())));
        }
        let out = PathBuf::from(std::env::args().nth(1).unwrap_or_else(|| "macprobe".into()));
        let home = std::env::temp_dir().join(format!("scooch-probe-{}", std::process::id()));
        if fs::create_dir_all(&out).is_err() || fs::create_dir_all(&home).is_err() {
            eprintln!("macprobe could not make its folders, so check the disk.");
            return ExitCode::from(2);
        }
        let home = home.canonicalize().unwrap_or(home);
        std::env::set_var("SCOOCH_HOME", &home);
        let Ok(log) = fs::File::create(out.join("raw.log")) else {
            eprintln!("macprobe could not write its log, so check the disk.");
            return ExitCode::from(2);
        };
        let bin = std::env::current_exe().ok().and_then(|exe| exe.parent().and_then(Path::parent).map(|folder| folder.join("scooch"))).unwrap_or_default();
        let runner = std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true");
        let mut probe = Probe {
            out,
            report: vec![format!("# Scooch macOS probe on {}", std::env::var("RUNNER_NAME").unwrap_or_else(|_| "this Mac".into()))],
            log,
            home,
            bin,
            desktop: dirs::desktop_dir().unwrap_or_default(),
            runner,
            failed: Vec::new(),
            made: Vec::new(),
            original: Vec::new(),
            subject: None,
            kind_before: None,
            allowed: false,
            runs: 0,
            limits: Vec::new(),
            stand: None,
            gpu: None,
            finder: true,
            motion: None,
            scanned: 0,
            struck: false,
            quiet: None,
            shared: Arc::new(Mutex::new(Vec::new())),
            last: Arc::new(Mutex::new(String::new())),
        };
        probe.section("1 Session facts", facts);
        if !probe.runner {
            probe.line(String::new());
            probe.line(format!("- {RUNNER_ONLY}"));
            probe.save();
            let _ = fs::remove_dir_all(&probe.home);
            return ExitCode::SUCCESS;
        }
        probe.section("2 Permission", permission);
        if std::env::var("SCOOCH_PROBE_FOCUS").is_ok_and(|focus| focus == "slides") {
            probe.section("3 Icons", icons);
            probe.section("4 First run", first_run);
            probe.section("5g Many slides", slides_check);
        } else {
            every_section(&mut probe);
        }
        probe.section("10 Cleanup", cleanup);
        probe.line(String::new());
        probe.line("## Result".to_string());
        probe.line(String::new());
        let failed = probe.failed.clone();
        if failed.is_empty() {
            probe.line("- every required measurement came back".to_string());
        }
        for failure in &failed {
            probe.line(format!("- **failed** {failure}"));
        }
        let limits = probe.limits.clone();
        for limit in &limits {
            probe.line(format!("- runner limit, not a failure: {limit}"));
        }
        probe.save();
        if failed.is_empty() {
            ExitCode::SUCCESS
        } else {
            ExitCode::FAILURE
        }
    }

    fn every_section(probe: &mut Probe) {
        probe.section("3 Icons", icons);
        probe.section("3b Live layer", live);
        probe.section("3c Helper colour and scale", colour_scale);
        probe.section("3d Reveal captures", reveal_check);
        probe.section("4 First run", first_run);
        probe.section("5 Move to new wallpaper", moves);
        probe.section("5b Scenery", scenery_check);
        probe.section("5c Glide", glide_check);
        probe.section("5g Many slides", slides_check);
        probe.section("5d Drags after a start", cold_drags_check);
        probe.section("5e Quiet minutes and a move after them", rest_check);
        probe.section("5f Finder after the drop", finder_after_drop);
        probe.section("6 Idle", idle);
        probe.section("7 Off", off);
        probe.section("8 Second cycle", second_cycle);
        probe.section("9 Spaces", spaces_check);
        probe.section("9b Program moved", program_moved);
        probe.section("9c Stop", stop_check);
        probe.section("9d Consent waiter", consent_waiter);
    }

    fn show(rect: &Rect) -> String {
        format!("{:.0},{:.0} {:.0}x{:.0}", rect.x, rect.y, rect.w, rect.h)
    }

    fn uid() -> u32 {
        unsafe { libc::getuid() }
    }

    fn target() -> String {
        format!("gui/{}/{}", uid(), launchd::LABEL)
    }

    fn facts(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        p.line(format!("- adapter {}", desktop.name()));
        let version = p.tool("/usr/bin/sw_vers", &[]);
        p.line(format!("- sw_vers {}", version.out.split_whitespace().collect::<Vec<_>>().join(" ")));
        let machine = p.tool("/usr/bin/uname", &["-m"]);
        p.line(format!("- machine {}", machine.out.trim()));
        let user = p.tool("/usr/bin/id", &["-un"]);
        let console = p.tool("/usr/bin/stat", &["-f", "%Su", "/dev/console"]);
        let manager = p.tool("/bin/launchctl", &["managername"]);
        p.line(format!("- user {}, console owner {}, launchd session {}", user.out.trim(), console.out.trim(), manager.out.trim()));
        p.line(format!("- SCOOCH_HOME {}, scooch binary {} exists {}", p.home.display(), p.bin.display(), p.bin.is_file()));
        let sizes: Vec<String> = [p.bin.clone(), p.bin.with_file_name("scoochw"), std::env::current_exe().unwrap_or_default()].iter().map(|path| format!("{} {:.2} MB", path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(), fs::metadata(path).map_or(0, |meta| meta.len()) as f64 / 1_048_576.0)).collect();
        p.line(format!("- program sizes {sizes:?}"));
        p.line(format!("- Desktop folder {} exists {}", p.desktop.display(), p.desktop.is_dir()));
        let displays = p.tool("/usr/sbin/system_profiler", &["SPDisplaysDataType"]);
        for line in displays.out.lines().map(str::trim).filter(|line| line.starts_with("Resolution") || line.starts_with("UI Looks like") || line.starts_with("Display Type") || line.starts_with("Main Display") || line.ends_with(':') && !line.starts_with("Graphics")) {
            p.line(format!("- display info {line}"));
        }
        let screens = desktop.screens()?;
        if screens.is_empty() {
            p.fail("no screen shows a desktop");
        }
        for screen in &screens {
            let shown = desktop.wallpaper(screen).ok().flatten();
            let (display, space) = spaces::split(&screen.id);
            let kind = store::describe(&screen.id);
            p.line(format!("- screen `{}` frame {} scale {} display {display} space {:?}", screen.id, show(&screen.frame), screen.scale, space));
            p.line(format!("  - wallpaper {:?}, store kind {:?}", shown, kind));
            p.original.push((screen.clone(), shown));
        }
        p.line(format!("- known Spaces {:?}", desktop.known().map_err(|error| error.to_string())));
        match spaces::window_server() {
            Some(monitors) => {
                for monitor in monitors {
                    p.line(format!("- window server monitor `{}` current {:?} full screen {} desktop Spaces {:?}", monitor.name, monitor.current, monitor.full_screen, monitor.spaces));
                }
            }
            None => p.line("- window server Spaces unreadable".to_string()),
        }
        p.line(format!("- preference Spaces {:?}", spaces::preferences()));
        let view = finder::view();
        p.line(format!("- Finder desktop view {view:?}"));
        p.line(format!("- in_use for the probe home {:?}", store::in_use(&p.home.join("out"))));
        let doctor = p.scooch(&["doctor"], LIMIT);
        p.line(format!("- scooch doctor exit {:?}", doctor.code));
        for line in doctor.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        if !doctor.ok() {
            p.fail("scooch doctor did not finish cleanly");
        }
        Ok(())
    }

    fn access_words(access: finder::Access) -> String {
        format!("{access:?}")
    }

    fn permission(p: &mut Probe) -> Result<()> {
        let access = finder::access(false);
        p.allowed = access == finder::Access::Allowed;
        p.line(format!("- Finder automation for the probe, asked without a prompt: {}", access_words(access)));
        p.line(format!("- screen capture preflight {}, post event preflight {}", unsafe { CGPreflightScreenCaptureAccess() }, unsafe { CGPreflightPostEventAccess() }));
        let chain = p.tool("/bin/ps", &["-o", "pid=,ppid=,comm=", "-ax"]);
        let mut parent = std::process::id() as i32;
        let table: Vec<(i32, i32, String)> = chain.out.lines().filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?, parts.collect::<Vec<_>>().join(" ")))
        }).collect();
        let mut ancestry = Vec::new();
        for _ in 0..12 {
            let Some((pid, ppid, name)) = table.iter().find(|(pid, _, _)| *pid == parent).cloned() else { break };
            ancestry.push(format!("{pid} {name}"));
            if ppid <= 1 {
                ancestry.push(format!("{ppid} launchd"));
                break;
            }
            parent = ppid;
        }
        p.line(format!("- probe ancestry {}", ancestry.join(" < ")));
        for (label, database) in [("user", dirs::home_dir().unwrap_or_default().join("Library/Application Support/com.apple.TCC/TCC.db")), ("system", PathBuf::from("/Library/Application Support/com.apple.TCC/TCC.db"))] {
            let query = "select service, client, auth_value, coalesce(indirect_object_identifier, '') from access where service in ('kTCCServiceAppleEvents', 'kTCCServiceAccessibility', 'kTCCServiceScreenCapture', 'kTCCServicePostEvent', 'kTCCServiceSystemPolicyAllFiles') order by service, client";
            let rows = p.tool("/usr/bin/sqlite3", &["-separator", " | ", &database.to_string_lossy(), query]);
            if rows.ok() {
                let lines: Vec<String> = rows.out.lines().map(str::to_string).collect();
                p.line(format!("- {label} TCC database {} rows for automation, accessibility, capture, events and disk", lines.len()));
                for row in lines {
                    p.line(format!("  - `{row}`"));
                }
            } else {
                p.line(format!("- {label} TCC database unreadable: {}", rows.text()));
            }
        }
        let plist = p.home.join("doctor-probe.plist");
        let output = p.home.join("doctor-launchd.txt");
        let label = "me.sohanbhat.scooch.doctorprobe";
        let text = format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n<key>Label</key><string>{label}</string>\n<key>ProgramArguments</key><array><string>{}</string><string>doctor</string></array>\n<key>RunAtLoad</key><true/>\n<key>EnvironmentVariables</key><dict><key>SCOOCH_HOME</key><string>{}</string></dict>\n<key>StandardOutPath</key><string>{}</string>\n<key>StandardErrorPath</key><string>{}</string>\n</dict>\n</plist>\n",
            p.bin.display(),
            p.home.display(),
            output.display(),
            output.display()
        );
        fs::write(&plist, text)?;
        let domain = format!("gui/{}", uid());
        let loaded = p.tool("/bin/launchctl", &["bootstrap", &domain, &plist.to_string_lossy()]);
        p.line(format!("- doctor started by launchd as {label}, bootstrap {}", if loaded.ok() { "ok".to_string() } else { loaded.text() }));
        let done = wait_until(Duration::from_secs(30), || fs::read_to_string(&output).is_ok_and(|text| text.contains("Login item")));
        let _ = p.tool("/bin/launchctl", &["bootout", &format!("{domain}/{label}")]);
        let text = fs::read_to_string(&output).unwrap_or_default();
        p.line(format!("- doctor under launchd finished after {:?} ms", done.map(|time| time.as_millis())));
        for line in text.lines() {
            p.line(format!("  - `{line}`"));
        }
        if !text.lines().any(|line| line.contains("Automation")) {
            p.fail("doctor under launchd printed no Automation line");
        }
        Ok(())
    }

    fn wait_until(limit: Duration, mut done: impl FnMut() -> bool) -> Option<Duration> {
        let started = Instant::now();
        while started.elapsed() < limit && !PAST.load(Ordering::SeqCst) {
            if done() {
                return Some(started.elapsed());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn filled(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
        RgbImage::from_raw(width, height, color.repeat((width * height) as usize)).unwrap_or_default()
    }

    fn flat(width: u32, height: u32) -> RgbImage {
        filled(width, height, GRAY)
    }

    fn subject_picture(width: u32, height: u32, at: (f32, f32)) -> RgbImage {
        disc_picture(width, height, at, scooch_core::ISLAND_WATER)
    }

    fn green_picture(width: u32, height: u32, at: (f32, f32)) -> RgbImage {
        disc_picture(width, height, at, [19, 90, 48])
    }

    fn disc_picture(width: u32, height: u32, at: (f32, f32), water: [u8; 3]) -> RgbImage {
        let mut image = filled(width, height, water);
        let (cx, cy) = (at.0 * width as f32, at.1 * height as f32);
        let radius = height as f32 / 7.0;
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            if ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() < radius {
                pixel.0 = [245, 196, 92];
            }
        }
        image
    }

    fn navy_picture(width: u32, height: u32, at: (f32, f32)) -> RgbImage {
        let mut image = filled(width, height, [24, 34, 58]);
        let (cx, cy) = (at.0 * width as f32, at.1 * height as f32);
        let radius = height as f32 / 7.0;
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            let distance = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt();
            if distance < radius {
                pixel.0 = [245, 196, 92];
            } else if (x / 24 + y / 24) % 2 == 0 {
                pixel.0 = [30, 42, 70];
            }
        }
        image
    }

    fn picture_file(p: &Probe, name: &str, image: &RgbImage) -> Result<PathBuf> {
        let path = p.home.join(name);
        fs::write(&path, scooch_core::encode_jpeg(image, 92)?)?;
        Ok(path)
    }

    fn main_screen(p: &Probe) -> Result<Screen> {
        p.original.first().map(|(screen, _)| screen.clone()).context("no screen was read in check 1")
    }

    fn pixels(screen: &Screen) -> (u32, u32) {
        ((screen.frame.w * screen.scale).round() as u32, (screen.frame.h * screen.scale).round() as u32)
    }

    fn place(p: &mut Probe, name: &str, at: (f32, f32)) -> Ran {
        let script = format!("with timeout of 15 seconds\ntell application \"Finder\" to set desktop position of item \"{name}\" of desktop to {{{}, {}}}\nend timeout", at.0.round() as i32, at.1.round() as i32);
        p.osa(&script)
    }

    fn position(p: &mut Probe, name: &str) -> Option<(f32, f32)> {
        let ran = p.osa(&format!("with timeout of 15 seconds\ntell application \"Finder\" to get desktop position of item \"{name}\" of desktop\nend timeout"));
        let numbers: Vec<f32> = ran.out.trim().split(", ").filter_map(|part| part.trim().parse().ok()).collect();
        (numbers.len() == 2).then(|| (numbers[0], numbers[1]))
    }

    fn make_probes(p: &mut Probe) -> Result<()> {
        for (name, folder) in PROBES {
            let path = p.desktop.join(name);
            if folder {
                fs::create_dir_all(&path)?;
            } else {
                fs::write(&path, b"scooch probe")?;
            }
            p.made.push(path);
        }
        Ok(())
    }

    fn capture(p: &mut Probe, name: &str) -> Option<RgbImage> {
        let path = p.out.join(name);
        let ran = p.tool("/usr/sbin/screencapture", &["-x", "-D", "1", "-t", "png", &path.to_string_lossy()]);
        if !ran.ok() {
            p.line(format!("- screencapture failed {}", ran.text()));
            return None;
        }
        match scooch_core::open(&path) {
            Ok(image) => Some(image),
            Err(error) => {
                p.line(format!("- capture unreadable {error}"));
                None
            }
        }
    }

    fn ink(image: &RgbImage, around: &Rect, scale: f32, background: [u8; 3]) -> Option<Rect> {
        let spare = 30.0;
        let x0 = ((around.x - spare) * scale).max(0.0) as u32;
        let y0 = ((around.y - spare) * scale).max(0.0) as u32;
        let x1 = (((around.x + around.w + spare) * scale) as u32).min(image.width());
        let y1 = (((around.y + around.h + spare) * scale) as u32).min(image.height());
        let (mut left, mut top, mut right, mut bottom) = (u32::MAX, u32::MAX, 0, 0);
        for y in y0..y1 {
            for x in x0..x1 {
                let pixel = image.get_pixel(x, y).0;
                let distance: i32 = (0..3).map(|channel| (pixel[channel] as i32 - background[channel] as i32).abs()).sum();
                if distance > 45 {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
            }
        }
        (left <= right && top <= bottom).then(|| Rect { x: left as f32 / scale, y: top as f32 / scale, w: (right - left + 1) as f32 / scale, h: (bottom - top + 1) as f32 / scale })
    }

    fn icons(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let gray = picture_file(p, "probe-flat.jpg", &flat(width, height))?;
        desktop.set_wallpaper(&screen, &gray)?;
        p.line(format!("- wallpaper set to a flat gray picture {}", gray.display()));
        make_probes(p)?;
        std::thread::sleep(Duration::from_millis(2500));
        let spots: Vec<(f32, f32)> = (0..PROBES.len()).map(|index| (screen.frame.x + 160.0 + 230.0 * index as f32, screen.frame.y + 200.0 + 60.0 * (index % 2) as f32)).collect();
        for ((name, _), spot) in PROBES.iter().zip(&spots) {
            let ran = place(p, name, *spot);
            p.line(format!("- set {name} to {:.0},{:.0} by Apple Event in {} ms {}", spot.0, spot.1, ran.took.as_millis(), if ran.ok() { "ok".to_string() } else { ran.text() }));
        }
        std::thread::sleep(Duration::from_millis(1500));
        for (name, _) in PROBES {
            let at = position(p, name);
            p.line(format!("- Finder reports {name} at {at:?}"));
        }
        if !p.allowed {
            p.fail("the probe may not read Finder, so the adapter rectangles could not be read");
            return Ok(());
        }
        let view = finder::view();
        let named = finder::named(&view)?;
        p.line(format!("- adapter read {} desktop items", named.len()));
        let shot = capture(p, "icons.png");
        let background = shot.as_ref().map(|image| image.get_pixel(image.width() * 3 / 5, image.height() * 3 / 4).0).unwrap_or(GRAY);
        p.line(format!("- background pixel {background:?}, capture {}", shot.as_ref().map_or("missing".to_string(), |image| format!("{}x{}", image.width(), image.height()))));
        let mut matched = 0;
        for (name, _) in PROBES {
            let stem = name.split('.').next().unwrap_or(name);
            let Some((seen, rect)) = named.iter().find(|(seen, _)| seen == name || seen.starts_with(stem)).cloned() else {
                p.line(format!("- adapter has no item named like {name}"));
                continue;
            };
            matched += 1;
            let truth = shot.as_ref().and_then(|image| ink(image, &rect, screen.scale, background));
            match truth {
                Some(truth) => p.line(format!(
                    "- `{seen}` adapter {} pixels {} margins left {:.0} top {:.0} right {:.0} bottom {:.0}",
                    show(&rect),
                    show(&truth),
                    truth.x - rect.x,
                    truth.y - rect.y,
                    (rect.x + rect.w) - (truth.x + truth.w),
                    (rect.y + rect.h) - (truth.y + truth.h)
                )),
                None => p.line(format!("- `{seen}` adapter {} pixels none found", show(&rect))),
            }
        }
        if matched < PROBES.len() {
            p.fail(format!("the adapter found {matched} of {} probe items", PROBES.len()));
        }
        Ok(())
    }

    fn state(p: &Probe) -> serde_json::Value {
        fs::read(p.home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn daemon_pid(p: &mut Probe) -> Option<i32> {
        let printed = p.tool("/bin/launchctl", &["print", &target()]);
        printed.out.lines().map(str::trim).find_map(|line| line.strip_prefix("pid = ").and_then(|pid| pid.trim().parse().ok()))
    }

    fn scooch_processes(p: &mut Probe) -> Vec<String> {
        let found = p.tool("/usr/bin/pgrep", &["-lx", "scooch"]);
        found.out.lines().map(str::to_string).filter(|line| !line.is_empty()).collect()
    }

    fn daemon_log(p: &Probe) -> String {
        fs::read_to_string(p.home.join("watch.log")).unwrap_or_default()
    }

    fn stutters(p: &mut Probe) -> bool {
        let log = daemon_log(p);
        let fresh = new_lines(&log, p.scanned);
        p.scanned = log.lines().count();
        for line in fresh {
            if let Some(rest) = line.split(" glide glided ").nth(1) {
                let words: Vec<&str> = rest.split_whitespace().collect();
                let number = |at: usize| words.get(at).and_then(|word| word.trim_end_matches(',').parse::<f64>().ok());
                if let Some((frames, slow)) = number(2).zip(number(4)).filter(|(frames, slow)| slow * 2.0 > *frames) {
                    p.line(format!("- glide {} stuttered, {slow} of {frames} frames slow at scale {}", words.first().unwrap_or(&"?"), words.get(8).map_or("unknown", |scale| scale.trim_end_matches(','))));
                }
            }
            if line.contains("glides in a row stuttered") {
                p.struck = true;
                p.fail("the strike rule fired, two glides in a row stuttered, so the daemon turned the glide off until it starts again");
            }
        }
        p.struck
    }

    fn scooch_pid(p: &Probe) -> Option<i32> {
        let pid: i32 = fs::read_to_string(p.home.join("watch.pid")).ok()?.trim().parse().ok()?;
        (unsafe { libc::kill(pid, 0) } == 0).then_some(pid)
    }

    fn problem(p: &Probe) -> Option<String> {
        state(p)["problem"].as_str().map(str::to_string)
    }

    fn asking(p: &Probe) -> bool {
        problem(p).is_some_and(|problem| problem == finder::ASKING || problem == finder::DENIED)
    }

    fn windows() -> Vec<(String, i64, String)> {
        let list = unsafe { CGWindowListCopyWindowInfo(1, 0) };
        if list.is_null() {
            return Vec::new();
        }
        let parsed = store::convert(unsafe { &*(list as *const objc2::runtime::AnyObject) });
        unsafe { CFRelease(list) };
        parsed
            .items()
            .unwrap_or(&[])
            .iter()
            .map(|window| {
                let owner = window.get("kCGWindowOwnerName").and_then(store::Plist::text).unwrap_or_default().to_string();
                let layer = window.get("kCGWindowLayer").and_then(store::Plist::number).unwrap_or_default() as i64;
                let name = window.get("kCGWindowName").and_then(store::Plist::text).unwrap_or_default().to_string();
                (owner, layer, name)
            })
            .collect()
    }

    fn prompt_owners(p: &mut Probe) -> Vec<String> {
        let common = ["Finder", "Dock", "Window Server", "WindowServer", "SystemUIServer", "Control Center", "Window Manager", "Spotlight", "Notification Center", "TextInputMenuAgent"];
        let shown: Vec<(String, i64, String)> = windows().into_iter().filter(|(owner, _, _)| !common.contains(&owner.as_str())).collect();
        p.line(format!("- windows on screen besides the desktop ones {shown:?}"));
        let mut owners: Vec<String> = shown.into_iter().map(|(owner, _, _)| owner).collect();
        owners.dedup();
        owners
    }

    fn wait_render(p: &Probe, limit: Duration, source: Option<&Path>, stop_on_consent: bool) -> Option<Duration> {
        let home = p.home.clone();
        let source = source.map(|path| path.to_string_lossy().into_owned());
        let started = Instant::now();
        while started.elapsed() < limit {
            let state: serde_json::Value = fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            let screens = state["screens"].as_object();
            let done = screens.is_some_and(|screens| !screens.is_empty() && screens.values().all(|entry| !entry["fingerprint"].is_null() && source.as_deref().is_none_or(|source| entry["source"].as_str() == Some(source))));
            if done {
                return Some(started.elapsed());
            }
            if stop_on_consent && state["problem"].as_str().is_some_and(|problem| problem == finder::ASKING || problem == finder::DENIED) {
                return None;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        None
    }

    fn finder_answers(p: &mut Probe) -> Option<u128> {
        let ran = p.osa("with timeout of 10 seconds\ntell application \"Finder\" to get name of startup disk\nend timeout");
        ran.ok().then_some(ran.took.as_millis())
    }

    fn dismiss_prompt(p: &mut Probe) -> Option<u128> {
        if let Some(answers) = finder_answers(p) {
            p.line(format!("- Finder answers Apple Events in {answers} ms"));
            return Some(answers);
        }
        let owners = prompt_owners(p);
        for owner in owners.iter().filter(|owner| ["UserNotificationCenter", "tccd", "universalAccessAuthWarn", "CoreServicesUIAgent"].contains(&owner.as_str())) {
            let ended = p.tool("/usr/bin/killall", &[owner]);
            p.line(format!("- closed the unanswered consent window by quitting {owner} {}, which answers nothing", if ended.ok() { "ok".to_string() } else { ended.text() }));
        }
        std::thread::sleep(Duration::from_secs(2));
        let answers = finder_answers(p);
        p.line(format!("- Finder answers Apple Events again {answers:?} ms"));
        if answers.is_none() {
            p.limit("an unanswered Finder consent window blocks Apple Events to Finder for every app on the runner until someone answers it");
        }
        answers
    }

    fn stand_in(p: &mut Probe, what: &str) -> Result<Option<Duration>> {
        if !asking(p) && !windows().iter().any(|(owner, _, _)| owner == "UserNotificationCenter") {
            return Ok(None);
        }
        p.limit("on a hosted runner the login item daemon waits for a Finder automation prompt that nobody can click, and the runner image pre-grants Finder automation only to bash, osascript and its own agents, which a launchd job does not inherit");
        let job = daemon_pid(p);
        let _ = p.tool("/bin/launchctl", &["bootout", &target()]);
        let _ = wait_until(Duration::from_secs(10), || !launchd::loaded());
        p.finder = dismiss_prompt(p).is_some();
        let _ = fs::remove_file(p.home.join("watch.pid"));
        let log = fs::OpenOptions::new().create(true).append(true).open(p.home.join("watch.log"))?;
        let started = Instant::now();
        let child = Command::new(&p.bin).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        p.stand = Some(child);
        let home = p.home.clone();
        wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.pid")).ok().and_then(|text| text.trim().parse::<i32>().ok()).is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)).context("the probe started daemon wrote no pid file")?;
        let pid = scooch_pid(p);
        p.line(format!("- {what}: booted out the login item job {job:?}, closed its consent window, and started scooch watch from the probe as {pid:?}, which inherits the runner's Finder automation"));
        Ok(Some(started.elapsed()))
    }

    fn reap(p: &mut Probe) {
        if let Some(mut child) = p.stand.take() {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(10) {
                if let Ok(Some(_)) = child.try_wait() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = child.kill();
            let _ = child.wait();
            p.line("- the probe started daemon had to be killed".to_string());
        }
    }

    fn ticks(p: &Probe) -> usize {
        daemon_log(p).lines().filter(|line| line.contains(" tick ")).count()
    }

    fn first_run(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let subject = picture_file(p, "probe-subject.jpg", &subject_picture(width, height, (0.5, 0.5)))?;
        desktop.set_wallpaper(&screen, &subject)?;
        p.subject = Some(subject.clone());
        let calm = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        for (index, (name, _)) in PROBES.iter().enumerate() {
            let _ = place(p, name, (calm.0, calm.1 + 110.0 * index as f32));
        }
        let _ = place(p, PROBES[0].0, (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5));
        p.line(format!("- {} sits on the subject and the other probe items sit in a calm column", PROBES[0].0));
        std::thread::sleep(Duration::from_millis(1500));
        let kind = format!("{:?}", store::describe(&screen.id));
        p.line(format!("- wallpaper set to a teal water test JPEG with one bright subject in the middle {}, store kind {kind}", subject.display()));
        p.kind_before = Some(kind);
        let started = Instant::now();
        let ran = p.scooch(&[], Duration::from_secs(60));
        p.line(format!("- bare scooch exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        let rendered_at = wait_render(p, Duration::from_secs(45), None, true).map(|_| started.elapsed());
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let ours = shown.as_ref().is_some_and(|path| path.starts_with(p.home.join("out")));
        p.line(format!("- first render by the login item {:?} ms after scooch started, wallpaper now {:?}, inside the probe home {ours}", rendered_at.map(|time| time.as_millis()), shown));
        let plist = launchd::plist_path()?;
        p.line(format!("- login item file {} exists {}", plist.display(), plist.exists()));
        if let Ok(text) = fs::read_to_string(&plist) {
            p.raw(format!("login item\n{text}"));
            p.line(format!("- login item passes SCOOCH_HOME {} and SCOOCH_TRACE {}", text.contains("SCOOCH_HOME"), text.contains("SCOOCH_TRACE")));
        }
        let pid = daemon_pid(p);
        let parent = pid.map(|pid| p.tool("/bin/ps", &["-o", "ppid=,comm=", "-p", &pid.to_string()]).out.trim().to_string());
        p.line(format!("- launchctl print finds the job {}, daemon pid {pid:?}, parent and command {parent:?}", launchd::loaded()));
        p.line(format!("- state on {} problem {:?}", state(p)["on"], problem(p)));
        let status = p.scooch(&["status"], LIMIT);
        p.line(format!("- scooch status says {:?}", status.text()));
        let log = daemon_log(p);
        p.line(format!("- daemon log {} lines", log.lines().count()));
        for line in log.lines().filter(|line| !line.contains("mouse up")).take(25) {
            p.line(format!("  - `{}`", line.chars().take(200).collect::<String>()));
        }
        if !plist.exists() || !launchd::loaded() {
            p.fail("bare scooch did not leave a loaded login item");
        }
        if pid.is_none() {
            p.fail("no daemon process runs under launchd");
        }
        if rendered_at.is_some() && ours {
            return Ok(());
        }
        if !asking(p) {
            p.fail("the daemon under launchd never rendered");
            return Ok(());
        }
        let swapped = stand_in(p, "first run")?;
        let rendered = wait_render(p, Duration::from_secs(45), None, false);
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let ours = shown.as_ref().is_some_and(|path| path.starts_with(p.home.join("out")));
        p.line(format!(
            "- first render by the probe started daemon {:?} ms after it started, wallpaper now {shown:?}, inside the probe home {ours}",
            swapped.zip(rendered).map(|(load, render)| (load + render).as_millis())
        ));
        if rendered.is_none() || !ours {
            p.fail("the probe started daemon never rendered");
        }
        if let Some(pid) = scooch_pid(p) {
            quiet_minute(p, pid, "the probe started daemon right after its launch, with no consent pending")?;
        }
        Ok(())
    }

    fn new_lines(text: &str, from: usize) -> Vec<String> {
        text.lines().skip(from).map(str::to_string).collect()
    }

    fn post(kind: u32, at: (f32, f32)) {
        unsafe {
            let event = CGEventCreateMouseEvent(std::ptr::null(), kind, Point { x: at.0 as f64, y: at.1 as f64 }, 0);
            if !event.is_null() {
                CGEventPost(0, event);
                CFRelease(event);
            }
        }
    }

    fn drag(from: (f32, f32), to: (f32, f32)) {
        post(5, from);
        std::thread::sleep(Duration::from_millis(120));
        post(1, from);
        std::thread::sleep(Duration::from_millis(250));
        for step in 1..=20 {
            let share = step as f32 / 20.0;
            post(6, (from.0 + (to.0 - from.0) * share, from.1 + (to.1 - from.1) * share));
            std::thread::sleep(Duration::from_millis(25));
        }
        std::thread::sleep(Duration::from_millis(150));
        post(2, to);
    }

    fn poke(p: &mut Probe) {
        if let Some(pid) = scooch_pid(p) {
            let _ = p.tool("/bin/kill", &["-USR1", &pid.to_string()]);
        }
    }

    fn away(p: &mut Probe, name: &str, calm: (f32, f32)) {
        let desktop = platform::current();
        let screen = main_screen(p);
        let before = desktop.as_ref().ok().zip(screen.as_ref().ok()).and_then(|(desktop, screen)| desktop.wallpaper(screen).ok().flatten());
        let _ = place(p, name, calm);
        std::thread::sleep(Duration::from_millis(300));
        poke(p);
        let _ = wait_until(Duration::from_secs(6), || desktop.as_ref().ok().zip(screen.as_ref().ok()).and_then(|(desktop, screen)| desktop.wallpaper(screen).ok().flatten()) != before);
        std::thread::sleep(Duration::from_millis(1200));
    }

    fn moves(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let calm = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.finder = false;
            p.line("- the moves need Apple Events to Finder, which the open consent window blocks, so they were skipped".to_string());
            return Ok(());
        }
        p.line(format!("- each round first moves {name} to a calm spot without timing it and pokes the daemon, then times one move onto the subject, with the scenery setting `{}`", state(p)["options"]["scenery"].as_str().unwrap_or("missing")));
        let mut times = Vec::new();
        let mut sources = std::collections::BTreeMap::<String, usize>::new();
        let first_line = daemon_log(p).lines().count();
        for round in 0..5 {
            away(p, name, calm);
            let before = desktop.wallpaper(&screen).ok().flatten();
            let seen = daemon_log(p).lines().count();
            let ran = place(p, name, subject);
            let moved = Instant::now();
            let changed = wait_until(Duration::from_secs(12), || desktop.wallpaper(&screen).ok().flatten() != before);
            let latency = changed.map(|_| moved.elapsed().as_millis());
            std::thread::sleep(Duration::from_millis(400));
            let wakes: Vec<String> = new_lines(&daemon_log(p), seen).iter().filter_map(|line| line.split(" wake ").nth(1).map(|source| source.trim().to_string())).collect();
            for wake in &wakes {
                *sources.entry(wake.clone()).or_default() += 1;
            }
            p.line(format!("- Apple Event move {} onto the subject took {} ms {}, new wallpaper after {latency:?} ms, wakes {wakes:?}", round + 1, ran.took.as_millis(), if ran.ok() { "ok" } else { "failed" }));
            times.extend(latency);
        }
        times.sort_unstable();
        p.line(format!("- Apple Event moves that brought a new wallpaper {} of 5, times {times:?} ms, median {:?}", times.len(), times.get(times.len() / 2)));
        p.line(format!("- wake sources during the Apple Event moves {sources:?}, render laps {:?} ms", render_laps(p, first_line)));
        if times.is_empty() {
            p.fail("no Apple Event move brought a new wallpaper");
        }
        let allowed = unsafe { CGPreflightPostEventAccess() };
        p.line(format!("- posting mouse events allowed by preflight {allowed}"));
        away(p, name, calm);
        let start = position(p, name).unwrap_or(calm);
        let before = desktop.wallpaper(&screen).ok().flatten();
        let seen = daemon_log(p).lines().count();
        drag(start, subject);
        let dropped = Instant::now();
        let changed = wait_until(Duration::from_secs(12), || desktop.wallpaper(&screen).ok().flatten() != before);
        let latency = changed.map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(400));
        let after = position(p, name);
        let wakes: Vec<String> = new_lines(&daemon_log(p), seen).iter().filter_map(|line| line.split(" wake ").nth(1).map(|source| source.trim().to_string())).collect();
        let moved = after.is_some_and(|after| (after.0 - start.0).abs() > 20.0 || (after.1 - start.1).abs() > 20.0);
        p.line(format!("- CGEvent drag from {:.0},{:.0} onto the subject at {:.0},{:.0}: item moved {moved} to {after:?}, new wallpaper after {latency:?} ms, wakes {wakes:?}", start.0, start.1, subject.0, subject.1));
        if allowed && (!moved || latency.is_none()) {
            p.fail("the CGEvent drag did not move the item and bring a new wallpaper");
        }
        if !allowed {
            p.limit("the runner does not let the probe post mouse events, so no real drag was tried");
        }
        Ok(())
    }

    fn tick_lines(p: &Probe, from: usize) -> Vec<String> {
        new_lines(&daemon_log(p), from).into_iter().filter(|line| line.contains(" tick ")).map(|line| line.chars().take(200).collect()).collect()
    }

    fn render_laps(p: &Probe, from: usize) -> Vec<u64> {
        new_lines(&daemon_log(p), from).iter().filter(|line| line.contains(" tick ") && line.contains("Rendered")).filter_map(|line| line.split(", ").find_map(|lap| lap.trim().strip_prefix("render ")).and_then(|millis| millis.trim().parse().ok())).collect()
    }

    fn named_laps(p: &Probe, from: usize, name: &str) -> Vec<u64> {
        new_lines(&daemon_log(p), from).iter().filter(|line| line.contains(" tick ") && line.contains("Rendered")).map(|line| line.split(", ").filter_map(|lap| lap.trim().strip_prefix(name)).filter_map(|millis| millis.trim().parse::<u64>().ok()).sum()).collect()
    }

    fn icon_rect(name: &str) -> Option<Rect> {
        finder::named(&finder::view()).ok()?.into_iter().find(|(found, _)| found == name).map(|(_, rect)| rect)
    }

    fn ring(a: &Shot, b: &Shot, icon: &Rect, screen: &Screen) -> usize {
        let scale = screen.scale;
        let (x0, y0) = (((icon.x - screen.frame.x - 40.0) * scale).max(0.0) as usize, ((icon.y - screen.frame.y - 40.0) * scale).max(0.0) as usize);
        let (x1, y1) = ((((icon.x - screen.frame.x + icon.w + 40.0) * scale) as usize).min(a.width), (((icon.y - screen.frame.y + icon.h + 40.0) * scale) as usize).min(a.height));
        let inside = |x: usize, y: usize| {
            let (px, py) = (x as f32 / scale + screen.frame.x, y as f32 / scale + screen.frame.y);
            px >= icon.x && px < icon.x + icon.w && py >= icon.y && py < icon.y + icon.h
        };
        (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).filter(|(x, y)| !inside(*x, *y) && !near(a.at(*x, *y), b.at(*x, *y))).count()
    }

    fn matches_file(shot: &Shot, file: &Path, icons: &[Rect], screen: &Screen) -> Option<f32> {
        let picture = scooch_core::open(file).ok()?;
        let (width, height) = (picture.width() as usize, picture.height() as usize);
        let clear = |x: usize, y: usize| {
            let (px, py) = (x as f32 / screen.scale + screen.frame.x, y as f32 / screen.scale + screen.frame.y);
            !icons.iter().any(|icon| px >= icon.x - 20.0 && px < icon.x + icon.w + 20.0 && py >= icon.y - 20.0 && py < icon.y + icon.h + 20.0)
        };
        let points: Vec<(usize, usize)> = (0..height).step_by(16).flat_map(|y| (0..width).step_by(16).map(move |x| (x, y))).filter(|(x, y)| clear(*x, *y) && *y > 40).collect();
        let same = points.iter().filter(|(x, y)| near(shot.at(*x, *y), picture.get_pixel(*x as u32, *y as u32).0)).count();
        Some(same as f32 / points.len().max(1) as f32)
    }

    fn scenery_check(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.finder = false;
            p.line("- the scenery check needs Apple Events to Finder, which the open consent window blocks, so it was skipped".to_string());
            return Ok(());
        }
        let setting = |p: &Probe| state(p)["options"]["scenery"].as_str().unwrap_or("missing").to_string();
        p.line(format!("- scenery setting in the state file at the start `{}`, the default for a new state", setting(p)));
        if setting(p) == "off" {
            let ran = p.scooch(&["scenery"], LIMIT);
            p.line(format!("- scooch scenery said {:?}", ran.text()));
        }
        let from = (screen.frame.x + screen.frame.w * 0.85, screen.frame.y + screen.frame.h * 0.25);
        let to = (screen.frame.x + screen.frame.w * 0.15, screen.frame.y + screen.frame.h * 0.75);
        away(p, name, from);
        let start = position(p, name).unwrap_or(from);
        let shown = || desktop.wallpaper(&screen).ok().flatten();
        let before = shown();
        let seen = daemon_log(p).lines().count();
        drag(start, to);
        let dropped = Instant::now();
        let on_time = wait_until(Duration::from_secs(12), || shown() != before).map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(1200));
        let icon = icon_rect(name);
        p.line(format!("- scenery on, CGEvent drag from {:.0},{:.0} to calm ground at {:.0},{:.0}: new wallpaper after {on_time:?} ms, the item now at {icon:?}", start.0, start.1, to.0, to.1));
        p.line(format!("  - daemon ticks {:?}", tick_lines(p, seen)));
        let on_shot = grab(display).context("the screen could not be captured with scenery on")?;
        let _ = scooch_core::encode_png(&on_shot.image()).map(|bytes| fs::write(p.out.join("scenery-on.png"), bytes));
        let before = shown();
        let seen = daemon_log(p).lines().count();
        let ran = p.scooch(&["scenery"], LIMIT);
        let switched = Instant::now();
        let off_switch = wait_until(Duration::from_secs(12), || shown() != before).map(|_| switched.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(1200));
        p.line(format!("- scooch scenery said {:?} and the setting is now `{}`, new wallpaper after {off_switch:?} ms", ran.text(), setting(p)));
        p.line(format!("  - daemon ticks {:?}", tick_lines(p, seen)));
        let off_shot = grab(display).context("the screen could not be captured with scenery off")?;
        let _ = scooch_core::encode_png(&off_shot.image()).map(|bytes| fs::write(p.out.join("scenery-off.png"), bytes));
        let painted = icon.as_ref().map(|icon| ring(&on_shot, &off_shot, icon, &screen));
        p.line(format!("- pixels within 40 points around the dragged item that differ between the scenery on and off captures {painted:?}"));
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let current = shown();
        let matched = current.as_deref().and_then(|file| matches_file(&off_shot, file, &icons, &screen));
        p.line(format!("- share of sample points away from the icons where the scenery off capture matches the wallpaper file {current:?} {matched:?}"));
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let back = position(p, name).unwrap_or(to);
        let before = shown();
        let seen = daemon_log(p).lines().count();
        drag(back, subject);
        let dropped = Instant::now();
        let off_time = wait_until(Duration::from_secs(12), || shown() != before).map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(600));
        p.line(format!("- scenery off, CGEvent drag onto the subject at {:.0},{:.0}: new wallpaper after {off_time:?} ms", subject.0, subject.1));
        p.line(format!("  - daemon ticks {:?}", tick_lines(p, seen)));
        let seen = daemon_log(p).lines().count();
        let mut times = Vec::new();
        for _ in 0..5 {
            away(p, name, from);
            let before = shown();
            let moved = Instant::now();
            let _ = place(p, name, subject);
            times.extend(wait_until(Duration::from_secs(12), || shown() != before).map(|_| moved.elapsed().as_millis()));
            std::thread::sleep(Duration::from_millis(400));
        }
        times.sort_unstable();
        p.line(format!("- scenery off, Apple Event moves onto the subject that brought a new wallpaper {} of 5, times {times:?} ms, median {:?}, render laps {:?} ms, to set beside the scenery on moves in section 5", times.len(), times.get(times.len() / 2), render_laps(p, seen)));
        let ran = p.scooch(&["scenery"], LIMIT);
        p.line(format!("- scooch scenery said {:?} and the setting is back to `{}`", ran.text(), setting(p)));
        if on_time.is_none() || off_switch.is_none() || off_time.is_none() {
            p.fail("a scenery step brought no new wallpaper");
        }
        if painted.is_none_or(|count| count < 200) {
            p.fail("no painted scenery showed around the dragged item");
        }
        if matched.is_none_or(|share| share < 0.9) {
            p.fail("the scenery off capture does not match the scenery off render");
        }
        bare_check(p, &screen, display, (to, subject), "navy", navy_picture)?;
        bare_check(p, &screen, display, (to, subject), "green", green_picture)
    }

    fn bare_check(p: &mut Probe, screen: &Screen, display: u32, (calm, onto): ((f32, f32), (f32, f32)), label: &str, picture: fn(u32, u32, (f32, f32)) -> RgbImage) -> Result<()> {
        let desktop = platform::current()?;
        let name = PROBES[0].0;
        let (width, height) = pixels(screen);
        let bare = picture_file(p, &format!("probe-{label}.jpg"), &picture(width, height, (0.5, 0.5)))?;
        let ran = p.scooch(&["use", &bare.to_string_lossy()], Duration::from_secs(60));
        let switched = wait_render(p, Duration::from_secs(45), Some(&bare), false);
        p.line(format!("- scooch use on a {label} picture with scenery `{}` exit {:?}, rendered after {:?} ms", state(p)["options"]["scenery"].as_str().unwrap_or("missing"), ran.code, switched.map(|time| time.as_millis())));
        if !ran.ok() || switched.is_none() {
            p.fail(format!("scooch use on the {label} picture did not render it"));
        }
        away(p, name, onto);
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let held = |p: &Probe| state(p)["screens"].as_object().and_then(|screens| screens.values().next().map(|entry| (entry["placement"].clone(), entry["output"].clone(), entry["belongs"]["answer"].clone())));
        let (kept, shot, seen) = (held(p), grab(display), daemon_log(p).lines().count());
        let _ = p.scooch(&["scenery"], LIMIT);
        quiet_log(p, Duration::from_millis(1500), Duration::from_secs(10));
        let (now, later) = (held(p), grab(display));
        let sets = new_lines(&daemon_log(p), seen).iter().filter(|line| line.split_whitespace().nth(1) == Some("set")).count();
        let moved = shot.as_ref().zip(later.as_ref()).zip(icon_rect(name)).map(|((a, b), icon)| ring(a, b, &icon, screen));
        let _ = p.scooch(&["scenery"], LIMIT);
        quiet_log(p, Duration::from_millis(1500), Duration::from_secs(10));
        let same = kept.as_ref().zip(now.as_ref()).is_some_and(|(a, b)| a.0 == b.0 && a.1 == b.1);
        p.line(format!("- {label} picture with the icon on the subject, scenery auto then off, the daemon's answer belongs {:?}, same placement and file {same}, wallpapers set {sets}, pixels changed around the icon {moved:?}", kept.as_ref().map(|held| held.2.clone())));
        if !same || sets > 0 || moved.is_none_or(|count| count >= 200) {
            p.fail(format!("switching scenery off moved the {label} picture or changed the screen"));
        }
        let start = position(p, name).unwrap_or(onto);
        let shown = || desktop.wallpaper(screen).ok().flatten();
        let before = shown();
        let seen = daemon_log(p).lines().count();
        drag(start, calm);
        let dropped = Instant::now();
        let swapped = wait_until(Duration::from_secs(12), || shown() != before).map(|_| dropped.elapsed().as_millis());
        std::thread::sleep(Duration::from_millis(1200));
        let icon = icon_rect(name);
        let on_shot = grab(display).with_context(|| format!("the screen could not be captured on the {label} picture"))?;
        p.line(format!("- {label} picture, scenery on, drag from the subject to calm ground at {:.0},{:.0}: new wallpaper after {swapped:?} ms, daemon ticks {:?}", calm.0, calm.1, tick_lines(p, seen)));
        let _ = p.scooch(&["scenery"], LIMIT);
        std::thread::sleep(Duration::from_millis(1500));
        let off_shot = grab(display).with_context(|| format!("the screen could not be captured on the {label} picture with scenery off"))?;
        let _ = p.scooch(&["scenery"], LIMIT);
        let painted = icon.as_ref().map(|icon| ring(&on_shot, &off_shot, icon, screen));
        p.line(format!("- pixels within 40 points around the dragged item that differ between scenery on and off on the {label} picture {painted:?}"));
        let seen = daemon_log(p).lines().count();
        let mut swaps = 0;
        for _ in 0..5 {
            away(p, name, calm);
            let before = shown();
            let _ = place(p, name, onto);
            poke(p);
            if wait_until(Duration::from_secs(12), || shown() != before).is_some() {
                swaps += 1;
            }
            std::thread::sleep(Duration::from_millis(400));
        }
        let decodes = named_laps(p, seen, "decode ");
        let mut laps = render_laps(p, seen);
        laps.sort_unstable();
        p.line(format!("- {label} picture, scenery `{}`, Apple Event moves from calm ground onto the subject that brought a new wallpaper {swaps} of 5, render laps {laps:?} ms, median {:?}, decode laps in tick order {decodes:?} ms, to set beside section 5 on a build where section 5 used this picture", state(p)["options"]["scenery"].as_str().unwrap_or("missing"), laps.get(laps.len() / 2)));
        if let Some(subject) = p.subject.clone() {
            let ran = p.scooch(&["use", &subject.to_string_lossy()], Duration::from_secs(60));
            let back = wait_render(p, Duration::from_secs(45), Some(&subject), false);
            p.line(format!("- back to the teal water picture exit {:?}, rendered after {:?} ms", ran.code, back.map(|time| time.as_millis())));
        }
        if swapped.is_none() {
            p.fail(format!("a drag on the {label} picture with scenery on brought no new wallpaper"));
        }
        if painted.is_none_or(|count| count >= 200) {
            p.fail(format!("scenery showed on a {label} picture, where it does not belong"));
        }
        Ok(())
    }

    fn rusage(pid: i32) -> Option<Usage> {
        let mut usage = Usage::default();
        (unsafe { proc_pid_rusage(pid, 2, &mut usage) } == 0).then_some(usage)
    }

    fn nanos(ticks: u64) -> u64 {
        let mut base = Timebase::default();
        unsafe { mach_timebase_info(&mut base) };
        if base.denom == 0 {
            return ticks;
        }
        ticks.saturating_mul(base.numer as u64) / base.denom as u64
    }

    fn threads(pid: i32) -> Vec<(u64, String, i32, u64)> {
        let mut handles = vec![0u64; 512];
        let bytes = unsafe { proc_pidinfo(pid, 6, 0, handles.as_mut_ptr() as *mut c_void, (handles.len() * 8) as i32) };
        let count = (bytes.max(0) as usize / 8).min(handles.len());
        let mut found = Vec::new();
        for handle in &handles[..count] {
            let mut info: ThreadInfo = unsafe { std::mem::zeroed() };
            let size = std::mem::size_of::<ThreadInfo>() as i32;
            if unsafe { proc_pidinfo(pid, 5, *handle, &mut info as *mut ThreadInfo as *mut c_void, size) } == size {
                let end = info.name.iter().position(|byte| *byte == 0).unwrap_or(info.name.len());
                let name = String::from_utf8_lossy(&info.name[..end]).into_owned();
                found.push((*handle, if name.is_empty() { "unnamed".to_string() } else { name }, info.run_state, info.user + info.system));
            }
        }
        found
    }

    fn stacks(text: &str) -> Vec<(u64, Vec<String>)> {
        let mut found = Vec::new();
        let mut frames: Vec<String> = Vec::new();
        for line in text.lines().map(str::trim).filter(|line| !line.is_empty()) {
            match line.parse::<u64>() {
                Ok(count) if !frames.is_empty() => found.push((count, std::mem::take(&mut frames))),
                Ok(_) => frames.clear(),
                Err(_) => frames.push(line.to_string()),
            }
        }
        found.sort_by_key(|stack| std::cmp::Reverse(stack.0));
        found
    }

    fn quiet_minute(p: &mut Probe, pid: i32, label: &str) -> Result<(u64, u64)> {
        let age = p.tool("/bin/ps", &["-o", "etime=", "-p", &pid.to_string()]).out.trim().to_string();
        let seen = daemon_log(p).lines().count();
        let traced = p.out.join(format!("oncpu-{pid}-{}.txt", now()));
        let script = format!("sched:::on-cpu /pid == {pid}/ {{ @[ustack(32)] = count(); }} tick-60s {{ exit(0); }}");
        let dtrace = Command::new("/usr/bin/sudo").args(["-n", "/usr/sbin/dtrace", "-q", "-x", "ustackframes=32", "-o", &traced.to_string_lossy(), "-n", &script]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).spawn();
        let threads_before = threads(pid);
        let before = rusage(pid).context("the daemon usage could not be read")?;
        let from = epoch();
        let mut buckets = Vec::new();
        let mut last = before;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_secs(5));
            if let Some(now) = rusage(pid) {
                buckets.push(now.idle_wakeups.saturating_sub(last.idle_wakeups));
                last = now;
            }
        }
        let to = epoch();
        let after = rusage(pid).context("the daemon usage could not be read after the quiet minute")?;
        let threads_after = threads(pid);
        if let Ok(mut child) = dtrace {
            let _ = wait_until(Duration::from_secs(15), || child.try_wait().ok().flatten().is_some());
            let _ = child.kill();
            let _ = child.wait();
        }
        let cpu = nanos((after.user + after.system).saturating_sub(before.user + before.system)) / 1_000_000;
        p.line(format!(
            "- {label} pid {pid}, running for {age} when the minute began, over a quiet 60 s: CPU {cpu} ms, idle wakeups {}, interrupt wakeups {}, footprint {:.1} MB",
            after.idle_wakeups.saturating_sub(before.idle_wakeups),
            after.interrupt_wakeups.saturating_sub(before.interrupt_wakeups),
            after.footprint as f64 / 1_048_576.0
        ));
        p.line(format!("- idle wakeups in each 5 s of that minute {buckets:?}"));
        let quiet = new_lines(&daemon_log(p), seen);
        p.line(format!("- the daemon logged {} ticks and wakes {:?}", quiet.iter().filter(|line| line.contains(" tick ")).count(), quiet.iter().filter_map(|line| line.split(" wake ").nth(1).map(str::trim)).collect::<Vec<_>>()));
        let busy: Vec<String> = threads_after
            .iter()
            .filter_map(|(handle, name, _, total)| {
                let earlier = threads_before.iter().find(|(seen, _, _, _)| seen == handle).map(|(_, _, _, total)| *total)?;
                let spent = total.saturating_sub(earlier) / 1000;
                (spent > 0).then(|| format!("{name} {spent} us"))
            })
            .collect();
        p.line(format!("- {} threads, CPU by thread during the minute {busy:?}", threads_after.len()));
        let stacks = stacks(&fs::read_to_string(&traced).unwrap_or_default());
        p.line(format!("- dtrace on-cpu stacks during the minute {} distinct, {} on-cpu events, kept as {}", stacks.len(), stacks.iter().map(|(count, _)| count).sum::<u64>(), traced.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
        for (count, frames) in stacks.iter().take(8) {
            let shown: Vec<String> = frames.iter().map(|frame| frame.split('+').next().unwrap_or(frame).to_string()).filter(|frame| !frame.starts_with("libsystem_kernel") && !frame.starts_with("scooch`0x")).take(9).collect();
            p.line(format!("  - {count} times: {}", shown.join(" < ")));
        }
        let logged = p.tool("/usr/bin/log", &["show", "--last", "100s", "--info", "--debug", "--style", "ndjson", "--predicate", &format!("processIdentifier == {pid}")]);
        let mut inside = Vec::new();
        for line in logged.out.lines() {
            let Ok(entry) = serde_json::from_str::<serde_json::Value>(line) else { continue };
            let stamp = entry["timestamp"].as_str().unwrap_or("");
            let when = chrono_free_epoch(stamp);
            if when >= from && when <= to {
                inside.push(format!("+{:.1} s {} {} {}: {}", when - from, entry["senderImagePath"].as_str().and_then(|path| path.rsplit('/').next()).unwrap_or("?"), entry["subsystem"].as_str().unwrap_or(""), entry["category"].as_str().unwrap_or(""), entry["eventMessage"].as_str().unwrap_or("").chars().take(110).collect::<String>()));
            }
        }
        p.line(format!("- unified log entries from the daemon inside the minute {}", inside.len()));
        for entry in inside.iter().take(15) {
            p.line(format!("  - `{entry}`"));
        }
        let file = p.out.join(format!("sample-{pid}-{}.txt", now()));
        let sampled = p.tool("/usr/bin/sample", &[&pid.to_string(), "5", "-file", &file.to_string_lossy()]);
        let text = fs::read_to_string(&file).unwrap_or_default();
        let tops: Vec<String> = text.lines().skip_while(|line| !line.starts_with("Sort by top of stack")).skip(1).take_while(|line| !line.trim().is_empty()).map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
        p.line(format!("- sample 5 s after the minute {}, leaf frames {tops:?}", if sampled.ok() { "ok" } else { "failed" }));
        Ok((after.idle_wakeups.saturating_sub(before.idle_wakeups), after.interrupt_wakeups.saturating_sub(before.interrupt_wakeups)))
    }

    fn watch_log(path: &Path) -> Vec<String> {
        fs::read_to_string(path).unwrap_or_default().lines().map(str::to_string).collect()
    }

    fn program_lines(path: &Path, from: usize) -> Vec<String> {
        watch_log(path).into_iter().skip(from).filter(|line| line.contains(" program ") && !line.contains(" program event flags ")).collect()
    }

    fn logged_within(path: &Path, from: usize, started: Instant, limit: Duration, wanted: impl Fn(&str) -> bool) -> Option<u128> {
        wait_until(limit, || watch_log(path).iter().skip(from).any(|line| wanted(line))).map(|_| started.elapsed().as_millis())
    }

    fn make_bundle(bundle: &Path, bin: &Path) -> Result<PathBuf> {
        let folder = bundle.join("Contents/MacOS");
        fs::create_dir_all(&folder)?;
        let exe = folder.join("scooch");
        fs::copy(bin, &exe)?;
        fs::write(
            bundle.join("Contents/Info.plist"),
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\">\n<dict>\n<key>CFBundleExecutable</key><string>scooch</string>\n<key>CFBundleIdentifier</key><string>me.sohanbhat.scooch.probe</string>\n<key>CFBundlePackageType</key><string>APPL</string>\n</dict>\n</plist>\n",
        )?;
        Ok(exe)
    }

    fn start_watched(exe: &Path, log: &Path, places: &str) -> Result<Child> {
        let file = fs::OpenOptions::new().create(true).append(true).open(log)?;
        Ok(Command::new(exe).arg("watch").env_remove("SCOOCH_HOME").env("SCOOCH_TRACE", "1").env("SCOOCH_LINK_PLACES", places).stdin(Stdio::null()).stdout(Stdio::null()).stderr(file).spawn()?)
    }

    fn stop_watched(child: &mut Child) {
        unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
        if wait_until(Duration::from_secs(10), || child.try_wait().ok().flatten().is_some()).is_none() {
            let _ = child.kill();
        }
        let _ = child.wait();
    }

    fn running(child: &mut Child) -> bool {
        child.try_wait().ok().flatten().is_none()
    }

    fn names(plist: &Path, exe: &Path) -> bool {
        fs::read_to_string(plist).unwrap_or_default().contains(&format!("<string>{}</string>", exe.display()))
    }

    fn names_soon(plist: &Path, exe: &Path) -> bool {
        wait_until(Duration::from_secs(2), || names(plist, exe)).is_some()
    }

    fn ramdisk(p: &mut Probe, name: &str) -> Result<(String, PathBuf)> {
        let attached = p.tool("/usr/bin/hdiutil", &["attach", "-nomount", "ram://262144"]);
        let device = attached.out.split_whitespace().next().unwrap_or_default().to_string();
        anyhow::ensure!(!device.is_empty(), "hdiutil made no ramdisk, {}", attached.text());
        let erased = p.tool("/usr/sbin/diskutil", &["erasevolume", "HFS+", name, &device]);
        if !erased.ok() {
            let _ = p.tool("/usr/bin/hdiutil", &["detach", &device, "-force"]);
            anyhow::bail!("diskutil could not make a volume on {device}, {}", erased.text());
        }
        Ok((device, PathBuf::from("/Volumes").join(name)))
    }

    struct Watching {
        local: PathBuf,
        bin: PathBuf,
        child: Option<Child>,
        exe: PathBuf,
        bundle: PathBuf,
        log: PathBuf,
        plist: PathBuf,
        real: PathBuf,
    }

    fn alive(run: &mut Watching) -> bool {
        run.child.as_mut().is_some_and(running)
    }

    fn halt(run: &mut Watching) {
        if let Some(mut child) = run.child.take() {
            stop_watched(&mut child);
        }
    }

    fn ensure_running(p: &mut Probe, run: &mut Watching) -> Result<()> {
        if !run.exe.is_file() {
            halt(run);
            make_bundle(&run.bundle, &p.bin)?;
            p.line(format!("- made {} again for the next case", run.bundle.display()));
        }
        if !names(&run.plist, &run.exe) {
            fs::write(&run.plist, launchd::plist(&run.exe, &run.real, None, true))?;
        }
        if alive(run) {
            return Ok(());
        }
        let state = run.real.join("state.json");
        if !fs::read_to_string(&state).unwrap_or_default().contains("\"on\": true") {
            fs::write(&state, "{\"on\": true}\n")?;
        }
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        run.child = Some(start_watched(&run.exe, &run.log, &link_places(run))?);
        let armed = logged_within(&run.log, from, started, Duration::from_secs(15), |line| line.contains(" program watched at "));
        p.line(format!("- started the daemon from {}, watch armed after {armed:?} ms", run.exe.display()));
        armed.map(drop).context("the daemon made no watch on its program")
    }

    fn restart(p: &mut Probe, run: &mut Watching) -> Result<()> {
        halt(run);
        ensure_running(p, run)
    }

    fn report_case(p: &mut Probe, name: &str, result: Result<()>) {
        if let Err(error) = result {
            p.fail(format!("the {name} case stopped on an error of the probe's own, {error:#}"));
        }
    }

    fn quiet_case(p: &mut Probe, run: &mut Watching) -> Result<()> {
        ensure_running(p, run)?;
        let pid = run.child.as_ref().map(|child| child.id() as i32).context("no daemon runs")?;
        let started = Instant::now();
        let rested = logged_within(&run.log, 0, started, Duration::from_secs(45), |line| line.contains(" sessions let go of "));
        std::thread::sleep(Duration::from_secs(3));
        let from = watch_log(&run.log).len();
        let threads_before = threads(pid);
        let calm = rusage(pid).context("the daemon usage could not be read")?;
        let mut buckets = Vec::new();
        let mut last = calm;
        for _ in 0..12 {
            std::thread::sleep(Duration::from_secs(5));
            if let Some(now) = rusage(pid) {
                buckets.push(now.idle_wakeups.saturating_sub(last.idle_wakeups));
                last = now;
            }
        }
        let later = rusage(pid).context("the daemon usage could not be read after the minute")?;
        let threads_after = threads(pid);
        let woke = (later.idle_wakeups.saturating_sub(calm.idle_wakeups), later.interrupt_wakeups.saturating_sub(calm.interrupt_wakeups));
        let lines: Vec<String> = watch_log(&run.log).into_iter().skip(from).collect();
        let events = lines.iter().filter(|line| line.contains(" program event flags ")).count();
        let busy: Vec<String> = threads_after
            .iter()
            .filter_map(|(handle, name, _, total)| {
                let earlier = threads_before.iter().find(|(seen, _, _, _)| seen == handle).map(|(_, _, _, total)| *total)?;
                let spent = total.saturating_sub(earlier) / 1000;
                (spent > 0).then(|| format!("{name} {spent} us"))
            })
            .collect();
        p.line(format!(
            "- a quiet 60 s with the program watched, begun 3 s after the daemon let its render sessions go {rested:?} ms into this case: idle and interrupt wakeups {woke:?}, the same minute of check 6 without the watch {:?}, idle wakeups in each 5 s {buckets:?}, kevent returns {events}, other log lines {lines:?}, CPU by thread {busy:?}",
            p.quiet
        ));
        if crate::woke_more(woke, p.quiet, events) {
            p.fail("the daemon with its program watched woke more at rest than without the watch");
        }
        Ok(())
    }

    fn link_places(run: &Watching) -> String {
        format!("{}:{}", run.local.display(), run.bin.display())
    }

    fn place_links(run: &Watching) -> Result<()> {
        fs::create_dir_all(&run.local)?;
        fs::create_dir_all(&run.bin)?;
        let link = run.local.join("scooch");
        let _ = fs::remove_file(&link);
        std::os::unix::fs::symlink(&run.exe, &link)?;
        fs::write(run.bin.join("scooch"), b"a program of its own")?;
        let foreign = run.bin.join("foreign");
        if fs::symlink_metadata(&foreign).is_err() {
            std::os::unix::fs::symlink("/usr/bin/true", &foreign)?;
        }
        Ok(())
    }

    fn others_stay(run: &Watching) -> bool {
        fs::read(run.bin.join("scooch")).is_ok_and(|bytes| bytes == b"a program of its own") && fs::read_link(run.bin.join("foreign")).is_ok_and(|points| points == Path::new("/usr/bin/true"))
    }

    fn move_case(p: &mut Probe, run: &mut Watching) -> Result<()> {
        ensure_running(p, run)?;
        place_links(run)?;
        let link = run.local.join("scooch");
        let aside = dirs::home_dir().context("no home folder")?.join("ScoochMoved");
        fs::create_dir_all(&aside)?;
        if !p.made.contains(&aside) {
            p.made.push(aside.clone());
        }
        let moved = aside.join("ScoochWatch.app");
        let moved_exe = moved.join("Contents/MacOS/scooch");
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        fs::rename(&run.bundle, &moved)?;
        let away = logged_within(&run.log, from, started, Duration::from_secs(25), |line| line.contains(" program moved from ") && line.contains("ScoochMoved"));
        let followed = names_soon(&run.plist, &moved_exe);
        let linked = fs::read_link(&link).ok();
        let said = program_lines(&run.log, from);
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        fs::rename(&moved, &run.bundle)?;
        let home_again = logged_within(&run.log, from, started, Duration::from_secs(25), |line| line.contains(" program moved from ") && line.contains("ScoochMoved"));
        let followed_back = names_soon(&run.plist, &run.exe);
        let linked_back = fs::read_link(&link).ok();
        let stayed = others_stay(run);
        let link_followed = linked.as_deref() == Some(moved_exe.as_path()) && linked_back.as_deref() == Some(run.exe.as_path());
        p.line(format!("- the link {} in a folder standing in for ~/.local/bin pointed at {linked:?} after the move and at {linked_back:?} after the move back, a regular file and a foreign link beside it untouched {stayed}", link.display()));
        if !link_followed || !stayed {
            p.fail("a move of the program did not take the link with it, or touched a file it must leave alone");
        }
        let alive = alive(run);
        p.line(format!("- a move to {}: decided after {away:?} ms, the login item names the new place {followed}, then back: decided after {home_again:?} ms, names the old place again {followed_back}, daemon alive {alive}, log {said:?} then {:?}", moved.display(), program_lines(&run.log, from)));
        if away.is_none() || !followed || home_again.is_none() || !followed_back || !alive {
            p.fail("a move of the program did not point the login item at the new place");
        }
        Ok(())
    }

    fn replace_case(p: &mut Probe, run: &mut Watching) -> Result<()> {
        ensure_running(p, run)?;
        let old = run.bundle.with_file_name(".ScoochWatch-old.app");
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        fs::rename(&run.bundle, &old)?;
        if !p.made.contains(&old) {
            p.made.push(old.clone());
        }
        make_bundle(&run.bundle, &p.bin)?;
        let replaced = logged_within(&run.log, from, started, Duration::from_secs(25), |line| line.contains(" program at ") && line.contains("is back at its old place"));
        let mark = watch_log(&run.log).iter().position(|line| line.contains("is back at its old place")).unwrap_or(from);
        let target = run.bundle.to_string_lossy().into_owned();
        let rearmed = logged_within(&run.log, mark, started, Duration::from_secs(5), |line| line.contains(" program watched at ") && line.contains(target.as_str()));
        let said = program_lines(&run.log, from);
        let acted = said.iter().any(|line| line.contains("moved to the Trash") || line.contains("moved from"));
        let alive = alive(run);
        let kept = names(&run.plist, &run.exe);
        p.line(format!("- replaced in place, the old app moved aside and a new one copied to its place: judged nothing after {replaced:?} ms, the watch armed again on the new copy after {rearmed:?} ms, acted anyway {acted}, daemon alive {alive}, login item unchanged {kept}, log {said:?}"));
        if replaced.is_none() || rearmed.is_none() || acted || !alive || !kept {
            p.fail("replacing the program in place changed Scooch or left it without a watch");
        }
        let _ = fs::remove_dir_all(&old);
        Ok(())
    }

    fn delete_case(p: &mut Probe, run: &mut Watching) -> Result<()> {
        let mark = watch_log(&run.log).len();
        ensure_running(p, run)?;
        let pid = run.child.as_ref().map(|child| child.id() as i32).context("no daemon runs")?;
        let rested = logged_within(&run.log, mark, Instant::now(), Duration::from_secs(45), |line| line.contains(" sessions let go of "));
        std::thread::sleep(Duration::from_secs(3));
        let aside = run.bundle.with_file_name(".ScoochWatch-kept.app");
        let _ = fs::remove_dir_all(&aside);
        let copied = p.tool("/usr/bin/ditto", &[&run.bundle.to_string_lossy(), &aside.to_string_lossy()]);
        anyhow::ensure!(copied.ok(), "ditto could not keep a copy of the app, {}", copied.text());
        if !p.made.contains(&aside) {
            p.made.push(aside.clone());
        }
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        fs::remove_dir_all(&run.bundle)?;
        let deleted = logged_within(&run.log, from, started, Duration::from_secs(10), |line| line.contains(" was deleted, so nothing changes"));
        let waiting = logged_within(&run.log, from, started, Duration::from_secs(10), |line| line.contains(" program folder watched at "));
        std::thread::sleep(Duration::from_secs(3));
        let quiet_from = watch_log(&run.log).len();
        let calm = rusage(pid).context("the daemon usage could not be read")?;
        std::thread::sleep(Duration::from_secs(60));
        let later = rusage(pid).context("the daemon usage could not be read after the minute")?;
        let woke = (later.idle_wakeups.saturating_sub(calm.idle_wakeups), later.interrupt_wakeups.saturating_sub(calm.interrupt_wakeups));
        let quiet_lines: Vec<String> = watch_log(&run.log).into_iter().skip(quiet_from).collect();
        let folder_events = quiet_lines.iter().filter(|line| line.contains(" program folder event ") || line.contains(" program event flags ")).count();
        let mark = watch_log(&run.log).len();
        let started = Instant::now();
        let back = p.tool("/usr/bin/ditto", &[&aside.to_string_lossy(), &run.bundle.to_string_lossy()]);
        let target = run.bundle.to_string_lossy().into_owned();
        let rearmed = logged_within(&run.log, mark, started, Duration::from_secs(10), |line| line.contains(" program watched at ") && line.contains(target.as_str()));
        let alive = alive(run);
        p.line(format!(
            "- the app deleted {rested:?} ms after the restarted daemon let its render sessions go: judged nothing after {deleted:?} ms, its folder watched after {waiting:?} ms, a quiet 60 s with only the folder watched: idle and interrupt wakeups {woke:?}, folder events {folder_events}, other log lines {quiet_lines:?}, the same minute of check 6 without any watch {:?}, the app copied back exit {:?} and the watch armed on it again after {rearmed:?} ms, daemon alive {alive}, log {:?}",
            p.quiet,
            back.code,
            program_lines(&run.log, from)
        ));
        if deleted.is_none() || waiting.is_none() || rearmed.is_none() || !alive {
            p.fail("after a delete the watch did not wait on the folder and arm again on the program put back");
        }
        if crate::woke_more(woke, p.quiet, folder_events) {
            p.fail("the daemon woke more at rest with the folder watched than without any watch");
        }
        let _ = fs::remove_dir_all(&aside);
        Ok(())
    }

    fn ramdisk_copy(p: &mut Probe, run: &mut Watching, desktop: &dyn platform::Desktop, screen: &Screen, volume: &Path) -> Result<()> {
        let copy = volume.join("ScoochWatch.app");
        let copied = p.tool("/usr/bin/ditto", &[&run.bundle.to_string_lossy(), &copy.to_string_lossy()]);
        anyhow::ensure!(copied.ok(), "ditto could not copy the app to the ramdisk, {}", copied.text());
        let showing = desktop.wallpaper(screen).ok().flatten();
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        fs::remove_dir_all(&run.bundle)?;
        let deleted = logged_within(&run.log, from, started, Duration::from_secs(10), |line| line.contains(" was deleted, so nothing changes"));
        std::thread::sleep(Duration::from_secs(12));
        let said = program_lines(&run.log, from);
        let acted = said.iter().any(|line| line.contains("moved to the Trash") || line.contains("moved from"));
        let alive = alive(run);
        let kept = names(&run.plist, &run.exe);
        let now = desktop.wallpaper(screen).ok().flatten();
        let ours = now.as_deref().is_some_and(|path| path.starts_with(run.real.join("out")));
        let on = fs::read_to_string(run.real.join("state.json")).unwrap_or_default().contains("\"on\": true");
        p.line(format!("- copied to a ramdisk at {} and the original deleted: judged nothing after {deleted:?} ms, acted anyway {acted}, daemon alive {alive}, login item unchanged {kept}, wallpaper before the delete {showing:?} and 12 s after {now:?}, still a picture Scooch made {ours}, saved state still on {on}, log {said:?}", copy.display()));
        if deleted.is_none() || acted || !alive || !kept || !ours || !on {
            p.fail("a copy to another disk and a delete of the original changed Scooch");
        }
        let back = p.tool("/usr/bin/ditto", &[&copy.to_string_lossy(), &run.bundle.to_string_lossy()]);
        p.line(format!("- copied the app back from the ramdisk exit {:?}", back.code));
        Ok(())
    }

    fn ramdisk_case(p: &mut Probe, run: &mut Watching, desktop: &dyn platform::Desktop, screen: &Screen) -> Result<()> {
        ensure_running(p, run)?;
        let (device, volume) = ramdisk(p, "ScoochCopy")?;
        let result = ramdisk_copy(p, run, desktop, screen, &volume);
        let _ = p.tool("/usr/bin/hdiutil", &["detach", &device, "-force"]);
        let restarted = restart(p, run);
        result.and(restarted)
    }

    fn put_back_on(p: &mut Probe, run: &mut Watching, volume: &Path) -> Result<()> {
        let app = volume.join("ScoochWatch.app");
        let exe = make_bundle(&app, &p.bin)?;
        let trash = volume.join(".Trashes").join(unsafe { libc::getuid() }.to_string());
        fs::create_dir_all(&trash)?;
        fs::write(&run.plist, launchd::plist(&exe, &run.real, None, true))?;
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        let mut child = start_watched(&exe, &run.log, &link_places(run))?;
        let mark = volume.to_string_lossy().into_owned();
        let armed = logged_within(&run.log, from, started, Duration::from_secs(15), |line| line.contains(" program watched at ") && line.contains(mark.as_str()));
        if armed.is_none() {
            stop_watched(&mut child);
            anyhow::bail!("the daemon run from the ramdisk made no watch on its program");
        }
        let inside = trash.join("ScoochWatch.app");
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        fs::rename(&app, &inside)?;
        std::thread::sleep(Duration::from_millis(1500));
        let moved_back = fs::rename(&inside, &app);
        let returned = moved_back.is_ok().then(|| started.elapsed().as_millis());
        let calmed = wait_until(Duration::from_secs(30), || {
            let said = program_lines(&run.log, from);
            said.iter().position(|line| line.contains("nothing changes")).is_some_and(|at| said.iter().skip(at).any(|line| line.contains(" program watched at ") && line.contains(mark.as_str())))
        })
        .map(|_| started.elapsed().as_millis());
        let said = program_lines(&run.log, from);
        let off = said.iter().any(|line| line.contains("moved to the Trash"));
        let alive = running(&mut child);
        let kept = names(&run.plist, &exe);
        p.line(format!("- Put Back inside 5 s on a ramdisk, the app moved into {} and back after {returned:?} ms ({moved_back:?}): judged nothing and the watch armed again on the app after {calmed:?} ms, turned off {off}, daemon alive {alive}, login item unchanged {kept}, log {said:?}", inside.display()));
        if returned.is_none_or(|ms| ms > 5_000) || calmed.is_none() || off || !alive || !kept {
            p.fail("a Put Back inside 5 s did not leave Scooch as it was");
        }
        stop_watched(&mut child);
        Ok(())
    }

    fn put_back_case(p: &mut Probe, run: &mut Watching) -> Result<()> {
        halt(run);
        let (device, volume) = ramdisk(p, "ScoochPut")?;
        let result = put_back_on(p, run, &volume);
        let _ = p.tool("/usr/bin/hdiutil", &["detach", &device, "-force"]);
        let restored = ensure_running(p, run);
        result.and(restored)
    }

    fn unreached_record(run: &Watching) -> Result<PathBuf> {
        let file = run.real.join("state.json");
        let mut state: serde_json::Value = serde_json::from_slice(&fs::read(&file)?)?;
        let output = run.real.join("out").join("probe-unreached-1.jpg");
        fs::create_dir_all(run.real.join("out"))?;
        let picture = PathBuf::from("/System/Library/Desktop Pictures/Solid Colors/Black.png");
        fs::write(&output, b"a picture a screen that is gone still shows")?;
        state["screens"]["probe-unreached"] = serde_json::json!({"source": picture, "original": picture, "output": output});
        fs::write(&file, serde_json::to_vec_pretty(&state)?)?;
        Ok(output)
    }

    fn trash_case(p: &mut Probe, run: &mut Watching, desktop: &dyn platform::Desktop, screen: &Screen, before: Option<&Path>) -> Result<()> {
        ensure_running(p, run)?;
        place_links(run)?;
        halt(run);
        let output = unreached_record(run)?;
        ensure_running(p, run)?;
        std::thread::sleep(Duration::from_secs(3));
        let recorded = fs::read_to_string(run.real.join("state.json")).unwrap_or_default().contains("probe-unreached");
        let shown = desktop.wallpaper(screen).ok().flatten();
        let from = watch_log(&run.log).len();
        let started = Instant::now();
        let trashed = p.osa(&format!("tell application \"Finder\" to delete (POSIX file \"{}\" as alias)", run.bundle.display()));
        let decided = logged_within(&run.log, from, started, Duration::from_secs(25), |line| line.contains(" program moved to the Trash at "));
        let turned = logged_within(&run.log, from, started, Duration::from_secs(25), |line| line.contains(" program in the Trash, Scooch turned off"));
        let removed = wait_until(Duration::from_secs(10), || !run.plist.exists()).map(|_| started.elapsed().as_millis());
        let said_removed = logged_within(&run.log, from, started, Duration::from_secs(5), |line| line.contains(" program in the Trash, the login item removed"));
        std::thread::sleep(Duration::from_secs(2));
        let staying = alive(run);
        let state = fs::read_to_string(run.real.join("state.json")).unwrap_or_default();
        let restoring = state.contains("\"restoring\": true") && state.contains("probe-unreached");
        let after = desktop.wallpaper(screen).ok().flatten();
        let restored = after.as_deref() == before;
        p.line(format!(
            "- moved to the Trash through Finder with a record of a screen probe-unreached that no display shows, recorded {recorded}, exit {:?}: decided after {decided:?} ms, turned off after {turned:?} ms, the login item file was gone after {removed:?} ms and said so after {said_removed:?} ms, the daemon stays to put back the screen that is gone {staying}, saved state restoring that record {restoring}, wallpaper before the Trash {shown:?}, after {after:?}, the one before Scooch {before:?}, back {restored}, log {:?}",
            trashed.code,
            program_lines(&run.log, from)
        ));
        if trashed.code != Some(0) || decided.is_none() || turned.is_none() || removed.is_none() || !staying || !restoring || !restored {
            p.fail("moving the program to the Trash with a screen it could not reach did not remove the login item at once, keep the daemon for that screen and put the wallpaper back");
        }
        let mark = watch_log(&run.log).len();
        halt(run);
        let state: serde_json::Value = serde_json::from_slice(&fs::read(run.real.join("state.json")).unwrap_or_default()).unwrap_or(serde_json::Value::Null);
        let parked = state["restoring"] != serde_json::Value::Bool(true) && state["on"] != serde_json::Value::Bool(true) && state["screens"]["probe-unreached"].is_object();
        p.line(format!("- the daemon stopped the way a logout stops it: state parked with the record kept {parked}, the output still there {}, plist {}, log {:?}", output.exists(), run.plist.exists(), watch_log(&run.log).into_iter().skip(mark).filter(|line| line.contains("parked")).collect::<Vec<_>>()));
        if !parked || run.plist.exists() {
            p.fail("a daemon that ended in an orderly way with a screen still pending and no login item did not leave the state parked");
        }
        let _ = fs::write(run.real.join("state.json"), "{\"on\": false}\n");
        let _ = fs::remove_file(&output);
        let link_gone = fs::symlink_metadata(run.local.join("scooch")).is_err();
        let stayed = others_stay(run);
        p.line(format!("- the link into the app in the folder standing in for ~/.local/bin removed {link_gone}, a regular file and a foreign link beside it untouched {stayed}"));
        if !link_gone || !stayed {
            p.fail("the Trash did not remove the link into the app, or removed a file it must leave alone");
        }
        Ok(())
    }

    fn program_moved(p: &mut Probe) -> Result<()> {
        if !scooch_processes(p).is_empty() || launchd::loaded() || launchd::plist_path().is_ok_and(|path| path.exists()) {
            let ran = p.scooch(&["off"], Duration::from_secs(60));
            p.line(format!("- scooch off before these checks exit {:?}: {}", ran.code, ran.text()));
            reap(p);
        }
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let home = dirs::home_dir().context("no home folder")?;
        let real = dirs::data_local_dir().context("no data folder")?.join("Scooch");
        let mut bundle = PathBuf::from("/Applications/ScoochWatch.app");
        if make_bundle(&bundle, &p.bin).is_err() {
            bundle = home.join("Applications/ScoochWatch.app");
            make_bundle(&bundle, &p.bin)?;
        }
        p.made.push(bundle.clone());
        fs::create_dir_all(&real)?;
        p.made.push(real.clone());
        fs::write(real.join("state.json"), "{\"on\": true}\n")?;
        let before = desktop.wallpaper(&screen).ok().flatten();
        let exe = bundle.join("Contents/MacOS/scooch");
        let links = std::env::temp_dir().join(format!("scooch-links-{}", std::process::id()));
        p.made.push(links.clone());
        let mut run = Watching { local: links.join("local"), bin: links.join("bin"), child: None, exe, bundle, log: p.out.join("program-watch.log"), plist: launchd::plist_path()?, real: real.clone() };
        let started = Instant::now();
        if let Err(error) = ensure_running(p, &mut run) {
            p.fail(format!("the daemon run from an app folder made no watch on its program, {error:#}"));
            halt(&mut run);
            let _ = fs::remove_file(&run.plist);
            return Ok(());
        }
        let ticked = logged_within(&run.log, 0, started, Duration::from_secs(20), |line| line.contains(" tick "));
        let showing = desktop.wallpaper(&screen).ok().flatten();
        let ours = showing.as_deref().is_some_and(|path| path.starts_with(real.join("out")));
        p.line(format!("- the daemon run from {} with the real data folder and a login item file naming it, first tick after {ticked:?} ms, wallpaper before {before:?}, now {showing:?}, Scooch's own picture showing {ours}", run.exe.display()));
        let result = quiet_case(p, &mut run);
        report_case(p, "quiet minute", result);
        let result = move_case(p, &mut run);
        report_case(p, "move", result);
        let result = ramdisk_case(p, &mut run, desktop.as_ref(), &screen);
        report_case(p, "copy to a ramdisk", result);
        let result = put_back_case(p, &mut run);
        report_case(p, "Put Back", result);
        let result = delete_case(p, &mut run);
        report_case(p, "delete and copy back", result);
        let result = replace_case(p, &mut run);
        report_case(p, "replace in place", result);
        let result = trash_case(p, &mut run, desktop.as_ref(), &screen, before.as_deref());
        report_case(p, "Trash through Finder", result);
        halt(&mut run);
        let _ = fs::remove_file(&run.plist);
        p.line(format!("- the trashed app stays in the runner's Trash, the daemon's log is in the artifact as {}", run.log.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
        Ok(())
    }

    fn chrono_free_epoch(stamp: &str) -> f64 {
        let parse = || -> Option<f64> {
            let (date, rest) = stamp.split_once(' ')?;
            let mut parts = date.split('-').map(|part| part.parse::<i64>().ok());
            let (year, month, day) = (parts.next()??, parts.next()??, parts.next()??);
            let clock = rest.get(..15)?;
            let mut time = clock.split(':');
            let (hour, minute) = (time.next()?.parse::<i64>().ok()?, time.next()?.parse::<i64>().ok()?);
            let second = time.next()?.parse::<f64>().ok()?;
            let offset = rest.get(15..)?;
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let offset_minutes = sign * (offset.get(1..3)?.parse::<i64>().ok()? * 60 + offset.get(3..5)?.parse::<i64>().ok()?);
            let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
            let days = 365 * y + y / 4 - y / 100 + y / 400 + (153 * m + 2) / 5 + day - 719469;
            Some((days * 86400 + hour * 3600 + minute * 60 - offset_minutes * 60) as f64 + second)
        };
        parse().unwrap_or(0.0)
    }

    fn idle(p: &mut Probe) -> Result<()> {
        std::thread::sleep(Duration::from_secs(3));
        let pid = scooch_pid(p).context("no scooch daemon runs")?;
        let label = if p.stand.is_some() { "the probe started daemon" } else { "the launchd daemon" };
        let ps = p.tool("/bin/ps", &["-o", "cputime=,rss=,ppid=", "-p", &pid.to_string()]);
        p.line(format!("- ps cputime, rss and parent {}", ps.out.trim()));
        p.quiet = Some(quiet_minute(p, pid, label)?);
        Ok(())
    }

    fn gone(p: &mut Probe, what: &str) {
        let plist = launchd::plist_path().map(|path| path.exists()).unwrap_or(true);
        let loaded = launchd::loaded();
        let left = scooch_processes(p);
        p.line(format!("- after {what}: login item file present {plist}, launchctl print finds the job {loaded}, scooch processes {left:?}"));
        if plist || loaded || !left.is_empty() {
            p.fail(format!("something of Scooch was left after {what}"));
        }
    }

    fn off(p: &mut Probe) -> Result<()> {
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        p.line(format!("- scooch off exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        std::thread::sleep(Duration::from_millis(1500));
        reap(p);
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let kind = format!("{:?}", store::describe(&screen.id));
        let same_path = shown.as_ref().zip(p.subject.as_ref()).is_some_and(|(shown, subject)| shown == subject || shown.canonicalize().ok() == subject.canonicalize().ok());
        let same_kind = p.kind_before.as_deref() == Some(kind.as_str());
        p.line(format!("- wallpaper now {shown:?}, back to the test JPEG {same_path}; store kind {kind}, same as before {same_kind}"));
        p.line(format!("- in_use for the probe home {:?}", store::in_use(&p.home.join("out"))));
        if !same_path || !same_kind {
            p.fail("off did not put the test JPEG back");
        }
        gone(p, "off");
        Ok(())
    }

    fn second_cycle(p: &mut Probe) -> Result<()> {
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let second = picture_file(p, "probe-second.jpg", &subject_picture(width, height, (0.35, 0.55)))?;
        let third = picture_file(p, "probe-third.jpg", &subject_picture(width, height, (0.65, 0.45)))?;
        let ran = p.scooch(&["use", &second.to_string_lossy()], Duration::from_secs(60));
        p.line(format!("- scooch use from off exit {:?} after {} ms", ran.code, ran.took.as_millis()));
        for line in ran.text().lines() {
            p.line(format!("  - `{line}`"));
        }
        let home = p.home.clone();
        let first = wait_until(Duration::from_secs(20), || fs::read_to_string(home.join("watch.log")).unwrap_or_default().lines().any(|line| line.contains(" tick ")) || fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok()).is_some_and(|state| !state["problem"].is_null()));
        let job = daemon_pid(p);
        let parent = job.map(|pid| p.tool("/bin/ps", &["-o", "ppid=", "-p", &pid.to_string()]).out.trim().to_string());
        p.line(format!("- login item loaded {}, daemon pid {job:?} with parent {parent:?}, first tick {:?} ms, problem {:?}", launchd::loaded(), first.map(|time| time.as_millis()), problem(p)));
        if job.is_none() || first.is_none() {
            p.fail("scooch use did not start a daemon under launchd");
        }
        for (what, arguments) in [("scooch use while it runs", vec!["use".to_string(), third.to_string_lossy().into_owned()]), ("scooch flip", vec!["flip".to_string()])] {
            let before = ticks(p);
            let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
            let ran = p.scooch(&arguments, Duration::from_secs(60));
            let home = p.home.clone();
            let ticked = wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.log")).unwrap_or_default().lines().filter(|line| line.contains(" tick ")).count() > before);
            let after = daemon_pid(p);
            p.line(format!("- {what} exit {:?} in {} ms, the same launchd daemon {} ticked {:?} ms later", ran.code, ran.took.as_millis(), after == job, ticked.map(|time| time.as_millis())));
            if ticked.is_none() || after != job {
                if let Some(pid) = after {
                    stuck(p, pid);
                }
                p.fail(format!("{what} did not reach the daemon under launchd"));
            }
        }
        p.line(format!("- mirror saved {}, source now {:?}", state(p)["options"]["mirror"], state(p)["screens"].as_object().and_then(|screens| screens.values().next().map(|entry| entry["source"].clone()))));
        if let Some(pid) = scooch_pid(p) {
            quiet_minute(p, pid, "the launchd daemon waiting for consent")?;
            std::thread::sleep(Duration::from_secs(120));
            let still = problem(p).is_some_and(|problem| problem == finder::ASKING) && windows().iter().any(|(owner, _, _)| owner == "UserNotificationCenter");
            p.line(format!("- two minutes later, the consent window is still open {still}"));
            quiet_minute(p, pid, "the same daemon still waiting for consent")?;
        }
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        p.line(format!("- scooch off exit {:?} after {} ms: {}", ran.code, ran.took.as_millis(), ran.text()));
        if !ran.ok() {
            p.fail("the second scooch off did not end cleanly");
        }
        std::thread::sleep(Duration::from_millis(1500));
        gone(p, "the second off");
        p.finder = dismiss_prompt(p).is_some();
        Ok(())
    }

    fn switch(p: &mut Probe, code: u32) -> Ran {
        let ran = p.osa(&format!("tell application \"System Events\" to key code {code} using control down"));
        std::thread::sleep(Duration::from_millis(2500));
        ran
    }

    fn entries(p: &Probe) -> Vec<String> {
        state(p)["screens"].as_object().map(|screens| screens.iter().map(|(id, entry)| format!("{id} output {}", entry["output"].as_str().unwrap_or("none"))).collect()).unwrap_or_default()
    }

    struct Shot {
        width: usize,
        height: usize,
        rgb: Vec<u8>,
    }

    impl Shot {
        fn at(&self, x: usize, y: usize) -> [u8; 3] {
            let index = (y.min(self.height.saturating_sub(1)) * self.width + x.min(self.width.saturating_sub(1))) * 3;
            [self.rgb[index], self.rgb[index + 1], self.rgb[index + 2]]
        }

        fn image(&self) -> RgbImage {
            RgbImage::from_raw(self.width as u32, self.height as u32, self.rgb.clone()).unwrap_or_default()
        }
    }

    fn unpack(image: *const c_void) -> Option<Shot> {
        if image.is_null() {
            return None;
        }
        let shot = unsafe {
            let (width, height, row, bits) = (CGImageGetWidth(image), CGImageGetHeight(image), CGImageGetBytesPerRow(image), CGImageGetBitsPerPixel(image));
            let data = CGDataProviderCopyData(CGImageGetDataProvider(image));
            let shot = if data.is_null() || bits != 32 {
                None
            } else {
                let bytes = std::slice::from_raw_parts(CFDataGetBytePtr(data), CFDataGetLength(data).max(0) as usize);
                let mut rgb = Vec::with_capacity(width * height * 3);
                for y in 0..height {
                    for x in 0..width {
                        let at = y * row + x * 4;
                        rgb.extend_from_slice(&[bytes[at + 2], bytes[at + 1], bytes[at]]);
                    }
                }
                Some(Shot { width, height, rgb })
            };
            if !data.is_null() {
                CFRelease(data);
            }
            CFRelease(image);
            shot
        };
        shot
    }

    static SLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    fn shot_file(arguments: &[&str]) -> Option<Shot> {
        let file = std::env::temp_dir().join(format!("scooch-shot-{}.png", std::process::id()));
        let done = Command::new("/usr/sbin/screencapture").args(arguments).arg(&file).stdout(Stdio::null()).stderr(Stdio::null()).status().ok()?.success();
        let image = done.then(|| scooch_core::open(&file).ok()).flatten();
        let _ = fs::remove_file(&file);
        let image = image?;
        Some(Shot { width: image.width() as usize, height: image.height() as usize, rgb: image.into_raw() })
    }

    fn grab(display: u32) -> Option<Shot> {
        if let Some(shot) = unpack(unsafe { CGDisplayCreateImage(display) }) {
            return Some(shot);
        }
        SLOW.store(true, std::sync::atomic::Ordering::SeqCst);
        shot_file(&["-x", "-C", "-D", "1", "-t", "png"]).or_else(|| shot_file(&["-x", "-D", "1", "-t", "png"]))
    }

    fn grab_window(number: i64) -> Option<Shot> {
        let null = Frame { x: f64::INFINITY, y: f64::INFINITY, w: 0.0, h: 0.0 };
        if let Some(shot) = unpack(unsafe { CGWindowListCreateImage(null, 8, number as u32, 1) }) {
            return Some(shot);
        }
        SLOW.store(true, std::sync::atomic::Ordering::SeqCst);
        shot_file(&["-x", "-o", &format!("-l{number}"), "-t", "png"])
    }

    fn picture_window(screen: &Screen) -> Option<layer::Seen> {
        let (desktop, _) = layer::levels();
        let mut found: Vec<layer::Seen> = layer::listing().into_iter().filter(|window| window.layer < desktop && layer::is_picture(window) && window.bounds.x < screen.frame.x + screen.frame.w && screen.frame.x < window.bounds.x + window.bounds.w && window.bounds.w >= screen.frame.w * 0.9).collect();
        found.sort_by_key(|window| std::cmp::Reverse(window.layer));
        found.into_iter().next()
    }

    fn magenta(pixel: [u8; 3]) -> bool {
        pixel[0] > 180 && pixel[1] < 100 && pixel[2] > 180
    }

    fn bar_at(shot: &Shot, row: usize) -> Option<f32> {
        let hits: Vec<usize> = (0..shot.width).filter(|x| magenta(shot.at(*x, row))).collect();
        (!hits.is_empty()).then(|| hits.iter().sum::<usize>() as f32 / hits.len() as f32)
    }

    fn near(pixel: [u8; 3], color: [u8; 3]) -> bool {
        (0..3).map(|channel| (pixel[channel] as i32 - color[channel] as i32).abs()).sum::<i32>() < 60
    }

    fn rgba(image: &RgbImage, width: u32, height: u32) -> Vec<u8> {
        let fitted = if image.width() == width && image.height() == height {
            image.clone()
        } else {
            let scene = scooch_core::Scene { width, height, icons: Vec::new() };
            let placement = scooch_core::fill(image.width(), image.height(), &scene);
            scooch_core::render(image, &placement, &scene, &scooch_core::Options::default())
        };
        fitted.pixels().flat_map(|pixel| [pixel.0[0], pixel.0[1], pixel.0[2], 255]).collect()
    }

    fn stats(values: &[f64]) -> String {
        if values.is_empty() {
            return "none".into();
        }
        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.total_cmp(b));
        let pick = |share: f64| sorted[((sorted.len() - 1) as f64 * share).round() as usize];
        format!("{} values, median {:.2} ms, p99 {:.2} ms, max {:.2} ms", sorted.len(), pick(0.5), pick(0.99), sorted[sorted.len() - 1])
    }

    fn over(values: &[f64], period: f64) -> usize {
        values.iter().filter(|value| **value > period * 1.5).count()
    }

    fn ports() -> Option<u32> {
        unsafe {
            let task = mach_task_self_;
            let (mut names, mut count, mut types, mut type_count) = (std::ptr::null_mut(), 0u32, std::ptr::null_mut(), 0u32);
            if mach_port_names(task, &mut names, &mut count, &mut types, &mut type_count) != 0 {
                return None;
            }
            vm_deallocate(task, names as usize, count as usize * 4);
            vm_deallocate(task, types as usize, type_count as usize * 4);
            Some(count)
        }
    }

    fn ours() -> Vec<layer::Seen> {
        let pid = std::process::id() as i64;
        layer::every_window().into_iter().filter(|window| window.pid == pid).collect()
    }

    fn accessory(marker: MainThreadMarker) {
        NSApplication::sharedApplication(marker).setActivationPolicy(NSApplicationActivationPolicy::Accessory);
    }

    fn table(p: &mut Probe, screen: &Screen) {
        let (desktop, icons) = layer::levels();
        p.line(format!("- desktop level {desktop}, desktop icon level {icons}"));
        p.line("- on-screen windows from the bottom up to just above the icons, owner, layer, number, name, bounds".to_string());
        let mut seen: Vec<layer::Seen> = layer::listing().into_iter().filter(|window| window.layer <= icons + 5).collect();
        seen.sort_by_key(|window| window.layer);
        for window in seen {
            let here = window.bounds.x < screen.frame.x + screen.frame.w && screen.frame.x < window.bounds.x + window.bounds.w;
            p.line(format!("  - `{}` layer {} ({:+} from desktop) number {} name `{}` bounds {} on this screen {here}", window.owner, window.layer, window.layer - desktop, window.number, window.name, show(&window.bounds)));
        }
    }

    struct Run {
        frames: Vec<Instant>,
        arrivals: Vec<f64>,
        outputs: Vec<f64>,
        callbacks: usize,
        rate: f64,
        hidden: usize,
        crossed: Instant,
    }

    fn animate(renderer: Renderer, display: u32, cross: f64, hold: f64, park: Option<f32>) -> std::thread::JoinHandle<(Renderer, Run)> {
        std::thread::spawn(move || {
            let mut renderer = renderer;
            let pace = layer::Pace::start(display);
            let rate = pace.as_ref().map_or(0.0, |pace| pace.rate);
            let begin = Instant::now();
            let crossed = begin + Duration::from_secs_f64(cross);
            let mut frames = Vec::new();
            let mut hidden = 0;
            loop {
                let t = begin.elapsed().as_secs_f64();
                let bar = if t < cross {
                    Some((t / cross) as f32)
                } else if t < cross + hold {
                    park
                } else {
                    break;
                };
                match layer::present(|| renderer.frame(bar)) {
                    Ok(()) => frames.push(Instant::now()),
                    Err(error) if error.contains("occluded") => {
                        hidden += 1;
                        std::thread::sleep(Duration::from_millis(4));
                    }
                    Err(_) => break,
                }
            }
            let paced = pace.map(|pace| pace.stop());
            let millis = |values: Vec<f64>| values.into_iter().map(|seconds| seconds * 1000.0).collect::<Vec<f64>>();
            let (arrivals, outputs, callbacks) = paced.map_or((Vec::new(), Vec::new(), 0), |paced| (millis(paced.arrivals), millis(paced.outputs), paced.callbacks));
            (renderer, Run { frames, arrivals, outputs, callbacks, rate, hidden, crossed })
        })
    }

    struct Look {
        at: f64,
        done: f64,
        bar: Option<f32>,
        changed: Vec<f32>,
        covered: Vec<f32>,
        through: usize,
    }

    fn intervals(frames: &[Instant]) -> Vec<f64> {
        frames.windows(2).map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1000.0).collect()
    }

    fn report_run(p: &mut Probe, run: &Run) {
        let period = if run.rate > 0.0 { 1000.0 / run.rate } else { 16.67 };
        let presents = intervals(&run.frames);
        p.line(format!("- display link nominal rate {:.2} Hz, {} callbacks, arrival intervals {}, longer than 1.5 periods {}", run.rate, run.callbacks, stats(&run.arrivals), over(&run.arrivals, period)));
        p.line(format!("- display link output times {}, longer than 1.5 periods {}", stats(&run.outputs), over(&run.outputs, period)));
        p.line(format!("- frames presented {}, skipped while macOS called the window occluded {}, intervals {}, longer than 1.5 periods {}", run.frames.len(), run.hidden, stats(&presents), over(&presents, period)));
    }

    fn cycles(p: &mut Probe, marker: MainThreadMarker, screen: &Screen, shared: bool) -> Result<()> {
        let pid = std::process::id() as i32;
        let pattern = |layer: &layer::Layer| Pattern { width: layer.pixels.0, height: layer.pixels.1, rgba: GRAY.iter().copied().chain([255]).cycle().take(layer.pixels.0 as usize * layer.pixels.1 as usize * 4).collect() };
        let before = (ours().len(), ports(), threads(pid).len(), rusage(pid));
        let mut opens = Vec::new();
        let mut shown = Vec::new();
        let mut stages = [0usize; 2];
        let mut first: Vec<String> = Vec::new();
        for index in 0..20 {
            let started = Instant::now();
            let layer = layer::open(marker, screen)?;
            shown.push(started.elapsed().as_secs_f64() * 1000.0);
            let gpu = match (&p.gpu, shared) {
                (Some(gpu), true) => gpu.clone(),
                _ => Gpu::new(false).map_err(anyhow::Error::msg)?,
            };
            let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &pattern(&layer)).map_err(anyhow::Error::msg)?;
            layer.commit();
            layer.reveal();
            layer::present(|| renderer.frame(Some(0.2))).map_err(anyhow::Error::msg)?;
            opens.push(started.elapsed().as_secs_f64() * 1000.0);
            layer::present(|| renderer.frame(Some(0.5))).map_err(anyhow::Error::msg)?;
            layer::present(|| renderer.frame(Some(0.8))).map_err(anyhow::Error::msg)?;
            let traces = layer.traces();
            let open_held = traces.held();
            drop(renderer);
            drop(gpu);
            let rendered_held = traces.held();
            drop(layer);
            let dropped = traces.held();
            let windows_dropped = ours().len();
            layer::spin(0.05);
            let spun = traces.held();
            for (slot, held) in [dropped, spun].iter().enumerate() {
                if held[0].is_some() {
                    stages[slot] += 1;
                }
            }
            if index == 0 {
                first = vec![
                    format!("open {open_held:?}"),
                    format!("renderer dropped {rendered_held:?}"),
                    format!("layer dropped {dropped:?} with our windows {windows_dropped}"),
                    format!("run loop spun {spun:?} with our windows {}", ours().len()),
                ];
            }
        }
        std::thread::sleep(Duration::from_millis(500));
        let after = (ours().len(), ports(), threads(pid).len(), rusage(pid));
        let memory = |usage: Option<Usage>| usage.map(|usage| (usage.resident as f64 / 1_048_576.0, usage.footprint as f64 / 1_048_576.0));
        p.line(format!(
            "- twenty opens with {} device: our windows {} then {}, Mach ports {:?} then {:?}, threads {} then {}, resident and footprint MB {:?} then {:?}",
            if shared { "one shared" } else { "a fresh" },
            before.0,
            after.0,
            before.1,
            after.1,
            before.2,
            after.2,
            memory(before.3),
            memory(after.3)
        ));
        p.line(format!("- open to visible {}, open to first frame {}", stats(&shown), stats(&opens)));
        p.line(format!("- references still held on the window, view and Metal layer in the first cycle, beyond the probe's own {first:?}"));
        p.line(format!("- of twenty windows, still alive after the layer dropped {}, after the run loop spun {}", stages[0], stages[1]));
        if stages[1] > 0 {
            p.fail("layer windows stayed alive after the run loop spun");
        }
        Ok(())
    }

    fn abandoned(p: &mut Probe, marker: MainThreadMarker, screen: &Screen) -> Result<()> {
        let gpu = match &p.gpu {
            Some(gpu) => gpu.clone(),
            None => Gpu::new(false).map_err(anyhow::Error::msg)?,
        };
        let pattern = |layer: &layer::Layer| Pattern { width: layer.pixels.0, height: layer.pixels.1, rgba: GRAY.iter().copied().chain([255]).cycle().take(layer.pixels.0 as usize * layer.pixels.1 as usize * 4).collect() };
        let layer = layer::open(marker, screen)?;
        let number = layer.number();
        let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &pattern(&layer)).map_err(anyhow::Error::msg)?;
        layer.commit();
        layer.reveal();
        layer::present(|| renderer.frame(Some(0.3))).map_err(anyhow::Error::msg)?;
        let (go, held) = std::sync::mpsc::channel::<()>();
        let worker = std::thread::spawn(move || {
            let _ = held.recv();
            let woke = layer::present(|| renderer.frame(Some(0.6)));
            drop(renderer);
            woke
        });
        layer.abandon();
        layer::spin(0.3);
        let on_screen = ours().iter().any(|window| window.number == number && window.on_screen);
        let _ = go.send(());
        let woke = worker.join().map_err(|_| anyhow::anyhow!("the held render thread panicked"))?;
        layer::spin(0.1);
        let next = layer::open(marker, screen).and_then(|next| {
            let mut renderer = Renderer::on(&gpu, Target::Layer(next.surface()), &pattern(&next)).map_err(anyhow::Error::msg)?;
            next.commit();
            next.reveal();
            let drawn = layer::present(|| renderer.frame(Some(0.5))).map_err(anyhow::Error::msg);
            drop(renderer);
            drop(next);
            drawn
        });
        layer::spin(0.2);
        let left = ours().iter().filter(|window| window.on_screen).count();
        p.line(format!("- a layer abandoned while its render thread was held: its window on screen {on_screen}, the thread's frame after waking {woke:?}, a fresh layer after it opened and drew {:?}, windows of ours on screen after {left}", next.as_ref().map(|_| ()).map_err(|error| format!("{error:#}"))));
        if on_screen || !woke.as_ref().is_err_and(|why| why.contains("occluded")) || next.is_err() || left > 0 {
            p.fail("an abandoned layer stayed on screen, drew after its thread woke, or kept the next layer from opening");
        }
        Ok(())
    }

    fn live(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let desktop = platform::current()?;
        let screen = desktop.screens()?.into_iter().next().context("no screen shows a desktop")?;
        let recorded = desktop.wallpaper(&screen).ok().flatten();
        p.line(format!("- screen `{}` {} scale {}, wallpaper recorded {recorded:?}", screen.id, show(&screen.frame), screen.scale));
        table(p, &screen);
        let (level, icons) = layer::levels();
        let chosen = layer::choose(&layer::listing(), &screen.frame, level, std::process::id() as i64);
        p.line(format!("- chosen level {chosen:?}, the desktop level, above the picture window and {} below the icon level", icons - level));
        let widgets: Vec<String> = layer::listing().into_iter().filter(|window| ["Widget", "NotificationCenter", "chronod"].iter().any(|word| window.owner.contains(word))).map(|window| format!("{} layer {}", window.owner, window.layer)).collect();
        p.line(format!("- widget windows {widgets:?}"));
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let baseline = grab(display).context("CGDisplayCreateImage gave nothing")?;
        let timed = Instant::now();
        let _ = grab(display);
        let grab_ms = timed.elapsed().as_secs_f64() * 1000.0;
        let cross = (grab_ms * 30.0 / 1000.0).clamp(0.5, 4.0);
        p.line(format!("- one screen capture takes {grab_ms:.0} ms here, so the bar crosses in {cross:.2} s to be caught several times on the way"));
        let _ = scooch_core::encode_png(&baseline.image()).map(|bytes| fs::write(p.out.join("live-before.png"), bytes));
        let paper = picture_window(&screen).context("no wallpaper window was found for this screen")?;
        let field = grab_window(paper.number).context("the wallpaper window could not be captured")?;
        p.line(format!("- field taken from the wallpaper window `{}` number {} layer {}, {}x{}", paper.owner, paper.number, paper.layer, field.width, field.height));
        let named: Vec<(String, Rect)> = finder::named(&finder::view()).unwrap_or_default().into_iter().filter(|(name, _)| name.starts_with("scooch")).collect();
        let scale = screen.scale;
        let field_color = field.at(field.width / 2, field.height * 3 / 4);
        let masks: Vec<(String, Vec<(usize, usize)>)> = named
            .iter()
            .map(|(name, rect)| {
                let mut ink = Vec::new();
                for y in ((rect.y - screen.frame.y) * scale) as usize..((rect.y - screen.frame.y + rect.h) * scale) as usize {
                    for x in ((rect.x - screen.frame.x) * scale) as usize..((rect.x - screen.frame.x + rect.w) * scale) as usize {
                        if !near(baseline.at(x, y), field.at(x, y)) {
                            ink.push((x, y));
                        }
                    }
                }
                (name.clone(), ink)
            })
            .collect();
        p.line(format!("- icon ink pixels {:?}", masks.iter().map(|(name, ink)| format!("{name} {}", ink.len())).collect::<Vec<_>>()));
        let cyan = picture_file(p, "probe-cyan.jpg", &RgbImage::from_raw(64, 64, [40u8, 200, 220].repeat(64 * 64)).unwrap_or_default())?;
        let opened = Instant::now();
        let layer = layer::open(marker, &screen)?;
        let (width, height) = layer.pixels;
        let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
        let device_ready = opened.elapsed();
        let renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba: rgba(&field.image(), width, height) }).map_err(anyhow::Error::msg)?;
        layer.commit();
        layer.reveal();
        p.gpu = Some(gpu.clone());
        p.line(format!("- layer number {} at level {}, {}x{} pixels, adapter {}, device ready after {} ms, render threads read visibility from the layer's own watcher {}", layer.number(), layer.level, width, height, gpu.name, device_ready.as_millis(), layer.watched()));
        let row = (height as f32 * 0.55) as usize;
        let samples: Vec<(usize, usize)> = [(0.1, 0.7), (0.5, 0.7), (0.7, 0.7), (0.9, 0.7), (0.55, 0.45), (0.85, 0.45)].iter().map(|(x, y)| ((width as f32 * x) as usize, (height as f32 * y) as usize)).collect();
        let set_after = opened.elapsed().as_secs_f64() * 1000.0 + cross * 1000.0 + 400.0;
        let worker = animate(renderer, layer.display, cross, 4.0, Some(0.3));
        let mut looks: Vec<Look> = Vec::new();
        let mut set_at: Option<Instant> = None;
        let mut system_shows: Option<f64> = None;
        let mut saved = 0;
        while !worker.is_finished() {
            let at = opened.elapsed().as_secs_f64() * 1000.0;
            let Some(shot) = grab(display) else { continue };
            let done = opened.elapsed().as_secs_f64() * 1000.0;
            let bar = bar_at(&shot, row);
            let share = |test: &dyn Fn(usize, usize) -> bool| -> Vec<f32> { masks.iter().map(|(_, ink)| ink.iter().filter(|(x, y)| test(*x, *y)).count() as f32 / ink.len().max(1) as f32).collect() };
            let changed = share(&|x, y| !near(shot.at(x, y), baseline.at(x, y)));
            let covered = share(&|x, y| near(shot.at(x, y), field.at(x, y)) || magenta(shot.at(x, y)));
            let through = samples.iter().filter(|(x, y)| near(shot.at(*x, *y), [40, 200, 220])).count();
            if (saved == 0 && at > 250.0) || (saved == 1 && set_at.is_some_and(|set| set.elapsed().as_millis() > 1500)) {
                let _ = scooch_core::encode_png(&shot.image()).map(|bytes| fs::write(p.out.join(format!("live-during-{saved}.png")), bytes));
                saved += 1;
            }
            looks.push(Look { at, done, bar, changed, covered, through });
            if set_at.is_none() && at > set_after {
                desktop.set_wallpaper(&screen, &cyan)?;
                set_at = Some(Instant::now());
            }
            if let (Some(set), None) = (set_at, system_shows) {
                let shown = picture_window(&screen).and_then(|window| grab_window(window.number)).is_some_and(|window| near(window.at(window.width / 2, window.height * 3 / 4), [40, 200, 220]));
                if shown {
                    system_shows = Some(set.elapsed().as_secs_f64() * 1000.0);
                }
            }
        }
        let (renderer, run) = worker.join().map_err(|_| anyhow::anyhow!("the render thread panicked"))?;
        drop(renderer);
        if !layer.watched() {
            p.fail("the layer lost its watcher, so wgpu asks AppKit from the render thread");
        }
        let traces = layer.traces();
        layer.close();
        let closed = Instant::now();
        layer::spin(0.4);
        let after = grab(display);
        let leftover = ours();
        let held = traces.held();
        let first = run.frames.first().map(|frame| (*frame - opened).as_secs_f64() * 1000.0);
        let last = run.frames.last().map(|frame| (*frame - opened).as_secs_f64() * 1000.0);
        p.line(format!("- cold open to first presented frame {:?} ms, last frame at {:?} ms, captures used the screencapture tool as a fallback {}", first.map(|value| value.round()), last.map(|value| value.round()), SLOW.load(std::sync::atomic::Ordering::SeqCst)));
        report_run(p, &run);
        let period = if run.rate > 0.0 { 1000.0 / run.rate } else { 1000.0 / 60.0 };
        let crossed = run.crossed.saturating_duration_since(opened).as_secs_f64() * 1000.0;
        let crossing: Vec<(f64, f32)> = looks.iter().filter(|look| first.is_some_and(|first| look.at >= first) && look.done <= crossed).filter_map(|look| look.bar.map(|bar| (look.at, bar))).collect();
        let steady = crossing.windows(2).all(|pair| pair[1].1 >= pair[0].1 - 2.0);
        p.line(format!("- {} captures in all, {} taken whole between the first presented frame and the end of the pattern's {:.0} ms crossing at {crossed:.0} ms, bar centres in pixels {:?}, increasing {steady}", looks.len(), crossing.len(), cross * 1000.0, crossing.iter().map(|(at, bar)| format!("{:.0} ms {:.0}", at, bar)).collect::<Vec<_>>()));
        let inside: Vec<&Look> = looks.iter().filter(|look| first.zip(last).is_some_and(|(first, last)| look.at >= first && look.at <= last)).collect();
        let settled_from = first.map(|first| first + 4.0 * period);
        let bare_times: Vec<f64> = inside.iter().filter(|look| look.bar.is_none()).map(|look| look.at).collect();
        let bare = bare_times.iter().filter(|at| settled_from.is_some_and(|from| **at > from)).count();
        p.line(format!("- captures started between the first and last pattern frame {}, without the layer {} at {:?} ms, of which started more than four refreshes ({:.0} ms) after the first frame was queued {bare}", inside.len(), bare_times.len(), bare_times.iter().map(|at| at.round()).collect::<Vec<_>>(), 4.0 * period));
        let worst = |pick: fn(&Look) -> &Vec<f32>| -> Vec<String> { masks.iter().enumerate().map(|(index, (name, _))| format!("{name} {:.3}", looks.iter().map(|look| pick(look).get(index).copied().unwrap_or_default()).fold(0.0f32, f32::max))).collect() };
        p.line(format!("- largest share of icon ink that changed in any capture, edge and shadow pixels blend with whatever lies below so they move with the bar {:?}", worst(|look| &look.changed)));
        p.line(format!("- largest share of icon ink showing only what the layer drew, as it would if the layer covered the icon {:?}", worst(|look| &look.covered)));
        let marked: Vec<String> = looks.iter().filter(|look| look.covered.iter().any(|share| *share > 0.5)).map(|look| format!("{:.0} ms {:?}", look.at, look.covered.iter().map(|share| (share * 1000.0).round() / 1000.0).collect::<Vec<_>>())).collect();
        p.line(format!("- captures where the layer covered more than half of an icon's ink {} {:?}", marked.len(), marked.iter().take(6).collect::<Vec<_>>()));
        let through = looks.iter().filter(|look| look.through > 0 && first.zip(last).is_some_and(|(first, last)| look.at >= first && look.at <= last)).count();
        p.line(format!("- wallpaper set {:?} ms after open, the system's own window showed it {:?} ms after the set, captures where the new picture showed through or above the layer {through}", set_at.map(|set| (set - opened).as_millis()), system_shows.map(|value| value.round())));
        let after_bar = after.as_ref().and_then(|shot| bar_at(shot, row));
        let after_cyan = after.as_ref().map(|shot| samples.iter().filter(|(x, y)| near(shot.at(*x, *y), [40, 200, 220])).count());
        if let Some(shot) = &after {
            let _ = scooch_core::encode_png(&shot.image()).map(|bytes| fs::write(p.out.join("live-after.png"), bytes));
        }
        p.line(format!("- after close at +{} ms: bar on screen {after_bar:?}, field samples showing the new picture {after_cyan:?} of {}, windows of ours {}, window, view and Metal layer still alive {held:?}", closed.elapsed().as_millis(), samples.len(), leftover.len()));
        if !steady || crossing.len() < 2 || bare > 0 || through > 0 || after_bar.is_some() || !leftover.is_empty() || after_cyan != Some(samples.len()) {
            p.fail("the live layer did not sit cleanly under the icons and go");
        }
        if !marked.is_empty() {
            p.fail("the layer covered an icon");
        }
        cycles(p, marker, &screen, true)?;
        cycles(p, marker, &screen, false)?;
        if let Err(error) = helpers(p) {
            p.fail(format!("the helper process could not be measured {error:#}"));
        }
        if let Err(error) = abandoned(p, marker, &screen) {
            p.fail(format!("the abandoned layer check could not run {error:#}"));
        }
        let _ = field_color;
        if let Some(recorded) = recorded.filter(|path| path.is_file()) {
            let _ = desktop.set_wallpaper(&screen, &recorded);
        }
        Ok(())
    }

    fn spin(seconds: f64) {
        unsafe { CFRunLoopRunInMode(kCFRunLoopDefaultMode, seconds, 0) };
    }

    fn memory(pid: i32) -> String {
        rusage(pid).map_or("unread".into(), |usage| format!("footprint {:.1} MB resident {:.1} MB", usage.footprint as f64 / 1_048_576.0, usage.resident as f64 / 1_048_576.0))
    }

    fn front() -> String {
        NSWorkspace::sharedWorkspace().frontmostApplication().and_then(|application| application.bundleIdentifier()).map(|name| name.to_string()).unwrap_or_default()
    }

    fn helper() -> ExitCode {
        let pid = std::process::id() as i32;
        let started = Instant::now();
        let say = |stage: &str, extra: String| {
            println!("stage {stage} at {} ms, {}{extra}", started.elapsed().as_millis(), memory(pid));
            let _ = std::io::stdout().flush();
        };
        say("start", String::new());
        let result = (|| -> Result<()> {
            let marker = MainThreadMarker::new().context("not on the main thread")?;
            let before = front();
            let application = NSApplication::sharedApplication(marker);
            application.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
            application.finishLaunching();
            say("app", String::new());
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            say("device", format!(", {}", gpu.name));
            let screen = platform::current()?.screens()?.into_iter().next().context("no screen")?;
            let layer = layer::open(marker, &screen)?;
            let (width, height) = layer.pixels;
            let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba: GRAY.iter().copied().chain([255]).cycle().take(width as usize * height as usize * 4).collect() }).map_err(anyhow::Error::msg)?;
            layer.commit();
            layer.reveal();
            layer::present(|| renderer.frame(Some(0.0))).map_err(anyhow::Error::msg)?;
            say("frame", String::new());
            for index in 1..30 {
                layer::present(|| renderer.frame(Some(index as f32 / 30.0))).map_err(anyhow::Error::msg)?;
            }
            spin(0.3);
            let me = NSRunningApplication::currentApplication();
            say("shown", format!(", front app before {before} after {}, this helper active {}, policy {:?}", front(), me.isActive(), me.activationPolicy()));
            drop(renderer);
            let traces = layer.traces();
            drop(layer);
            layer::spin(0.3);
            say("closed", format!(", window, view and Metal layer still alive {:?}, windows of ours {}", traces.held(), ours().len()));
            drop(gpu);
            spin(0.5);
            say("dropped", String::new());
            let calm = rusage(pid);
            spin(5.0);
            let later = rusage(pid);
            let woke = calm.zip(later).map(|(calm, later)| (later.idle_wakeups.saturating_sub(calm.idle_wakeups), later.interrupt_wakeups.saturating_sub(calm.interrupt_wakeups)));
            say("idle", format!(", idle and interrupt wakeups over the last 5 s {woke:?}"));
            Ok(())
        })();
        if let Err(error) = result {
            println!("stage failed {error:#}");
        }
        ExitCode::SUCCESS
    }

    fn helpers(p: &mut Probe) -> Result<()> {
        use std::io::BufRead as _;
        let exe = std::env::current_exe()?;
        for round in ["first", "second"] {
            let spawned = Instant::now();
            let mut child = Command::new(&exe).arg("helper").stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
            let pid = child.id();
            let lines: Vec<(u128, String)> = std::io::BufReader::new(child.stdout.take().context("no helper output")?).lines().map_while(|line| line.ok()).map(|line| (spawned.elapsed().as_millis(), line)).collect();
            let status = child.wait()?;
            p.line(format!("- {round} helper process {pid} exited {:?} after {} ms", status.code(), spawned.elapsed().as_millis()));
            if !status.success() {
                p.fail(format!("the {round} launch measurement helper did not end cleanly"));
            }
            for (at, line) in &lines {
                p.line(format!("  - +{at} ms from spawn: {line}"));
            }
            let logged = p.tool("/usr/bin/log", &["show", "--last", "60s", "--info", "--debug", "--style", "ndjson", "--predicate", &format!("processIdentifier == {pid}")]);
            let launch: Vec<String> = logged.out.lines().filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok()).filter(|entry| entry["senderImagePath"].as_str().is_some_and(|path| path.contains("launch_measurement"))).map(|entry| format!("{} {}", entry["timestamp"].as_str().unwrap_or(""), entry["eventMessage"].as_str().unwrap_or("").chars().take(100).collect::<String>())).collect();
            p.line(format!("  - launch measurement log lines {launch:?}"));
        }
        Ok(())
    }

    fn space_layer(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let screen = platform::current()?.screens()?.into_iter().next().context("no screen")?;
        let layer = layer::open(marker, &screen)?;
        let gpu = match &p.gpu {
            Some(gpu) => gpu.clone(),
            None => Gpu::new(false).map_err(anyhow::Error::msg)?,
        };
        let (width, height) = layer.pixels;
        let mut renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba: GRAY.iter().copied().chain([255]).cycle().take(width as usize * height as usize * 4).collect() }).map_err(anyhow::Error::msg)?;
        layer.commit();
        layer.reveal();
        layer::present(|| renderer.frame(Some(0.5))).map_err(anyhow::Error::msg)?;
        let number = layer.number();
        let showing = || layer::listing().iter().any(|window| window.number == number);
        let one = showing();
        switch(p, 124);
        let two = showing();
        switch(p, 123);
        let back = showing();
        drop(renderer);
        layer.close();
        p.line(format!("- the layer on the first Space is listed on screen there {one}, on the second Space {two}, back on the first {back}"));
        if !one || two || !back {
            p.fail("the layer did not stay on its own Space");
        }
        Ok(())
    }

    fn pace(out: PathBuf) -> ExitCode {
        let _ = fs::create_dir_all(&out);
        let mut lines = vec!["# Scooch layer pacing on this Mac".to_string(), String::new()];
        let result = (|| -> Result<()> {
            let marker = MainThreadMarker::new().context("not on the main thread")?;
            accessory(marker);
            let desktop = platform::current()?;
            let screen = desktop.screens()?.into_iter().next().context("no screen shows a desktop")?;
            let source = picture::reported(marker, &screen.id).ok().flatten().filter(|path| path.is_file()).unwrap_or_else(|| PathBuf::from(store::DEFAULT_DESKTOP));
            let picture = desktop.decode(&source)?;
            lines.push(format!("- screen `{}` {} scale {}, field from {}", screen.id, show(&screen.frame), screen.scale, source.display()));
            let pid = std::process::id() as i32;
            lines.push(format!("- before any device {}", memory(pid)));
            let made = Instant::now();
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            lines.push(format!("- device {} ready after {} ms, {}", gpu.name, made.elapsed().as_millis(), memory(pid)));
            let mut field: Option<Vec<u8>> = None;
            for index in 0..5 {
                let started = Instant::now();
                let layer = layer::open(marker, &screen)?;
                let shown = started.elapsed().as_millis();
                let (width, height) = layer.pixels;
                let rgba = field.get_or_insert_with(|| rgba(&picture, width, height)).clone();
                let renderer = Renderer::on(&gpu, Target::Layer(layer.surface()), &Pattern { width, height, rgba }).map_err(anyhow::Error::msg)?;
                layer.commit();
                layer.reveal();
                let (renderer, run) = animate(renderer, layer.display, 0.5, 0.6, None).join().map_err(|_| anyhow::anyhow!("the render thread panicked"))?;
                drop(renderer);
                let traces = layer.traces();
                layer.close();
                let lifetime = started.elapsed().as_millis();
                layer::spin(0.1);
                let first = run.frames.first().map(|frame| (*frame - started).as_millis());
                let period = if run.rate > 0.0 { 1000.0 / run.rate } else { 16.67 };
                let presents = intervals(&run.frames);
                lines.push(format!("- open {} level {}, {}x{}, open to visible {shown} ms, open to first frame {first:?} ms, window open for {lifetime} ms, window, view and Metal layer alive after close {:?}, windows of ours {}", index + 1, layer::levels().0, width, height, traces.held(), ours().len()));
                lines.push(format!("  - display link {:.2} Hz, {} callbacks, arrival intervals {}, longer than 1.5 periods {}", run.rate, run.callbacks, stats(&run.arrivals), over(&run.arrivals, period)));
                lines.push(format!("  - display link output times {}, longer than 1.5 periods {}", stats(&run.outputs), over(&run.outputs, period)));
                lines.push(format!("  - frames {}, skipped while occluded {}, intervals {}, longer than 1.5 periods {}", run.frames.len(), run.hidden, stats(&presents), over(&presents, period)));
                std::thread::sleep(Duration::from_millis(300));
            }
            lines.push(format!("- with the device after five opens {}", memory(pid)));
            drop(gpu);
            spin(0.5);
            lines.push(format!("- after the device was dropped {}", memory(pid)));
            lines.push(format!("- windows of ours left {}", ours().len()));
            Ok(())
        })();
        if let Err(error) = result {
            lines.push(format!("- **stopped** {error:#}"));
        }
        for line in &lines {
            println!("{line}");
        }
        let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        ExitCode::SUCCESS
    }

    fn test_still(width: u32, height: u32) -> RgbImage {
        let third = (height / 3).max(1);
        let bars: [[u8; 3]; 10] = [[255, 0, 0], [0, 255, 0], [0, 0, 255], [255, 255, 0], [0, 255, 255], [255, 0, 255], [255, 255, 255], [30, 30, 30], [128, 128, 128], [200, 120, 60]];
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                let pixel = if y < third {
                    [(x * 255 / width.max(1)) as u8, (y * 255 / third) as u8, 160]
                } else if y < 2 * third {
                    bars[(x as usize * bars.len() / width.max(1) as usize).min(bars.len() - 1)]
                } else if (x + y) % 2 == 0 {
                    [250, 250, 250]
                } else {
                    [10, 10, 10]
                };
                rgb.extend_from_slice(&pixel);
            }
        }
        RgbImage::from_raw(width, height, rgb).unwrap_or_default()
    }

    fn still_file(path: &Path, image: &RgbImage) -> Result<()> {
        let mut out = Vec::new();
        let mut encoder = jpeg_encoder::Encoder::new(&mut out, 100);
        encoder.set_sampling_factor(jpeg_encoder::SamplingFactor::F_1_1);
        if let Some(profile) = picture::profile() {
            encoder.add_icc_profile(&profile)?;
        }
        encoder.encode(image.as_raw(), image.width() as u16, image.height() as u16, jpeg_encoder::ColorType::Rgb)?;
        fs::write(path, out)?;
        Ok(())
    }

    struct Difference {
        mean: f64,
        p99: u8,
        max: u8,
        over: usize,
        counted: usize,
    }

    fn difference(a: &Shot, b: &Shot, keep: &dyn Fn(usize, usize) -> bool) -> Difference {
        let mut counts = [0usize; 256];
        for y in 0..a.height.min(b.height) {
            for x in 0..a.width.min(b.width) {
                if keep(x, y) {
                    let (one, two) = (a.at(x, y), b.at(x, y));
                    let worst = (0..3).map(|channel| one[channel].abs_diff(two[channel])).max().unwrap_or(0);
                    counts[worst as usize] += 1;
                }
            }
        }
        let counted: usize = counts.iter().sum();
        let total: usize = counts.iter().enumerate().map(|(value, count)| value * count).sum();
        let mut seen = 0;
        let mut p99 = 0u8;
        for (value, count) in counts.iter().enumerate() {
            seen += count;
            if seen * 100 >= counted * 99 {
                p99 = value as u8;
                break;
            }
        }
        let max = counts.iter().rposition(|count| *count > 0).unwrap_or(0) as u8;
        Difference { mean: total as f64 / counted.max(1) as f64, p99, max, over: counts[3..].iter().sum(), counted }
    }

    fn calm_shot(display: u32) -> Option<Shot> {
        let mut last = grab(display)?;
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(300));
            let next = grab(display)?;
            let same = difference(&last, &next, &|_, _| true).max == 0;
            last = next;
            if same {
                break;
            }
        }
        Some(last)
    }

    fn away_from_icons(screen: &Screen) -> impl Fn(usize, usize) -> bool {
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let (scale, frame) = (screen.scale, screen.frame);
        move |x, y| {
            let (px, py) = (x as f32 / scale + frame.x, y as f32 / scale + frame.y);
            py - frame.y >= 30.0 && !icons.iter().any(|icon| px >= icon.x - 12.0 && px < icon.x + icon.w + 12.0 && py >= icon.y - 12.0 && py < icon.y + icon.h + 12.0)
        }
    }

    fn still_through_helper(p: &mut Probe, screen: &Screen, label: &str) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        let desktop = platform::current()?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let (width, height) = pixels(screen);
        let file = p.home.join(format!("probe-still-{width}x{height}.jpg"));
        still_file(&file, &test_still(width, height))?;
        desktop.set_wallpaper(screen, &file)?;
        std::thread::sleep(Duration::from_millis(800));
        let plain = calm_shot(display).context("the plain desktop could not be captured")?;
        let mut helper = scooch::live::Helper::spawn(&p.bin, Duration::from_secs(5))?;
        let ready = helper.warm(scooch::live::Hints::default(), None, None, Duration::from_secs(30))?;
        let shown = helper.open(1, screen, &file, Duration::from_secs(5))?;
        std::thread::sleep(Duration::from_millis(400));
        let layered = grab(display).context("the layer could not be captured")?;
        let keep = away_from_icons(screen);
        let found = difference(&plain, &layered, &keep);
        let pid = helper.pid as i64;
        let closed = helper.close(1, Duration::from_secs(3));
        std::thread::sleep(Duration::from_millis(300));
        let left = layer::every_window().into_iter().filter(|window| window.pid == pid).count();
        drop(helper);
        let _ = scooch_core::encode_png(&layered.image()).map(|bytes| fs::write(p.out.join(format!("still-{width}x{height}.png")), bytes));
        p.line(format!(
            "- {label}: still {width}x{height} on a {}x{} point screen at scale {}, adapter {} ready after {} ms, open to shown {shown} ms, closed {:?}, windows of the helper left {left}",
            screen.frame.w, screen.frame.h, screen.scale, ready.adapter, ready.ms, closed.map_err(|error| error.to_string())
        ));
        p.line(format!("  - plain desktop against the layer showing the same still, {} pixels outside the icons and menu bar: mean {:.3}, p99 {}, max {} of 255, pixels off by more than 2 {}", found.counted, found.mean, found.p99, found.max, found.over));
        if found.max > 2 {
            p.fail(format!("the layer did not show the still exactly as the system does at {label}"));
        }
        Ok(())
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Gone {
        AtSet,
        AfterClose,
    }

    struct Hunt {
        name: &'static str,
        trials: u32,
        layer: bool,
        under: bool,
        hold: Duration,
        through: Option<&'static str>,
        gone: Gone,
    }

    const HUNTS: [Hunt; 6] = [
        Hunt { name: "opaque layer, picture set under it, the old picture file deleted at the set as the daemon does, close 600 ms after the set", trials: 30, layer: true, under: true, hold: Duration::from_millis(600), through: None, gone: Gone::AtSet },
        Hunt { name: "plain set with no layer, the old picture file deleted at the set", trials: 20, layer: false, under: true, hold: Duration::from_millis(1200), through: None, gone: Gone::AtSet },
        Hunt { name: "see through layer, picture set under it, the old picture file deleted at the set, close 600 ms after the set", trials: 20, layer: true, under: true, hold: Duration::from_millis(600), through: Some("layer"), gone: Gone::AtSet },
        Hunt { name: "opaque layer, picture set under it, the old picture file deleted after the close", trials: 15, layer: true, under: true, hold: Duration::from_millis(600), through: None, gone: Gone::AfterClose },
        Hunt { name: "opaque layer, picture set under it, the old picture file deleted at the set, close 3 s after the set", trials: 10, layer: true, under: true, hold: Duration::from_millis(3000), through: None, gone: Gone::AtSet },
        Hunt { name: "plain set with no layer, the old picture file deleted after 1.2 s", trials: 10, layer: false, under: true, hold: Duration::from_millis(1200), through: None, gone: Gone::AfterClose },
    ];
    const HUNT_AFTER: Duration = Duration::from_millis(500);
    const HUNT_REGION: (f32, f32, f32, f32) = (0.1, 0.3, 0.9, 0.6);

    fn tone(width: u32, height: u32, from: [u8; 3], to: [u8; 3]) -> RgbImage {
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for _ in 0..height {
            for x in 0..width {
                let t = x as f32 / width.max(1) as f32;
                for channel in 0..3 {
                    rgb.push((f32::from(from[channel]) + (f32::from(to[channel]) - f32::from(from[channel])) * t).round() as u8);
                }
            }
        }
        RgbImage::from_raw(width, height, rgb).unwrap_or_default()
    }

    fn band(display: u32, screen: &Screen) -> Option<Shot> {
        let rect = Frame { x: f64::from(screen.frame.w * HUNT_REGION.0), y: f64::from(screen.frame.h * HUNT_REGION.1), w: f64::from(screen.frame.w * (HUNT_REGION.2 - HUNT_REGION.0)), h: f64::from(screen.frame.h * (HUNT_REGION.3 - HUNT_REGION.1)) };
        unpack(unsafe { CGDisplayCreateImageForRect(display, rect) })
    }

    fn tone_of(rgb: impl Iterator<Item = [u8; 3]>) -> (f32, [f32; 3]) {
        let (mut dark, mut count, mut sum) = (0usize, 0usize, [0f64; 3]);
        for pixel in rgb {
            count += 1;
            if pixel.iter().all(|channel| *channel <= 12) {
                dark += 1;
            }
            for channel in 0..3 {
                sum[channel] += f64::from(pixel[channel]);
            }
        }
        let count = count.max(1);
        (dark as f32 / count as f32, sum.map(|total| (total / count as f64) as f32))
    }

    fn region_tone(shot: &Shot, region: (f32, f32, f32, f32)) -> (f32, [f32; 3]) {
        let (left, top) = ((shot.width as f32 * region.0) as usize, (shot.height as f32 * region.1) as usize);
        let (right, bottom) = ((shot.width as f32 * region.2) as usize, (shot.height as f32 * region.3) as usize);
        tone_of((top..bottom.max(top + 1)).step_by(6).flat_map(|y| (left..right.max(left + 1)).step_by(6).map(move |x| (x, y))).map(|(x, y)| shot.at(x, y)))
    }

    fn sort_of(found: (f32, [f32; 3]), a: [f32; 3], b: [f32; 3]) -> char {
        let (dark, mean) = found;
        let apart = |want: [f32; 3]| (0..3).map(|channel| (mean[channel] - want[channel]).abs()).sum::<f32>() / 3.0;
        if dark >= 0.9 {
            'K'
        } else if dark < 0.02 && apart(a) < 20.0 {
            'A'
        } else if dark < 0.02 && apart(b) < 20.0 {
            'B'
        } else {
            '~'
        }
    }

    fn below_desktop() -> Vec<String> {
        let (desktop, _) = layer::levels();
        layer::listing().into_iter().filter(|window| window.layer <= desktop).map(|window| format!("{} {} '{}' at {} alpha {:.3}", window.number, window.owner, window.name, window.layer, window.alpha)).collect()
    }

    struct Caught {
        at: f64,
        took: f64,
        sort: char,
        ours: bool,
        tone: (f32, [f32; 3]),
    }

    struct Tried {
        caught: Vec<Caught>,
        before: Vec<String>,
        armed: Vec<String>,
        after: Vec<String>,
        under: Option<char>,
        under_black: Option<char>,
        ours_black: Option<char>,
        held: f64,
    }

    fn shows(display: u32, screen: &Screen, want: char, a: [f32; 3], b: [f32; 3], limit: Duration) -> bool {
        let started = Instant::now();
        while started.elapsed() < limit {
            if band(display, screen).is_some_and(|shot| sort_of(region_tone(&shot, (0.0, 0.0, 1.0, 1.0)), a, b) == want) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        false
    }

    fn whole(window: Option<i64>, a: [f32; 3], b: [f32; 3]) -> Option<char> {
        window.and_then(grab_window).map(|shot| sort_of(region_tone(&shot, HUNT_REGION), a, b))
    }

    fn watch_close(display: u32, screen: &Screen, pid: Option<u32>, after: Duration, tones: ([f32; 3], [f32; 3]), out: &Path, saved: &mut usize) -> (Vec<Caught>, Option<char>, Option<char>) {
        let (a, b) = tones;
        let started = Instant::now();
        let mut caught = Vec::new();
        let (mut under_black, mut ours_black) = (None, None);
        while started.elapsed() < after {
            let mine = pid.and_then(|pid| layer::listing().into_iter().find(|window| window.pid == i64::from(pid)).map(|window| window.number));
            let at = started.elapsed().as_secs_f64() * 1000.0;
            let Some(shot) = band(display, screen) else { continue };
            let took = started.elapsed().as_secs_f64() * 1000.0 - at;
            let tone = region_tone(&shot, (0.0, 0.0, 1.0, 1.0));
            let sort = sort_of(tone, a, b);
            if sort == 'K' && under_black.is_none() {
                under_black = whole(picture_window(screen).map(|window| window.number), a, b).or(Some('?'));
                ours_black = mine.and_then(|number| whole(Some(number), a, b));
                if *saved < 8 {
                    *saved += 1;
                    let _ = scooch_core::encode_jpeg(&shot.image(), 80).map(|bytes| fs::write(out.join(format!("black-{saved}.jpg")), bytes));
                }
            }
            caught.push(Caught { at, took, sort, ours: mine.is_some(), tone });
        }
        (caught, under_black, ours_black)
    }

    fn hunt_trial(helper: Option<&mut scooch::live::Helper>, hunt: &Hunt, id: u32, files: (&Path, &Path), (display, screen): (u32, &Screen), tones: ([f32; 3], [f32; 3]), (out, saved): (&Path, &mut usize)) -> Result<Tried> {
        let desktop = platform::current()?;
        let (a, b) = tones;
        let (first, second) = if hunt.under { files } else { (files.1, files.1) };
        desktop.set_wallpaper(screen, first)?;
        let start = if hunt.under { 'A' } else { 'B' };
        if !shows(display, screen, start, a, b, Duration::from_secs(4)) {
            anyhow::bail!("the first picture did not show within 4 s");
        }
        std::thread::sleep(Duration::from_millis(200));
        let before = below_desktop();
        let Some(helper) = helper.filter(|_| hunt.layer) else {
            let set = Instant::now();
            desktop.set_wallpaper(screen, second)?;
            if hunt.gone == Gone::AtSet {
                let _ = fs::remove_file(first);
            }
            let (caught, under_black, _) = watch_close(display, screen, None, hunt.hold, tones, out, saved);
            let _ = fs::remove_file(first);
            return Ok(Tried { caught, before, armed: Vec::new(), after: below_desktop(), under: None, under_black, ours_black: None, held: set.elapsed().as_secs_f64() * 1000.0 });
        };
        helper.open(id, screen, first, Duration::from_secs(5))?;
        std::thread::sleep(Duration::from_millis(150));
        let set = Instant::now();
        if hunt.under {
            desktop.set_wallpaper(screen, second)?;
            if hunt.gone == Gone::AtSet {
                let _ = fs::remove_file(first);
            }
        }
        helper.ask_settle(id, second, None)?;
        helper.settled(id, Duration::from_secs(3))?;
        while set.elapsed() < hunt.hold {
            std::thread::sleep(Duration::from_millis(5));
        }
        let armed = below_desktop();
        let under = if id.is_multiple_of(2) { whole(picture_window(screen).map(|window| window.number), a, b) } else { None };
        let held = set.elapsed().as_secs_f64() * 1000.0;
        helper.send(&scooch::live::Command::Close { id })?;
        let (caught, under_black, ours_black) = watch_close(display, screen, Some(helper.pid), HUNT_AFTER, tones, out, saved);
        if hunt.under {
            let _ = fs::remove_file(first);
        }
        helper.expect(Duration::from_secs(3), |reply| match reply {
            scooch::live::Reply::Closed { id: closed } if *closed == id => Some(Ok(())),
            scooch::live::Reply::Failed { id: Some(failed), why } if *failed == id => Some(Err(anyhow::anyhow!("{why}"))),
            _ => None,
        })?;
        Ok(Tried { caught, before, armed, after: below_desktop(), under, under_black, ours_black, held })
    }

    fn black_frames(out: PathBuf) -> ExitCode {
        let _ = fs::create_dir_all(&out);
        let mut lines = vec!["# Black frames at the close of the layer".to_string(), String::new()];
        if !std::env::var("GITHUB_ACTIONS").is_ok_and(|value| value == "true") {
            lines.push(format!("- {RUNNER_ONLY}"));
            let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
            return ExitCode::SUCCESS;
        }
        let icon = dirs::desktop_dir().unwrap_or_default().join("scooch-black-probe.txt");
        let _ = fs::write(&icon, "probe");
        std::thread::sleep(Duration::from_secs(3));
        let result = hunt(&out, &mut lines);
        let _ = fs::remove_file(&icon);
        if let Err(error) = result {
            lines.push(format!("- **stopped** {error:#}"));
        }
        let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        ExitCode::SUCCESS
    }

    fn hunt(out: &Path, lines: &mut Vec<String>) -> Result<()> {
        let say = |lines: &mut Vec<String>, text: String| {
            println!("{text}");
            lines.push(text);
            let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        };
        let marker = MainThreadMarker::new().context("not on the main thread")?;
        let desktop = platform::current()?;
        let screen = desktop.screens()?.into_iter().next().context("no screen")?;
        let original = desktop.wallpaper(&screen).ok().flatten();
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let (width, height) = pixels(&screen);
        let home = std::env::temp_dir().join(format!("scooch-black-{}", std::process::id()));
        fs::create_dir_all(&home)?;
        let bin = std::env::current_exe()?.parent().and_then(Path::parent).map(|folder| folder.join("scooch")).context("no folder for the scooch program")?;
        let (one, two) = (tone(width, height, [60, 120, 210], [140, 190, 240]), tone(width, height, [235, 150, 70], [250, 215, 140]));
        let mean = |image: &RgbImage| {
            let shot = Shot { width: image.width() as usize, height: image.height() as usize, rgb: image.as_raw().clone() };
            region_tone(&shot, HUNT_REGION).1
        };
        let tones = (mean(&one), mean(&two));
        let (bytes_a, bytes_b) = (scooch_core::encode_jpeg(&one, 92)?, scooch_core::encode_jpeg(&two, 92)?);
        say(lines, format!("- screen `{}` {} at scale {}, display {display}, pictures {width}x{height}, A mean {:?}, B mean {:?}, original {:?}", screen.id, show(&screen.frame), screen.scale, tones.0, tones.1, original));
        say(lines, "- each capture is a band of the display from 10 to 90 percent across and 30 to 60 percent down, K black, A the first picture, B the second, ~ neither, captured from the close for 500 ms, times in ms from the close. Even trials capture the picture window alone before the close, odd trials do not, so the odd ones keep the timing of a slide".to_string());
        let mut saved = 0usize;
        let mut file = 0u32;
        for hunt in HUNTS.iter() {
            say(lines, String::new());
            say(lines, format!("## {}", hunt.name));
            say(lines, String::new());
            let env: Vec<(&str, &str)> = hunt.through.map(|through| (scooch::live::SEE_THROUGH, through)).into_iter().collect();
            let mut helper = if hunt.layer {
                let mut helper = scooch::live::Helper::spawn_with(&bin, Duration::from_secs(5), &env)?;
                let ready = helper.warm(scooch::live::Hints::default(), None, None, Duration::from_secs(30))?;
                say(lines, format!("- helper {} on {}", helper.pid, ready.adapter));
                Some(helper)
            } else {
                None
            };
            let (mut black_trials, mut black_captures, mut captures, mut ours_during) = (0, 0, 0, 0);
            let mut intervals = Vec::new();
            for trial in 1..=hunt.trials {
                file += 1;
                let (a, b) = (home.join(format!("a-{file}.jpg")), home.join(format!("b-{file}.jpg")));
                fs::write(&a, &bytes_a)?;
                fs::write(&b, &bytes_b)?;
                match hunt_trial(helper.as_mut(), hunt, trial, (&a, &b), (display, &screen), tones, (out, &mut saved)) {
                    Ok(tried) => {
                        let blacks: Vec<&Caught> = tried.caught.iter().filter(|caught| caught.sort == 'K').collect();
                        captures += tried.caught.len();
                        black_captures += blacks.len();
                        ours_during += blacks.iter().filter(|caught| caught.ours).count();
                        if !blacks.is_empty() {
                            black_trials += 1;
                        }
                        intervals.extend(tried.caught.iter().map(|caught| caught.took));
                        let sorts: String = tried.caught.iter().map(|caught| caught.sort).collect();
                        let marks: Vec<String> = tried.caught.iter().filter(|caught| caught.sort != 'B').map(|caught| format!("{} at {:.0}+{:.0} ms ours listed {} dark {:.2} mean {:?}", caught.sort, caught.at, caught.took, caught.ours, caught.tone.0, caught.tone.1.map(|value| value.round()))).collect();
                        say(lines, format!("- trial {trial}: close {:.0} ms after the set, picture window before the close {:?}, captures {sorts}, not B {marks:?}, at the first black the picture window alone {:?} and the layer alone {:?}", tried.held, tried.under, tried.under_black, tried.ours_black));
                        if trial == 1 || !blacks.is_empty() {
                            say(lines, format!("  - windows at or below the desktop before {:?}", tried.before));
                            say(lines, format!("  - before the close {:?}", tried.armed));
                            say(lines, format!("  - after {:?}", tried.after));
                        }
                    }
                    Err(error) => say(lines, format!("- trial {trial}: stopped {error:#}")),
                }
            }
            drop(helper.take());
            intervals.sort_by(f64::total_cmp);
            let middle = intervals.get(intervals.len() / 2).copied().unwrap_or(0.0);
            say(lines, format!("- summary: {black_trials} of {} trials had a black capture, {black_captures} black of {captures} captures, {ours_during} of the black captures began with the layer window still listed, median capture took {middle:.1} ms", hunt.trials));
        }
        if let Some(original) = original.filter(|path| path.is_file()) {
            let _ = desktop.set_wallpaper(&screen, &original);
        }
        let _ = fs::remove_dir_all(&home);
        Ok(())
    }

    const REVEAL_OPENS: u32 = 20;
    const REVEAL_WATCH: Duration = Duration::from_millis(150);
    const REVEAL_STRIDE: usize = 4;

    fn helper_windows(pid: u32) -> Vec<i64> {
        layer::every_window().into_iter().filter(|window| window.pid == i64::from(pid)).map(|window| window.number).collect()
    }

    fn new_window(pid: u32, screen: &Screen, before: &[i64]) -> Option<i64> {
        layer::listing().into_iter().find(|window| window.pid == i64::from(pid) && !before.contains(&window.number) && window.bounds.x < screen.frame.x + screen.frame.w && screen.frame.x < window.bounds.x + window.bounds.w && window.bounds.w >= screen.frame.w * 0.9).map(|window| window.number)
    }

    fn sampled(shot: &Shot) -> Vec<[u8; 3]> {
        (0..shot.height).step_by(REVEAL_STRIDE).flat_map(|y| (0..shot.width).step_by(REVEAL_STRIDE).map(move |x| (x, y))).map(|(x, y)| shot.at(x, y)).collect()
    }

    fn black(pixels: &[[u8; 3]]) -> bool {
        !pixels.is_empty() && pixels.iter().filter(|pixel| pixel.iter().all(|channel| *channel <= 8)).count() * 100 >= pixels.len() * 95
    }

    fn largest_gap(a: &[[u8; 3]], b: &[[u8; 3]]) -> Option<u8> {
        (a.len() == b.len() && !a.is_empty()).then(|| a.iter().zip(b).map(|(one, two)| (0..3).map(|channel| one[channel].abs_diff(two[channel])).max().unwrap_or(0)).max().unwrap_or(0))
    }

    fn reveal_check(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let screen = platform::current()?.screens()?.into_iter().next().context("no screen")?;
        let (width, height) = pixels(&screen);
        let file = p.home.join(format!("probe-still-{width}x{height}.jpg"));
        if !file.is_file() {
            still_file(&file, &test_still(width, height))?;
        }
        p.line("- the capture by window number sees the layer while it is still hidden, so it tells whether the window server has the first frame before the layer is shown. It cannot see the unhide itself, and a flash shorter than the gap between two captures can fall between them".to_string());
        match reveal_captures(p, &screen, "default transaction present", None) {
            Ok(seen) => {
                if seen.bad > 0 {
                    p.fail(format!("{} of {REVEAL_OPENS} opens with the default reveal had a black or empty capture after shown", seen.bad));
                }
                if seen.saw < REVEAL_CONTROL {
                    p.fail(format!("the control was blind, it saw an empty layer in {} of {REVEAL_OPENS} opens of the default reveal where at least {REVEAL_CONTROL} are needed, so the reveal is not proven on this Mac", seen.saw));
                }
            }
            Err(error) => p.fail(format!("the reveal captures with the default reveal could not run {error:#}")),
        }
        if let Err(error) = reveal_captures(p, &screen, "spin, reported only", Some(scooch::live::SPIN)) {
            p.line(format!("- the reveal captures with the spin could not run {error:#}"));
        }
        Ok(())
    }

    const REVEAL_CONTROL: usize = 3;
    const REVEAL_QUICK: Duration = Duration::from_millis(8);

    struct Revealed {
        saw: usize,
        bad: usize,
    }

    struct Opened {
        saw: bool,
        bad: bool,
        quick: bool,
        reveal: Option<u64>,
        shown: Option<u128>,
    }

    fn reveal_open(p: &mut Probe, helper: &mut scooch::live::Helper, screen: &Screen, file: &Path, id: u32) -> Result<Opened> {
        let before = helper_windows(helper.pid);
        helper.ask_open(id, screen, &scooch::live::Stills::of(file))?;
        let asked = Instant::now();
        let (mut number, mut shown, mut failed, mut drawn) = (None, None::<Instant>, None, None);
        let mut captures: Vec<(Instant, Option<Vec<[u8; 3]>>)> = Vec::new();
        while asked.elapsed() < Duration::from_secs(5) && failed.is_none() && shown.is_none_or(|shown| shown.elapsed() < REVEAL_WATCH) {
            if number.is_none() {
                number = new_window(helper.pid, screen, &before);
            }
            if let Some(number) = number {
                let at = Instant::now();
                captures.push((at, grab_window(number).map(|shot| sampled(&shot))));
            }
            let wait = if number.is_some() { Duration::ZERO } else { Duration::from_millis(2) };
            while let Some(reply) = helper.next(wait) {
                match reply {
                    scooch::live::Reply::Shown { id: got, .. } if got == id => shown = Some(helper.arrived().unwrap_or_else(Instant::now)),
                    scooch::live::Reply::Drawn { id: got, reveal, revealing, .. } if got == id => drawn = Some((reveal, revealing)),
                    scooch::live::Reply::Failed { id: got, why } if got.is_none_or(|got| got == id) => failed = Some(why),
                    _ => {}
                }
                if wait.is_zero() {
                    continue;
                }
                break;
            }
        }
        if drawn.is_none() && shown.is_some() {
            drawn = helper
                .expect(Duration::from_millis(500), |reply| match reply {
                    scooch::live::Reply::Drawn { id: got, reveal, revealing, .. } if *got == id => Some(Ok((*reveal, revealing.clone()))),
                    _ => None,
                })
                .ok();
        }
        std::thread::sleep(Duration::from_millis(400));
        let settled = number.and_then(grab_window).map(|shot| sampled(&shot));
        let _ = helper.close(id, Duration::from_secs(3));
        let Some(shown) = shown else {
            p.line(format!("  - open {id}: no shown within 5 s, window {number:?}, {failed:?}"));
            return Ok(Opened { saw: false, bad: false, quick: false, reveal: None, shown: None });
        };
        let (early, late): (Vec<_>, Vec<_>) = captures.iter().partition(|(at, _)| *at < shown);
        let empty = |list: &[&(Instant, Option<Vec<[u8; 3]>>)]| list.iter().filter(|(_, shot)| shot.as_ref().is_none_or(Vec::is_empty)).count();
        let dark = |list: &[&(Instant, Option<Vec<[u8; 3]>>)]| list.iter().filter(|(_, shot)| shot.as_deref().is_some_and(black)).count();
        let largest = late.iter().filter_map(|(_, shot)| shot.as_deref().zip(settled.as_deref()).and_then(|(shot, settled)| largest_gap(shot, settled))).max();
        let seen_after = late.len() - empty(&late) - dark(&late);
        let saw = empty(&early) + dark(&early) > 0 && seen_after > 0;
        let bad = empty(&late) + dark(&late) > 0;
        let to_shown = shown.saturating_duration_since(asked).as_millis();
        let times = |list: &[&(Instant, Option<Vec<[u8; 3]>>)]| list.iter().map(|(at, _)| at.saturating_duration_since(asked).as_millis()).collect::<Vec<_>>();
        p.line(format!(
            "  - open {id}: window {number:?}, Open to shown {to_shown} ms, before shown {} captures at {:?} ms after Open with {} black and {} empty, after shown {} captures at {:?} ms with {} black and {} empty, largest difference after shown from the settled layer {largest:?} of 255, settled capture black {:?}, the control saw the hidden layer {saw}, reveal and its steps {drawn:?}",
            early.len(),
            times(&early),
            dark(&early),
            empty(&early),
            late.len(),
            times(&late),
            dark(&late),
            empty(&late),
            settled.as_deref().map(black)
        ));
        let quick = late.first().is_some_and(|(at, _)| at.saturating_duration_since(shown) <= REVEAL_QUICK);
        Ok(Opened { saw, bad, quick, reveal: drawn.map(|(reveal, _)| reveal), shown: Some(to_shown) })
    }

    fn spread(values: &[u128]) -> String {
        let mut sorted = values.to_vec();
        sorted.sort_unstable();
        match (sorted.get(sorted.len() / 2), sorted.first(), sorted.last()) {
            (Some(middle), Some(low), Some(high)) => format!("median {middle} ms from {low} to {high} ms"),
            _ => "none".to_string(),
        }
    }

    fn reveal_captures(p: &mut Probe, screen: &Screen, label: &str, switch: Option<&str>) -> Result<Revealed> {
        let (width, height) = pixels(screen);
        let file = p.home.join(format!("probe-still-{width}x{height}.jpg"));
        let env: Vec<(&str, &str)> = switch.map(|name| (name, "1")).into_iter().collect();
        let mut helper = scooch::live::Helper::spawn_with(&p.bin, Duration::from_secs(5), &env)?;
        helper.warm(scooch::live::Hints::default(), None, None, Duration::from_secs(30))?;
        p.line(format!("- reveal captures with the {label}, {REVEAL_OPENS} opens of the helper's layer over the test still, the helper's new window captured alone by its number from the moment it is listed until {} ms after shown, every {REVEAL_STRIDE}th pixel compared, the captures before shown are the control", REVEAL_WATCH.as_millis()));
        let mut opened = Vec::new();
        for id in 1..=REVEAL_OPENS {
            opened.push(reveal_open(p, &mut helper, screen, &file, id)?);
        }
        drop(helper);
        let saw = opened.iter().filter(|open| open.saw).count();
        let bad = opened.iter().filter(|open| open.bad).count();
        let reveals: Vec<u128> = opened.iter().filter_map(|open| open.reveal.map(u128::from)).collect();
        let shown: Vec<u128> = opened.iter().filter_map(|open| open.shown).collect();
        let quick = opened.iter().filter(|open| open.quick).count();
        p.line(format!(
            "  - with the {label}: the control saw the hidden layer in {saw} of {REVEAL_OPENS} opens, in every open {}, opens with a black or empty capture after shown {bad} of {REVEAL_OPENS}, opens whose first capture after shown began within {} ms of it {quick} of {REVEAL_OPENS}, reveal {}, Open to shown {}",
            saw == opened.len(),
            REVEAL_QUICK.as_millis(),
            spread(&reveals),
            spread(&shown)
        ));
        Ok(Revealed { saw, bad })
    }

    fn modes(display: u32) -> Vec<(usize, usize, usize, usize, bool, *const c_void)> {
        unsafe {
            let keys = [kCGDisplayShowDuplicateLowResolutionModes];
            let values = [kCFBooleanTrue];
            let options = CFDictionaryCreate(std::ptr::null(), keys.as_ptr(), values.as_ptr(), 1, &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
            let list = CGDisplayCopyAllDisplayModes(display, options);
            let mut found = Vec::new();
            if !list.is_null() {
                for index in 0..CFArrayGetCount(list) {
                    let mode = CFArrayGetValueAtIndex(list, index);
                    found.push((CGDisplayModeGetWidth(mode), CGDisplayModeGetHeight(mode), CGDisplayModeGetPixelWidth(mode), CGDisplayModeGetPixelHeight(mode), CGDisplayModeIsUsableForDesktopGUI(mode), CGDisplayModeRetain(mode)));
                }
                CFRelease(list);
            }
            if !options.is_null() {
                CFRelease(options);
            }
            found
        }
    }

    fn switch_mode(display: u32, mode: *const c_void) -> bool {
        unsafe {
            let mut config: *mut c_void = std::ptr::null_mut();
            if CGBeginDisplayConfiguration(&mut config) != 0 {
                return false;
            }
            CGConfigureDisplayWithDisplayMode(config, display, mode, std::ptr::null());
            CGCompleteDisplayConfiguration(config, 1) == 0
        }
    }

    fn colour_scale(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let desktop = platform::current()?;
        let screen = desktop.screens()?.into_iter().next().context("no screen")?;
        let recorded = desktop.wallpaper(&screen).ok().flatten();
        still_through_helper(p, &screen, "native mode")?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let current = unsafe { CGDisplayCopyDisplayMode(display) };
        let listed = modes(display);
        p.line(format!("- display modes, points and pixels {:?}", listed.iter().map(|(w, h, pw, ph, usable, _)| format!("{w}x{h} as {pw}x{ph}{}", if *usable { "" } else { " unusable" })).collect::<Vec<_>>()));
        let doubled = listed.iter().filter(|(w, _, pw, _, usable, _)| *usable && *pw == 2 * *w && *w >= 640).max_by_key(|(w, _, _, _, _, _)| *w).map(|mode| mode.5);
        match doubled {
            Some(mode) if switch_mode(display, mode) => {
                std::thread::sleep(Duration::from_secs(3));
                let scaled = platform::current()?.screens()?.into_iter().next().context("no screen after the switch")?;
                p.line(format!("- switched to a doubled mode, the screen now reads {} by {} points at scale {}", scaled.frame.w, scaled.frame.h, scaled.scale));
                let result = still_through_helper(p, &scaled, "HiDPI mode");
                let back = !current.is_null() && switch_mode(display, current);
                std::thread::sleep(Duration::from_secs(3));
                p.line(format!("- switched back to the first mode {back}"));
                result?;
            }
            Some(_) => p.limit("the runner listed a doubled display mode but would not switch to it, so scale 2 is unproven here"),
            None => p.limit("the runner's virtual display offers no doubled mode, so scale 2 is unproven here"),
        }
        for (_, _, _, _, _, mode) in listed {
            unsafe { CGDisplayModeRelease(mode) };
        }
        if !current.is_null() {
            unsafe { CGDisplayModeRelease(current) };
        }
        if let Some(recorded) = recorded.filter(|path| path.is_file()) {
            let _ = desktop.set_wallpaper(&screen, &recorded);
        }
        Ok(())
    }

    struct Seen {
        at: f64,
        took: f64,
        half: RgbImage,
        inked: f32,
        step: Spot,
        bare: bool,
    }

    #[derive(Clone, Copy, Debug, Default)]
    struct Spot {
        worst: u8,
        x: usize,
        y: usize,
        before: [u8; 3],
        after: [u8; 3],
    }

    fn spot_where(a: &Shot, b: &Shot, keep: &impl Fn(usize, usize) -> bool) -> Spot {
        let mut found = Spot::default();
        for y in 0..a.height.min(b.height) {
            for x in 0..a.width.min(b.width) {
                if !keep(x, y) {
                    continue;
                }
                let (one, two) = (a.at(x, y), b.at(x, y));
                let worst = (0..3).map(|channel| one[channel].abs_diff(two[channel])).max().unwrap_or(0);
                if worst > found.worst {
                    found = Spot { worst, x, y, before: one, after: two };
                }
            }
        }
        found
    }

    fn spot_outside(a: &Shot, b: &Shot, from_row: usize, skip: Option<[usize; 4]>) -> Spot {
        let mut found = Spot::default();
        for y in from_row..a.height.min(b.height) {
            for x in 0..a.width.min(b.width) {
                if skip.is_some_and(|[left, top, right, bottom]| x >= left && x < right && y >= top && y < bottom) {
                    continue;
                }
                let (one, two) = (a.at(x, y), b.at(x, y));
                let worst = (0..3).map(|channel| one[channel].abs_diff(two[channel])).max().unwrap_or(0);
                if worst > found.worst {
                    found = Spot { worst, x, y, before: one, after: two };
                }
            }
        }
        found
    }

    struct Dragged {
        dropped: f64,
        skip: Option<[usize; 4]>,
        seen: Vec<Seen>,
        moment: Option<String>,
        log: Vec<String>,
        old: Option<PathBuf>,
        old_still: Option<RgbImage>,
        old_tables: Option<Vec<u8>>,
    }

    fn half_shot(shot: &Shot) -> RgbImage {
        let (width, height) = (shot.width / 2, shot.height / 2);
        let mut rgb = Vec::with_capacity(width * height * 3);
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&shot.at(x * 2, y * 2));
            }
        }
        RgbImage::from_raw(width as u32, height as u32, rgb).unwrap_or_default()
    }

    fn halved(image: &RgbImage) -> RgbImage {
        let (width, height) = (image.width() / 2, image.height() / 2);
        let mut rgb = Vec::with_capacity((width * height * 3) as usize);
        for y in 0..height {
            for x in 0..width {
                rgb.extend_from_slice(&image.get_pixel(x * 2, y * 2).0);
            }
        }
        RgbImage::from_raw(width, height, rgb).unwrap_or_default()
    }

    fn stamp_of(line: &str) -> Option<f64> {
        line.split_whitespace().next()?.parse().ok()
    }

    fn helper_pids(p: &mut Probe) -> Vec<i32> {
        p.tool("/usr/bin/pgrep", &["-f", "live-helper"]).out.lines().filter_map(|line| line.trim().parse().ok()).collect()
    }

    fn strong_ink(baseline: &Shot, field: &Shot, rect: &Rect, screen: &Screen) -> Vec<(usize, usize)> {
        let scale = screen.scale;
        let mut ink = Vec::new();
        for y in (((rect.y - screen.frame.y) * scale).max(0.0) as usize..((rect.y - screen.frame.y + rect.h) * scale) as usize).step_by(2) {
            for x in (((rect.x - screen.frame.x) * scale).max(0.0) as usize..((rect.x - screen.frame.x + rect.w) * scale) as usize).step_by(2) {
                let (one, two) = (baseline.at(x, y), field.at(x, y));
                if (0..3).map(|channel| one[channel].abs_diff(two[channel]) as u32).sum::<u32>() > 150 {
                    ink.push((x, y));
                }
            }
        }
        ink
    }

    fn tables_of(p: &Probe, output: &Path) -> Option<Vec<u8>> {
        let name = output.file_name()?.to_string_lossy().into_owned();
        let found = fs::read_dir(p.home.join("cache").join("tables")).ok()?.flatten().find(|file| file.file_name().to_string_lossy().starts_with(&format!("{name}.")) && file.file_name().to_string_lossy().ends_with(".tables"))?;
        fs::read(found.path()).ok()
    }

    fn watch_drag(p: &mut Probe, display: u32, screen: &Screen, from: (f32, f32), to: (f32, f32), masks: &[Vec<(usize, usize)>]) -> Option<Dragged> {
        let baseline = grab(display)?;
        let old = platform::current().ok().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let old_tables = old.as_deref().and_then(|old| tables_of(p, old));
        let old_still = old.as_deref().and_then(|path| objc2::rc::autoreleasepool(|_| picture::decode(path)).ok()).map(|image| halved(&image));
        let menu = (30.0 * screen.scale) as usize;
        let lines = daemon_log(p).lines().count();
        let grow = 6usize;
        let skip = finder::named(&finder::view()).ok().and_then(|items| items.into_iter().map(|(_, rect)| rect).find(|rect| from.0 >= rect.x && from.0 <= rect.x + rect.w && from.1 >= rect.y && from.1 <= rect.y + rect.h)).map(|rect| {
            let pixel = |value: f32, origin: f32| ((value - origin) * screen.scale).max(0.0) as usize;
            let (left, top) = (pixel(rect.x + to.0 - from.0, screen.frame.x), pixel(rect.y + to.1 - from.1, screen.frame.y));
            let (right, bottom) = (pixel(rect.x + rect.w + to.0 - from.0, screen.frame.x), pixel(rect.y + rect.h + to.1 - from.1, screen.frame.y));
            [left.saturating_sub(grow), top.saturating_sub(grow), right + grow, bottom + grow]
        });
        drag(from, to);
        let dropped = epoch();
        let started = Instant::now();
        let mut seen = Vec::new();
        let mut previous: Option<Shot> = None;
        let mut done: Option<f64> = None;
        let mut checks = 0;
        let mut moment = None;
        while started.elapsed() < Duration::from_secs(8) {
            let bare = !layer::listing().iter().any(|window| window.owner == "scooch");
            let at = epoch();
            let Some(shot) = grab(display) else { continue };
            let took = epoch() - at;
            if moment.is_none() && quick_dark(&shot) >= crate::BLACK_SHARE {
                let windows = below_desktop();
                let paper = picture_window(screen).and_then(|window| grab_window(window.number)).map(|shot| quick_dark(&shot));
                let ours: Vec<Option<f32>> = layer::listing().into_iter().filter(|window| window.owner == "scooch").map(|window| grab_window(window.number).map(|shot| quick_dark(&shot))).collect();
                moment = Some(format!("a black capture began {:.0} ms after the drop with the layer listed {}, right after it the windows at or below the desktop {windows:?}, dark share of the picture window alone {paper:?} and of the layer windows alone {ours:?}", (at - dropped) * 1000.0, !bare));
            }
            let inked = masks.iter().map(|ink| ink.iter().filter(|(x, y)| !near(shot.at(*x, *y), baseline.at(*x, *y))).count() as f32 / ink.len().max(1) as f32).fold(0.0, f32::max);
            let step = previous.as_ref().map_or_else(Spot::default, |previous| spot_outside(previous, &shot, menu, skip));
            seen.push(Seen { at, took, half: half_shot(&shot), inked, step, bare });
            previous = Some(shot);
            checks += 1;
            if done.is_none() && checks % 3 == 0 && new_lines(&daemon_log(p), lines).iter().any(|line| line.contains(" tick ") && !line.contains("Unchanged")) {
                done = Some(epoch());
            }
            if done.is_some_and(|done| epoch() > done + 1.0) {
                break;
            }
        }
        let log = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ") || line.contains(" tick ")).collect();
        Some(Dragged { dropped, skip, seen, moment, log, old, old_still, old_tables })
    }

    struct Frames {
        stream: *mut c_void,
        queue: *mut c_void,
        seen: Arc<Mutex<Vec<(u64, f32)>>>,
        idle: Arc<std::sync::atomic::AtomicUsize>,
        offset: f64,
    }

    const BGRA: i32 = 0x4247_5241;
    const READ_ONLY: u32 = 1;

    fn surface_dark(surface: *mut c_void) -> Option<f32> {
        if surface.is_null() || unsafe { IOSurfaceLock(surface, READ_ONLY, std::ptr::null_mut()) } != 0 {
            return None;
        }
        let (width, height, row, base) = unsafe { (IOSurfaceGetWidth(surface), IOSurfaceGetHeight(surface), IOSurfaceGetBytesPerRow(surface), IOSurfaceGetBaseAddress(surface) as *const u8) };
        let (mut dark, mut count) = (0usize, 0usize);
        if !base.is_null() {
            for y in (height / 10..height * 85 / 100).step_by(4) {
                for x in (width / 20..width * 95 / 100).step_by(4) {
                    let pixel = unsafe { std::slice::from_raw_parts(base.add(y * row + x * 4), 3) };
                    count += 1;
                    if pixel.iter().all(|channel| *channel <= 12) {
                        dark += 1;
                    }
                }
            }
        }
        unsafe { IOSurfaceUnlock(surface, READ_ONLY, std::ptr::null_mut()) };
        (count > 0).then(|| dark as f32 / count as f32)
    }

    fn mach_seconds(ticks: u64) -> f64 {
        let mut base = Timebase::default();
        unsafe { mach_timebase_info(&mut base) };
        if base.denom == 0 {
            return ticks as f64 / 1e9;
        }
        ticks as f64 * f64::from(base.numer) / f64::from(base.denom) / 1e9
    }

    impl Frames {
        fn start(display: u32, screen: &Screen) -> Option<Frames> {
            let (width, height) = pixels(screen);
            let seen: Arc<Mutex<Vec<(u64, f32)>>> = Arc::new(Mutex::new(Vec::new()));
            let idle = Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let (kept, waited) = (seen.clone(), idle.clone());
            let handler = block2::RcBlock::new(move |status: i32, time: u64, surface: *mut c_void, _update: *const c_void| {
                if status == 1 {
                    waited.fetch_add(1, Ordering::SeqCst);
                }
                if status != 0 {
                    return;
                }
                if let (Some(dark), Ok(mut kept)) = (surface_dark(surface), kept.lock()) {
                    kept.push((time, dark));
                }
            });
            let queue = unsafe { dispatch_queue_create(c"scooch-probe-frames".as_ptr(), std::ptr::null()) };
            let stream = unsafe { CGDisplayStreamCreateWithDispatchQueue(display, (width / 4).max(1) as usize, (height / 4).max(1) as usize, BGRA, std::ptr::null(), queue, &handler) };
            if stream.is_null() {
                unsafe { dispatch_release(queue) };
                return None;
            }
            let offset = epoch() - mach_seconds(unsafe { mach_absolute_time() });
            if unsafe { CGDisplayStreamStart(stream) } != 0 {
                unsafe {
                    CFRelease(stream);
                    dispatch_release(queue);
                }
                return None;
            }
            Some(Frames { stream, queue, seen, idle, offset })
        }

        fn stop(self) -> (Vec<(f64, f32)>, usize) {
            unsafe { CGDisplayStreamStop(self.stream) };
            std::thread::sleep(Duration::from_millis(100));
            unsafe {
                CFRelease(self.stream);
                dispatch_release(self.queue);
            }
            let seen = self.seen.lock().map(|seen| seen.clone()).unwrap_or_default();
            (seen.into_iter().map(|(time, dark)| (self.offset + mach_seconds(time), dark)).collect(), self.idle.load(Ordering::SeqCst))
        }
    }

    fn quick_dark(shot: &Shot) -> f32 {
        let (top, bottom) = (shot.height / 10, shot.height * 85 / 100);
        let (left, right) = (shot.width / 20, shot.width * 95 / 100);
        let samples: Vec<[u8; 3]> = (top..bottom).step_by(8).flat_map(|y| (left..right).step_by(8).map(move |x| (x, y))).map(|(x, y)| shot.at(x, y)).collect();
        samples.iter().filter(|pixel| pixel.iter().all(|channel| *channel <= 12)).count() as f32 / samples.len().max(1) as f32
    }

    fn logged(dragged: &Dragged, word: &str) -> Option<f64> {
        dragged.log.iter().find(|line| line.contains(word)).and_then(|line| stamp_of(line))
    }

    fn mean_gap(a: &RgbImage, b: &RgbImage, region: &[usize]) -> f32 {
        if region.is_empty() || a.dimensions() != b.dimensions() {
            return f32::MAX;
        }
        let (x, y) = (a.as_raw(), b.as_raw());
        let total: u64 = region.iter().map(|index| (0..3).map(|channel| u64::from(x[index * 3 + channel].abs_diff(y[index * 3 + channel]))).sum::<u64>()).sum();
        total as f32 / (region.len() * 3) as f32
    }

    fn side(shot: &RgbImage, old: &RgbImage, new: &RgbImage, apart: &[usize]) -> char {
        if apart.is_empty() {
            return '=';
        }
        let (a, b) = (mean_gap(shot, old, apart), mean_gap(shot, new, apart));
        if a < 3.0 && a * 3.0 < b {
            'A'
        } else if b < 3.0 && b * 3.0 < a {
            'B'
        } else {
            '~'
        }
    }

    const SEEN_GAP: f64 = 0.125;

    struct Judged {
        rest: bool,
        enough: bool,
        moving: usize,
        rate: Option<f64>,
        gap: Option<f64>,
        frames: Option<u32>,
        faults: Vec<String>,
        recorded: crate::Recorded,
    }

    fn judge_glide(p: &mut Probe, dragged: &Dragged, screen: &Screen, label: &str, evidence: Option<&str>, quiet: bool) -> Judged {
        let desktop = platform::current().ok();
        let new = desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let decode = |path: &Option<PathBuf>| path.as_deref().and_then(|path| objc2::rc::autoreleasepool(|_| picture::decode(path)).ok()).map(|image| halved(&image));
        let (Some(old_still), Some(new_still)) = (dragged.old_still.clone(), decode(&new)) else {
            p.line(format!("- {label}: the two stills could not be read, old {:?} new {new:?}", dragged.old));
            return Judged { rest: false, enough: false, moving: 0, rate: None, gap: None, frames: None, faults: Vec::new(), recorded: crate::judge_record(&[], &[], dragged.dropped) };
        };
        let (width, height) = (old_still.width() as usize, old_still.height() as usize);
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let half = screen.scale / 2.0;
        let keep: Vec<usize> = (0..height)
            .step_by(2)
            .flat_map(|y| (0..width).step_by(2).map(move |x| (x, y)))
            .filter(|(x, y)| {
                let (px, py) = (*x as f32 / half + screen.frame.x, *y as f32 / half + screen.frame.y);
                py - screen.frame.y >= 30.0 && !icons.iter().any(|icon| px >= icon.x - 12.0 && px < icon.x + icon.w + 12.0 && py >= icon.y - 12.0 && py < icon.y + icon.h + 12.0)
            })
            .map(|(x, y)| y * width + x)
            .collect();
        let (o, n) = (old_still.as_raw(), new_still.as_raw());
        let apart: Vec<usize> = keep.iter().copied().filter(|index| (0..3).any(|channel| o[index * 3 + channel].abs_diff(n[index * 3 + channel]) > 24)).collect();
        let new_tables = new.as_deref().and_then(|new| tables_of(p, new));
        let frames: Vec<(f32, RgbImage)> = match (&dragged.old_tables, &new_tables, p.subject.clone()) {
            (Some(old_tables), Some(new_tables), Some(subject)) => objc2::rc::autoreleasepool(|_| picture::decode(&subject)).ok().map_or(Vec::new(), |image| {
                let canvas = scooch_core::canvas(&image, (width * 2) as u32, (height * 2) as u32);
                match (scooch_core::Bundle::tables_from_bytes(old_tables, canvas.clone()), scooch_core::Bundle::tables_from_bytes(new_tables, canvas)) {
                    (Ok(a), Ok(b)) => (0..=20).map(|step| step as f32 / 20.0).map(|s| (s, halved(&scooch_core::frame(&a, &b, s)))).collect(),
                    _ => Vec::new(),
                }
            }),
            _ => Vec::new(),
        };
        let classes: Vec<char> = dragged.seen.iter().map(|seen| side(&seen.half, &old_still, &new_still, &apart)).collect();
        let fitted: Vec<Option<(f32, f32)>> = dragged.seen.iter().map(|seen| frames.iter().map(|(s, frame)| (*s, mean_gap(&seen.half, frame, &keep))).min_by(|a, b| a.1.total_cmp(&b.1))).collect();
        let glide_at = logged(dragged, "shown and gliding");
        let settled = logged(dragged, " glide settled");
        let given_up = logged(dragged, " glide gave up the settle");
        let budget = logged(dragged, " glide skipped, settle budget");
        let landed = settled.or(given_up);
        let closed = logged(dragged, "after its wallpaper was set");
        let set = logged(dragged, " glide settle ");
        let glided = logged(dragged, " glide glided");
        let appeared = classes.iter().position(|class| *class == 'B');
        let returned = appeared.map_or(0, |first| classes[first..].iter().filter(|class| **class == 'A').count());
        let moving: Vec<usize> = (0..classes.len()).filter(|index| classes[*index] == '~' && fitted[*index].is_some_and(|(s, _)| s > 0.04 && s < 0.96)).collect();
        let inside: Vec<usize> = (0..classes.len()).filter(|index| glide_at.is_some_and(|at| dragged.seen[*index].at >= at) && landed.is_none_or(|landed| dragged.seen[*index].at <= landed) && fitted[*index].is_some()).collect();
        let window: Vec<(f32, f32)> = inside.iter().filter_map(|index| fitted[*index]).collect();
        let steady = crate::glide_progress(&window);
        let mut said = Vec::new();
        let drawn = crate::drew_frames(&dragged.log);
        let followed: Vec<(f64, f64, f32)> = inside.iter().filter_map(|index| fitted[*index].filter(|(_, gap)| *gap <= crate::UNEXPLAINED_FIT).map(|(s, _)| (dragged.seen[*index].at, dragged.seen[*index].at + dragged.seen[*index].took, s))).collect();
        let recorded = crate::judge_record(&drawn, &followed, dragged.dropped);
        let dark = |seen: &Seen| {
            let raw = seen.half.as_raw();
            keep.iter().filter(|index| raw.get(*index * 3..*index * 3 + 3).is_some_and(|pixel| pixel.iter().all(|channel| *channel <= 12))).count() as f32 / keep.len().max(1) as f32
        };
        let faults = crate::slide_faults(&dragged.seen.iter().enumerate().map(|(index, seen)| ((seen.at - dragged.dropped) * 1000.0, fitted[index].map(|(_, gap)| gap), dark(seen))).collect::<Vec<_>>());
        let settled_old = classes.iter().position(|class| *class == 'A');
        let first_move = settled_old.and_then(|from| (from..classes.len()).find(|index| classes[*index] != 'A' && classes[*index] != '=')).map(|index| (dragged.seen[index].at - dragged.dropped) * 1000.0);
        let span = glide_at.zip(glided).map(|(from, to)| to - from);
        let rate = span.map(|span| dragged.seen.iter().filter(|seen| glide_at.is_some_and(|at| seen.at >= at) && seen.at <= glide_at.unwrap_or(0.0) + span).count() as f64 / span.max(0.001));
        let expected = rate.map(|rate| rate * 0.45 * 0.62);
        let gap = glide_at.zip(glided).and_then(|(from, to)| dragged.seen.windows(2).filter(|pair| pair[1].at >= from && pair[0].at <= to).map(|pair| pair[1].at - pair[0].at).reduce(f64::max));
        let frames_drawn = dragged.log.iter().find_map(|line| line.split(" glide glided ").nth(1).and_then(|rest| rest.split("with ").nth(1)).and_then(|rest| rest.split_whitespace().next()).and_then(|count| count.parse::<u32>().ok()));
        let inked = dragged.seen.iter().map(|seen| seen.inked).fold(0.0, f32::max);
        let handed = settled.and_then(|settled| dragged.seen.windows(2).filter(|pair| pair[0].at >= settled && closed.is_none_or(|closed| pair[1].at <= closed + 1.0)).map(|pair| &pair[1]).max_by_key(|seen| seen.step.worst));
        let fade = settled.and_then(|settled| dragged.seen.windows(2).find(|pair| pair[0].at < settled && pair[1].at >= settled).map(|pair| pair[1].step));
        let layered = settled.zip(closed).map(|(settled, closed)| dragged.seen.iter().filter(|seen| seen.at >= settled && seen.at < closed).count());
        let hand_over = settled.map(|_| handed.map_or(0, |seen| seen.step.worst));
        let bare: Vec<usize> = closed.map_or_else(Vec::new, |closed| (0..dragged.seen.len()).filter(|index| dragged.seen[*index].bare && dragged.seen[*index].at >= closed && dragged.seen[*index].at <= closed + 1.0).collect());
        let gone = bare.first().map(|index| dragged.seen[*index].at);
        let bare_new = bare.iter().all(|index| matches!(classes[*index], 'B' | '='));
        let bare_step = bare.iter().filter(|index| **index > 0 && bare.contains(&(**index - 1))).map(|index| dragged.seen[*index].step.worst).max().unwrap_or(0);
        let gave_up_cleanly = given_up.is_some() && budget.is_some() && !bare.is_empty() && bare_new && bare_step <= 2;
        let hand_spot = handed.map(|seen| format!("{} ms after the drop at pixel {},{} from {:?} to {:?}", ((seen.at - dragged.dropped) * 1000.0).round(), seen.step.x, seen.step.y, seen.step.before, seen.step.after));
        let at = |time: Option<f64>| time.map(|time| ((time - dragged.dropped) * 1000.0).round());
        said.push(format!(
            "- {label}: {} captures in {:.0} ms, {} pixels differ between the two stills, classes {}, {} moving between the two places, old picture after the new {returned}, glide progress steady {steady} over {} captures",
            dragged.seen.len(),
            dragged.seen.last().map_or(0.0, |seen| (seen.at - dragged.dropped) * 1000.0),
            apart.len(),
            classes.iter().collect::<String>(),
            moving.len(),
            window.len()
        ));
        said.push(format!("  - captures a second during the glide {:?}, largest gap between captures during the glide {:?} ms, so a 450 ms glide should show about {:?} captures with progress between 0.05 and 0.95, frames the helper drew {frames_drawn:?}", rate.map(|rate| rate.round()), gap.map(|gap| (gap * 1000.0).round()), expected.map(|value| value.round())));
        said.push(format!("  - drop to first moving frame on screen {:?} ms, glide sent {:?} ms, wallpaper set {:?} ms, glided {:?} ms, settled {:?} ms, settle given up {:?} ms, closed {:?} ms", first_move.map(f64::round), at(glide_at), at(set), at(glided), at(settled), at(given_up), at(closed)));
        let masked = dragged.skip.map_or("nothing masked, the dropped icon was not found".to_string(), |[left, top, right, bottom]| format!("outside the dropped icon's box of {}x{} px at {left},{top}, its Finder rect grown by 6 px", right - left, bottom - top));
        said.push(format!("  - largest share of a still icon's solid ink that changed {inked:.3}, largest change between captures from settled to one second after close {hand_over:?} of 255 {}, {masked}, captures of the settled layer before close {layered:?}, CPU frames to fit {}", hand_spot.unwrap_or_default(), frames.len()));
        let gone_ms = closed.zip(gone).map(|(closed, gone)| ((gone - closed) * 1000.0).round());
        if given_up.is_some() {
            let given_ms = dragged.log.iter().find_map(|line| line.split(" glide gave up the settle after ").nth(1)).and_then(|rest| rest.split_whitespace().next()).and_then(|ms| ms.parse::<u64>().ok());
            let stayed = glided.zip(gone).map(|(glided, gone)| ((gone - glided) * 1000.0).round());
            said.push(format!("  - the daemon gave up the settle after {given_ms:?} ms with its skip line logged {}, the glide's last frame stayed on screen {stayed:?} ms until the layer was gone, the first capture with no layer window left began {gone_ms:?} ms after the close was sent, the hand over judged on the {} captures that began with no layer window left, all of them the new picture {bare_new}, largest change between them {bare_step} of 255", budget.is_some(), bare.len()));
        } else {
            said.push(format!("  - the first capture with no layer window left began {gone_ms:?} ms after the close was sent"));
        }
        said.push(format!("  - the change across the settle fade from the glide's last frame to the still, which ends before settled, {:?} of 255 at pixel {:?} from {:?} to {:?}", fade.map(|step| step.worst), fade.map(|step| (step.x, step.y)), fade.map(|step| step.before), fade.map(|step| step.after)));
        said.push(format!("  - daemon log {:?}", dragged.log.iter().map(|line| line.chars().take(200).collect::<String>()).collect::<Vec<_>>()));
        if let Some(name) = evidence {
            let folder = p.out.join(name);
            let _ = fs::create_dir_all(&folder);
            let mut table = vec!["| capture | ms after drop | class | fitted progress | gap to it | gap to old | gap to new |".to_string(), "|---|---|---|---|---|---|---|".to_string()];
            for (index, seen) in dragged.seen.iter().enumerate() {
                let ms = ((seen.at - dragged.dropped) * 1000.0).round();
                let fit = fitted[index];
                let _ = scooch_core::encode_jpeg(&seen.half, 85).map(|bytes| fs::write(folder.join(format!("{index:03}-{ms}ms-{}.jpg", classes[index])), bytes));
                table.push(format!("| {index} | {ms} | {} | {} | {} | {:.2} | {:.2} |", classes[index], fit.map_or("none".into(), |(s, _)| format!("{s:.2}")), fit.map_or("none".into(), |(_, gap)| format!("{gap:.2}")), mean_gap(&seen.half, &old_still, &apart), mean_gap(&seen.half, &new_still, &apart)));
            }
            let _ = fs::write(folder.join("captures.md"), table.join("\n") + "\n");
            let picked: Vec<&Seen> = dragged.seen.iter().filter(|seen| landed.is_none_or(|landed| seen.at <= landed + 0.1)).collect();
            if picked.len() >= 9 {
                let chosen: Vec<RgbImage> = (0..9).map(|index| halved(&picked[index * (picked.len() - 1) / 8].half)).collect();
                let (w, h) = (chosen[0].width(), chosen[0].height());
                let mut rgb = vec![0u8; (w * 9 * h * 3) as usize];
                for (index, image) in chosen.iter().enumerate() {
                    for y in 0..h.min(image.height()) {
                        for x in 0..w.min(image.width()) {
                            let at = ((y * w * 9 + index as u32 * w + x) * 3) as usize;
                            rgb[at..at + 3].copy_from_slice(&image.get_pixel(x, y).0);
                        }
                    }
                }
                if let Some(strip) = RgbImage::from_raw(w * 9, h, rgb) {
                    let _ = scooch_core::encode_png(&strip).map(|bytes| fs::write(p.out.join(format!("{name}-strip.png")), bytes));
                }
            }
            said.push(format!("  - every capture saved in {name} with captures.md, nine frame strip {name}-strip.png"));
        }
        said.push(format!("  - the helper's record of what it presented, {} frames, {} moving between the two places, steady and ending on the new place {}, captures inside the glide that agree with it {}, that do not {:?}", drawn.len(), recorded.moving, recorded.steady, recorded.agreed, recorded.disagreed));
        if !quiet {
            for line in said {
                p.line(line);
            }
        }
        let landing = (settled.is_some() && hand_over.is_some_and(|value| value <= 2)) || gave_up_cleanly;
        Judged { rest: returned == 0 && steady && inked <= 0.1 && landing && faults.is_empty(), enough: gap.is_some_and(|gap| gap <= SEEN_GAP), moving: moving.len(), rate, gap, frames: frames_drawn, faults, recorded }
    }

    fn stuck(p: &mut Probe, pid: i32) {
        let file = p.out.join(format!("stuck-{pid}-{}.txt", now()));
        let _ = p.tool("/usr/bin/sample", &[&pid.to_string(), "2", "-file", &file.to_string_lossy()]);
        let text = fs::read_to_string(&file).unwrap_or_default();
        let ours: Vec<String> = text.lines().filter(|line| line.contains("scooch::") || line.contains("Thread_")).map(|line| line.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(160).collect()).take(40).collect();
        p.line(format!("  - sample of the daemon {pid} kept as {}, its threads and our frames {ours:?}", file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()));
        let log = daemon_log(p);
        let tail: Vec<&str> = log.lines().rev().take(25).collect::<Vec<_>>().into_iter().rev().collect();
        p.line(format!("  - the last lines of the daemon log {tail:?}"));
    }

    fn quiet_log(p: &Probe, quiet: Duration, limit: Duration) -> bool {
        let started = Instant::now();
        let mut count = daemon_log(p).lines().count();
        let mut since = Instant::now();
        while started.elapsed() < limit {
            std::thread::sleep(Duration::from_millis(100));
            let now = daemon_log(p).lines().count();
            if now != count {
                count = now;
                since = Instant::now();
            } else if since.elapsed() >= quiet {
                return true;
            }
        }
        false
    }

    fn wallpaper_after(p: &mut Probe, screen: &Screen, act: impl FnOnce(&mut Probe)) -> (Option<u128>, Vec<String>) {
        let desktop = platform::current().ok();
        let before = desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let lines = daemon_log(p).lines().count();
        act(p);
        let acted = Instant::now();
        let changed = wait_until(Duration::from_secs(12), || desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten()) != before).map(|_| acted.elapsed().as_millis());
        let home = p.home.clone();
        let _ = wait_until(Duration::from_secs(5), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains(" tick ")));
        std::thread::sleep(Duration::from_millis(600));
        let log = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ") || line.contains(" tick ")).map(|line| line.chars().take(if line.contains(" tick ") { 2000 } else { 180 }).collect()).collect();
        (changed, log)
    }

    fn commanded(line: &str) -> bool {
        ["glide open", "glide closed", "glide settle", "glide glided", "glide interrupted", "glide slid"].iter().any(|marker| line.contains(marker))
    }

    fn primed(line: &str) -> bool {
        line.contains(" glide primed ")
    }

    fn wait_parked(p: &mut Probe, since: usize, limit: Duration) -> Option<f64> {
        let home = p.home.clone();
        let mut found = None;
        let _ = wait_until(limit, || {
            found = crate::parked_in(&watch_log(&home.join("watch.log")).split_off(since.min(watch_log(&home.join("watch.log")).len())));
            found.is_some()
        });
        found
    }

    fn cover(marker: MainThreadMarker, screen: &Screen) -> Result<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
        use objc2::MainThreadOnly as _;
        let (display, _) = picture::screen_for(marker, &screen.id)?;
        let frame = display.frame();
        let window = unsafe {
            objc2_app_kit::NSWindow::initWithContentRect_styleMask_backing_defer_screen(objc2_app_kit::NSWindow::alloc(marker), objc2_foundation::NSRect::new(objc2_foundation::NSPoint::new(0.0, 0.0), frame.size), objc2_app_kit::NSWindowStyleMask::Borderless, objc2_app_kit::NSBackingStoreType::Buffered, false, Some(&display))
        };
        unsafe { window.setReleasedWhenClosed(false) };
        window.setFrame_display(frame, true);
        window.setOpaque(true);
        window.setBackgroundColor(Some(&objc2_app_kit::NSColor::blackColor()));
        window.orderFrontRegardless();
        spin(0.6);
        Ok(window)
    }

    fn motion_now(p: &mut Probe) -> Option<String> {
        let read = p.tool("/usr/bin/defaults", &["read", "com.apple.universalaccess", "reduceMotion"]);
        read.ok().then(|| read.out.trim().to_string())
    }

    fn set_motion(p: &mut Probe, reduce: bool) -> bool {
        if p.motion.is_none() {
            p.motion = Some(motion_now(p));
        }
        p.tool("/usr/bin/defaults", &["write", "com.apple.universalaccess", "reduceMotion", "-bool", if reduce { "true" } else { "false" }]).ok()
    }

    fn put_motion_back(p: &mut Probe) {
        let Some(before) = p.motion.take() else { return };
        let put = match &before {
            None => p.tool("/usr/bin/defaults", &["delete", "com.apple.universalaccess", "reduceMotion"]),
            Some(value) => p.tool("/usr/bin/defaults", &["write", "com.apple.universalaccess", "reduceMotion", "-bool", if value == "1" { "true" } else { "false" }]),
        };
        let after = motion_now(p);
        p.line(format!("- reduce motion put back as found, before {before:?} now {after:?} {}", if put.ok() || before.is_none() { "ok" } else { "failed" }));
        if after != before {
            p.fail("reduce motion was not put back as the probe found it");
        }
    }

    #[derive(Default)]
    struct Cold {
        tick: Option<String>,
        set: Option<u128>,
        glided: bool,
        skip: Option<String>,
        waited: Option<f64>,
        said: Option<String>,
        line: String,
        beside: String,
    }

    fn writer_threads(pid: i32) -> Vec<String> {
        threads(pid).into_iter().map(|(_, name, _, _)| name).filter(|name| ["scooch-still", "scooch-canvas", "scooch-quit"].contains(&name.as_str())).collect()
    }

    fn helpers_now() -> Vec<(i32, f64, f64)> {
        let pids: Vec<i32> = Command::new("/usr/bin/pgrep").args(["-f", "live-helper"]).output().map(|out| String::from_utf8_lossy(&out.stdout).lines().filter_map(|line| line.trim().parse().ok()).collect()).unwrap_or_default();
        let now = unsafe { mach_absolute_time() };
        pids.into_iter().filter_map(|pid| rusage(pid).map(|usage| (pid, nanos(now.saturating_sub(usage.started)) as f64 / 1e6, nanos(usage.user + usage.system) as f64 / 1e6))).collect()
    }

    fn quiet_daemon(p: &Probe, helpers: usize, limit: Duration) -> (u128, bool) {
        let started = Instant::now();
        let mut count = daemon_log(p).lines().count();
        let mut since = Instant::now();
        loop {
            std::thread::sleep(Duration::from_millis(50));
            let now = daemon_log(p).lines().count();
            if now != count {
                count = now;
                since = Instant::now();
            }
            let idle = since.elapsed() >= Duration::from_millis(1500) && scooch_pid(p).is_some_and(|pid| writer_threads(pid).is_empty()) && helpers_now().len() == helpers;
            if idle || started.elapsed() >= limit {
                return (started.elapsed().as_millis(), idle);
            }
        }
    }

    fn number_after(line: &str, key: &str) -> Option<f64> {
        line.split(key).nth(1).and_then(|rest| rest.split([',', ' ']).next()).and_then(|value| value.parse::<f64>().ok())
    }

    fn rounded(value: Option<f64>) -> String {
        value.map_or("none".to_string(), |value| format!("{}", value.round()))
    }

    fn move_line(log: &[String]) -> Option<String> {
        log.iter().find(|line| line.contains(" glide slid") || (line.contains(" glide skipped, ") && line.contains(" times from the "))).cloned()
    }

    fn cold_drag(p: &mut Probe, screen: &Screen, name: &str, (subject, aside): ((f32, f32), (f32, f32))) -> Cold {
        let desktop = platform::current().ok();
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let before = desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten());
        let lines = daemon_log(p).lines().count();
        drag(from, to);
        let dropped = epoch();
        let acted = Instant::now();
        let writers = scooch_pid(p).map(writer_threads).unwrap_or_default();
        let helpers = helpers_now();
        let set = wait_until(Duration::from_secs(12), || desktop.as_ref().and_then(|desktop| desktop.wallpaper(screen).ok().flatten()) != before).map(|_| acted.elapsed().as_millis());
        let after = helpers_now();
        let helper = helpers.iter().min_by(|a, b| a.1.total_cmp(&b.1)).map(|(pid, age, cpu)| format!("helper {pid} age {age:.0} ms at the drop, helper cpu {:.0} ms from the drop to the set", after.iter().find(|(later, _, _)| later == pid).map_or(0.0, |(_, _, later)| later - cpu))).unwrap_or_else(|| "no helper at the drop".to_string());
        let beside = format!("{helper}, {} helpers at the drop, writer threads at the drop {writers:?}", helpers.len());
        let home = p.home.clone();
        let _ = wait_until(Duration::from_secs(6), || move_line(&new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines)).is_some());
        std::thread::sleep(Duration::from_millis(800));
        let log = new_lines(&daemon_log(p), lines);
        let said = move_line(&log);
        Cold {
            tick: log.iter().find(|line| line.contains(" tick ") && line.contains("Rendered")).map(|line| {
                let took = line.split(" tick ").nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|ms| ms.parse::<f64>().ok()).unwrap_or(0.0);
                let began = stamp_of(line).map(|ended| ((ended - took / 1000.0 - dropped) * 1000.0).round());
                format!("began {began:?} ms after the drop, took {took} ms, {}", line.split(")] ").nth(1).unwrap_or_default().chars().take(1400).collect::<String>())
            }),
            set,
            glided: said.as_deref().is_some_and(|line| line.contains(" glide slid")),
            skip: said.as_deref().and_then(|line| line.split(" glide skipped, ").nth(1)).map(|rest| rest.split(',').next().unwrap_or(rest).to_string()),
            waited: said.as_deref().and_then(|line| number_after(line, ", waited ")),
            said,
            line: String::new(),
            beside,
        }
    }

    fn cold_one(p: &mut Probe, screen: &Screen, name: &str, places: ((f32, f32), (f32, f32)), (label, index): (&str, usize), last: &mut Instant) -> Cold {
        cold_noted(p, screen, name, places, (label, index), last, "")
    }

    fn cold_noted(p: &mut Probe, screen: &Screen, name: &str, places: ((f32, f32), (f32, f32)), (label, index): (&str, usize), last: &mut Instant, note: &str) -> Cold {
        let gap = last.elapsed().as_secs_f64();
        let cold = cold_drag(p, screen, name, places);
        *last = Instant::now();
        let line = format!(
            "  - {label} {}: {gap:.0} s after the move before it, {note}glided {}, skip {:?}, waited {}, set {:?} ms by the probe, launch took 0 ms, {}, tick laps {:?}, the daemon's line {:?}",
            index + 1,
            cold.glided,
            cold.skip,
            rounded(cold.waited),
            cold.set,
            cold.beside,
            cold.tick,
            cold.said.as_deref().map(|line| line.split(" glide ").nth(1).unwrap_or(line).chars().take(900).collect::<String>())
        );
        p.line(line.clone());
        Cold { line, ..cold }
    }

    fn phase(cold: &[Cold], key: &str) -> String {
        let values: Vec<f64> = cold.iter().filter_map(|cold| cold.said.as_deref().and_then(|line| number_after(line, key))).collect();
        stats(&values)
    }

    fn restart_daemon(p: &mut Probe, slow: bool) -> Result<bool> {
        restart_daemon_with(p, slow, None)
    }

    fn restart_daemon_with(p: &mut Probe, slow: bool, lag: Option<u64>) -> Result<bool> {
        let Some(mut child) = p.stand.take() else { return Ok(false) };
        let _ = child.kill();
        let _ = child.wait();
        stutters(p);
        p.struck = false;
        let _ = fs::remove_file(p.home.join("watch.pid"));
        let log = fs::OpenOptions::new().create(true).append(true).open(p.home.join("watch.log"))?;
        let mut command = Command::new(&p.bin);
        command.arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
        if slow {
            command.env(scooch::live::SLOW_HELLO, SLOW_HELLO_MS.to_string());
        } else {
            command.env_remove(scooch::live::SLOW_HELLO);
        }
        match lag {
            Some(ms) => command.env(scooch::live::READ_LAG, ms.to_string()),
            None => command.env_remove(scooch::live::READ_LAG),
        };
        p.stand = Some(command.spawn()?);
        let home = p.home.clone();
        wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.pid")).ok().and_then(|text| text.trim().parse::<i32>().ok()).is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)).context("the restarted daemon wrote no pid file")?;
        Ok(true)
    }

    const SLOW_HELLO_MS: u64 = 10000;
    const TRIES: usize = 3;

    fn laps_of(line: &str) -> Option<(f64, Vec<(String, f64)>)> {
        let rest = line.split("began Some(").nth(1)?;
        let began = rest.split(')').next()?.parse::<f64>().ok()?;
        let laps = rest.split(" ms after the drop, took ").nth(1)?.split(", sessions hold").next()?;
        Some((began, laps.split(", ").skip(1).filter_map(|lap| lap.rsplit_once(' ').and_then(|(name, value)| Some((name.to_string(), value.parse::<f64>().ok()?)))).collect()))
    }

    fn own_time(line: &str) -> Option<f64> {
        let (_, laps) = laps_of(line)?;
        let lap = |wanted: &str| laps.iter().find(|(name, _)| name == wanted).map_or(0.0, |(_, value)| *value);
        let launched = number_after(line, "launch took ").or_else(|| number_after(line, "in ms wake ").zip(number_after(line, ", launch ")).map(|(wake, launch)| (launch - wake).max(0.0)));
        Some(number_after(line, "waited ").unwrap_or(0.0) + lap("early") + lap("open") + launched.unwrap_or(0.0))
    }

    fn slow_said(slow: &[String], off: &[String]) -> (String, Vec<String>) {
        let (paired, judged) = crate::judge_pairs(slow, off);
        let footed: Vec<&String> = slow.iter().zip(&paired).filter(|(_, pair)| pair.footing.is_none()).map(|(line, _)| line).collect();
        let direct: Vec<Option<f64>> = footed.iter().map(|line| own_time(line)).collect();
        let bundles = footed.iter().filter(|line| laps_of(line).is_some_and(|(_, laps)| laps.iter().any(|(name, _)| name == "bundle"))).count();
        let round_of = |values: &[Option<f64>]| values.iter().map(|value| value.map_or("none".to_string(), |value| format!("{value:.0}"))).collect::<Vec<_>>().join(", ");
        let mut failures = Vec::new();
        let said = match &judged {
            crate::Slow::Judged { cpu, glide, daemon, wall, footed: count } => {
                if *cpu > crate::SLOW_LIMIT {
                    failures.push(format!("a slow helper made the tick thread spend {cpu:.0} ms more CPU from the wake to the plain swap than the glide off, the median of {count} quiet pairs dragged the same way, more than {} ms", crate::SLOW_LIMIT));
                }
                if *glide > crate::SLOW_LIMIT {
                    failures.push(format!("a slow helper held the plain swap back by {glide:.0} ms of glide pieces against the glide off, the median of {count} quiet pairs dragged the same way, more than {} ms", crate::SLOW_LIMIT));
                }
                if direct.iter().any(|value| value.is_none_or(|value| value > crate::SLOW_LIMIT)) {
                    failures.push("a slow helper held the plain swap back by more than 60 ms of the daemon's own time".to_string());
                }
                if bundles > 0 {
                    failures.push(format!("{bundles} of {} slow drags built glide tables before the plain swap", footed.len()));
                }
                format!(
                    "the slow pairs have footing in {count} of {}, each slow drag against its off drag by the tick thread's CPU from the wake to the set [{}] ms, median {cpu:.0} ms, by the glide pieces before the set [{}] ms, median {glide:.0} ms, limit {} ms for both, by the whole daemon's CPU [{}] ms, median {} ms, and by the wall clock from the drop to the set [{}] ms, median {} ms, neither judged, by the daemon's own time waited plus the early and open laps plus the launch per slow drag with footing [{}] ms, glide tables built before the plain swap by {bundles} of them",
                    paired.len(),
                    round_of(&paired.iter().map(|pair| pair.cpu).collect::<Vec<_>>()),
                    round_of(&paired.iter().map(|pair| pair.glide).collect::<Vec<_>>()),
                    crate::SLOW_LIMIT,
                    round_of(&paired.iter().map(|pair| pair.daemon).collect::<Vec<_>>()),
                    rounded(*daemon),
                    round_of(&paired.iter().map(|pair| pair.wall).collect::<Vec<_>>()),
                    rounded(*wall),
                    round_of(&direct)
                )
            }
            crate::Slow::Unfooted(why) => format!("the slow pairs have no footing, {why}"),
        };
        (said, failures)
    }

    fn slow_rounds(p: &mut Probe, screen: &Screen, name: &str, places: ((f32, f32), (f32, f32))) {
        let (slow, off) = quiet_round(p, screen, name, places);
        quiet_report(p, &slow, &off);
        let (said, failures) = slow_said(&slow, &off);
        p.line(format!("- {said}"));
        if crate::judge_pairs(&slow, &off).1.held_back().is_none() {
            p.fail(format!("the slow helper pairs had no footing after {TRIES} tries of each pair, so the plain swap was never judged, which is the footing failing and not a pass"));
        }
        for failure in failures {
            p.fail(failure);
        }
    }

    fn slow_check(paths: Vec<String>) -> ExitCode {
        let mut failed = false;
        for path in paths {
            let Ok(text) = fs::read_to_string(&path) else {
                println!("{path} could not be read");
                failed = true;
                continue;
            };
            println!("{path}");
            let (slow, off) = crate::chosen_pairs(&text, TRIES);
            let (said, failures) = slow_said(&slow, &off);
            println!("  {said}");
            for failure in &failures {
                println!("  fails, {failure}");
            }
            failed |= !failures.is_empty() || crate::judge_pairs(&slow, &off).1.held_back().is_none();
        }
        if failed {
            ExitCode::FAILURE
        } else {
            ExitCode::SUCCESS
        }
    }

    const QUIET_PAIRS: usize = 7;

    fn quiet_round(p: &mut Probe, screen: &Screen, name: &str, places: ((f32, f32), (f32, f32))) -> (Vec<String>, Vec<String>) {
        p.line(format!("- {QUIET_PAIRS} pairs where each timed drag goes from aside onto the subject after an untimed Apple Event move back aside, and waits for a quiet daemon first, its log quiet for 1.5 s, no thread of it writing a file and only the helper the drag expects alive, at most 4 s, and a pair without footing is dragged again up to {TRIES} tries"));
        let mut last = Instant::now();
        let (mut slow, mut off) = (Vec::new(), Vec::new());
        for index in 0..QUIET_PAIRS {
            for attempt in 1..=TRIES {
                let _ = place(p, name, places.1);
                poke(p);
                let (waited, idle) = quiet_daemon(p, 1, Duration::from_secs(4));
                let slow_line = cold_noted(p, screen, name, places, (&format!("try {attempt} slow"), index), &mut last, &format!("the daemon quiet after {waited} ms {idle}, ")).line;
                let _ = p.scooch(&["glide"], LIMIT);
                let _ = place(p, name, places.1);
                poke(p);
                let (waited, idle) = quiet_daemon(p, 0, Duration::from_secs(4));
                let off_line = cold_noted(p, screen, name, places, (&format!("try {attempt} off"), index), &mut last, &format!("the daemon quiet after {waited} ms {idle}, ")).line;
                let _ = p.scooch(&["glide"], LIMIT);
                let footing = crate::pair_of(&crate::drag_of(&slow_line), &crate::drag_of(&off_line)).footing;
                if footing.is_none() || attempt == TRIES {
                    slow.push(slow_line);
                    off.push(off_line);
                    break;
                }
                p.line(format!("  - pair {} try {attempt} had no footing, {}, so it is dragged again", index + 1, footing.unwrap_or_default()));
            }
        }
        (slow, off)
    }

    fn quiet_report(p: &mut Probe, slow: &[String], off: &[String]) {
        let (slow, off): (Vec<crate::Drag>, Vec<crate::Drag>) = (slow.iter().map(|line| crate::drag_of(line)).collect(), off.iter().map(|line| crate::drag_of(line)).collect());
        let paired: Vec<crate::Paired> = slow.iter().zip(&off).map(|(slow, off)| crate::pair_of(slow, off)).collect();
        let round = |values: Vec<Option<f64>>| values.iter().map(|value| value.map_or("none".to_string(), |value| format!("{value:.0}"))).collect::<Vec<_>>().join(", ");
        let footed: Vec<&crate::Paired> = paired.iter().filter(|pair| pair.footing.is_none()).collect();
        let walls: Vec<f64> = footed.iter().filter_map(|pair| pair.wall).collect();
        let cpus: Vec<f64> = footed.iter().filter_map(|pair| pair.cpu).collect();
        p.line(format!(
            "- quiet pairs with footing {} of {}, slow against off from the drop to the set [{}] ms, median {}, daemon cpu to the set slow against off [{}] ms, median {}, tick thread cpu slow [{}] off [{}] ms, from the wake to the set slow [{}] off [{}] ms, footing missed {:?}",
            footed.len(),
            paired.len(),
            round(paired.iter().map(|pair| pair.wall).collect()),
            rounded(crate::median(&walls)),
            round(paired.iter().map(|pair| pair.cpu).collect()),
            rounded(crate::median(&cpus)),
            round(slow.iter().map(|drag| drag.thread).collect()),
            round(off.iter().map(|drag| drag.thread).collect()),
            round(slow.iter().map(|drag| drag.wake_to_set()).collect()),
            round(off.iter().map(|drag| drag.wake_to_set()).collect()),
            paired.iter().filter_map(|pair| pair.footing.clone()).collect::<Vec<_>>()
        ));
        let mut names: Vec<String> = Vec::new();
        for drag in slow.iter().chain(&off) {
            for (name, _) in &drag.laps {
                if !names.contains(name) {
                    names.push(name.clone());
                }
            }
        }
        let pieces: Vec<String> = names.iter().map(|name| {
            let median_of = |drags: &[crate::Drag]| crate::median(&drags.iter().filter(|drag| drag.laps.iter().any(|(lap, _)| lap == name)).map(|drag| drag.lap(name)).collect::<Vec<_>>());
            format!("{name} {} against {}", rounded(median_of(&slow)), rounded(median_of(&off)))
        }).collect();
        p.line(format!("- quiet pairs, median of each piece slow against off in ms, {}", pieces.join(", ")));
    }

    fn cold_drags_check(p: &mut Probe) -> Result<()> {
        let since = daemon_log(p).lines().count();
        if restart_daemon(p, false)? {
            let calm = quiet_log(p, Duration::from_millis(1500), Duration::from_secs(15));
            p.line(format!("- the daemon restarted before the drags so they start with no strikes and a helper of its own, its log quiet after {calm}"));
        }
        let result = cold_drags(p, since);
        put_motion_back(p);
        if p.stand.is_some() {
            let _ = restart_daemon(p, false);
        }
        result
    }

    fn cold_drags(p: &mut Probe, since: usize) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let screen = main_screen(p)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.line("- the drags need Apple Events to Finder, which the open consent window blocks, so they were skipped".to_string());
            return Ok(());
        }
        if scooch_pid(p).is_none() {
            p.fail("no daemon runs for the drags after a start");
            return Ok(());
        }
        set_motion(p, false);
        poke(p);
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let aside = (screen.frame.x + screen.frame.w * 0.38, screen.frame.y + screen.frame.h * 0.5);
        let places = (subject, aside);
        if state(p)["glide"] != serde_json::json!(true) {
            let _ = p.scooch(&["glide"], LIMIT);
        }
        let parked = wait_parked(p, since, Duration::from_secs(60));
        p.line(format!("- after the restart the helper parked {} ms after its launch", parked.map_or("never".to_string(), |ms| ms.to_string())));
        if parked.is_none() {
            p.fail("the helper never parked within 60 s of the daemon's start, so the first move was not judged, which is the footing failing and not a pass");
            return Ok(());
        }
        let mut last = Instant::now();
        let mut moves = vec![("the first move after the start", cold_one(p, &screen, name, places, ("first after the start", 0), &mut last))];
        for (index, gap) in [10u64, 40, 40, 40].into_iter().enumerate() {
            std::thread::sleep(Duration::from_secs(gap).saturating_sub(last.elapsed()));
            let label = if gap == 10 { "a move 10 s after the last" } else { "a move 40 s after the last" };
            moves.push((label, cold_one(p, &screen, name, places, (&format!("{gap} s apart"), index), &mut last)));
        }
        let round: Vec<Cold> = moves.iter().map(|(_, cold)| Cold { tick: cold.tick.clone(), set: cold.set, glided: cold.glided, skip: cold.skip.clone(), waited: cold.waited, said: cold.said.clone(), line: cold.line.clone(), beside: cold.beside.clone() }).collect();
        p.line(format!("- moves after a start, 10 s and 40 s apart glided {} of {}, from the drop shown {}, first moving frame {}, set {}", round.iter().filter(|cold| cold.glided).count(), round.len(), phase(&round, ", shown "), phase(&round, ", first moving frame "), phase(&round, ", set ")));
        for (label, cold) in &moves {
            if !cold.glided {
                p.fail(format!("{label} did not slide with the helper parked, skipped as {:?}", cold.skip));
            }
        }
        old_tables(p, &screen, name, places);
        back_to_back(p, &screen, name, places)?;
        system_pick(p, &screen);
        if restart_daemon(p, true)? {
            p.line(format!("- the daemon restarted with {}={SLOW_HELLO_MS}, so each helper it starts says hello {} s late and has not parked, then {QUIET_PAIRS} pairs, each a drag soon after a helper started followed by the same drag with the glide off", scooch::live::SLOW_HELLO, SLOW_HELLO_MS / 1000));
            let home = p.home.clone();
            let _ = wait_until(Duration::from_secs(10), || fs::read_to_string(home.join("watch.log")).unwrap_or_default().lines().rev().take(40).any(|line| line.contains(" tick ")));
            slow_rounds(p, &screen, name, places);
            p.line(format!("- the glide setting after the pairs is {}", state(p)["glide"]));
            restart_daemon(p, false)?;
        } else {
            p.line("- the daemon here is the login item, so the slow helper hint could not be set and the drags before a park were skipped".to_string());
        }
        bare_wakes(p);
        built_not_shown(p, marker, &screen);
        prime_nothing(p, marker, &screen, name);
        Ok(())
    }

    fn built_not_shown(p: &mut Probe, marker: MainThreadMarker, screen: &Screen) {
        let before = ours().len();
        match layer::make(marker, screen) {
            Ok(built) => {
                let during = ours();
                let flagged = during.iter().filter(|window| window.on_screen).count();
                let listed = layer::listing().into_iter().filter(|window| window.pid == std::process::id() as i64).count();
                p.line(format!("- control in the probe's own process, a layer built and never ordered in, window number {}: every-window entries of the probe {before} before and {} with it, {flagged} of them flagged on screen, {listed} in the on-screen listing, layers {:?}", built.number(), during.len(), during.iter().map(|window| window.layer).collect::<Vec<_>>()));
                built.close();
                spin(0.2);
                p.line(format!("  - after closing it, every-window entries of the probe {}", ours().len()));
            }
            Err(error) => p.line(format!("- control in the probe's own process could not build a layer {error:#}")),
        }
    }

    fn click(at: (f32, f32)) {
        post(5, at);
        std::thread::sleep(Duration::from_millis(120));
        post(1, at);
        std::thread::sleep(Duration::from_millis(120));
        post(2, at);
    }

    fn empty_spot(screen: &Screen, icons: &[Rect]) -> (f32, f32) {
        let clear = |(x, y): (f32, f32)| icons.iter().all(|icon| x < icon.x - 60.0 || x > icon.x + icon.w + 60.0 || y < icon.y - 60.0 || y > icon.y + icon.h + 60.0);
        let frame = screen.frame;
        (2..9)
            .flat_map(|row| (1..10).map(move |column| (frame.x + frame.w * column as f32 / 10.0, frame.y + frame.h * row as f32 / 10.0)))
            .find(|spot| clear(*spot))
            .unwrap_or((frame.x + frame.w * 0.85, frame.y + frame.h * 0.75))
    }

    fn primes_from(p: &mut Probe, act: impl FnOnce()) -> (usize, usize, Vec<String>) {
        std::thread::sleep(Duration::from_millis(5500));
        let lines = daemon_log(p).lines().count();
        act();
        std::thread::sleep(Duration::from_millis(1200));
        let log = new_lines(&daemon_log(p), lines);
        let at_drag = log.iter().filter(|line| line.contains(" glide primed ") && line.ends_with("at the drag start")).count();
        let at_wake = log.iter().filter(|line| line.contains(" glide primed ") && line.ends_with("at the wake")).count();
        let starts = log.iter().filter_map(|line| line.split(" drag start at ").nth(1)).map(|rest| rest.split(", ").skip(1).collect::<Vec<_>>().join(", ")).collect();
        (at_drag, at_wake, starts)
    }

    fn prime_nothing(p: &mut Probe, marker: MainThreadMarker, screen: &Screen, name: &str) {
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let Some(icon) = position(p, name) else {
            p.fail("the probe icon was not found for the drags that must prime nothing");
            return;
        };
        let empty = empty_spot(screen, &icons);
        let mut said = Vec::new();
        let mut primed = 0;
        let clicked = primes_from(p, || click(icon));
        primed += clicked.0;
        said.push(format!("a click on the icon with no move: at the drag start {}, at the wake {}, drag start lines {:?}", clicked.0, clicked.1, clicked.2));
        let bare = primes_from(p, || drag(empty, (empty.0 + 80.0, empty.1)));
        primed += bare.0;
        said.push(format!("a drag that begins on empty desktop: at the drag start {}, at the wake {}, drag start lines {:?}", bare.0, bare.1, bare.2));
        match cover(marker, screen) {
            Ok(window) => {
                let under = primes_from(p, || drag(icon, (icon.0 + 80.0, icon.1)));
                window.close();
                spin(0.3);
                primed += under.0;
                said.push(format!("a drag that begins on the icon under another app's window: at the drag start {}, at the wake {}, drag start lines {:?}", under.0, under.1, under.2));
            }
            Err(error) => said.push(format!("the covering window could not be made {error:#}")),
        }
        if position(p, name).is_some_and(|now| (now.0 - icon.0).abs() > 5.0 || (now.1 - icon.1).abs() > 5.0) {
            let _ = place(p, name, icon);
        }
        p.line(format!("- three gestures that must prime nothing at the drag start: {}", said.join("; ")));
        if primed > 0 {
            p.fail("a click, a drag that begins on empty desktop or one under another app's window primed the helper at the drag start");
        }
    }

    struct Windows {
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        handle: std::thread::JoinHandle<Vec<(f64, usize)>>,
    }

    fn watch_windows() -> Windows {
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stopping = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut samples = Vec::new();
            let mut pids: Vec<i32> = Vec::new();
            let mut asked = Instant::now() - Duration::from_secs(1);
            while !stopping.load(std::sync::atomic::Ordering::SeqCst) {
                if asked.elapsed() > Duration::from_millis(100) {
                    pids = Command::new("/usr/bin/pgrep").args(["-f", "live-helper"]).output().map(|out| String::from_utf8_lossy(&out.stdout).lines().filter_map(|line| line.trim().parse().ok()).collect()).unwrap_or_default();
                    asked = Instant::now();
                }
                let shown = objc2::rc::autoreleasepool(|_| layer::every_window().into_iter().filter(|window| window.on_screen && pids.contains(&(window.pid as i32))).count());
                samples.push((epoch(), shown));
                std::thread::sleep(Duration::from_millis(8));
            }
            samples
        });
        Windows { stop, handle }
    }

    fn stop_windows(windows: Windows) -> Vec<(f64, usize)> {
        windows.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        windows.handle.join().unwrap_or_default()
    }

    fn left_at_sets(samples: &[(f64, usize)], log: &[String]) -> Vec<(String, usize)> {
        let mut found = Vec::new();
        for (index, line) in log.iter().enumerate() {
            if !(line.contains(" glide skipped, ") && line.contains(" times from the ")) {
                continue;
            }
            let Some(set) = log[..index].iter().rev().find(|line| line.split_whitespace().nth(1) == Some("set")).and_then(|line| stamp_of(line)) else { continue };
            let left = samples.iter().filter(|(at, _)| *at >= set && *at <= set + 0.15).map(|(_, shown)| *shown).max().unwrap_or(0);
            let reason = line.split(" glide skipped, ").nth(1).and_then(|rest| rest.split(',').next()).unwrap_or_default().to_string();
            found.push((reason, left));
        }
        found
    }

    const PROBE_LAG_MS: u64 = 8000;

    fn back_to_back(p: &mut Probe, screen: &Screen, name: &str, places: ((f32, f32), (f32, f32))) -> Result<()> {
        let since = daemon_log(p).lines().count();
        if !restart_daemon_with(p, false, Some(PROBE_LAG_MS))? {
            p.line("- the daemon here is the login item, so the read lag hint could not be set and the drag inside the lag was skipped".to_string());
            return Ok(());
        }
        let parked = wait_parked(p, since, Duration::from_secs(60));
        let lines = daemon_log(p).lines().count();
        let windows = watch_windows();
        let first = cold_drag(p, screen, name, places);
        let second = cold_drag(p, screen, name, places);
        let samples = stop_windows(windows);
        let log = new_lines(&daemon_log(p), lines);
        let slid_at = log.iter().position(|line| line.contains(" glide slid, "));
        let primed = slid_at.is_some_and(|slid_at| log[slid_at..].iter().any(|line| line.contains(" glide primed ") && line.ends_with("at the drag start")));
        let checked = left_at_sets(&samples, &log);
        p.line(format!("- with the daemon told the read lag is {PROBE_LAG_MS} ms, as on Windows, the helper parked {parked:?} ms after its launch, then two drags back to back: the first glided {}, the second glided {} with skip {:?}, the second primed at its drag start {primed}, helper windows on screen within 150 ms of each plain set {checked:?}, over {} window samples", first.glided, second.glided, second.skip, samples.len()));
        let since = daemon_log(p).lines().count();
        restart_daemon(p, false)?;
        let _ = wait_parked(p, since, Duration::from_secs(60));
        if parked.is_none() || !first.glided || second.glided || second.skip.as_deref() != Some("output not shown") || !primed {
            p.fail("the second drag inside the read lag was not a primed move that did not slide, so the window rule was not judged, which is the footing failing and not a pass");
        } else if checked.is_empty() || checked.iter().any(|(_, left)| *left > 0) {
            p.fail("a window primed inside the read lag was still on screen when the plain swap set its picture");
        }
        Ok(())
    }

    fn system_pick(p: &mut Probe, screen: &Screen) {
        let Some(desktop) = platform::current().ok() else { return };
        let before = desktop.wallpaper(screen).ok().flatten();
        let source = state(p)["screens"].as_object().and_then(|screens| screens.values().next().and_then(|entry| entry["source"].as_str().map(PathBuf::from)));
        let picked = p.home.join("probe-system-pick.jpg");
        let made = source.as_deref().and_then(|source| fs::copy(source, &picked).ok());
        let home = p.home.clone();
        let parked = wait_until(Duration::from_secs(60), || crate::parked_in(&watch_log(&home.join("watch.log"))).is_some()).is_some() && !helper_pids(p).is_empty();
        let _ = quiet_log(p, Duration::from_millis(1500), Duration::from_secs(15));
        if made.is_none() || !parked {
            p.fail(format!("the picture picked in the system's settings was not judged, picked {:?}, a parked helper {parked}, which is the footing failing and not a pass", made));
            return;
        }
        let lines = daemon_log(p).lines().count();
        let windows = watch_windows();
        let picked_at = epoch();
        let set = desktop.set_wallpaper(screen, &picked);
        std::thread::sleep(Duration::from_secs(3));
        let samples = stop_windows(windows);
        let log = new_lines(&daemon_log(p), lines);
        let woke = log.iter().any(|line| line.contains(" wake ") || line.contains(" tick "));
        let own = log.iter().find(|line| line.split_whitespace().nth(1) == Some("set")).and_then(|line| stamp_of(line));
        let primed: Vec<&String> = log.iter().filter(|line| primed(line) && own.is_none_or(|own| stamp_of(line).is_some_and(|at| at < own))).collect();
        let shown = samples.iter().filter(|(at, _)| own.is_some_and(|own| *at >= own)).map(|(_, shown)| *shown).max().unwrap_or(0);
        let earlier = samples.iter().filter(|(at, _)| own.is_none_or(|own| *at < own)).map(|(_, shown)| *shown).max().unwrap_or(0);
        p.line(format!("- a picture picked in the system's settings, set {:?}: the daemon woke {woke} and set its own picture {:?} s after the pick, primes before that {}, helper windows on screen from that set on at most {shown}, before it at most {earlier} from a prime made over Scooch's own picture before the pick, over {} samples in 3 s, before the pick the screen showed {before:?}", set.as_ref().map_err(|error| error.to_string()), own.map(|own| own - picked_at), primed.len(), samples.len()));
        if set.is_err() || !woke || own.is_none() {
            p.fail("the picture picked in the system's settings did not wake the daemon, so it was not judged, which is the footing failing and not a pass");
        } else if !primed.is_empty() || shown > 0 {
            p.fail("a picture picked in the system's settings was covered by a helper window with Scooch's old picture");
        }
        if let Some(source) = source {
            let _ = desktop.set_wallpaper(screen, &source);
            std::thread::sleep(Duration::from_secs(3));
        }
    }

    fn old_tables(p: &mut Probe, screen: &Screen, name: &str, places: ((f32, f32), (f32, f32))) {
        let output = state(p)["screens"].as_object().and_then(|screens| screens.values().next().and_then(|entry| entry["output"].as_str().map(PathBuf::from)));
        let prefix = output.as_ref().and_then(|output| output.file_name()).map(|name| format!("{}.", name.to_string_lossy()));
        let file = prefix.and_then(|prefix| fs::read_dir(p.home.join("cache").join("tables")).ok()?.flatten().map(|entry| entry.path()).find(|path| path.extension().is_some_and(|ext| ext == "tables") && path.file_name().is_some_and(|file| file.to_string_lossy().starts_with(&prefix))));
        let rewritten = file.as_ref().and_then(|file| {
            let mut bytes = fs::read(file).ok()?;
            let body = bytes.len().checked_sub(8).filter(|body| *body > 8)?;
            let version = u32::from_le_bytes(bytes[4..8].try_into().ok()?);
            let back = scooch_core::TABLES_VERSION.checked_sub(1).filter(|_| version == scooch_core::TABLES_VERSION)?;
            bytes[4..8].copy_from_slice(&back.to_le_bytes());
            let sum = scooch::live::canvas::fingerprint(0, 0, &bytes[..body]);
            bytes[body..].copy_from_slice(&sum.to_le_bytes());
            fs::write(file, &bytes).ok()
        });
        let lines = daemon_log(p).lines().count();
        let windows = watch_windows();
        let first = cold_drag(p, screen, name, places);
        let samples = stop_windows(windows);
        let log = new_lines(&daemon_log(p), lines);
        let named = log.iter().any(|line| line.contains("the tables come from another version"));
        let opened = log.iter().any(|line| line.contains(" glide open"));
        let primed_at_drag = log.iter().any(|line| line.contains(" glide primed ") && line.ends_with("at the drag start"));
        let checked = left_at_sets(&samples, &log);
        p.line(format!("- the drag over tables from another version primed at its drag start {primed_at_drag}, helper windows on screen within 150 ms of its plain set {checked:?}"));
        if !primed_at_drag {
            p.fail("the drag over tables from another version did not prime at its drag start, so the window rule was not judged, which is the footing failing and not a pass");
        } else if checked.is_empty() || checked.iter().any(|(_, left)| *left > 0) {
            p.fail("a primed window was still on screen when the drag over tables from another version set its picture");
        }
        let mut again = Vec::new();
        for _ in 0..2 {
            std::thread::sleep(Duration::from_millis(1500));
            let next = cold_drag(p, screen, name, places);
            again.push(next.glided);
            if next.glided {
                break;
            }
        }
        p.line(format!("- tables rewritten from version {} to {} {:?}, then a drag: logged another version {named}, new wallpaper {:?} ms, glided {}, a layer opened {opened}, skip {:?}, the drags after it glided {again:?}", scooch_core::TABLES_VERSION, scooch_core::TABLES_VERSION - 1, rewritten.is_some(), first.set, first.glided, first.skip));
        if rewritten.is_none() || !named || first.set.is_none() || first.glided || opened || !again.contains(&true) {
            p.fail("tables from another version did not give one plain swap at once and then a glide");
        }
    }

    fn bare_wakes(p: &mut Probe) {
        std::thread::sleep(Duration::from_secs(12));
        let before = helper_pids(p);
        let lines = daemon_log(p).lines().count();
        let mut listed = Vec::new();
        for _ in 0..30 {
            poke(p);
            std::thread::sleep(Duration::from_millis(400));
            let pids = helper_pids(p);
            let every: Vec<layer::Seen> = layer::every_window().into_iter().filter(|window| pids.contains(&(window.pid as i32))).collect();
            if !every.is_empty() {
                listed.push(format!("{} listed, {} flagged on screen, layers {:?}", every.len(), every.iter().filter(|window| window.on_screen).count(), every.iter().map(|window| window.layer).collect::<Vec<_>>()));
            }
        }
        std::thread::sleep(Duration::from_millis(6000));
        let after = helper_pids(p);
        let left = layer::every_window().into_iter().filter(|window| after.contains(&(window.pid as i32))).count();
        let log = new_lines(&daemon_log(p), lines);
        let primes = log.iter().filter(|line| primed(line)).count();
        let ticks = log.iter().filter(|line| line.contains(" tick ")).count();
        let sent: Vec<String> = log.iter().filter(|line| commanded(line)).map(|line| line.chars().take(160).collect()).collect();
        p.line(format!("- thirty wakes with nothing moved: ticks {ticks}, primes {primes}, helper commands past a prime {sent:?}, the parked helper before {before:?} and after {after:?}, its windows while primed {listed:?}, its windows six seconds after the last wake {left}"));
        if primes > crate::BARE_PRIMES || !sent.is_empty() || before != after || before.len() != 1 || left > 0 {
            p.fail("thirty bare wakes primed more often than the rule allows, opened a layer, changed the parked helper or left a window");
        }
    }

    type Layout = Vec<(String, [i64; 4])>;

    fn layout(view: &finder::View) -> (Option<Layout>, Duration) {
        let started = Instant::now();
        let icons = finder::named(view).ok().map(|icons| {
            let mut icons: Layout = icons.into_iter().map(|(name, rect)| (name, [rect.x.round() as i64, rect.y.round() as i64, rect.w.round() as i64, rect.h.round() as i64])).collect();
            icons.sort();
            icons
        });
        (icons, started.elapsed())
    }

    fn finder_after_drop(p: &mut Probe) -> Result<()> {
        let screen = main_screen(p)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.line("- the reads need Apple Events to Finder, which the open consent window blocks, so they were skipped".to_string());
            return Ok(());
        }
        if scooch_pid(p).is_none() {
            p.line("- no daemon runs, so the drags were skipped".to_string());
            return Ok(());
        }
        if state(p)["glide"] != serde_json::json!(true) {
            let _ = p.scooch(&["glide"], LIMIT);
        }
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let aside = (screen.frame.x + screen.frame.w * 0.38, screen.frame.y + screen.frame.h * 0.5);
        let offsets = [0u64, 20, 40, 60, 80];
        let view = finder::view();
        p.line("- ten mouse drags of one icon by CGEvent, the daemon running with the glide on, and after each drop the probe's own Finder reads by Apple Event at 0, 20, 40, 60 and 80 ms after its drop stamp, each against the layout read 2.5 s later".to_string());
        let mut hits = [0usize; 5];
        let mut firsts = Vec::new();
        for index in 0..10 {
            let from = position(p, name).unwrap_or(subject);
            let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
            let (before, _) = layout(&view);
            drag(from, to);
            let dropped = Instant::now();
            let mut reads = Vec::new();
            for offset in offsets {
                std::thread::sleep(Duration::from_millis(offset).saturating_sub(dropped.elapsed()));
                let began = dropped.elapsed();
                let (icons, took) = layout(&view);
                reads.push((offset, began, took, icons));
            }
            std::thread::sleep(Duration::from_millis(2500));
            let (last, _) = layout(&view);
            let finals: Vec<bool> = reads.iter().map(|(_, _, _, icons)| icons.is_some() && *icons == last).collect();
            for (hit, shown) in hits.iter_mut().zip(&finals) {
                *hit += usize::from(*shown);
            }
            let first = reads.iter().zip(&finals).find(|(_, shown)| **shown).map(|((offset, ..), _)| *offset);
            firsts.push(first);
            let said: Vec<String> = reads.iter().zip(&finals).map(|((offset, began, took, icons), shown)| format!("{offset} ms read at {} ms took {} ms {}", began.as_millis(), took.as_millis(), if icons.is_none() { "unread" } else if *shown { "final" } else if *icons == before { "before" } else { "other" })).collect();
            p.line(format!("  - drag {}: the layout moved {}, first read with the final layout {}, reads {}", index + 1, last.is_some() && last != before, first.map_or("none".to_string(), |first| format!("{first} ms")), said.join(", ")));
        }
        let earliest = offsets.iter().zip(&hits).find(|(_, hits)| **hits == 10).map(|(offset, _)| *offset);
        p.line(format!("- the earliest read after the drop that showed the final layout in ten of ten {}, reads with the final layout per offset {}, first such read per drag {:?}", earliest.map_or("none".to_string(), |earliest| format!("{earliest} ms")), offsets.iter().zip(&hits).map(|(offset, hits)| format!("{offset} ms {hits} of 10")).collect::<Vec<_>>().join(", "), firsts));
        Ok(())
    }

    fn rest_wakes(p: &mut Probe) -> Result<()> {
        let whole = daemon_log(p);
        let primes = whole.lines().filter(|line| primed(line)).count();
        let ticks = whole.lines().filter(|line| line.contains(" tick ")).count();
        let bare = whole.lines().filter(|line| line.contains(" tick ") && line.contains("Unchanged") && !["Rendered", "Plain", "Reapplied", "Kept", "Interrupted"].iter().any(|outcome| line.contains(outcome))).count();
        p.line(format!("- over the whole probe so far: ticks {ticks}, ticks that changed nothing {bare}, primes {primes}"));
        if scooch_pid(p).is_none() {
            p.line("- no daemon runs, so the five quiet minutes were skipped".to_string());
            return Ok(());
        }
        let found = motion_now(p);
        set_motion(p, false);
        poke(p);
        let parked = wait_parked(p, whole.lines().count(), Duration::from_secs(60)).or_else(|| (found.as_deref() != Some("1")).then(|| crate::parked_in(&watch_log(&p.home.join("watch.log")))).flatten());
        p.line(format!("- reduce motion found as {found:?} and turned off for the quiet minutes, the helper parked {} ms after its launch", parked.map_or("never".to_string(), |ms| ms.to_string())));
        if parked.is_none() {
            p.fail("the helper never parked within 60 s before the quiet minutes, so the move after them was not judged, which is the footing failing and not a pass");
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(2));
        let lines = daemon_log(p).lines().count();
        let started = Instant::now();
        std::thread::sleep(Duration::from_secs(300));
        let log = new_lines(&daemon_log(p), lines);
        let mut kinds: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
        for line in log.iter().filter(|line| !line.contains(" glide ")) {
            if let Some(kind) = line.split(" wake ").nth(1).and_then(|rest| rest.split_whitespace().next()) {
                *kinds.entry(kind.to_string()).or_default() += 1;
            }
        }
        let ticks = log.iter().filter(|line| line.contains(" tick ")).count();
        let mouse = log.iter().filter(|line| line.contains("mouse up")).count();
        let primes = log.iter().filter(|line| primed(line)).count();
        p.line(format!("- five quiet minutes with nothing done on the runner: wakes {} by kind {kinds:?}, mouse ups {mouse}, ticks {ticks}, primes {primes}", kinds.values().sum::<usize>()));
        let screen = main_screen(p)?;
        if dismiss_prompt(p).is_none() {
            p.line("- the move after the quiet minutes needs Apple Events to Finder, which the open consent window blocks, so it was skipped".to_string());
            return Ok(());
        }
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let aside = (screen.frame.x + screen.frame.w * 0.38, screen.frame.y + screen.frame.h * 0.5);
        let mut last = started;
        let moved = cold_one(p, &screen, PROBES[0].0, (subject, aside), ("5 min apart", 0), &mut last);
        if !moved.glided {
            p.fail(format!("a move five minutes after the last did not slide with the helper parked, skipped as {:?}", moved.skip));
        }
        Ok(())
    }

    fn rest_check(p: &mut Probe) -> Result<()> {
        let result = rest_wakes(p);
        put_motion_back(p);
        result
    }

    fn glide_check(p: &mut Probe) -> Result<()> {
        let result = glide_steps(p);
        put_motion_back(p);
        result
    }

    const SLIDES: usize = 36;

    fn slides_check(p: &mut Probe) -> Result<()> {
        let focus = std::env::var("SCOOCH_PROBE_FOCUS").is_ok_and(|focus| focus == "slides");
        let mut result = many_slides(p, "the layer as built", 0, (if focus { 50 } else { SLIDES }, false));
        if focus {
            let see = [(scooch::live::SEE_THROUGH, "layer")];
            for (kind, env, stream) in [("a see through layer", &see[..], false), ("the layer as built while a display stream runs", &[][..], true)] {
                let since = watch_log(&p.home.join("watch.log")).len();
                if result.is_ok() && restart_daemon_env(p, env).unwrap_or(false) {
                    result = many_slides(p, kind, since, (50, stream));
                }
            }
            let _ = restart_daemon_env(p, &[]);
        }
        put_motion_back(p);
        result
    }

    fn restart_daemon_env(p: &mut Probe, env: &[(&str, &str)]) -> Result<bool> {
        let Some(mut child) = p.stand.take() else { return Ok(false) };
        let _ = child.kill();
        let _ = child.wait();
        stutters(p);
        p.struck = false;
        let _ = fs::remove_file(p.home.join("watch.pid"));
        let log = fs::OpenOptions::new().create(true).append(true).open(p.home.join("watch.log"))?;
        let mut command = Command::new(&p.bin);
        command.arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").env_remove(scooch::live::SLOW_HELLO).env_remove(scooch::live::READ_LAG).env_remove(scooch::live::SEE_THROUGH).envs(env.iter().copied()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(log);
        p.stand = Some(command.spawn()?);
        let home = p.home.clone();
        wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.pid")).ok().and_then(|text| text.trim().parse::<i32>().ok()).is_some_and(|pid| unsafe { libc::kill(pid, 0) } == 0)).context("the restarted daemon wrote no pid file")?;
        p.line(format!("- the daemon started again with {env:?}"));
        Ok(true)
    }

    fn drew_near(log: &[String], at: f64, reach: f64) -> Vec<String> {
        log.iter()
            .filter_map(|line| line.split(" glide drew ").nth(1))
            .flat_map(|record| record.split(", ").map(str::to_string).collect::<Vec<_>>())
            .filter(|frame| frame.split_whitespace().nth(1).and_then(|stamp| stamp.parse::<f64>().ok()).or_else(|| frame.split_whitespace().next().and_then(|stamp| stamp.parse::<f64>().ok())).is_some_and(|stamp| (stamp - at).abs() <= reach))
            .collect()
    }

    fn many_slides(p: &mut Probe, kind: &str, since: usize, (count, stream): (usize, bool)) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let screen = main_screen(p)?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.line("- the slides need Apple Events to Finder, which the open consent window blocks, so they were skipped".to_string());
            return Ok(());
        }
        if scooch_pid(p).is_none() {
            p.fail("no daemon runs for the slides");
            return Ok(());
        }
        set_motion(p, false);
        poke(p);
        let parked = wait_parked(p, since, Duration::from_secs(60));
        if parked.is_none() {
            p.fail("the helper never parked within 60 s, so no slide was watched, which is the footing failing and not a pass");
            return Ok(());
        }
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let aside = (screen.frame.x + screen.frame.w * 0.38, screen.frame.y + screen.frame.h * 0.5);
        let (mut watched, mut glided, mut judged, mut by_record, mut captures, mut black) = (0, 0, 0, 0, 0, 0);
        let (mut streamed, mut displayed) = (0, 0);
        let mut faults = Vec::new();
        let mut skipped = Vec::new();
        for index in 0..count {
            quiet_log(p, Duration::from_millis(800), Duration::from_secs(10));
            let from = position(p, name).unwrap_or(subject);
            let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
            let frames = if stream { Frames::start(display, &screen) } else { None };
            let dragged = watch_drag(p, display, &screen, from, to, &[]);
            let (shown, idle) = frames.map(Frames::stop).unwrap_or_default();
            let Some(dragged) = dragged else { continue };
            watched += 1;
            streamed += shown.len();
            let dark: Vec<f64> = shown.iter().filter(|(_, dark)| *dark >= crate::BLACK_SHARE).map(|(at, _)| ((at - dragged.dropped) * 1000.0).round()).collect();
            let longest = shown.windows(2).map(|pair| pair[1].0 - pair[0].0).fold(0.0f64, f64::max);
            if !dark.is_empty() {
                let closed = logged(&dragged, "after its wallpaper was set").map(|closed| ((closed - dragged.dropped) * 1000.0).round());
                p.line(format!("  - slide {}: the display showed black in {} frames at {dark:?} ms after the drop, the close {closed:?} ms, {} frames and {idle} idle reports, longest gap {:.0} ms", index + 1, dark.len(), shown.len(), longest * 1000.0));
                faults.push(format!("slide {} the display showed a black desktop at {dark:?} ms after the drop", index + 1));
                displayed += dark.len();
            }
            captures += dragged.seen.len();
            match crate::glide_try(&dragged.log, true) {
                crate::Try::Glided => {}
                crate::Try::Skipped(reason) | crate::Try::Fault(reason) => {
                    skipped.push(format!("{}: {reason}", index + 1));
                    continue;
                }
            }
            glided += 1;
            let label = format!("slide {}", index + 1);
            let evidence = format!("slide-{}", index + 1);
            let seen = judge_glide(p, &dragged, &screen, &label, None, true);
            if seen.enough {
                judged += 1;
            } else if seen.recorded.agreed >= 1 && seen.recorded.steady {
                by_record += 1;
            }
            for why in &seen.recorded.disagreed {
                faults.push(format!("slide {} {why}, so the screen did not show what the helper drew", index + 1));
            }
            if !seen.faults.is_empty() {
                let _ = judge_glide(p, &dragged, &screen, &label, Some(&evidence), false);
                let closed = logged(&dragged, "after its wallpaper was set");
                for fault in &seen.faults {
                    black += usize::from(fault.contains("black desktop"));
                    let at = fault.split(" at ").nth(1).and_then(|rest| rest.split(" ms").next()).and_then(|ms| ms.parse::<f64>().ok()).map(|ms| dragged.dropped + ms / 1000.0);
                    let near = at.map(|at| drew_near(&dragged.log, at, 0.15)).unwrap_or_default();
                    p.line(format!("  - slide {}: {fault}, the close {:?} ms after the drop, what the helper presented within 150 ms of it {near:?}, {}", index + 1, closed.map(|closed| ((closed - dragged.dropped) * 1000.0).round()), dragged.moment.clone().unwrap_or_else(|| "no black capture was seen while watching".into())));
                    faults.push(format!("slide {} {fault}", index + 1));
                }
            }
        }
        let odd = faults.iter().filter(|fault| fault.contains("neither picture")).count();
        p.line(format!("- with {kind}, the display stream sent {streamed} frames over the slides, {displayed} of them black"));
        p.line(format!("- with {kind}, {count} slides dragged, {watched} watched, {glided} glided, {judged} judged by dense captures and {by_record} by the helper's record checked against the captures, {captures} captures in all, {black} black captures, {odd} captures no frame or picture explains, skips {skipped:?}"));
        for fault in faults {
            p.fail(fault);
        }
        if glided * 2 < count {
            p.fail(format!("only {glided} of {count} slides glided, so the slides were not watched on footing"));
        }
        Ok(())
    }

    fn glide_steps(p: &mut Probe) -> Result<()> {
        let marker = MainThreadMarker::new().context("the probe is not on the main thread")?;
        accessory(marker);
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let (_, display) = picture::screen_for(marker, &screen.id)?;
        let name = PROBES[0].0;
        if dismiss_prompt(p).is_none() {
            p.finder = false;
            p.line("- the glide check needs Apple Events to Finder, which the open consent window blocks, so it was skipped".to_string());
            return Ok(());
        }
        let Some(daemon) = scooch_pid(p) else {
            p.fail("no daemon runs for the glide check");
            return Ok(());
        };
        p.line(format!("- glide setting `{}`, scenery `{}`, daemon {daemon}", state(p)["glide"], state(p)["options"]["scenery"].as_str().unwrap_or("missing")));
        let calm = quiet_log(p, Duration::from_millis(1500), Duration::from_secs(15));
        let rest_before = memory(daemon);
        let found = motion_now(p);
        let wrote = set_motion(p, false);
        let now = motion_now(p);
        let lines = daemon_log(p).lines().count();
        poke(p);
        let home = p.home.clone();
        let read = wait_until(Duration::from_secs(6), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains("glide reduce motion off")));
        let last_reading = daemon_log(p).lines().rev().find(|line| line.contains(" glide reduce motion ")).map(str::to_string);
        p.line(format!("- daemon log quiet before the setup {calm}, reduce motion found as {found:?}, turned off {wrote}, now {now:?}, the daemon logged reading it as off after {:?} ms, its last reading `{}`", read.map(|time| time.as_millis()), last_reading.clone().unwrap_or_default()));
        if !last_reading.is_some_and(|line| line.ends_with(" off")) {
            p.fail("the daemon never logged reduce motion as off after the probe turned it off");
            return Ok(());
        }
        let subject = (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5);
        let aside = (screen.frame.x + screen.frame.w * 0.38, screen.frame.y + screen.frame.h * 0.5);
        let mut setup_log = Vec::new();
        for (step, target) in [(1, aside), (2, subject)] {
            quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
            let (setup, log) = wallpaper_after(p, &screen, |p| {
                let _ = place(p, name, target);
                poke(p);
            });
            p.line(format!("- setup move {step} of {name} to {:.0},{:.0}, new wallpaper {setup:?} ms after the Apple Event returned, daemon log {log:?}", target.0, target.1));
            setup_log = log;
        }
        let tables = setup_log.iter().any(|line| line.contains(" tick ") && line.contains("Rendered") && (line.contains(", tables") || line.contains(", later")));
        p.line(format!("- the last setup move rendered and wrote tables {tables}"));
        if !tables {
            p.fail("the setup moves wrote no tables, so the timed drag could not glide");
        }
        let parked = wait_parked(p, 0, Duration::from_secs(60));
        p.line(format!("- the helper parked {} ms after its launch, by its line in the log", parked.map_or("never".to_string(), |ms| ms.to_string())));
        if parked.is_none() {
            p.fail("the helper never parked within 60 s, so no glide was judged, which is the footing failing and not a pass");
            return Ok(());
        }
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let paper = picture_window(&screen).and_then(|window| grab_window(window.number));
        let baseline = grab(display).context("the desktop could not be captured")?;
        let still: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().filter(|(found, _)| found.starts_with("scooch") && found != name).map(|(_, rect)| rect).collect();
        let masks: Vec<Vec<(usize, usize)>> = paper.as_ref().map(|field| still.iter().map(|rect| strong_ink(&baseline, field, rect, &screen)).collect()).unwrap_or_default();
        p.line(format!("- solid icon ink pixels of the still icons {:?}", masks.iter().map(Vec::len).collect::<Vec<_>>()));
        let mut verdict: Option<bool> = None;
        let mut unseen = Vec::new();
        let mut during = String::new();
        let mut held = None;
        let mut tries = Vec::new();
        for attempt in 1..=3 {
            if attempt > 1 {
                quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
            }
            let from = position(p, name).unwrap_or(subject);
            let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
            let dragged = watch_drag(p, display, &screen, from, to, &masks).context("the glide drag could not be watched")?;
            if attempt == 1 {
                during = memory(daemon);
                held = daemon_log(p).lines().rev().find_map(|line| line.split("sessions hold ").nth(1).map(str::to_string));
            }
            let evidence = if attempt == 1 { "glide".to_string() } else { format!("glide-{attempt}") };
            let judged = judge_glide(p, &dragged, &screen, &format!("glide on, CGEvent drag, attempt {attempt}"), Some(&evidence), false);
            let tried = crate::glide_try(&dragged.log, true);
            tries.push(tried.clone());
            match tried {
                crate::Try::Skipped(reason) => {
                    p.line(format!("  - attempt {attempt} did not glide, the daemon skipped it as {reason}, which it counts as normal, so it is not judged and the drag is tried again"));
                    continue;
                }
                crate::Try::Fault(reason) => {
                    p.line(format!("  - attempt {attempt} did not glide, the daemon skipped it as {reason}, which it does not count as normal"));
                    verdict = Some(false);
                    break;
                }
                crate::Try::Glided => {}
            }
            if !judged.faults.is_empty() {
                for fault in &judged.faults {
                    p.fail(fault.clone());
                }
                if let Some(moment) = &dragged.moment {
                    p.line(format!("  - {moment}"));
                }
                verdict = Some(false);
                break;
            }
            if !judged.rest {
                verdict = Some(false);
                break;
            }
            if !judged.recorded.disagreed.is_empty() {
                for why in &judged.recorded.disagreed {
                    p.fail(format!("{why}, so the screen did not show what the helper drew"));
                }
                verdict = Some(false);
                break;
            }
            if judged.enough {
                p.line(format!("  - captures between the old and the new picture judged, {} between, at least 2 needed", judged.moving));
                verdict = Some(judged.moving >= 2);
                break;
            }
            if judged.recorded.agreed >= 1 {
                p.line(format!("  - the captures were too sparse to follow the glide, so it is judged by the helper's record of what it presented, {} frames moving between the two places, steady and ending on the new place {}, and {} captures inside the glide agree with it", judged.recorded.moving, judged.recorded.steady, judged.recorded.agreed));
                verdict = Some(judged.recorded.steady);
                break;
            }
            p.line(format!("  - captures between the old and the new picture not judged, {} captures a second, largest gap {:?} ms, two in-between captures are certain only with no gap over {} ms", judged.rate.map_or(0.0, f64::round), judged.gap.map(|gap| (gap * 1000.0).round()), (SEEN_GAP * 1000.0) as u32));
            unseen.push(judged.frames);
        }
        let unglided = crate::none_glided(&tries);
        let good = match (verdict, &unglided) {
            (Some(good), _) => good,
            (None, Some(_)) => true,
            (None, None) => {
                let drew = unseen.iter().all(|frames| frames.is_some_and(|frames| frames >= 3));
                let which = if unseen.len() == 1 { "the one try that glided and was not judged".to_string() } else { format!("each of the {} tries that glided and were not judged", unseen.len()) };
                p.limit(format!("the frames between the old and the new picture could not be judged on this runner, since screen capture slowed below one every {} ms during {which}, while the helper drew {unseen:?} frames and every other glide check passed", (SEEN_GAP * 1000.0) as u32));
                drew
            }
        };
        p.line(format!("  - daemon {daemon} before the burst {rest_before}, right after the glide {during}, its sessions hold {held:?}"));
        std::thread::sleep(Duration::from_millis(1000));
        let pids = helper_pids(p);
        let left = layer::every_window().into_iter().filter(|window| pids.contains(&(window.pid as i32))).count();
        p.line(format!("- helper processes {pids:?} with windows left {left}"));
        if let Some(said) = unglided {
            p.fail(said);
        }
        if !good || left > 0 {
            p.fail("the glide did not move steadily under the icons and hand over cleanly");
        }
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let keep = away_from_icons(&screen);
        let before = grab(display).context("the desktop could not be captured")?;
        let lines = daemon_log(p).lines().count();
        poke(p);
        let poked = Instant::now();
        let mut worst = Spot::default();
        let mut worst_at = 0;
        let mut shots = 0;
        while poked.elapsed() < Duration::from_millis(2500) {
            if let Some(shot) = grab(display) {
                let found = spot_where(&before, &shot, &keep);
                if found.worst > worst.worst {
                    worst = found;
                    worst_at = poked.elapsed().as_millis();
                    let _ = scooch_core::encode_png(&shot.image()).map(|bytes| fs::write(p.out.join("idle-wake-worst.png"), bytes));
                }
                shots += 1;
            }
        }
        let _ = scooch_core::encode_png(&before.image()).map(|bytes| fs::write(p.out.join("idle-wake-before.png"), bytes));
        let woke: Vec<String> = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ") || line.contains(" tick ")).map(|line| line.chars().take(180).collect()).collect();
        let sent = woke.iter().filter(|line| commanded(line)).count();
        let primes = woke.iter().filter(|line| primed(line)).count();
        p.line(format!("- a wake that changes nothing: helper commands past a prime {sent}, primes {primes}, {shots} captures over 2.5 s, largest change from before the wake {} of 255 at {worst_at} ms at pixel {},{} from {:?} to {:?}, outside the icons and the menu bar, captures kept as idle-wake-before.png and idle-wake-worst.png, log {woke:?}", worst.worst, worst.x, worst.y, worst.before, worst.after));
        if sent > 0 || primes > 1 || worst.worst > 2 {
            p.fail("a wake that changed nothing opened a layer, primed more than once or changed the screen");
        }
        let calm_a = (screen.frame.x + screen.frame.w * 0.15, screen.frame.y + screen.frame.h * 0.75);
        let calm_b = (screen.frame.x + screen.frame.w * 0.25, screen.frame.y + screen.frame.h * 0.75);
        let mut laps = Vec::new();
        for (index, target) in [calm_a, calm_b, calm_a, calm_b].into_iter().enumerate() {
            quiet_log(p, Duration::from_millis(800), Duration::from_secs(10));
            let from = position(p, name).unwrap_or(subject);
            let (time, log) = wallpaper_after(p, &screen, |_| drag(from, target));
            let render = log.iter().find(|line| line.contains(" tick ") && line.contains("Rendered")).map(|line| line.chars().take(260).collect::<String>());
            laps.push(format!("move {} new wallpaper {time:?} ms, tick {render:?}", index + 1));
        }
        p.line(format!("- scenery moves on calm ground, the first leaves the subject and the rest hold the placement {laps:?}"));
        let (mut looked, mut moved, mut right, mut half) = (0, 0, 0, 0);
        let mut seen = Vec::new();
        for index in 0..12 {
            quiet_log(p, Duration::from_millis(800), Duration::from_secs(10));
            let lines = daemon_log(p).lines().count();
            let from = position(p, name).unwrap_or(subject);
            let target = if index % 2 == 0 { aside } else { subject };
            drag(from, target);
            let home = p.home.clone();
            let ticked = |count: usize| {
                let home = home.clone();
                move || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().filter(|line| line.contains(" tick ")).count() >= count
            };
            let _ = wait_until(Duration::from_secs(6), ticked(1));
            std::thread::sleep(Duration::from_millis(1500));
            poke(p);
            let _ = wait_until(Duration::from_secs(4), ticked(2));
            let log = new_lines(&daemon_log(p), lines);
            let prints: Vec<(String, bool)> = log.iter().enumerate().filter_map(|(at, line)| line.split("snapshot icons ").nth(1).map(|print| (print.trim().to_string(), log[at..].iter().find(|later| later.contains(" tick ")).is_some_and(|tick| tick.contains("Rendered"))))).collect();
            let last = prints.last().map(|(print, _)| print.clone()).unwrap_or_default();
            let quick = log.iter().find_map(|line| line.split("quick look after ").nth(1).map(str::to_string));
            if let Some(quick) = &quick {
                looked += 1;
                let went = quick.ends_with("moved true");
                let print = quick.split("icons ").nth(1).and_then(|rest| rest.split_whitespace().next()).unwrap_or_default().to_string();
                if went {
                    moved += 1;
                    if print == last {
                        right += 1;
                    }
                }
            }
            half += prints.iter().filter(|(print, rendered)| *rendered && *print != last).count();
            seen.push(format!("{}: {}", index + 1, quick.unwrap_or_else(|| "no quick look".into())));
        }
        p.line(format!("- twelve drags: quick looks {looked}, saw the icons moved {moved}, of those had the final layout {right}, renders from a layout that was not the final one {half}, per drag {seen:?}"));
        if half > 0 {
            p.fail("a quick look rendered a half finished move");
        }
        let ran = p.scooch(&["glide"], LIMIT);
        p.line(format!("- scooch glide said {:?}", ran.text()));
        let back = position(p, name).unwrap_or(subject);
        let away = if (back.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let (off, off_log) = wallpaper_after(p, &screen, |_| drag(back, away));
        p.line(format!("- glide off, the same drag back: new wallpaper after {off:?} ms, log {off_log:?}"));
        if off.is_none() || off_log.iter().any(|line| line.contains(" glide open")) {
            p.fail("with the glide off the drag back did not swap plainly");
        }
        let ran = p.scooch(&["glide"], LIMIT);
        p.line(format!("- scooch glide said {:?}", ran.text()));
        let wrote = set_motion(p, true);
        let back = position(p, name).unwrap_or(subject);
        let away = if (back.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let (time, log) = wallpaper_after(p, &screen, |_| drag(back, away));
        let glided = log.iter().any(|line| line.contains(" glide open"));
        let read = log.iter().any(|line| line.contains("glide skipped, reduce motion"));
        p.line(format!("- reduce motion on {wrote}, drag: new wallpaper after {time:?} ms, glided {glided}, the daemon logged reduce motion {read}, log {log:?}"));
        if glided || !read {
            p.fail("the daemon did not take today's path with reduce motion on");
        }
        set_motion(p, false);
        std::thread::sleep(Duration::from_millis(500));
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let (_, warm_log) = wallpaper_after(p, &screen, |_| drag(from, to));
        p.line(format!("- a drag with reduce motion off again, so the next move has tables, log {warm_log:?}"));
        quiet_log(p, Duration::from_millis(1000), Duration::from_secs(10));
        let struck = stutters(p);
        if struck {
            p.line("- the strike rule turned the glide off for this daemon, so the covered move, the kill and the drag after it are judged only as plain swaps".to_string());
        }
        let window = cover(marker, &screen)?;
        let target = if position(p, name).is_some_and(|at| (at.0 - subject.0).abs() < 5.0) { aside } else { subject };
        let (covered, covered_log) = wallpaper_after(p, &screen, |p| {
            let _ = place(p, name, target);
            poke(p);
        });
        window.orderOut(None);
        window.close();
        drop(window);
        spin(0.4);
        let rendered = |log: &[String]| log.iter().find(|line| line.contains(" tick ") && line.contains("Rendered")).cloned();
        let tick = |log: &[String]| rendered(log).and_then(|line| line.split(" tick ").nth(1).and_then(|rest| rest.split_whitespace().next()).and_then(|ms| ms.parse::<u64>().ok()));
        let laps = |log: &[String], names: &[&str]| rendered(log).map(|line| line.split(", ").filter_map(|part| part.rsplit_once(' ')).filter(|(name, _)| names.iter().any(|wanted| name.ends_with(wanted))).filter_map(|(_, ms)| ms.parse::<u64>().ok()).sum::<u64>());
        let hidden = covered_log.iter().any(|line| line.contains("glide skipped, hidden"));
        let covered_waited = covered_log.iter().find_map(|line| line.split(" glide skipped, ").nth(1).and_then(|rest| rest.split("waited ").nth(1)).and_then(|rest| rest.split(' ').next()).and_then(|ms| ms.parse::<u64>().ok()));
        let at_once = !covered_log.iter().any(|line| line.contains("shown and gliding")) && covered_waited.is_none_or(|waited| waited <= 20);
        let delay = laps(&covered_log, &["early", "tables", "open"]);
        p.line(format!("- a move while a window covers the whole desktop: new wallpaper after {covered:?} ms, the daemon answered hidden {hidden}, the decision waited {covered_waited:?} ms, a plain swap at once {at_once}, time the glide added to the tick {delay:?} ms, tick {:?} ms against {:?} ms for the glide off move, log {covered_log:?}", tick(&covered_log), tick(&off_log)));
        if covered.is_none() || (!struck && (!(hidden || at_once) || delay.is_none_or(|delay| delay > 400))) {
            p.fail("a covered desktop delayed the plain swap");
        }
        std::thread::sleep(Duration::from_millis(1500));
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let lines = daemon_log(p).lines().count();
        let shown_before = desktop.wallpaper(&screen).ok().flatten();
        drag(from, to);
        let home = p.home.clone();
        let opened = wait_until(Duration::from_secs(5), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains(" glide open")));
        let killed = helper_pids(p);
        for pid in &killed {
            let _ = p.tool("/bin/kill", &["-9", &pid.to_string()]);
        }
        let _ = wait_until(Duration::from_secs(6), || new_lines(&fs::read_to_string(home.join("watch.log")).unwrap_or_default(), lines).iter().any(|line| line.contains(" tick ")));
        std::thread::sleep(Duration::from_millis(1200));
        let now_shown = desktop.wallpaper(&screen).ok().flatten();
        let recorded = state(p)["screens"].as_object().and_then(|screens| screens.values().next().and_then(|entry| entry["output"].as_str().map(PathBuf::from)));
        let shot = grab(display);
        let icons: Vec<Rect> = finder::named(&finder::view()).unwrap_or_default().into_iter().map(|(_, rect)| rect).collect();
        let matches = shot.as_ref().zip(now_shown.as_ref()).and_then(|(shot, file)| matches_file(shot, file, &icons, &screen));
        let alive = scooch_pid(p).is_some();
        let kill_log: Vec<String> = new_lines(&daemon_log(p), lines).into_iter().filter(|line| line.contains(" glide ")).map(|line| line.chars().take(180).collect()).collect();
        p.line(format!("- helper killed mid glide after the layer opened {:?} ms: killed {killed:?}, wallpaper changed {}, it is the recorded output {}, screen matches the file {matches:?}, daemon alive {alive}, log {kill_log:?}", opened.map(|time| time.as_millis()), now_shown != shown_before, now_shown == recorded));
        std::thread::sleep(Duration::from_millis(1500));
        let from = position(p, name).unwrap_or(subject);
        let to = if (from.0 - subject.0).abs() < 5.0 { aside } else { subject };
        let again = watch_drag(p, display, &screen, from, to, &masks).context("the drag after the kill could not be watched")?;
        let glided_again = logged(&again, " glide settled").or_else(|| logged(&again, " glide gave up the settle")).is_some();
        p.line(format!("- the next drag glides again {glided_again}, log {:?}", again.log.iter().map(|line| line.chars().take(160).collect::<String>()).collect::<Vec<_>>()));
        if !alive || now_shown != recorded || !matches.is_some_and(|share| share >= 0.9) || (!struck && (killed.is_empty() || !glided_again)) {
            p.fail("killing the helper mid glide was not handled cleanly");
        }
        let last = Instant::now();
        let lines = daemon_log(p).lines().count();
        for _ in 0..30 {
            poke(p);
            std::thread::sleep(Duration::from_millis(400));
        }
        let woken = new_lines(&daemon_log(p), lines);
        let ticks = woken.iter().filter(|line| line.contains(" tick ")).count();
        let sent: Vec<String> = woken.iter().filter(|line| commanded(line)).map(|line| line.chars().take(160).collect()).collect();
        let primes = woken.iter().filter(|line| primed(line)).count();
        p.line(format!("- thirty wakes with nothing moved over {} s after the last drag: ticks {ticks}, primes {primes}, helper commands past a prime {} {sent:?}", last.elapsed().as_secs(), sent.len()));
        if !sent.is_empty() || primes > crate::BARE_PRIMES {
            p.fail("a wake with nothing moved opened a layer, or bare wakes primed more often than the rule allows");
        }
        let before = helper_pids(p);
        std::thread::sleep(Duration::from_secs(35).saturating_sub(last.elapsed()));
        let pids = helper_pids(p);
        let held = pids.first().map_or("none".to_string(), |pid| memory(*pid));
        p.line(format!("- 35 s after the last drag helper processes {pids:?}, before the rest {before:?}, the parked helper {held}, daemon {}, its log after the rest {:?}", memory(daemon), daemon_log(p).lines().rev().find(|line| line.contains("sessions let go")).map(|line| line.chars().take(120).collect::<String>())));
        if pids.len() != 1 || pids != before {
            p.fail("the parked helper was not one steady process through the rest");
        }
        quiet_minute(p, daemon, "the daemon with glide on, 35 s after its last glide")?;
        if let Some(helper) = pids.first().copied() {
            let woke = quiet_minute(p, helper, "the parked helper, 35 s after its last glide")?;
            if woke.0 > crate::SPARE_WAKEUPS {
                p.fail(format!("the parked helper woke {} times at rest over a quiet minute", woke.0));
            }
        }
        Ok(())
    }

    const BALLOON: &[u8] = include_bytes!("../../../assets/photos/balloon-sky.jpg");

    fn smoother(t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
    }

    fn bench_scene(width: u32, height: u32, moved: bool) -> scooch_core::Scene {
        let (w, h) = (152.0f32, 168.0f32);
        let mut icons: Vec<Rect> = (0..6).map(|index| Rect { x: width as f32 - 230.0, y: 60.0 + 200.0 * index as f32, w, h }).collect();
        icons.push(if moved { Rect { x: width as f32 * 0.25, y: height as f32 * 0.70, w, h } } else { Rect { x: width as f32 * 0.48, y: height as f32 * 0.42, w, h } });
        scooch_core::Scene { width, height, icons }
    }

    fn timed(values: &[f64]) -> String {
        format!("{}, over 16.7 ms {}", stats(values), values.iter().filter(|value| **value > 16.7).count())
    }

    const COLD: [u32; 2] = [2880, 1800];

    fn cold_split(picture_file: PathBuf, canvas_file: PathBuf) -> ExitCode {
        let result = (|| -> Result<Vec<f64>> {
            let ms = |since: Instant| since.elapsed().as_secs_f64() * 1000.0;
            let limit = Duration::from_secs(10);
            let clock = Instant::now();
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            gpu.ready().map_err(anyhow::Error::msg)?;
            let device = ms(clock);
            let [found, opened, shaders] = gpu.made.map(|part| part.as_secs_f64() * 1000.0);
            let clock = Instant::now();
            let cached = scooch::live::canvas::read(&canvas_file)?;
            let read = ms(clock);
            let clock = Instant::now();
            let canvas = cached.canvas()?;
            let check = ms(clock);
            let clock = Instant::now();
            let expanded = scooch_live::rgba(&canvas.pixels);
            let expand = ms(clock);
            drop(expanded);
            let clock = Instant::now();
            let mipped = gpu.try_picture(&canvas).map_err(anyhow::Error::msg)?;
            let upload = ms(clock);
            gpu.idle(limit).map_err(anyhow::Error::msg)?;
            let upload_done = ms(clock);
            drop(mipped);
            let clock = Instant::now();
            let levels = scooch_live::levels(&canvas);
            let levels_cpu = ms(clock);
            drop(levels);
            let clock = Instant::now();
            let uploaded = gpu.try_picture_levels(cached.width, cached.height, &cached.levels()).map_err(anyhow::Error::msg)?;
            let stored_upload = ms(clock);
            gpu.idle(limit).map_err(anyhow::Error::msg)?;
            let stored_done = ms(clock);
            let clock = Instant::now();
            let image = objc2::rc::autoreleasepool(|_| picture::decode(&picture_file))?;
            let decode = ms(clock);
            let clock = Instant::now();
            let _ = scooch_core::canvas(&image, COLD[0], COLD[1]);
            let made = ms(clock);
            let clock = Instant::now();
            let heavy = scooch_live::heavy(COLD[0], COLD[1], &uploaded);
            let glide = gpu.glide(&uploaded, &heavy, &heavy).map_err(anyhow::Error::msg)?;
            let look = [(Layered::Glide(&glide, 0.5), 1.0)];
            let _ = gpu.time(COLD[0], COLD[1], &look);
            let _ = gpu.time(COLD[0], COLD[1], &look);
            Ok(vec![device, read, upload, upload_done, levels_cpu, stored_upload, stored_done, decode, made, ms(clock), found, opened, shaders, expand, check])
        })();
        match result {
            Ok(split) => {
                println!("split {}", split.iter().map(|value| format!("{value:.1}")).collect::<Vec<_>>().join(" "));
                ExitCode::SUCCESS
            }
            Err(error) => {
                println!("failed {error:#}");
                ExitCode::FAILURE
            }
        }
    }

    fn cold_start(out: PathBuf) -> ExitCode {
        let _ = fs::create_dir_all(&out);
        let mut lines = vec!["# Scooch helper cold start".to_string(), String::new()];
        let result = (|| -> Result<()> {
            let load = Command::new("/usr/bin/uptime").output().map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string()).unwrap_or_default();
            lines.push(format!("- uptime before `{load}`"));
            let me = std::env::current_exe()?;
            let program = me.parent().and_then(Path::parent).map(|folder| folder.join("scooch")).filter(|path| path.is_file()).context("the scooch program is not next to the examples folder")?;
            let file = out.join("balloon-sky.jpg");
            fs::write(&file, BALLOON)?;
            let image = objc2::rc::autoreleasepool(|_| picture::decode(&file))?;
            let canvas = scooch_core::canvas(&image, COLD[0], COLD[1]);
            let canvas_file = out.join("balloon.canvas");
            let print = scooch::live::canvas::fingerprint(canvas.width, canvas.height, &canvas.pixels);
            scooch::live::canvas::write(&canvas_file, canvas.width, canvas.height, &scooch_live::levels(&canvas), print)?;
            let size = fs::metadata(&canvas_file).map(|meta| meta.len()).unwrap_or(0);
            let (mut totals, mut hellos, mut warms, mut gone) = (Vec::new(), Vec::new(), Vec::new(), 0);
            let (mut adapter, mut from, mut scale) = (String::new(), String::new(), None);
            for _ in 0..10 {
                let started = Instant::now();
                let mut helper = scooch::live::Helper::launch(&program)?;
                let warm = helper.ask_warm_with(scooch::live::Hints::default(), Some(&file), Some(COLD), Some(&canvas_file), std::collections::BTreeMap::new())?;
                helper.expect(Duration::from_secs(10), |reply| matches!(reply, scooch::live::Reply::Hello { .. }).then_some(Ok(())))?;
                hellos.push(started.elapsed().as_secs_f64() * 1000.0);
                let ready = helper.ready_for(warm, Duration::from_secs(30))?;
                totals.push(started.elapsed().as_secs_f64() * 1000.0);
                warms.push(ready.ms as f64);
                adapter = format!("{} of kind {}", ready.adapter, ready.kind);
                from = ready.from.unwrap_or_default();
                scale = ready.scale;
                let pid = helper.pid;
                drop(helper);
                let alive = Command::new("/bin/kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|status| status.success());
                if !alive {
                    gone += 1;
                }
                std::thread::sleep(Duration::from_millis(300));
            }
            lines.push(format!("- helper launched cold ten times, each warmed from a {}x{} canvas file of {size} bytes and ended, adapter {adapter}, canvas from {from}, first scale {scale:?}", canvas.width, canvas.height));
            lines.push(format!("  - launch to ready {}", stats(&totals)));
            lines.push(format!("  - process start, launch to hello {}", stats(&hellos)));
            lines.push(format!("  - warm inside the helper, device with shaders, canvas read, check and upload of the stored mips {}", stats(&warms)));
            lines.push(format!("  - helpers gone after their run {gone} of 10"));
            let mut helper = scooch::live::Helper::launch(&program)?;
            let parked = helper.ask_park()?;
            let ready = helper.ready_for(parked, Duration::from_secs(30))?;
            std::thread::sleep(Duration::from_secs(2));
            let footprint = |pid: u32| rusage(pid as i32).map_or("unread".to_string(), |usage| format!("{:.1} MB", usage.footprint as f64 / 1_048_576.0));
            let bare = footprint(helper.pid);
            let warm = helper.ask_warm_with(scooch::live::Hints::default(), Some(&file), Some(COLD), Some(&canvas_file), std::collections::BTreeMap::new())?;
            helper.ready_for(warm, Duration::from_secs(30))?;
            std::thread::sleep(Duration::from_secs(1));
            let loaded = footprint(helper.pid);
            std::thread::sleep(Duration::from_secs(32));
            let before = rusage(helper.pid as i32);
            std::thread::sleep(Duration::from_secs(30));
            let after = rusage(helper.pid as i32);
            let woke = before.zip(after).map(|(before, after)| (after.idle_wakeups.saturating_sub(before.idle_wakeups), after.interrupt_wakeups.saturating_sub(before.interrupt_wakeups), nanos((after.user + after.system).saturating_sub(before.user + before.system)) / 1_000_000));
            lines.push(format!("- a parked helper on {} of kind {}, parked {} ms after its launch: footprint parked {bare}, with one {}x{} canvas loaded {loaded}, 32 s after its last command {}, over the 30 s after that idle and interrupt wakeups and CPU ms {woke:?}", ready.adapter, ready.kind, ready.ms, canvas.width, canvas.height, footprint(helper.pid)));
            drop(helper);
            let children = || {
                let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
                unsafe { libc::getrusage(libc::RUSAGE_CHILDREN, &mut usage) };
                let ms = |time: libc::timeval| time.tv_sec as f64 * 1000.0 + time.tv_usec as f64 / 1000.0;
                ms(usage.ru_utime) + ms(usage.ru_stime)
            };
            for held in [70u64, 300] {
                let (mut cpu, mut exits, mut left) = (Vec::new(), Vec::new(), 0);
                for _ in 0..10 {
                    let before = children();
                    let mut helper = scooch::live::Helper::launch(&program)?;
                    helper.ask_warm_with(scooch::live::Hints::default(), None, None, None, std::collections::BTreeMap::new())?;
                    std::thread::sleep(Duration::from_millis(held));
                    let pid = helper.pid;
                    let quit = Instant::now();
                    drop(helper);
                    exits.push(quit.elapsed().as_secs_f64() * 1000.0);
                    if Command::new("/bin/kill").args(["-0", &pid.to_string()]).stderr(Stdio::null()).status().is_ok_and(|status| status.success()) {
                        left += 1;
                    }
                    cpu.push(children() - before);
                    std::thread::sleep(Duration::from_millis(300));
                }
                lines.push(format!("- an early launch thrown away after {held} ms, device only and then quit, ten times, helpers left running {left}"));
                lines.push(format!("  - CPU time of the helper, user and system {}", stats(&cpu)));
                lines.push(format!("  - quit to exit {}", stats(&exits)));
            }
            let mut parts: [Vec<f64>; 15] = Default::default();
            let mut failed = Vec::new();
            for _ in 0..10 {
                let ran = Command::new(&me).arg("cold-split").arg(&file).arg(&canvas_file).stderr(Stdio::null()).output()?;
                let text = String::from_utf8_lossy(&ran.stdout).to_string();
                match text.lines().find_map(|line| line.strip_prefix("split ")) {
                    Some(values) => {
                        for (slot, value) in values.split_whitespace().filter_map(|value| value.parse::<f64>().ok()).enumerate().take(parts.len()) {
                            parts[slot].push(value);
                        }
                    }
                    None => failed.push(text.trim().to_string()),
                }
            }
            let names = [
                "device with shader compile",
                "canvas file read, every level",
                "upload with mips made on the GPU instead, CPU side",
                "upload with mips made on the GPU instead, until the GPU is idle",
                "mips made on the CPU, as the daemon does once per canvas",
                "upload of the stored mips, CPU side",
                "upload of the stored mips, until the GPU is idle",
                "the old path, source picture decode",
                "the old path, canvas resample",
                "two timing frames, now after ready",
                "of the device, finding the adapter",
                "of the device, opening it",
                "of the device, compiling the shaders",
                "of the upload, three channels to four on the CPU",
                "the canvas for the tables taken from the file and checked against its fingerprint, beside the upload",
            ];
            lines.push(format!("- the warm steps in ten fresh processes, failed {failed:?}"));
            for (name, values) in names.iter().zip(&parts) {
                lines.push(format!("  - {name} {}", stats(values)));
            }
            let _ = fs::remove_file(&file);
            let _ = fs::remove_file(&canvas_file);
            let load = Command::new("/usr/bin/uptime").output().map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string()).unwrap_or_default();
            lines.push(format!("- uptime after `{load}`"));
            Ok(())
        })();
        if let Err(error) = result {
            lines.push(format!("- **stopped** {error:#}"));
        }
        for line in &lines {
            println!("{line}");
        }
        let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        ExitCode::SUCCESS
    }

    fn gpu_bench(out: PathBuf) -> ExitCode {
        let _ = fs::create_dir_all(&out);
        let mut lines = vec!["# Scooch GPU glide bench".to_string(), String::new()];
        let result = (|| -> Result<()> {
            let marker = MainThreadMarker::new().context("not on the main thread")?;
            accessory(marker);
            let load = Command::new("/usr/bin/uptime").output().map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string()).unwrap_or_default();
            lines.push(format!("- uptime before `{load}`"));
            let pid = std::process::id() as i32;
            let file = out.join("balloon-sky.jpg");
            fs::write(&file, BALLOON)?;
            let image = objc2::rc::autoreleasepool(|_| picture::decode(&file))?;
            let _ = fs::remove_file(&file);
            let (width, height) = (2880u32, 1800u32);
            let options = scooch_core::Options { scenery: scooch_core::Scenery::Auto, ..Default::default() };
            let made = Instant::now();
            let analysis = scooch_core::analyze(&image);
            let (one, two) = (bench_scene(width, height, false), bench_scene(width, height, true));
            let first = scooch_core::solve(&analysis, &one, &options, None);
            let second = scooch_core::solve(&analysis, &two, &options, Some(&first));
            let a = scooch_core::bundle(&image, &analysis, &first, &one, &options);
            let b = scooch_core::bundle(&image, &analysis, &second, &two, &options);
            let canvas = scooch_core::canvas(&image, width, height);
            lines.push(format!("- balloon picture {}x{}, two {width}x{height} bundles with scenery and the canvas made on the CPU in {} ms, compatible {}", image.width(), image.height(), made.elapsed().as_millis(), scooch_core::compatible(&a, &b)));
            lines.push(format!("- before any device {}", memory(pid)));
            let started = Instant::now();
            let gpu = Gpu::new(false).map_err(anyhow::Error::msg)?;
            gpu.ready().map_err(anyhow::Error::msg)?;
            lines.push(format!("- device {} with shaders ready after {} ms, {}", gpu.name, started.elapsed().as_millis(), memory(pid)));
            let started = Instant::now();
            let picture = gpu.picture(&canvas);
            let glide = gpu.glide(&picture, &a, &b).map_err(anyhow::Error::msg)?;
            lines.push(format!("- canvas {}x{} with {} mip levels and both tables uploaded in {} ms, {}", picture.width, picture.height, picture.levels, started.elapsed().as_millis(), memory(pid)));
            let frame = |index: usize| smoother((index % 27) as f32 / 26.0);
            let metal = objc2_quartz_core::CAMetalLayer::new();
            metal.setOpaque(true);
            let mut screen = gpu.screen(Target::Layer(objc2::rc::Retained::as_ptr(&metal) as *mut c_void), width, height).map_err(anyhow::Error::msg)?;
            for index in 0..10 {
                layer::present(|| screen.show(&[(Layered::Glide(&glide, frame(index)), 1.0)])).map_err(anyhow::Error::msg)?;
            }
            let mut paced = Vec::new();
            let mut last = Instant::now();
            for index in 0..300 {
                layer::present(|| screen.show(&[(Layered::Glide(&glide, frame(index)), 1.0)])).map_err(anyhow::Error::msg)?;
                let now = Instant::now();
                paced.push((now - last).as_secs_f64() * 1000.0);
                last = now;
            }
            let drained = Instant::now();
            drop(screen);
            lines.push(format!("- 300 glide frames presented to an offscreen Metal layer with three drawables, time between frames {}, the last frames finished {} ms after the loop", timed(&paced), drained.elapsed().as_millis()));
            let mut timed_frames = Vec::new();
            for index in 0..300 {
                let took = gpu.time(width, height, &[(Layered::Glide(&glide, frame(index)), 1.0)]).map_err(anyhow::Error::msg)?;
                timed_frames.push(took.as_secs_f64() * 1000.0);
            }
            lines.push(format!("- 300 glide frames each drawn alone to an offscreen target, submit until the GPU finished {}", timed(&timed_frames)));
            lines.push(format!("- with the device and pictures {}", memory(pid)));
            drop(glide);
            drop(picture);
            drop(gpu);
            spin(0.5);
            lines.push(format!("- after the device was dropped {}", memory(pid)));
            let load = Command::new("/usr/bin/uptime").output().map(|ran| String::from_utf8_lossy(&ran.stdout).trim().to_string()).unwrap_or_default();
            lines.push(format!("- uptime after `{load}`"));
            Ok(())
        })();
        if let Err(error) = result {
            lines.push(format!("- **stopped** {error:#}"));
        }
        for line in &lines {
            println!("{line}");
        }
        let _ = fs::write(out.join("report.md"), lines.join("\n") + "\n");
        ExitCode::SUCCESS
    }

    fn spaces_check(p: &mut Probe) -> Result<()> {
        let before = spaces::window_server().map(|monitors| monitors.iter().map(|monitor| monitor.spaces.len()).sum::<usize>());
        let ui = p.osa("tell application \"System Events\" to get UI elements enabled");
        p.line(format!("- System Events UI scripting enabled {}", ui.text()));
        let opened = p.tool("/usr/bin/open", &["-a", "Mission Control"]);
        std::thread::sleep(Duration::from_millis(1500));
        let added = p.osa("with timeout of 15 seconds\ntell application \"System Events\" to tell process \"Dock\" to click (first button whose description is \"add desktop\") of group 2 of group 1 of group 1\nend timeout");
        std::thread::sleep(Duration::from_millis(1500));
        let _ = p.osa("tell application \"System Events\" to key code 53");
        std::thread::sleep(Duration::from_millis(1500));
        let after = spaces::window_server().map(|monitors| monitors.iter().map(|monitor| monitor.spaces.len()).sum::<usize>());
        p.line(format!("- Mission Control opened {}, add desktop click {}, desktop Spaces before {before:?} after {after:?}", if opened.ok() { "ok" } else { "failed" }, if added.ok() { "ok".to_string() } else { added.text() }));
        if after <= before {
            p.line("- a second Space could not be made on this runner without private APIs, so the Spaces cycle stops here".to_string());
            return Ok(());
        }
        let screen = main_screen(p)?;
        let display = spaces::split(&screen.id).0.to_string();
        let first = spaces::current(&display);
        let switched = switch(p, 124);
        let second = spaces::current(&display);
        p.line(format!("- switch right {}, showing Space {second:?} was {first:?}", if switched.ok() { "ok".to_string() } else { switched.text() }));
        if second == first || second.is_none() {
            p.line("- switching Spaces did not work on this runner, so the Spaces cycle stops here".to_string());
            return Ok(());
        }
        switch(p, 123);
        if let Err(error) = space_layer(p) {
            p.fail(format!("the layer could not be checked on Spaces {error:#}"));
        }
        let subject = p.subject.clone().context("no test picture")?;
        let _ = place(p, PROBES[0].0, (screen.frame.x + screen.frame.w * 0.5, screen.frame.y + screen.frame.h * 0.5));
        let second_id = spaces::join(&display, second.as_deref());
        let ran = p.scooch(&["use", &subject.to_string_lossy()], Duration::from_secs(60));
        p.line(format!("- scooch use on the first Space exit {:?}", ran.code));
        if wait_render(p, Duration::from_secs(30), Some(&subject), true).is_none() {
            stand_in(p, "Spaces")?;
        }
        let one = wait_render(p, Duration::from_secs(30), Some(&subject), false);
        p.line(format!("- first Space rendered {:?}, entries {:?}", one.map(|time| time.as_millis()), entries(p)));
        switch(p, 124);
        let home = p.home.clone();
        let two = wait_until(Duration::from_secs(25), || {
            let state: serde_json::Value = fs::read(home.join("state.json")).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
            state["screens"].as_object().is_some_and(|screens| screens.len() >= 2 && screens.get(&second_id).is_some_and(|entry| !entry["output"].is_null()))
        });
        p.line(format!("- second Space entry with its own output after {:?} ms: {:?}", two.map(|time| time.as_millis()), entries(p)));
        switch(p, 123);
        let ran = p.scooch(&["off"], Duration::from_secs(60));
        reap(p);
        p.line(format!("- scooch off from the first Space exit {:?}: {}", ran.code, ran.text()));
        p.line(format!("- after off, restoring {}, entries {:?}, login item loaded {}", state(p)["restoring"], entries(p), launchd::loaded()));
        let plist = fs::read_to_string(launchd::plist_path()?).unwrap_or_default();
        let restorer = daemon_pid(p);
        let parent = restorer.map(|pid| p.tool("/bin/ps", &["-o", "ppid=", "-p", &pid.to_string()]).out.trim().to_string());
        p.line(format!("- the restoring daemon is the real login item {}, pid {restorer:?} with parent {parent:?}", !plist.is_empty() && plist.contains("<string>watch</string>")));
        switch(p, 124);
        let finished = wait_until(Duration::from_secs(30), || !launchd::loaded() && !launchd::installed());
        let back = platform::current()?.screens()?.first().cloned().and_then(|screen| platform::current().ok()?.wallpaper(&screen).ok().flatten());
        p.line(format!("- second Space shows {back:?}, the login item removed itself after {:?} ms, tally {}", finished.map(|time| time.as_millis()), state(p)["tally"]));
        switch(p, 123);
        p.line(format!("- back on the first Space, it now reads as {:?}, and the window server lists {:?}", spaces::current(&display), spaces::window_server().map(|monitors| monitors.into_iter().map(|monitor| monitor.spaces).collect::<Vec<_>>())));
        if one.is_none() || two.is_none() || finished.is_none() || back.as_ref() != Some(&subject) {
            p.fail("the Spaces cycle did not finish");
        }
        Ok(())
    }

    const STOP_LIMIT: Duration = Duration::from_secs(6);

    fn pid_lives(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    fn where_threads_sit(text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut thread: Option<(String, usize, Vec<String>)> = None;
        let mut stack: Vec<(usize, String)> = Vec::new();
        let finish = |thread: Option<(String, usize, Vec<String>)>, found: &mut Vec<String>| {
            if let Some((name, _, path)) = thread {
                if name.contains("main-thread") || name.contains("scooch") {
                    found.push(format!("{name}: {}", path.join(" > ")));
                }
            }
        };
        for line in text.lines().skip_while(|line| !line.starts_with("Call graph:")).skip(1) {
            if line.trim().is_empty() {
                break;
            }
            let content = line.trim_start_matches([' ', '+', '!', ':', '|']);
            let depth = line.len() - content.len();
            let rest = content.split_once(' ').map_or(content, |(_, rest)| rest).trim();
            if rest.starts_with("Thread_") {
                finish(thread.take(), &mut found);
                let name = rest.split_once(char::is_whitespace).map_or(rest, |(_, name)| name).split("  (").next().unwrap_or(rest).trim().to_string();
                thread = Some((name, 0, Vec::new()));
                stack.clear();
                continue;
            }
            let frame = rest.split("  (in").next().unwrap_or(rest).trim().to_string();
            while stack.last().is_some_and(|(at, _)| *at >= depth) {
                stack.pop();
            }
            stack.push((depth, frame.clone()));
            if let Some((_, deepest, path)) = thread.as_mut() {
                if depth > *deepest {
                    *deepest = depth;
                    let ours: Vec<String> = stack.iter().filter(|(_, name)| name.contains("scooch") || name.contains("dispatch_sync") || name.contains("AE")).map(|(_, name)| name.chars().take(90).collect()).collect();
                    *path = ours.into_iter().rev().take(6).collect::<Vec<_>>().into_iter().rev().chain(std::iter::once(frame)).collect();
                }
            }
        }
        finish(thread.take(), &mut found);
        found
    }

    fn sample_threads(p: &mut Probe, pid: i32, name: &str, seconds: &str) -> Vec<String> {
        let file = p.out.join(format!("sample-{name}.txt"));
        let ran = p.tool("/usr/bin/sample", &[&pid.to_string(), seconds, "-file", &file.to_string_lossy()]);
        let threads = where_threads_sit(&fs::read_to_string(&file).unwrap_or_default());
        if threads.is_empty() {
            return vec![format!("no sample, exit {:?} {}", ran.code, ran.text().chars().take(200).collect::<String>())];
        }
        threads
    }

    #[derive(Clone, Copy, PartialEq)]
    enum Ending {
        Bootout,
        Term,
    }

    fn close_consent(p: &mut Probe) -> Vec<String> {
        let mut closed = Vec::new();
        for owner in ["UserNotificationCenter", "universalAccessAuthWarn", "CoreServicesUIAgent"] {
            if p.tool("/usr/bin/killall", &[owner]).ok() {
                closed.push(owner.to_string());
            }
        }
        closed
    }

    fn stop_round(p: &mut Probe, name: &str, what: &str, start: impl FnOnce(&Path) -> Result<()>, ending: Ending) -> Result<()> {
        let program = p.bin.clone();
        if let Err(error) = start(&program) {
            p.fail(format!("{what}: the start failed, {error:#}"));
            return Ok(());
        }
        let mut job = None;
        wait_until(Duration::from_secs(10), || {
            job = daemon_pid(p);
            job.is_some()
        });
        let mut holder = None;
        wait_until(Duration::from_secs(10), || {
            holder = scooch_pid(p);
            holder.is_some()
        });
        std::thread::sleep(Duration::from_secs(3));
        let pending = asking(p) || windows().iter().any(|(owner, _, _)| owner == "UserNotificationCenter");
        let before = p.tool("/bin/launchctl", &["print", &target()]);
        let _ = fs::write(p.out.join(format!("launchctl-{name}-before.txt")), &before.out);
        p.line(format!("- {what}: launchctl names the job pid {job:?}, the lock is held by {holder:?}, a Finder consent window is open {pending}, the saved problem {:?}", problem(p)));
        if job.is_none() || job != holder {
            p.fail(format!("{what}: launchctl names the job pid {job:?} while the daemon that holds the lock is {holder:?}"));
        }
        let Some(pid) = job.or(holder) else { return Ok(()) };
        let threads = sample_threads(p, pid, &format!("{name}-before"), "1");
        p.line(format!("  - where the daemon's threads sit before the end, {threads:?}"));
        let sent = Instant::now();
        let mut bootout = None;
        let how = match ending {
            Ending::Bootout => {
                bootout = Command::new("/bin/launchctl").args(["bootout", &target()]).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().ok();
                "the boot out"
            }
            Ending::Term => {
                unsafe { libc::kill(pid, libc::SIGTERM) };
                "SIGTERM"
            }
        };
        let gone = wait_until(STOP_LIMIT, || !pid_lives(pid));
        let took = gone.map(|_| sent.elapsed().as_millis());
        if took.is_none() {
            let stuck = sample_threads(p, pid, &format!("{name}-stuck"), "3");
            let after = p.tool("/bin/launchctl", &["print", &target()]);
            let _ = fs::write(p.out.join(format!("launchctl-{name}-after.txt")), &after.out);
            p.line(format!("  - {} s after {how} the daemon {pid} still ran, where its threads sit, {stuck:?}, launchctl print kept in launchctl-{name}-after.txt", STOP_LIMIT.as_secs()));
            p.fail(format!("{what}: the daemon {pid} still ran {} s after {how}, the stop cap of 5 s and one second more", STOP_LIMIT.as_secs()));
            if wait_until(Duration::from_secs(25), || !pid_lives(pid)).is_none() {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
        let answered = bootout.and_then(|child| child.wait_with_output().ok()).map(|output| format!(", launchctl bootout exit {:?} {}", output.status.code(), String::from_utf8_lossy(&output.stderr).trim()));
        p.line(format!("- {what}: after {how} the daemon {pid} was gone after {took:?} ms{}", answered.unwrap_or_default()));
        let closed = close_consent(p);
        p.line(format!("  - closed the consent windows of {closed:?}, which answers nothing"));
        Ok(())
    }

    fn render_stop(p: &mut Probe, picture: &Path) -> Result<()> {
        if launchd::loaded() {
            let _ = p.tool("/bin/launchctl", &["bootout", &target()]);
        }
        let _ = fs::remove_file(p.home.join("watch.pid"));
        let log = fs::OpenOptions::new().create(true).append(true).open(p.home.join("watch.log"))?;
        let mut child = Command::new(&p.bin).arg("watch").env("SCOOCH_HOME", &p.home).env("SCOOCH_TRACE", "1").stdin(Stdio::null()).stdout(Stdio::null()).stderr(log).spawn()?;
        let pid = child.id() as i32;
        let home = p.home.clone();
        let held = wait_until(Duration::from_secs(15), || fs::read_to_string(home.join("watch.pid")).ok().and_then(|text| text.trim().parse::<i32>().ok()) == Some(pid));
        let drawn = wait_render(p, Duration::from_secs(40), None, false);
        p.line(format!("- a daemon started from the probe as {pid}, which inherits the runner's Finder automation, held the lock after {held:?} and drew after {drawn:?}"));
        let mark = daemon_log(p).lines().count();
        let asked = scooch::facade::use_picture(&p.bin, picture);
        let handed = Instant::now();
        let busy = wait_until(Duration::from_secs(5), || daemon_log(p).lines().skip(mark).any(|line| line.contains(" snapshot ") || line.contains(" wake ")));
        let sent = Instant::now();
        unsafe { libc::kill(pid, libc::SIGTERM) };
        let mut ended = None;
        while sent.elapsed() < STOP_LIMIT + Duration::from_secs(4) {
            if let Ok(Some(status)) = child.try_wait() {
                ended = Some((status.code(), sent.elapsed().as_millis()));
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let stuck = if ended.is_none() { sample_threads(p, pid, "render-stuck", "3") } else { Vec::new() };
        if ended.is_none() {
            let _ = child.kill();
            let _ = child.wait();
        }
        let lines: Vec<String> = daemon_log(p).lines().skip(mark).map(str::to_string).collect();
        let ticked = lines.iter().any(|line| line.contains(" tick "));
        let state = state(p);
        let whole_state = state.is_object();
        let desktop = platform::current()?;
        let screen = main_screen(p)?;
        let shown = desktop.wallpaper(&screen).ok().flatten();
        let whole_picture = shown.as_deref().is_some_and(|path| scooch_core::open(path).is_ok());
        p.line(format!(
            "- SIGTERM {} ms after the new picture was handed over with error {:?}, the tick seen running after {busy:?}: ended with exit and ms {ended:?}, the tick finished first {ticked}, state.json reads whole {whole_state}, daemon mark {:?}, the wallpaper {shown:?} decodes whole {whole_picture}",
            sent.duration_since(handed).as_millis(),
            asked.as_ref().err().map(|error| format!("{error:#}")),
            state["daemon"]
        ));
        if !ended.is_some_and(|(code, took)| code == Some(0) && took < STOP_LIMIT.as_millis()) {
            p.fail(format!("SIGTERM during a tick ended the daemon {ended:?}, where it must end with exit 0 within the stop cap of 5 s and one second more, threads {stuck:?}"));
        }
        if !whole_state || !whole_picture {
            p.fail("a stop during a tick left a state or a wallpaper that does not read whole");
        }
        Ok(())
    }

    fn consent_windows() -> usize {
        windows().iter().filter(|(owner, _, _)| owner == "UserNotificationCenter").count()
    }

    fn consent_lines(p: &Probe, from: usize) -> Vec<String> {
        daemon_log(p).lines().skip(from).filter(|line| line.contains(" consent ")).map(str::to_string).collect()
    }

    fn waits(threads: &[String]) -> bool {
        threads.iter().any(|thread| thread.starts_with("scooch-consent"))
    }

    fn consent_waiter(p: &mut Probe) -> Result<()> {
        if !scooch_processes(p).is_empty() || launchd::loaded() || launchd::plist_path().is_ok_and(|path| path.exists()) {
            let ran = p.scooch(&["off"], Duration::from_secs(60));
            p.line(format!("- scooch off before these checks exit {:?}: {}", ran.code, ran.text()));
            reap(p);
        }
        let closed = close_consent(p);
        std::thread::sleep(Duration::from_secs(1));
        p.limit("a runner cannot press Allow or Don't Allow, so the wake at Allow is proven by tests and this case shows the waiter, the windows and the asks");
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let picture = picture_file(p, "consent.jpg", &subject_picture(width, height, (0.4, 0.5)))?;
        let mark = daemon_log(p).lines().count();
        let on = p.scooch(&["use", &picture.to_string_lossy()], Duration::from_secs(60));
        let mut first = None;
        wait_until(Duration::from_secs(10), || {
            first = daemon_pid(p);
            first.is_some()
        });
        let asked = wait_until(Duration::from_secs(15), || asking(p));
        let shown = wait_until(Duration::from_secs(10), || consent_windows() >= 1);
        let windows_first = consent_windows();
        let tick_a = first_tick(p, mark);
        p.line(format!("- closed {closed:?} first, then scooch use turned on with exit {:?}, job A {first:?}, its first tick took {tick_a:?} ms, the saved problem asks for Allow after {asked:?}, a consent window after {shown:?}, consent windows {windows_first}", on.code));
        if !tick_a.is_some_and(|took| took < 1000) {
            p.fail(format!("job A's first tick with the permission undecided took {tick_a:?} ms, where it ends in under 1000 ms"));
        }
        if shown.is_none() {
            p.fail("no consent window came for job A");
        }
        let Some(a) = first else {
            p.fail("job A did not start");
            return Ok(());
        };
        let threads_a = sample_threads(p, a, "consent-a", "1");
        p.line(format!("  - job A's threads {threads_a:?}, its consent lines {:?}", consent_lines(p, mark)));
        let _ = Command::new("/bin/launchctl").args(["bootout", &target()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let ended = wait_until(STOP_LIMIT, || !pid_lives(a));
        std::thread::sleep(Duration::from_secs(1));
        let windows_left = consent_windows();
        let stayed = windows_left > 0;
        p.line(format!("- job A booted out, gone after {ended:?}, consent windows still on screen {windows_left}, so its window {}", if stayed { "stayed after its asker ended" } else { "went away with its asker" }));
        let from = daemon_log(p).lines().count();
        let plist = launchd::plist_path()?;
        let booted = p.tool("/bin/launchctl", &["bootstrap", &format!("gui/{}", uid()), &plist.to_string_lossy()]);
        let mut second = None;
        wait_until(Duration::from_secs(10), || {
            second = daemon_pid(p);
            second.is_some()
        });
        std::thread::sleep(Duration::from_secs(4));
        let windows_second = consent_windows();
        let Some(b) = second else {
            p.fail(format!("job B did not start from the same login item, bootstrap exit {:?} {}", booted.code, booted.text()));
            return Ok(());
        };
        let threads_b = sample_threads(p, b, "consent-b", "1");
        let tick_b = first_tick(p, from);
        p.line(format!("- job B {b} started from the same login item as a login would, its first tick took {tick_b:?} ms, consent windows {windows_second}, its threads {threads_b:?}, its consent lines {:?}", consent_lines(p, from)));
        if !tick_b.is_some_and(|took| took < 1000) {
            p.fail(format!("job B's first tick with the permission undecided took {tick_b:?} ms, where it ends in under 1000 ms"));
        }
        if !waits(&threads_b) {
            p.fail("job B keeps no waiter for the answer while the permission is undecided");
        }
        let elsewhere: Vec<&String> = threads_b.iter().filter(|thread| !thread.starts_with("scooch-consent") && thread.contains("AEDeterminePermissionToAutomateTarget")).collect();
        if !elsewhere.is_empty() {
            p.fail(format!("a thread of job B other than scooch-consent sits in a permission check, {elsewhere:?}"));
        }
        let early = consent_lines(p, from).iter().filter(|line| line.contains(" asks whether")).count();
        p.line(format!("  - job B asked {early} times while job A's window {}", if stayed { "was still on screen, judged" } else { "was gone, reported only" }));
        if stayed && early > 0 {
            p.fail("job B asked while job A's consent window was still on screen");
        }
        let Some((woke, buckets)) = quiet_buckets(b) else {
            p.fail("job B's usage could not be read over its first minute");
            return Ok(());
        };
        p.line(format!("- job B's first 60 s with the permission undecided, reported only since it holds the launch of the process: idle and interrupt wakeups {woke:?}, idle wakeups in each 5 s {buckets:?}"));
        let Some((woke, buckets)) = quiet_buckets(b) else {
            p.fail("job B's usage could not be read over its second minute");
            return Ok(());
        };
        p.line(format!("- job B's second 60 s, once the launch is over, with its waiter waiting: idle and interrupt wakeups {woke:?}, idle wakeups in each 5 s {buckets:?}, the same minute of check 6 without any watch {:?}", p.quiet));
        if crate::woke_more(woke, p.quiet, 0) {
            p.fail("job B's waiter woke more at rest than the daemon in check 6");
        }
        let listening = consent_lines(p, from).iter().any(|line| line.contains("waits for the answer without asking again"));
        p.line(format!("  - job B's own ask {}", if listening { "came back without a decision, so its waiter waits without asking" } else { "still waits on the open window, so its waiter is that ask" }));
        let mut closes = 0;
        let mut waits_now = false;
        while closes < 3 && !waits_now {
            let closed = close_consent(p);
            closes += 1;
            std::thread::sleep(Duration::from_secs(3));
            waits_now = consent_lines(p, from).iter().any(|line| line.contains("waits for the answer without asking again"));
            p.line(format!("  - close {closes} of the consent window unanswered, quit {closed:?}, consent windows now {}, job B waits without asking {waits_now}", consent_windows()));
        }
        let windows_closed = consent_windows();
        let threads_after = sample_threads(p, b, "consent-b-after", "1");
        let lines = consent_lines(p, from);
        let asks = lines.iter().filter(|line| line.contains(" asks whether")).count();
        p.line(format!("- after {closes} closes job B listens {waits_now}, consent windows {windows_closed}, it asked {asks} times in all, its consent lines {lines:?}, its threads {threads_after:?}"));
        if !waits_now || !waits(&threads_after) {
            p.fail("after the window closed unanswered job B kept no waiter that waits without asking");
        }
        if asks != 1 {
            p.fail(format!("job B asked the person {asks} times before it listened, where it asks once"));
        }
        let nudged_from = daemon_log(p).lines().count();
        let opened = p.tool("/usr/bin/open", &["-a", "Calculator"]);
        std::thread::sleep(Duration::from_secs(3));
        let windows_nudged = consent_windows();
        let rechecks = consent_lines(p, nudged_from);
        let _ = p.tool("/usr/bin/killall", &["Calculator"]);
        let asks = consent_lines(p, from).iter().filter(|line| line.contains(" asks whether")).count();
        p.line(format!("- another app came to the front, open exit {:?}, job B's consent lines since {rechecks:?}, consent windows {windows_nudged} against {windows_closed} before, asks in all {asks}", opened.code));
        if !rechecks.iter().any(|line| line.contains("checked again without asking")) {
            p.fail("job B did not check again when another app came to the front");
        }
        if windows_nudged > windows_closed {
            p.fail("a check without asking brought up another consent window");
        }
        if asks != 1 {
            p.fail(format!("job B asked the person {asks} times, where it asks once"));
        }
        std::thread::sleep(Duration::from_secs(2));
        let rest_from = daemon_log(p).lines().count();
        let Some((woke, buckets)) = quiet_buckets(b) else {
            p.fail("job B's usage could not be read over its listening minute");
            return Ok(());
        };
        let rested: Vec<String> = consent_lines(p, rest_from).into_iter().filter(|line| line.contains("checked again without asking")).collect();
        p.line(format!("- a quiet 60 s of job B while its waiter listens: idle and interrupt wakeups {woke:?}, idle wakeups in each 5 s {buckets:?}, checks without asking {} {rested:?}, the same minute of check 6 without any watch {:?}", rested.len(), p.quiet));
        if rested.len() > 3 {
            p.fail(format!("job B's listening waiter checked {} times in a quiet minute, where only an event from macOS makes it check", rested.len()));
        }
        if crate::woke_more(woke, p.quiet, 0) {
            p.fail("job B's listening waiter woke more at rest than the daemon in check 6");
        }
        let poked_from = daemon_log(p).lines().count();
        let poke = p.scooch(&["use", &picture.to_string_lossy()], Duration::from_secs(60));
        let back = wait_until(Duration::from_secs(10), || consent_windows() >= 1);
        let threads_poked = sample_threads(p, b, "consent-b-poked", "1");
        let asks = consent_lines(p, from).iter().filter(|line| line.contains(" asks whether")).count();
        p.line(format!("- a poke through scooch use exit {:?}: a consent window again after {back:?}, job B's consent lines since {:?}, its threads {threads_poked:?}, asks in all {asks}", poke.code, consent_lines(p, poked_from)));
        if back.is_none() || !threads_poked.iter().any(|thread| thread.starts_with("scooch-consent") && thread.contains("AEDeterminePermissionToAutomateTarget")) || asks != 2 {
            p.fail(format!("a poke did not bring the question back once, window {back:?}, asks {asks}"));
        }
        let closed = close_consent(p);
        std::thread::sleep(Duration::from_secs(2));
        p.line(format!("- closed {closed:?} unanswered again, job B's consent lines since the poke {:?}", consent_lines(p, poked_from)));
        let _ = Command::new("/bin/launchctl").args(["bootout", &target()]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
        let gone = wait_until(STOP_LIMIT, || !pid_lives(b));
        let closed = close_consent(p);
        p.line(format!("- job B booted out, gone after {gone:?}, closed {closed:?}"));
        if gone.is_none() {
            p.fail(format!("job B still ran {} s after its boot out", STOP_LIMIT.as_secs()));
        }
        Ok(())
    }

    fn first_tick(p: &Probe, from: usize) -> Option<u64> {
        daemon_log(p).lines().skip(from).find_map(|line| line.split_once(" tick ").and_then(|(_, rest)| rest.split_once(" ms").and_then(|(took, _)| took.trim().parse().ok())))
    }

    fn quiet_buckets(pid: i32) -> Option<((u64, u64), Vec<u64>)> {
        let calm = rusage(pid)?;
        let mut last = calm;
        let mut buckets = Vec::new();
        for _ in 0..12 {
            std::thread::sleep(Duration::from_secs(5));
            let now = rusage(pid)?;
            buckets.push(now.idle_wakeups.saturating_sub(last.idle_wakeups));
            last = now;
        }
        Some(((last.idle_wakeups.saturating_sub(calm.idle_wakeups), last.interrupt_wakeups.saturating_sub(calm.interrupt_wakeups)), buckets))
    }

    fn stop_check(p: &mut Probe) -> Result<()> {
        if !scooch_processes(p).is_empty() || launchd::loaded() || launchd::plist_path().is_ok_and(|path| path.exists()) {
            let ran = p.scooch(&["off"], Duration::from_secs(60));
            p.line(format!("- scooch off before these checks exit {:?}: {}", ran.code, ran.text()));
            reap(p);
        }
        let screen = main_screen(p)?;
        let (width, height) = pixels(&screen);
        let first = picture_file(p, "stop-first.jpg", &subject_picture(width, height, (0.3, 0.4)))?;
        let large = picture_file(p, "stop-large.jpg", &navy_picture(width.max(1) * 3, height.max(1) * 3, (0.7, 0.5)))?;
        p.limit("on a hosted runner the login item job waits for a Finder consent window nobody answers, which is the state the app's probe saw when a boot out left its daemon running");
        stop_round(p, "use", "the window's picture through facade::use_picture, then a boot out", |program| scooch::facade::use_picture(program, &first), Ending::Bootout)?;
        stop_round(p, "on", "the window's Turn on through facade::turn_on, then a boot out", |program| scooch::facade::turn_on(program).map(drop), Ending::Bootout)?;
        stop_round(p, "term", "facade::turn_on again, then SIGTERM to the job while its consent window is open", |program| scooch::facade::turn_on(program).map(drop), Ending::Term)?;
        render_stop(p, &large)?;
        Ok(())
    }

    fn cleanup(p: &mut Probe) -> Result<()> {
        let kept = fs::copy(p.home.join("watch.log"), p.out.join("watch.log"));
        p.line(format!("- the daemon's log copied into the artifact as watch.log {:?} bytes", kept.ok()));
        put_motion_back(p);
        if p.stand.is_some() || launchd::loaded() || launchd::plist_path().is_ok_and(|path| path.exists()) {
            let ran = p.scooch(&["off"], Duration::from_secs(60));
            p.line(format!("- scooch off during cleanup exit {:?}: {}", ran.code, ran.text()));
        }
        reap(p);
        if launchd::loaded() {
            let _ = p.tool("/bin/launchctl", &["bootout", &target()]);
        }
        if let Ok(path) = launchd::plist_path() {
            let _ = fs::remove_file(path);
        }
        for left in scooch_processes(p) {
            if let Some(pid) = left.split_whitespace().next() {
                let _ = p.tool("/bin/kill", &[pid]);
            }
        }
        let made = p.made.clone();
        for path in made {
            let removed = if path.is_dir() { fs::remove_dir_all(&path) } else { fs::remove_file(&path) };
            if path.exists() {
                p.line(format!("- could not delete {} {:?}", path.display(), removed.err()));
            }
        }
        let desktop = platform::current()?;
        let original = p.original.iter().find_map(|(_, shown)| shown.clone()).filter(|path| path.is_file());
        for screen in desktop.screens().unwrap_or_default() {
            if let Some(shown) = &original {
                let restored = desktop.set_wallpaper(&screen, shown);
                p.line(format!("- wallpaper of `{}` set back to {} {:?}", screen.id, shown.display(), restored.map_err(|error| error.to_string())));
            }
        }
        let removed = fs::remove_dir_all(&p.home);
        p.line(format!("- probe home removed {:?}", removed.map_err(|error| error.to_string())));
        gone(p, "cleanup");
        let leftovers: Vec<String> = fs::read_dir(&p.desktop).map(|entries| entries.flatten().map(|entry| entry.file_name().to_string_lossy().into_owned()).filter(|name| name.starts_with("scooch")).collect()).unwrap_or_default();
        p.line(format!("- probe files left on the Desktop {leftovers:?}"));
        if !leftovers.is_empty() {
            p.fail("probe files were left on the Desktop");
        }
        if p.out.join("icons.png").exists() {
            p.line("- the icon capture is kept in the artifact as icons.png".to_string());
        }
        Ok(())
    }
}
