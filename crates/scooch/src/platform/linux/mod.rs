pub mod parse;

#[cfg(target_os = "linux")]
pub fn desktop() -> anyhow::Result<Box<dyn super::Desktop>> {
    anyhow::bail!("this desktop adapter is not written yet")
}
