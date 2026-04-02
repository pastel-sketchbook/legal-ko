use anyhow::{Context, Result};
use std::path::PathBuf;

/// Config directory: `~/.config/legal-ko/`
pub fn config_dir() -> Result<PathBuf> {
    let dir = dirs::config_dir()
        .context("Cannot determine config directory")?
        .join("legal-ko");
    Ok(dir)
}
