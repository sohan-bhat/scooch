#![cfg_attr(windows, windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    if std::env::args_os().nth(1).is_some_and(|first| first == scooch::live::HELPER) {
        return scooch::live::helper::main();
    }
    scooch::run()
}
