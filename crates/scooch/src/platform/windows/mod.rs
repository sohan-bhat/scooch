pub mod layout;
pub mod wakes;

#[cfg(target_os = "windows")]
pub mod com;
#[cfg(target_os = "windows")]
pub mod picture;
#[cfg(target_os = "windows")]
pub mod shell;

#[cfg(target_os = "windows")]
pub fn desktop() -> anyhow::Result<Box<dyn super::Desktop>> {
    anyhow::bail!("this desktop adapter is not written yet")
}
