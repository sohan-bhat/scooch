use std::process::ExitCode;

#[cfg(target_os = "macos")]
#[used]
#[link_section = "__TEXT,__info_plist"]
static INFO: [u8; include_bytes!("Info.plist").len()] = *include_bytes!("Info.plist");

fn main() -> ExitCode {
    scooch::run()
}
