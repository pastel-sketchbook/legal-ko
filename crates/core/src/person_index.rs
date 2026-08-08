//! Persistent person (법조인) index for instant name lookups across precedents.
//!
//! On first search, we scan precedent documents concurrently (up to
//! `CONCURRENT_FETCHES` at a time) and build an in-memory index mapping
//! person names to the precedent IDs where they appear. The index is then
//! persisted to the legal-ko cache directory so subsequent searches are
//! instant (~1ms for 123K entries).
//!
//! The index is rebuilt only when the precedent-kr clone is pulled to a new
//! git commit (via `zmd precedents`/`zmd sync`); otherwise it is reused
//! indefinitely. Without git available, the number of known precedents is
//! used as a fallback staleness heuristic.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use futures::stream::{self, StreamExt};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, instrument, warn};

use crate::cache;
use crate::models::{PersonRole, PrecedentEntry, PrecedentSortOrder};
use crate::{client, parser};

/// Maximum number of concurrent HTTP fetches during index building.
const CONCURRENT_FETCHES: usize = 50;

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
    /// Git HEAD of the precedent-kr clone this index was built from. Used to
    /// detect staleness cheaply: the index stays valid while HEAD is unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head: Option<String>,
    /// Name → associations.
    pub entries: HashMap<String, Vec<PersonIndexEntry>>,
}

