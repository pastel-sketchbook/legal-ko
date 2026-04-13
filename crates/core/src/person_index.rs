//! Persistent person (법조인) index for instant name lookups across precedents.
//!
//! On first search, we scan precedent documents concurrently (up to
//! `CONCURRENT_FETCHES` at a time) and build an in-memory index mapping
//! person names to the precedent IDs where they appear. The index is then
//! persisted to `~/.cache/legal-ko/person_index.json` so subsequent searches
//! are instant (~1ms for 123K entries).
//!
//! The index is rebuilt when the cache expires or when the number of known
//! precedents has grown significantly since the last build.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::models::{PersonRole, PrecedentEntry};
use crate::{client, parser};

/// Maximum number of concurrent HTTP fetches during index building.
const CONCURRENT_FETCHES: usize = 50;

/// TTL for the person index cache (7 days).
const PERSON_INDEX_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// A single person→precedent association stored in the index.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonIndexEntry {
    /// Precedent ID (e.g. "대법원/2023다12345")
    pub precedent_id: String,
    /// Role in this case
    pub role: PersonRole,
    /// Optional qualifier (e.g. "재판장", "주심")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub qualifier: Option<String>,
}

/// The full person index: name → list of precedent associations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonIndex {
    /// Number of precedent documents that were scanned to build this index.
    pub scanned_count: usize,
    /// Name → associations.
    pub entries: HashMap<String, Vec<PersonIndexEntry>>,
}

