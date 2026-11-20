use std::ffi::OsString;
use std::process::Command;
use std::sync::OnceLock;

type Changes = Vec<(OsString, Option<OsString>)>;

static CHILD_ENV: OnceLock<Changes> = OnceLock::new();

pub fn child_env(changes: Changes) {
    let _ = CHILD_ENV.set(changes);
}

pub fn clean(command: &mut Command) -> &mut Command {
    for (name, value) in CHILD_ENV.get().map(Vec::as_slice).unwrap_or_default() {
        match value {
            Some(value) => command.env(name, value),
            None => command.env_remove(name),
        };
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_gets_a_set_name_loses_a_removed_one_and_keeps_the_rest() {
        child_env(vec![("SCOOCH_CHILD_SET".into(), Some("set".into())), ("SCOOCH_CHILD_GONE".into(), None)]);
        child_env(vec![("SCOOCH_CHILD_SET".into(), Some("second".into()))]);
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "echo \"$SCOOCH_CHILD_SET ${SCOOCH_CHILD_GONE-removed} $SCOOCH_CHILD_KEPT\""]).env("SCOOCH_CHILD_GONE", "inherited").env("SCOOCH_CHILD_KEPT", "kept");
        let output = clean(&mut command).output().unwrap();
        assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "set removed kept");
    }
}
