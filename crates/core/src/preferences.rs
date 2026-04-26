use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, warn};

/// Default theme name (must match a name in the TUI's `theme::THEMES`).
pub const DEFAULT_THEME: &str = "Default";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    /// Theme name (must match a name in the TUI's `theme::THEMES`).
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Last-used AI agent name (e.g. `"OpenCode"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,

    /// Split view ratio (left panel share, 0.0–1.0). Persisted for restore-on-startup.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub split_ratio: Option<f64>,
}

fn default_theme() -> String {
    DEFAULT_THEME.to_string()
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            theme: DEFAULT_THEME.to_string(),
            agent: None,
            split_ratio: None,
        }
    }
}

impl Preferences {
    /// Path to preferences file: ~/.config/legal-ko/preferences.json
    fn path() -> Result<PathBuf> {
        Ok(crate::config::config_dir()?.join("preferences.json"))
    }

    /// Load preferences from disk, falling back to defaults on any error.
    #[must_use]
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(p) => p,
            Err(e) => {
                warn!(error = %e, "Failed to load preferences");
                Self::default()
            }
        }
    }

    fn try_load() -> Result<Self> {
        let path = Self::path()?;
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "No preferences file found");
                return Ok(Self::default());
            }
            Err(e) => {
                return Err(
                    anyhow::anyhow!(e).context(format!("Failed to read {}", path.display()))
                );
            }
        };
        match serde_json::from_str::<Self>(&content) {
            Ok(prefs) => {
                debug!(theme = %prefs.theme, "Loaded preferences");
                Ok(prefs)
            }
            Err(e) => {
                let bak = path.with_extension("json.bak");
                warn!(
                    backup = %bak.display(),
                    error = %e,
                    "Corrupt preferences.json, renaming",
                );
                let _ = std::fs::rename(&path, &bak);
                Ok(Self::default())
            }
        }
    }

    /// Save preferences to disk.
    ///
    /// # Errors
    ///
    /// Returns an error if the config directory cannot be created, serialization
    /// fails, or the file cannot be written.
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize preferences")?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("Failed to rename {}", path.display()))?;
        debug!(path = %path.display(), "Saved preferences");
        Ok(())
    }
}
