use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fmt::Write;
use std::path::PathBuf;
use tracing::debug;

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
    hash.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

/// Try to read cached content for a given path.
///
/// # Errors
///
/// Returns an error if the cache file exists but cannot be read.
pub fn read_cache(path: &str) -> Result<Option<String>> {
    let file = cache_dir()?.join(cache_key(path));
    if file.exists() {
        debug!("Cache hit for {path}");
        let content = std::fs::read_to_string(&file)
            .with_context(|| format!("Failed to read cache file {}", file.display()))?;
        Ok(Some(content))
    } else {
        debug!("Cache miss for {path}");
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
    std::fs::write(&file, content)
        .with_context(|| format!("Failed to write cache file {}", file.display()))?;

    debug!("Cached content for {path}");
    Ok(())
}