impl PersonIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scanned_count: 0,
            entries: HashMap::new(),
        }
    }

    /// Look up all precedent associations for a given person name.
    #[must_use]
    pub fn search(&self, name: &str, role: Option<&PersonRole>) -> Vec<&PersonIndexEntry> {
        self.entries
            .get(name)
            .map(|entries| {
                entries
                    .iter()
                    .filter(|e| role.is_none() || Some(&e.role) == role)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Whether this index is stale relative to the current number of precedents.
    #[must_use]
    pub fn is_stale(&self, current_count: usize) -> bool {
        if self.scanned_count == 0 {
            return true;
        }
        // Integer arithmetic: stale if current > scanned * 1.05
        // Equivalent to: current * 100 > scanned * 105
        current_count * 100 > self.scanned_count * 105
    }
}

impl Default for PersonIndex {
    fn default() -> Self {
        Self::new()
    }
}

// ── Cache I/O ─────────────────────────────────────────────────

fn cache_dir() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("Cannot determine cache directory")?
        .join("legal-ko");
    Ok(dir)
}

fn person_index_path() -> Result<PathBuf> {
    Ok(cache_dir()?.join("person_index.json"))
}

/// Read the person index from disk cache.
///
/// Returns `None` if the file doesn't exist or has expired.
///
/// # Errors
///
/// Returns an error if the cache file exists but cannot be read or parsed.
pub fn read_person_index() -> Result<Option<PersonIndex>> {
    let path = person_index_path()?;
    if !path.exists() {
        debug!("Person index cache not found");
        return Ok(None);
    }

    if let Ok(metadata) = path.metadata()
        && let Ok(modified) = metadata.modified()
        && let Ok(age) = modified.elapsed()
        && age > PERSON_INDEX_TTL
    {
        debug!(age_secs = age.as_secs(), "Person index cache expired");
        return Ok(None);
    }

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read person index {}", path.display()))?;
    let index: PersonIndex =
        serde_json::from_str(&content).with_context(|| "Failed to parse person index JSON")?;
    debug!(
        entries = index.entries.len(),
        scanned = index.scanned_count,
        "Loaded person index from cache"
    );
    Ok(Some(index))
}

/// Write the person index to disk cache (atomic rename).
///
/// # Errors
///
/// Returns an error if the cache directory cannot be created or the file
/// cannot be written.
pub fn write_person_index(index: &PersonIndex) -> Result<()> {
    let dir = cache_dir()?;
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create cache dir {}", dir.display()))?;

    let path = person_index_path()?;
    let tmp = path.with_extension("tmp");
    let json = serde_json::to_string(index).context("Failed to serialize person index")?;
    std::fs::write(&tmp, json)
        .with_context(|| format!("Failed to write temp person index {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("Failed to rename person index {}", path.display()))?;

    info!(
        entries = index.entries.len(),
        scanned = index.scanned_count,
        "Wrote person index to cache"
    );
    Ok(())
}

// ── Index building (concurrent) ───────────────────────────────

/// Result of scanning a single precedent document.
struct ScanResult {
    precedent_id: String,
    persons: Vec<crate::models::PersonRef>,
}

/// Build a person index by scanning precedent documents concurrently.
///
/// Fetches documents using `buffer_unordered(CONCURRENT_FETCHES)` for ~50x
/// throughput vs sequential. Calls `on_progress(scanned, total)` periodically.
///
/// The returned index contains all person→precedent associations found.
pub async fn build_person_index<F>(
    http: &reqwest::Client,
    entries: &[PrecedentEntry],
    mut on_progress: F,
) -> PersonIndex
where
    F: FnMut(usize, usize),
{
    let total = entries.len();
    info!(total, "Building person index with concurrent fetches");

    // Collect futures eagerly so the iterator doesn't borrow `entries`
    // (all needed data is cloned into each future).
    let scan_futures: Vec<_> = entries
        .iter()
        .map(|entry| {
            let http = http.clone();
            let id = entry.id.clone();
            let path = entry.path.clone();
            async move {
                let persons = match client::load_precedent_content(&http, &path).await {
                    Ok(content) => parser::extract_persons(&content),
                    Err(_) => Vec::new(),
                };
                ScanResult {
                    precedent_id: id,
                    persons,
                }
            }
        })
        .collect();

    let mut index = PersonIndex {
        scanned_count: 0,
        entries: HashMap::new(),
    };

    let mut stream = stream::iter(scan_futures).buffer_unordered(CONCURRENT_FETCHES);
    let mut scanned = 0usize;
    let progress_interval = (total / 100).max(1);

    while let Some(result) = stream.next().await {
        scanned += 1;
        for person in &result.persons {
            index
                .entries
                .entry(person.name.clone())
                .or_default()
                .push(PersonIndexEntry {
                    precedent_id: result.precedent_id.clone(),
                    role: person.role.clone(),
                    qualifier: person.qualifier.clone(),
                });
        }
        if scanned.is_multiple_of(progress_interval) || scanned == total {
            on_progress(scanned, total);
        }
    }

    index.scanned_count = scanned;
    info!(
        scanned,
        unique_names = index.entries.len(),
        "Person index build complete"
    );

    index
}

/// Search for a person name across precedent entries, using the cached index
/// if available. If the index is missing or stale, builds it concurrently
/// first, caches it, then searches.
///
/// Returns matching `PrecedentEntry` values with their roles.
pub async fn search_persons<F>(
    http: &reqwest::Client,
    name: &str,
    role: Option<&PersonRole>,
    all_entries: &[PrecedentEntry],
    on_progress: F,
) -> Vec<PersonSearchResult>
where
    F: FnMut(usize, usize),
{
    let index = get_or_build_index(http, all_entries, on_progress).await;

    // Look up matches in the index
    let hits = index.search(name, role);

    // Map hits back to PrecedentEntry values
    let entry_map: HashMap<&str, &PrecedentEntry> =
        all_entries.iter().map(|e| (e.id.as_str(), e)).collect();

    hits.iter()
        .filter_map(|hit| {
            entry_map
                .get(hit.precedent_id.as_str())
                .map(|&entry| PersonSearchResult {
                    entry: entry.clone(),
                    role: hit.role.clone(),
                    qualifier: hit.qualifier.clone(),
                })
        })
        .collect()
}

/// A search result with the matched entry and the role/qualifier that matched.
#[derive(Debug, Clone)]
pub struct PersonSearchResult {
    pub entry: PrecedentEntry,
    pub role: PersonRole,
    pub qualifier: Option<String>,
}

/// Load the cached person index, or build it if missing/stale.
async fn get_or_build_index<F>(
    http: &reqwest::Client,
    all_entries: &[PrecedentEntry],
    on_progress: F,
) -> PersonIndex
where
    F: FnMut(usize, usize),
{
    // Try loading from cache (blocking I/O)
    let cached = tokio::task::spawn_blocking(read_person_index)
        .await
        .unwrap_or_else(|_| Ok(None));

    if let Ok(Some(index)) = cached {
        if !index.is_stale(all_entries.len()) {
            info!(
                scanned = index.scanned_count,
                names = index.entries.len(),
                "Using cached person index"
            );
            return index;
        }
        info!(
            cached = index.scanned_count,
            current = all_entries.len(),
            "Person index stale, rebuilding"
        );
    }

    // Build fresh index
    let index = build_person_index(http, all_entries, on_progress).await;

    // Save to disk in background
    let index_for_cache = index.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = write_person_index(&index_for_cache) {
            warn!(error = %e, "Failed to write person index cache");
        }
    });

    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_person_index_search() {
        let mut index = PersonIndex::new();
        index.scanned_count = 2;
        index.entries.insert(
            "김철수".to_string(),
            vec![
                PersonIndexEntry {
                    precedent_id: "A".to_string(),
                    role: PersonRole::Judge,
                    qualifier: Some("재판장".to_string()),
                },
                PersonIndexEntry {
                    precedent_id: "B".to_string(),
                    role: PersonRole::Attorney,
                    qualifier: None,
                },
            ],
        );

        // No role filter
        let results = index.search("김철수", None);
        assert_eq!(results.len(), 2);

        // Filter by judge
        let results = index.search("김철수", Some(&PersonRole::Judge));
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].precedent_id, "A");

        // Non-existent name
        let results = index.search("박영희", None);
        assert!(results.is_empty());
    }

    #[test]
    fn test_person_index_staleness() {
        let mut index = PersonIndex::new();
        assert!(index.is_stale(100)); // empty is always stale

        index.scanned_count = 100;
        assert!(!index.is_stale(100)); // same count → not stale
        assert!(!index.is_stale(104)); // < 5% growth → not stale
        assert!(index.is_stale(106)); // > 5% growth → stale
    }
}
