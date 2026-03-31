use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::{debug, warn};

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Bookmarks {
    /// Set of bookmarked law IDs (법령MST)
    pub ids: HashSet<String>,
}

impl Bookmarks {
    /// Path to bookmarks file: ~/.config/legal-ko/bookmarks.json
    fn path() -> Result<PathBuf> {
        let dir = dirs::config_dir()
            .context("Cannot determine config directory")?
            .join("legal-ko");
        Ok(dir.join("bookmarks.json"))
    }

    /// Load bookmarks from disk. Returns empty set if file doesn't exist.
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(b) => b,
            Err(e) => {
                warn!("Failed to load bookmarks: {e}");
                Self::default()
            }
        }
    }

    fn try_load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            debug!("No bookmarks file at {}", path.display());
            return Ok(Self::default());
        }
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        let bookmarks: Self =
            serde_json::from_str(&content).context("Failed to parse bookmarks.json")?;
        debug!("Loaded {} bookmarks", bookmarks.ids.len());
        Ok(bookmarks)
    }

    /// Save bookmarks to disk
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let content =
            serde_json::to_string_pretty(self).context("Failed to serialize bookmarks")?;
        std::fs::write(&path, content)
            .with_context(|| format!("Failed to write {}", path.display()))?;
        debug!("Saved {} bookmarks to {}", self.ids.len(), path.display());
        Ok(())
    }

    /// Toggle a bookmark. Returns true if now bookmarked, false if removed.
    pub fn toggle(&mut self, id: &str) -> bool {
        if self.ids.contains(id) {
            self.ids.remove(id);
            false
        } else {
            self.ids.insert(id.to_string());
            true
        }
    }

    /// Check if a law is bookmarked
    pub fn is_bookmarked(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}
