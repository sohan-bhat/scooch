pub const NORMAL_SKIPS: [&str; 7] = ["hidden", "no room", "too large", "helper cold", "nothing new", "helper budget", "system picture"];

pub const PARKED_FAULTS: [&str; 2] = ["helper cold", "helper budget"];

#[derive(Clone, Debug, PartialEq)]
pub enum Try {
    Glided,
    Skipped(String),
    Fault(String),
}

pub fn glide_try(log: &[String], parked: bool) -> Try {
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

pub fn parked_in(log: &[String]) -> Option<f64> {
    let launched = log.iter().rposition(|line| line.contains(" glide helper launched to park"));
    let (at, line) = log.iter().enumerate().rev().find(|(_, line)| line.contains(" glide helper parked on "))?;
    if launched.is_some_and(|launched| launched > at) {
        return None;
    }
    line.split(" glide helper parked on ").nth(1).and_then(|rest| rest.split(" ms after its launch").next()).and_then(|head| head.rsplit(' ').next()).and_then(|ms| ms.parse().ok())
}

pub fn slide_skip(slide: usize, tried: &Try) -> Option<Result<String, String>> {
    match tried {
        Try::Glided => None,
        Try::Skipped(reason) => Some(Ok(format!("{slide}: {reason}"))),
        Try::Fault(reason) => Some(Err(format!("slide {slide} did not glide with the helper parked, the daemon skipped it as {reason}, which it does not count as normal"))),
    }
}
