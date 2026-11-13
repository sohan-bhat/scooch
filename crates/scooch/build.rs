use embed_manifest::manifest::{DpiAwareness, Setting, SupportedOS};
use embed_manifest::{embed_manifest, new_manifest};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var_os("CARGO_CFG_WINDOWS").is_none() {
        return;
    }
    let manifest = new_manifest("Scooch")
        .supported_os(SupportedOS::Windows8..=SupportedOS::Windows10)
        .dpi_awareness(DpiAwareness::PerMonitorV2)
        .long_path_aware(Setting::Enabled);
    if let Err(error) = embed_manifest(manifest) {
        println!("cargo:warning=the Windows manifest was not embedded {error}");
        return;
    }
    let msvc = std::env::var("CARGO_CFG_TARGET_ENV").is_ok_and(|env| env == "msvc");
    let Some(out) = std::env::var_os("OUT_DIR") else { return };
    let file = std::path::Path::new(&out).join("manifest.xml");
    if msvc && file.is_file() {
        let path = file.canonicalize().unwrap_or(file);
        println!("cargo:rustc-link-arg-examples=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg-examples=/MANIFESTINPUT:{}", path.display());
        println!("cargo:rustc-link-arg-examples=/MANIFESTUAC:NO");
    }
}
