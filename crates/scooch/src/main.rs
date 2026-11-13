use std::process::ExitCode;

#[cfg(target_os = "macos")]
#[used]
#[link_section = "__TEXT,__info_plist"]
static INFO: [u8; include_bytes!("Info.plist").len()] = *include_bytes!("Info.plist");

fn main() -> ExitCode {
    #[cfg(target_os = "macos")]
    if std::env::args_os().nth(1).is_some_and(|first| first == scooch::live::HELPER) {
        return scooch::live::helper::main();
    }
    scooch::run()
}
