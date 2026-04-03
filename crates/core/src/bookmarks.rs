use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;
use tracing::{debug, warn};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Bookmarks {
    /// Set of bookmarked law IDs (path-derived, e.g. "kr/민법/법률")
    #[serde(default)]
    pub ids: HashSet<String>,
}

impl Bookmarks {
    /// Path to bookmarks file: ~/.config/legal-ko/bookmarks.json
    fn path() -> Result<PathBuf> {
        Ok(crate::config::config_dir()?.join("bookmarks.json"))
    }

    /// Load bookmarks from disk. Returns empty set if file doesn't exist.
    pub fn load() -> Self {
        match Self::try_load() {
            Ok(b) => b,
            Err(e) => {
                warn!(error = %e, "Failed to load bookmarks");
                Self::default()
            }
        }
    }

    fn try_load() -> Result<Self> {
        let path = Self::path()?;
        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(path = %path.display(), "No bookmarks file found");
                return Ok(Self::default());
            }
            Err(e) => {
                return Err(
                    anyhow::anyhow!(e).context(format!("Failed to read {}", path.display()))
                );
            }
        };
        match serde_json::from_str::<Self>(&content) {
            Ok(bookmarks) => {
                debug!(count = bookmarks.ids.len(), "Loaded bookmarks");
                Ok(bookmarks)
            }
            Err(e) => {
                // Rename corrupt file so it's not silently overwritten on next save
                let bak = path.with_extension("json.bak");
                warn!(backup = %bak.display(), error = %e, "Corrupt bookmarks.json, renaming");
                let _ = std::fs::rename(&path, &bak);
                Ok(Self::default())
            }
        }
    }

    /// Save bookmarks to disk.
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
            serde_json::to_string_pretty(self).context("Failed to serialize bookmarks")?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, &content)
            .with_context(|| format!("Failed to write {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("Failed to rename {}", path.display()))?;
        debug!(count = self.ids.len(), path = %path.display(), "Saved bookmarks");
        Ok(())
    }

    /// Toggle a bookmark. Returns true if now bookmarked, false if removed.
    #[must_use]
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
    #[must_use]
    pub fn is_bookmarked(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
}
