use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::path::PathBuf;
use std::time::Duration;
use tracing::debug;

const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Get the cache directory: ~/.cache/legal-ko/
fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("Cannot determine cache directory")?
        .join("legal-ko");
    Ok(dir)
}

/// Generate a cache key from a file path
fn cache_key(path: &str) -> String {
    let hash = Sha256::digest(path.as_bytes());
    let mut s = String::with_capacity(64);
    for b in &hash {
        let _ = write!(s, "{b:02x}");
    }
    s
}

/// Try to read cached content for a given path.
///
/// # Errors
///
/// Returns an error if the cache file exists but cannot be read.
pub fn read_cache(path: &str) -> Result<Option<String>> {
    let file = cache_dir()?.join(cache_key(path));
    if file.exists() {
        // Check TTL
        if let Ok(metadata) = file.metadata()
            && let Ok(modified) = metadata.modified()
            && let Ok(age) = modified.elapsed()
            && age > CACHE_TTL
        {
            debug!(path, age_secs = age.as_secs(), "Cache expired");
            return Ok(None);
        }

        debug!(path, "Cache hit");
        let content = std::fs::read_to_string(&file)
            .with_context(|| format!("Failed to read cache file {}", file.display()))?;
        Ok(Some(content))
    } else {
        debug!(path, "Cache miss");
        Ok(None)
    }
}

/// Write content to cache for a given path.
///
/// # Errors
///
/// Returns an error if the cache directory cannot be created or the file
/// cannot be written.
pub fn write_cache(path: &str, content: &str) -> Result<()> {
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;

    let file = dir.join(cache_key(path));
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, content)
        .with_context(|| format!("Failed to write temp cache file {}", tmp.display()))?;
    std::fs::rename(&tmp, &file)
        .with_context(|| format!("Failed to rename cache file {}", file.display()))?;

    debug!(path, "Cached content");
    Ok(())
}
