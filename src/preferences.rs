use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::theme;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    /// Theme name (must match a name in `theme::THEMES`).
    pub theme: String,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: theme::THEMES[0].name.to_string(),
        }
    }
}

impl Preferences {
    /// Path to preferences file: ~/.config/legal-ko/preferences.json
    fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Cannot determine config directory")?
            .join("legal-ko");
        Ok(dir.join("preferences.json"))
    }

    /// Load preferences from disk, falling back to defaults on any error.
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(p) => p,
            Err(e) => {
                warn!("Failed to load preferences: {e}");
                Self::default()
            }
        }
    }

    fn try_load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            debug!("No preferences file at {}", path.display());
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let prefs: Self =
            serde_json::from_str(&content).context("Failed to parse preferences.json")?;
        debug!("Loaded preferences: theme={}", prefs.theme);
        Ok(prefs)
    }

    /// Save preferences to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize preferences")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        debug!("Saved preferences to {}", path.display());
        Ok(())
    }
}
