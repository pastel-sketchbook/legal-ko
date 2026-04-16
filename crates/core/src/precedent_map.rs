use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::crossref;

/// Persistent map from law names (and articles) to citing precedent paths.
///
/// Built once from `.qmd/data.db` by scanning all precedent docs'
/// `## 참조조문` sections. Cached to `~/.cache/legal-ko/precedent_map.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecedentMap {
    /// Number of precedent documents that were scanned to produce this map.
    pub scanned_count: usize,
    /// law_title → set of precedent paths (e.g. "민사/대법원/2000다10048")
    pub law_to_precedents: HashMap<String, Vec<String>>,
    /// (law_title, article) → set of precedent paths
    /// The key format is "law_title\0article" (e.g. "민법\0제840조")
    pub article_to_precedents: HashMap<String, Vec<String>>,
}

impl PrecedentMap {
    /// Build the map by scanning all precedent documents in `.qmd/data.db`.
    ///
    /// `known_law_names` is the list of law titles from the metadata index
    /// (used by `crossref::match_statute_refs`).
    pub fn build(db_path: &Path, known_law_names: &[String]) -> Result<Self> {
        let conn = Connection::open(db_path)
            .with_context(|| format!("Failed to open {}", db_path.display()))?;

        let mut stmt = conn.prepare(
            "SELECT d.path, c.doc FROM documents d
             JOIN content c ON d.hash = c.hash
             WHERE d.collection = 'precedents' AND d.active = 1",
        )?;

        let mut law_map: HashMap<String, HashSet<String>> = HashMap::new();
        let mut article_map: HashMap<String, HashSet<String>> = HashMap::new();
        let mut scanned = 0usize;

        let rows = stmt.query_map([], |row| {
            let path: String = row.get(0)?;
            let doc: String = row.get(1)?;
            Ok((path, doc))
        })?;

        for row in rows {
            let (path, doc) = row?;
            let precedent_id = path.strip_suffix(".md").unwrap_or(&path).to_string();

            let statute_refs = crossref::extract_statute_refs(&doc);
            if !statute_refs.is_empty() {
                let matches = crossref::match_statute_refs(&statute_refs, known_law_names);
                for m in &matches {
                    law_map
                        .entry(m.statute_ref.law_name.clone())
                        .or_default()
                        .insert(precedent_id.clone());

                    let article_key =
                        format!("{}\0{}", m.statute_ref.law_name, m.statute_ref.article);
                    article_map
                        .entry(article_key)
                        .or_default()
                        .insert(precedent_id.clone());
                }
            }
            scanned += 1;
            if scanned.is_multiple_of(10_000) {
                debug!(scanned, "Precedent map build progress");
            }
        }

        info!(
            scanned,
            laws = law_map.len(),
            articles = article_map.len(),
            "Precedent map built"
        );

        let law_to_precedents = law_map
            .into_iter()
            .map(|(k, v)| {
                let mut vec: Vec<String> = v.into_iter().collect();
                vec.sort();
                (k, vec)
            })
            .collect();

        let article_to_precedents = article_map
            .into_iter()
            .map(|(k, v)| {
                let mut vec: Vec<String> = v.into_iter().collect();
                vec.sort();
                (k, vec)
            })
            .collect();

        Ok(Self {
            scanned_count: scanned,
            law_to_precedents,
            article_to_precedents,
        })
    }

    /// Get the number of precedents citing a given law.
    #[must_use]
    pub fn law_count(&self, law_title: &str) -> usize {
        self.law_to_precedents.get(law_title).map_or(0, Vec::len)
    }

    /// Get the number of precedents citing a specific article of a law.
    #[must_use]
    pub fn article_count(&self, law_title: &str, article: &str) -> usize {
        let key = format!("{law_title}\0{article}");
        self.article_to_precedents.get(&key).map_or(0, Vec::len)
    }

    /// Get the precedent IDs (paths without .md) citing a given law.
    #[must_use]
    pub fn law_precedents(&self, law_title: &str) -> &[String] {
        self.law_to_precedents
            .get(law_title)
            .map_or(&[], Vec::as_slice)
    }

    /// Get the precedent IDs citing a specific article.
    #[must_use]
    pub fn article_precedents(&self, law_title: &str, article: &str) -> &[String] {
        let key = format!("{law_title}\0{article}");
        self.article_to_precedents
            .get(&key)
            .map_or(&[], Vec::as_slice)
    }

    /// Make the article key from law_title + article label (for lookup).
    #[must_use]
    pub fn article_key(law_title: &str, article: &str) -> String {
        format!("{law_title}\0{article}")
    }
}

/// Build minimal `PrecedentEntry` records from the `.qmd/data.db` documents table.
///
/// Used as a fallback when precedent metadata from GitHub is unavailable.
/// Derives `case_type`, `court_name`, and `case_number` from the path pattern
/// `{case_type}/{court}/{case_number}.md`.
pub fn entries_from_db(db_path: &Path) -> Result<Vec<crate::models::PrecedentEntry>> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open {}", db_path.display()))?;

    let mut stmt = conn.prepare(
        "SELECT path, title FROM documents WHERE collection = 'precedents' AND active = 1",
    )?;

    let entries: Vec<crate::models::PrecedentEntry> = stmt
        .query_map([], |row| {
            let path: String = row.get(0)?;
            let case_name: String = row.get(1)?;
            Ok((path, case_name))
        })?
        .filter_map(|r| r.ok())
        .map(|(path, case_name)| {
            let id = path.strip_suffix(".md").unwrap_or(&path).to_string();
            // Parse path: "민사/대법원/2000다10048.md"
            let parts: Vec<&str> = id.split('/').collect();
            let (case_type, court_name, case_number) = if parts.len() == 3 {
                (
                    parts[0].to_string(),
                    parts[1].to_string(),
                    parts[2].to_string(),
                )
            } else {
                (String::new(), String::new(), id.clone())
            };
            crate::models::PrecedentEntry {
                id,
                case_name,
                case_number,
                ruling_date: String::new(),
                court_name,
                case_type,
                ruling_type: String::new(),
                path,
            }
        })
        .collect();

    info!(
        count = entries.len(),
        "Built precedent entries from data.db"
    );
    Ok(entries)
}

/// Query the number of active precedent documents in `.qmd/data.db`.
///
/// Used to decide whether the cached `PrecedentMap` is still valid.
pub fn db_precedent_count(db_path: &Path) -> Result<usize> {
    let conn = Connection::open(db_path)
        .with_context(|| format!("Failed to open {}", db_path.display()))?;
    let count: i64 = conn.query_row(
        "SELECT COUNT(*) FROM documents WHERE collection = 'precedents' AND active = 1",
        [],
        |row| row.get(0),
    )?;
    let count = count as usize;
    Ok(count)
}
