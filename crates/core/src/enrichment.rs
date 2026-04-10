//! Batch enrichment of law entry metadata from YAML frontmatter.
//!
//! On startup, the metadata index (built from the GitHub tree API) only
//! has titles and categories inferred from file paths.  Departments, dates,
//! and accurate status values live in each file's YAML frontmatter.
//!
//! This module:
//! 1. Loads a persistent cache (`~/.cache/legal-ko/enriched.json`).
//! 2. Applies cached metadata to entries immediately.
//! 3. Fetches frontmatter (first 1 KB) for un-cached entries in concurrent
//!    batches and parses it.
//! 4. Saves the updated cache to disk.
//!
//! The TUI uses this via streaming: it receives batches of enriched entries
//! progressively and re-sorts/re-filters as they arrive.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Semaphore;
use tracing::{debug, info, warn};

use crate::cache::{self, EnrichedMeta, EnrichmentCache};
use crate::client;
use crate::models::LawEntry;
use crate::parser;

/// Maximum concurrent HTTP requests for frontmatter fetches.
const CONCURRENCY: usize = 50;

/// Apply cached enrichment data to entries in-place.
///
/// Returns the number of entries that were enriched.
#[must_use]
pub fn apply_cache(entries: &mut [LawEntry], cache: &EnrichmentCache) -> usize {
    let mut count = 0;
    for entry in entries.iter_mut() {
        if let Some(meta) = cache.get(&entry.id) {
            if !meta.category.is_empty() {
                entry.category = meta.category.clone();
            }
            if !meta.departments.is_empty() {
                entry.departments.clone_from(&meta.departments);
            }
            if !meta.promulgation_date.is_empty() {
                entry.promulgation_date.clone_from(&meta.promulgation_date);
            }
            if !meta.enforcement_date.is_empty() {
                entry.enforcement_date.clone_from(&meta.enforcement_date);
            }
            if !meta.status.is_empty() {
                entry.status.clone_from(&meta.status);
            }
            count += 1;
        }
    }
    count
}

/// An enriched entry ready to be applied to the master list.
#[derive(Debug, Clone)]
pub struct EnrichedEntry {
    pub id: String,
    pub meta: EnrichedMeta,
}

/// Fetch frontmatter for a batch of entries, parse it, and return enrichment data.
///
/// Entries that already have a cache hit are skipped.  Failed fetches are
/// silently skipped (best-effort).
///
/// The `on_batch` callback is invoked after each sub-batch of `CONCURRENCY`
/// completes, allowing the caller to apply progressive updates.
///
/// Returns the full set of enriched entries (for cache persistence).
pub async fn fetch_and_enrich(
    client: &reqwest::Client,
    entries: &[LawEntry],
    existing_cache: EnrichmentCache,
    mut on_batch: impl FnMut(Vec<EnrichedEntry>),
) -> EnrichmentCache {
    // Find entries that need enrichment
    let needs_fetch: Vec<&LawEntry> = entries
        .iter()
        .filter(|e| !existing_cache.contains_key(&e.id))
        .collect();

    info!(
        total = entries.len(),
        cached = existing_cache.len(),
        to_fetch = needs_fetch.len(),
        "Starting batch enrichment"
    );

    // Start with existing cache (taken by value, no clone needed)
    let mut cache = existing_cache;

    if needs_fetch.is_empty() {
        return cache;
    }

    let semaphore = Arc::new(Semaphore::new(CONCURRENCY));

    // Process in chunks to deliver progressive updates
    for chunk in needs_fetch.chunks(CONCURRENCY) {
        let mut handles = Vec::with_capacity(chunk.len());

        for entry in chunk {
            let sem = Arc::clone(&semaphore);
            let client = client.clone();
            let path = entry.path.clone();
            let id = entry.id.clone();

            handles.push(tokio::spawn(async move {
                let Ok(_permit) = sem.acquire().await else {
                    return None; // Semaphore closed — skip gracefully
                };
                match client::fetch_frontmatter(&client, &path).await {
                    Ok(raw) => {
                        let fm = parser::parse_frontmatter(&raw);
                        let meta = EnrichedMeta {
                            category: fm
                                .get("법령구분")
                                .map_or_else(String::new, |v| v.as_str().to_string()),
                            departments: fm
                                .get("소관부처")
                                .map_or_else(Vec::new, super::parser::FrontmatterValue::as_list),
                            promulgation_date: fm
                                .get("공포일자")
                                .map_or_else(String::new, |v| v.as_str().to_string()),
                            enforcement_date: fm
                                .get("시행일자")
                                .map_or_else(String::new, |v| v.as_str().to_string()),
                            status: fm
                                .get("상태")
                                .map_or_else(String::new, |v| v.as_str().to_string()),
                        };
                        Some(EnrichedEntry { id, meta })
                    }
                    Err(e) => {
                        debug!(path, error = %e, "Failed to fetch frontmatter");
                        None
                    }
                }
            }));
        }

        // Collect results for this batch
        let mut batch = Vec::new();
        for handle in handles {
            if let Ok(Some(enriched)) = handle.await {
                cache.insert(enriched.id.clone(), enriched.meta.clone());
                batch.push(enriched);
            }
        }

        if !batch.is_empty() {
            debug!(batch_size = batch.len(), "Enrichment batch ready");
            on_batch(batch);
        }
    }

    info!(total = cache.len(), "Batch enrichment complete");
    cache
}

/// Load the enrichment cache from disk.
///
/// Returns an empty map on any error (best-effort).
#[must_use]
pub fn load_cache() -> EnrichmentCache {
    cache::read_enrichment_cache().unwrap_or_else(|e| {
        warn!(error = %e, "Failed to read enrichment cache");
        HashMap::new()
    })
}

/// Save the enrichment cache to disk (best-effort).
pub fn save_cache(cache: &EnrichmentCache) {
    if let Err(e) = cache::write_enrichment_cache(cache) {
        warn!(error = %e, "Failed to write enrichment cache");
    }
}