impl PersonIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> Self {
        Self {
            scanned_count: 0,
            head: None,
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

fn person_index_path() -> Result<PathBuf> {
    Ok(cache::cache_dir()?.join("person_index.json"))
}

/// Read the person index from disk cache.
///
/// Returns `None` if the file doesn't exist or cannot be parsed. Validity
/// (HEAD / staleness) is decided by the caller in [`get_or_build_index`].
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
    let dir = cache::cache_dir()?;
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

/// Insert persons from a scan result into the index.
fn index_scan_result(index: &mut PersonIndex, result: &ScanResult) {
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
}

/// Build a person index by reading precedent files from the local zmd clone.
///
/// Uses Rayon for parallel file I/O — typically completes in ~10s for 123K
/// files (vs ~30 min over HTTP). Calls `on_progress(scanned, total)` after
/// each Rayon chunk.
pub fn build_person_index_from_clone<F>(
    clone_dir: &Path,
    entries: &[PrecedentEntry],
    mut on_progress: F,
) -> PersonIndex
where
    F: FnMut(usize, usize),
{
    let total = entries.len();
    info!(total, path = %clone_dir.display(), "Building person index from local clone");

    // Scan all entries in parallel
    let results: Vec<ScanResult> = entries
        .par_iter()
        .filter_map(|entry| {
            let file_path = clone_dir.join(&entry.path);
            let content = std::fs::read_to_string(&file_path).ok()?;
            let persons = parser::extract_persons(&content);
            Some(ScanResult {
                precedent_id: entry.id.clone(),
                persons,
            })
        })
        .collect();

    let mut index = PersonIndex {
        scanned_count: 0,
        head: None,
        entries: HashMap::new(),
    };

    for (i, result) in results.iter().enumerate() {
        index_scan_result(&mut index, result);
        let progress_interval = (total / 100).max(1);
        if (i + 1).is_multiple_of(progress_interval) || i + 1 == total {
            on_progress(i + 1, total);
        }
    }

    index.scanned_count = results.len();
    info!(
        scanned = index.scanned_count,
        unique_names = index.entries.len(),
        "Person index build complete (local)"
    );

    index
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
        head: None,
        entries: HashMap::new(),
    };

    let mut stream = stream::iter(scan_futures).buffer_unordered(CONCURRENT_FETCHES);
    let mut scanned = 0usize;
    let progress_interval = (total / 100).max(1);

    while let Some(result) = stream.next().await {
        scanned += 1;
        index_scan_result(&mut index, &result);
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
#[instrument(skip(http, all_entries, on_progress))]
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

/// Sort person search results by the given order.
///
/// - `CaseName`: sort by case name, then case number.
/// - `RulingDate`: sort by ruling date descending (newest first), then case name.
pub fn sort_person_results(results: &mut [PersonSearchResult], order: PrecedentSortOrder) {
    match order {
        PrecedentSortOrder::CaseName => {
            results.sort_by(|a, b| {
                a.entry
                    .case_name
                    .cmp(&b.entry.case_name)
                    .then_with(|| a.entry.case_number.cmp(&b.entry.case_number))
            });
        }
        PrecedentSortOrder::RulingDate => {
            results.sort_by(|a, b| {
                let da = if a.entry.ruling_date.is_empty() {
                    ""
                } else {
                    &a.entry.ruling_date
                };
                let db = if b.entry.ruling_date.is_empty() {
                    ""
                } else {
                    &b.entry.ruling_date
                };
                db.cmp(da)
                    .then_with(|| a.entry.case_name.cmp(&b.entry.case_name))
            });
        }
        PrecedentSortOrder::RulingDateAsc => {
            results.sort_by(|a, b| {
                let da = if a.entry.ruling_date.is_empty() {
                    ""
                } else {
                    &a.entry.ruling_date
                };
                let db = if b.entry.ruling_date.is_empty() {
                    ""
                } else {
                    &b.entry.ruling_date
                };
                da.cmp(db)
                    .then_with(|| a.entry.case_name.cmp(&b.entry.case_name))
            });
        }
    }
}

/// Path to the zmd precedent-kr clone (same logic as client.rs).
fn zmd_precedent_clone_dir() -> Option<PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .ok()?;
    let path = PathBuf::from(home).join(".cache/legal-ko/zmd/repos/precedent-kr");
    if path.join(".git").is_dir() {
        Some(path)
    } else {
        None
    }
}

/// Whether a cached index is still fresh given the current clone state.
///
/// When the clone's git HEAD is known, freshness is decided solely by the HEAD
/// match (the index was built from that exact repo state). Without git we fall
/// back to the count-based staleness heuristic.
fn index_is_fresh(index: &PersonIndex, current_head: Option<&str>, current_count: usize) -> bool {
    match current_head {
        Some(h) => index.head.as_deref() == Some(h),
        None => !index.is_stale(current_count),
    }
}

/// Load the cached person index, or build it if missing/stale.
///
/// Cache validity is tied to the git HEAD of the precedent-kr clone: the
/// index is reused for as long as the clone hasn't been pulled to a new
/// commit, so a full rebuild only happens after `zmd precedents`/`zmd sync`.
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
        let clone_dir = zmd_precedent_clone_dir();
        let head = clone_dir.as_deref().and_then(client::git_head);
        if index_is_fresh(&index, head.as_deref(), all_entries.len()) {
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

    // Prefer building from local clone (Rayon, ~10s) over HTTP (~30 min)
    let index = if let Some(clone_dir) = zmd_precedent_clone_dir() {
        let entries = all_entries.to_vec();
        let head = client::git_head(&clone_dir);
        tokio::task::spawn_blocking(move || {
            let mut index = build_person_index_from_clone(&clone_dir, &entries, |_, _| {});
            index.head = head;
            index
        })
        .await
        .unwrap_or_else(|_| {
            warn!("Local clone scan panicked, falling back to HTTP");
            PersonIndex::new()
        })
    } else {
        build_person_index(http, all_entries, on_progress).await
    };

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

    #[test]
    fn test_index_freshness_with_head() {
        let mut index = PersonIndex::new();
        index.scanned_count = 100;
        index.head = Some("abc123".to_string());

        // Matching HEAD → fresh regardless of count growth or time.
        assert!(index_is_fresh(&index, Some("abc123"), 100));
        assert!(index_is_fresh(&index, Some("abc123"), 200));

        // HEAD changed → stale, rebuild to pick up the new clone state.
        assert!(!index_is_fresh(&index, Some("def456"), 100));

        // No current HEAD (git unavailable) → count-based heuristic.
        assert!(index_is_fresh(&index, None, 100));

        // No HEAD available anywhere → count-based heuristic.
        let mut no_head = PersonIndex::new();
        no_head.scanned_count = 100;
        assert!(index_is_fresh(&no_head, None, 104));
        assert!(!index_is_fresh(&no_head, None, 106));
    }
}
