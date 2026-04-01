use anyhow::Result;
use tracing::debug;

use crate::models::LawEntry;

/// Fallback substring search on law titles. Returns matching IDs in input order.
#[must_use]
pub fn naive_search_ids(entries: &[LawEntry], query: &str, limit: usize) -> Vec<String> {
    let query_lower = query.to_lowercase();
    entries
        .iter()
        .filter(|e| e.title.to_lowercase().contains(&query_lower))
        .take(limit)
        .map(|e| e.id.clone())
        .collect()
}

/// Search facade. Wraps an optional Meilisearch backend.
pub struct Searcher {
    #[cfg(feature = "meilisearch")]
    backend: Option<MeiliBackend>,
    #[cfg(not(feature = "meilisearch"))]
    _priv: (),
}

impl Searcher {
    /// Create a `Searcher` from environment variables.
    ///
    /// Returns a disabled searcher if `LEGAL_KO_MEILI_URL` is not set or the
    /// `meilisearch` feature is not enabled.
    pub fn from_env() -> Self {
        #[cfg(feature = "meilisearch")]
        {
            match MeiliBackend::from_env() {
                Some(backend) => {
                    tracing::info!(url = %backend.url, index = %backend.index_uid, "Meilisearch backend configured");
                    Self {
                        backend: Some(backend),
                    }
                }
                None => {
                    debug!("LEGAL_KO_MEILI_URL not set; Meilisearch disabled");
                    Self { backend: None }
                }
            }
        }
        #[cfg(not(feature = "meilisearch"))]
        {
            debug!("meilisearch feature not enabled");
            Self { _priv: () }
        }
    }

    /// Returns `true` if a Meilisearch backend is configured.
    #[must_use]
    pub fn is_enabled(&self) -> bool {
        #[cfg(feature = "meilisearch")]
        {
            self.backend.is_some()
        }
        #[cfg(not(feature = "meilisearch"))]
        {
            false
        }
    }

    /// Index (or re-index) law entries into Meilisearch.
    ///
    /// No-op if the backend is disabled. Safe to call on every startup — it
    /// replaces documents in-place using the primary key.
    ///
    /// # Errors
    ///
    /// Returns an error if the Meilisearch backend is configured but the
    /// indexing request fails.
    #[allow(clippy::unused_async)]
    pub async fn warmup(&self, entries: &[LawEntry]) -> Result<()> {
        #[cfg(feature = "meilisearch")]
        {
            if let Some(ref backend) = self.backend {
                return backend.warmup(entries).await;
            }
        }
        let _ = entries;
        Ok(())
    }

    /// Search for law entries by query. Returns ranked law IDs.
    ///
    /// Returns `Err` only if the Meilisearch backend is configured but the
    /// request fails. Returns `Ok(vec![])` if the backend is disabled.
    ///
    /// # Errors
    ///
    /// Returns an error if the Meilisearch search request fails.
    #[allow(clippy::unused_async)]
    pub async fn search_ids(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        #[cfg(feature = "meilisearch")]
        {
            if let Some(ref backend) = self.backend {
                return backend.search_ids(query, limit).await;
            }
        }
        let _ = (query, limit);
        Ok(vec![])
    }
}

// ── Meilisearch backend (feature-gated) ──────────────────────

#[cfg(feature = "meilisearch")]
struct MeiliBackend {
    client: meilisearch_sdk::client::Client,
    url: String,
    index_uid: String,
}

#[cfg(feature = "meilisearch")]
impl MeiliBackend {
    /// Environment variable names.
    const ENV_URL: &str = "LEGAL_KO_MEILI_URL";
    const ENV_KEY: &str = "LEGAL_KO_MEILI_KEY";
    const ENV_INDEX: &str = "LEGAL_KO_MEILI_INDEX";
    const DEFAULT_INDEX: &str = "legal_ko_laws";

    fn from_env() -> Option<Self> {
        let url = std::env::var(Self::ENV_URL).ok()?;
        let api_key = std::env::var(Self::ENV_KEY).ok();
        let index_uid =
            std::env::var(Self::ENV_INDEX).unwrap_or_else(|_| Self::DEFAULT_INDEX.to_string());

        let client = meilisearch_sdk::client::Client::new(&url, api_key.as_deref()).ok()?;

        Some(Self {
            client,
            url,
            index_uid,
        })
    }

    async fn warmup(&self, entries: &[LawEntry]) -> Result<()> {
        tracing::info!(
            index = %self.index_uid,
            count = entries.len(),
            "Indexing law entries into Meilisearch"
        );

        let index = self.client.index(&self.index_uid);

        // Configure searchable and filterable attributes
        let settings = meilisearch_sdk::settings::Settings::new()
            .with_searchable_attributes(["title", "category", "departments"])
            .with_filterable_attributes(["category", "departments", "status"]);

        index
            .set_settings(&settings)
            .await?
            .wait_for_completion(&self.client, None, None)
            .await?;

        // Add/replace all documents (primary key = "id")
        index
            .add_or_replace(entries, Some("id"))
            .await?
            .wait_for_completion(&self.client, None, None)
            .await?;

        tracing::info!(
            index = %self.index_uid,
            "Meilisearch indexing complete ({} documents)",
            entries.len()
        );
        Ok(())
    }

    async fn search_ids(&self, query: &str, limit: usize) -> Result<Vec<String>> {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct IdOnly {
            id: String,
        }

        let index = self.client.index(&self.index_uid);
        let results = index
            .search()
            .with_query(query)
            .with_limit(limit)
            .with_attributes_to_retrieve(meilisearch_sdk::search::Selectors::Some(&["id"]))
            .execute::<IdOnly>()
            .await?;

        let ids: Vec<String> = results.hits.into_iter().map(|h| h.result.id).collect();
        debug!(query, hits = ids.len(), "Meilisearch search completed");
        Ok(ids)
    }
}
