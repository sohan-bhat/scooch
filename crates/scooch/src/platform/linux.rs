use super::Desktop;
use anyhow::{bail, Result};

pub fn desktop() -> Result<Box<dyn Desktop>> {
    bail!("this desktop adapter is not written yet")
}
