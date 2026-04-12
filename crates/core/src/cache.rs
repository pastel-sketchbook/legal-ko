use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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

// ── Enrichment metadata cache ────────────────────────────────

/// TTL for the enrichment cache file (7 days).
const ENRICHMENT_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Cached metadata fields extracted from YAML frontmatter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnrichedMeta {
    pub category: String,
    pub departments: Vec<String>,
    pub promulgation_date: String,
    pub enforcement_date: String,
    pub status: String,
}

/// The full enrichment cache: law ID → extracted metadata.
pub type EnrichmentCache = HashMap<String, EnrichedMeta>;

/// Path to the enrichment cache file.
fn enrichment_cache_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("enriched.json"))
}

/// Read the enrichment cache from disk.
///
/// Returns an empty map if the file doesn't exist or has expired.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn read_enrichment_cache() -> Result<EnrichmentCache> {
    let path = enrichment_cache_path()?;
    if !path.exists() {
        debug!("Enrichment cache not found");
        return Ok(HashMap::new());
    }

    // Check TTL
    if let Ok(metadata) = path.metadata()
        && let Ok(modified) = metadata.modified()
        && let Ok(age) = modified.elapsed()
        && age > ENRICHMENT_TTL
    {
        debug!(age_secs = age.as_secs(), "Enrichment cache expired");
        return Ok(HashMap::new());
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read enrichment cache {}", path.display()))?;
    let cache: EnrichmentCache =
        serde_json::from_str(&content).with_context(|| "Failed to parse enrichment cache JSON")?;
    debug!(entries = cache.len(), "Loaded enrichment cache");
    Ok(cache)
}

/// Write the enrichment cache to disk (atomic rename).
///
/// # Errors
///
/// Returns an error if the cache directory cannot be created or the file
/// cannot be written.
pub fn write_enrichment_cache(cache: &EnrichmentCache) -> Result<()> {
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;

    let path = enrichment_cache_path()?;
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string(cache).context("Failed to serialize enrichment cache")?;
    std::fs::write(&tmp, json)
        .with_context(|| format!("Failed to write temp enrichment cache {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("Failed to rename enrichment cache {}", path.display()))?;

    debug!(entries = cache.len(), "Wrote enrichment cache");
    Ok(())
}

// ── Precedent metadata cache ─────────────────────────────────

use crate::models::PrecedentMetadataIndex;

/// TTL for the precedent metadata cache (24 hours).
const PRECEDENT_META_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// Path to the precedent metadata cache file.
fn precedent_meta_cache_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("precedent_meta.json"))
}

/// Read the precedent metadata index from disk cache.
///
/// Returns `None` if the file doesn't exist or has expired.
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
pub fn read_precedent_meta_cache() -> Result<Option<PrecedentMetadataIndex>> {
    let path = precedent_meta_cache_path()?;
    if !path.exists() {
        debug!("Precedent metadata cache not found");
        return Ok(None);
    }

    // Check TTL
    if let Ok(metadata) = path.metadata()
        && let Ok(modified) = metadata.modified()
        && let Ok(age) = modified.elapsed()
        && age > PRECEDENT_META_TTL
    {
        debug!(age_secs = age.as_secs(), "Precedent metadata cache expired");
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read precedent meta cache {}", path.display()))?;
    let index: PrecedentMetadataIndex = serde_json::from_str(&content)
        .with_context(|| "Failed to parse precedent meta cache JSON")?;
    debug!(entries = index.len(), "Loaded precedent metadata cache");
    Ok(Some(index))
}

/// Write the precedent metadata index to disk cache (atomic rename).
///
/// # Errors
///
/// Returns an error if the cache directory cannot be created or the file
/// cannot be written.
pub fn write_precedent_meta_cache(index: &PrecedentMetadataIndex) -> Result<()> {
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;

    let path = precedent_meta_cache_path()?;
    let tmp = path.with_extension("tmp");
    let json =
        serde_json::to_string(index).context("Failed to serialize precedent metadata cache")?;
    std::fs::write(&tmp, &json).with_context(|| {
        format!(
            "Failed to write temp precedent meta cache {}",
            tmp.display()
        )
    })?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("Failed to rename precedent meta cache {}", path.display()))?;

    debug!(entries = index.len(), "Wrote precedent metadata cache");
    Ok(())
}
