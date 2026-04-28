//! Native zmd-compatible query path.
//!
//! Performs FTS5 + vec0 hybrid search directly against the
//! `.qmd/data.db` SQLite database (path from
//! [`crate::native_indexer::default_db_path`]), with the same
//! schema and scoring as the upstream `zmd` (zig-qmd) binary —
//! eliminating the per-query subprocess fork/exec cost and the
//! need to load llama.cpp for the default (non-rerank) flow.
//!
//! # Algorithm parity with zmd
//!
//! - **FTS**: `bm25(documents_fts, 1.5, 4.0, 1.0)`, ordered ascending,
//!   normalised to `|s| / (1 + |s|)` (zmd: `search.zig::searchFTS`).
//! - **Vector**: `vec0` `MATCH vec_f32(?) AND k = 200`, scored as
//!   `1 / (1 + distance)`, best chunk per document kept
//!   (zmd: `search.zig::searchVecNative`).
//! - **Fusion**: Reciprocal Rank Fusion with k=60
//!   (zmd: `search.zig::reciprocalRankFusion`).
//! - **Query embedding**: FNV-384 fallback prefixed with `"query: "`,
//!   matching the embeddings written by [`crate::native_indexer`].
//!
//! Reranking is intentionally not implemented here — the goal of this
//! module is the fast hybrid path used by 99% of queries.

use std::collections::HashMap;

use anyhow::{Context, Result};
use rusqlite::params;

use crate::crossref;
use crate::native_indexer::{ZmdDb, fnv_embed, format_query_for_embedding};

/// RRF constant matching zmd's `RRF_K`.
const RRF_K: f64 = 60.0;

/// FTS5 result limit (per zmd's `searchFTS` which uses `LIMIT 100`).
const FTS_LIMIT: usize = 100;

/// `vec0` kNN k constant (per zmd's `searchVecNative` which uses `k = 200`).
const VEC_K: usize = 200;

/// One scored search hit.
#[derive(Debug, Clone, serde::Serialize)]
pub struct QueryHit {
    pub id: i64,
    pub collection: String,
    pub path: String,
    pub title: String,
    pub hash: String,
    pub score: f64,
}

/// Options controlling a hybrid query.
#[derive(Debug, Clone)]
pub struct QueryOptions {
    /// Maximum results to return.
    pub limit: usize,
    /// Restrict to a single collection (e.g. `"laws"` or `"precedents"`).
    pub collection: Option<String>,
    /// When false, only run the FTS branch (fastest path, no embedding).
    pub enable_vector: bool,
}

impl Default for QueryOptions {
    fn default() -> Self {
        Self {
            limit: 20,
            collection: None,
            enable_vector: true,
        }
    }
}

// ── FTS5 query parser (mirrors zmd::buildFTS5Query) ──────────────────

/// Convert user input into an FTS5 query string handling negation
/// (`-foo`), prefix match (`foo*`), and hyphenated terms (quoted).
#[must_use]
pub fn build_fts5_query(input: &str) -> String {
    let mut out = String::new();
    let mut first = true;

    for raw in input.split(' ') {
        if raw.is_empty() {
            continue;
        }

        let mut token = raw;
        let mut is_negation = false;
        if let Some(rest) = token.strip_prefix('-') {
            is_negation = true;
            token = rest;
        }

        let mut prefix_match = false;
        if let Some(rest) = token.strip_suffix('*') {
            prefix_match = true;
            token = rest;
        }

        if token.is_empty() {
            continue;
        }

        if !first {
            out.push(' ');
        }
        first = false;

        if is_negation {
            out.push('-');
        }

        if token.contains('-') {
            out.push('"');
            out.push_str(token);
            out.push('"');
        } else {
            out.push_str(token);
        }

        if prefix_match {
            out.push('*');
        }
    }

    out
}

// ── FTS branch ───────────────────────────────────────────────────────

/// Run a BM25 full-text search.
///
/// # Errors
///
/// Returns an error if the prepared statement fails.
pub fn fts_search(db: &ZmdDb, query: &str, collection: Option<&str>) -> Result<Vec<QueryHit>> {
    let fts_query = build_fts5_query(query);
    if fts_query.is_empty() {
        return Ok(Vec::new());
    }

    // Always query without collection filter in SQL (fast path), then
    // post-filter in Rust. The alternative (AND d.collection = ?) causes
    // catastrophic performance on broad terms (e.g. "민법" matching 100K+ docs).
    let sql = "SELECT d.id, d.collection, d.path, d.title, d.hash, \
                      bm25(documents_fts, 1.5, 4.0, 1.0) as score \
               FROM documents_fts \
               JOIN documents d ON documents_fts.rowid = d.id \
               WHERE documents_fts MATCH ?1 AND score < 0 \
               ORDER BY score LIMIT ?2";

    // Fetch more when filtering by collection to ensure we get enough results.
    let fetch_limit = if collection.is_some() {
        FTS_LIMIT * 10
    } else {
        FTS_LIMIT
    };

    let mut stmt = db.conn().prepare(sql)?;
    let mut rows = stmt.query(params![fts_query, fetch_limit as i64])?;

    let mut hits = Vec::new();
    while let Some(row) = rows.next()? {
        let coll: String = row.get(1)?;
        if let Some(wanted) = collection
            && coll != wanted
        {
            continue;
        }
        let raw_score: f64 = row.get(5)?;
        let score = if raw_score < 0.0 {
            raw_score.abs() / (1.0 + raw_score.abs())
        } else {
            0.0
        };
        hits.push(QueryHit {
            id: row.get(0)?,
            collection: coll,
            path: row.get(2)?,
            title: row.get(3)?,
            hash: row.get(4)?,
            score,
        });
        if hits.len() >= FTS_LIMIT {
            break;
        }
    }
    Ok(hits)
}

// ── Vector branch ────────────────────────────────────────────────────

/// Encode an embedding as a JSON array string `[v1,v2,...]`.
fn encode_embedding_json(embedding: &[f32]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(embedding.len() * 12 + 2);
    s.push('[');
    for (i, v) in embedding.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "{v}");
    }
    s.push(']');
    s
}

/// Run a kNN vector search via the `vec0` virtual table.
///
/// # Errors
///
/// Returns an error if the prepared statement fails.
pub fn vector_search(db: &ZmdDb, query: &str, collection: Option<&str>) -> Result<Vec<QueryHit>> {
    // Build query embedding using the same FNV fallback that
    // native_indexer used for documents — guarantees compatibility
    // with the persisted `content_vectors_idx`.
    let prepared = format_query_for_embedding(query.as_bytes());
    let q_embedding = fnv_embed(&prepared);
    let q_json = encode_embedding_json(&q_embedding);

    // vec0 forbids extra WHERE clauses on the MATCH query itself, so
    // collection filtering is applied to the join with `documents`.
    let sql = "SELECT d.id, d.hash, d.collection, d.path, d.title, v.distance \
               FROM content_vectors_idx v \
               JOIN documents d ON d.hash = v.hash \
               WHERE d.active = 1 \
                 AND v.embedding MATCH vec_f32(?1) \
                 AND k = ?2 \
               ORDER BY v.distance ASC";

    let mut stmt = db.conn().prepare(sql)?;
    let mut rows = stmt.query(params![q_json, VEC_K as i64])?;

    // Best chunk per document.
    let mut best_by_doc: HashMap<i64, QueryHit> = HashMap::new();
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let hash: String = row.get(1)?;
        let coll: String = row.get(2)?;
        let path: String = row.get(3)?;
        let title: String = row.get(4)?;
        let dist: f64 = row.get(5)?;

        if let Some(wanted) = collection
            && coll != wanted
        {
            continue;
        }

        let score = 1.0 / (1.0 + dist);
        let candidate = QueryHit {
            id,
            collection: coll,
            path,
            title,
            hash,
            score,
        };

        match best_by_doc.get(&id) {
            Some(existing) if existing.score >= candidate.score => {}
            _ => {
                best_by_doc.insert(id, candidate);
            }
        }
    }

    let mut hits: Vec<QueryHit> = best_by_doc.into_values().collect();
    hits.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    Ok(hits)
}

// ── RRF fusion (mirrors zmd::reciprocalRankFusion) ───────────────────

/// Merge ranked result lists with Reciprocal Rank Fusion (k=60).
fn rrf_fuse(lists: &[&[QueryHit]]) -> Vec<QueryHit> {
    struct Entry {
        score: f64,
        hit: QueryHit,
    }
    let mut seen: HashMap<i64, Entry> = HashMap::new();

    for list in lists {
        for (rank, hit) in list.iter().enumerate() {
            #[allow(clippy::cast_precision_loss)]
            let rrf = 1.0 / (RRF_K + (rank + 1) as f64);
            seen.entry(hit.id)
                .and_modify(|e| e.score += rrf)
                .or_insert(Entry {
                    score: rrf,
                    hit: hit.clone(),
                });
        }
    }

    let mut out: Vec<(f64, QueryHit)> = seen.into_values().map(|e| (e.score, e.hit)).collect();
    out.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    out.into_iter()
        .map(|(score, mut hit)| {
            hit.score = score;
            hit
        })
        .collect()
}

// ── Hybrid query (public entry point) ────────────────────────────────

/// Run the full hybrid query pipeline (FTS + optional vector + RRF).
///
/// # Errors
///
/// Returns an error if any underlying SQL query fails.
pub fn hybrid_query(db: &ZmdDb, query: &str, opts: &QueryOptions) -> Result<Vec<QueryHit>> {
    let collection = opts.collection.as_deref();

    let fts_hits = fts_search(db, query, collection).context("FTS branch failed")?;

    if !opts.enable_vector {
        let mut out = fts_hits;
        out.truncate(opts.limit);
        return Ok(out);
    }

    let vec_hits = vector_search(db, query, collection).context("vector branch failed")?;

    let mut fused = rrf_fuse(&[fts_hits.as_slice(), vec_hits.as_slice()]);
    fused.truncate(opts.limit);
    Ok(fused)
}

// ── Similarity search (query → precedents → cited laws) ──────────────

/// A law article cited by one or more precedents in the similarity results.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CitedLaw {
    /// Law name as written in the precedent (e.g. "민법").
    pub law_name: String,
    /// Article (e.g. "제840조").
    pub article: String,
    /// Optional detail (paragraph/item).
    pub detail: Option<String>,
    /// How many of the top precedents cited this statute+article.
    pub cite_count: usize,
    /// If a matching document was found in the laws collection, its path.
    pub law_doc_path: Option<String>,
}

/// Result of a similarity search: precedents + the laws they cite.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SimilarityResult {
    /// Top precedents matching the query.
    pub precedents: Vec<QueryHit>,
    /// Laws cited by those precedents (deduplicated, ordered by cite count).
    pub cited_laws: Vec<CitedLaw>,
}

/// Options for similarity search.
#[derive(Debug, Clone)]
pub struct SimilarityOptions {
    /// Max precedents to retrieve from the hybrid query.
    pub precedent_limit: usize,
    /// Max cited laws to return.
    pub law_limit: usize,
    /// Whether to use vector branch for the initial precedent search.
    pub enable_vector: bool,
}

impl Default for SimilarityOptions {
    fn default() -> Self {
        Self {
            precedent_limit: 10,
            law_limit: 20,
            enable_vector: true,
        }
    }
}

/// Read document content from the `content` table by hash.
fn read_content(db: &ZmdDb, hash: &str) -> Result<String> {
    let sql = "SELECT doc FROM content WHERE hash = ?1";
    db.conn()
        .query_row(sql, params![hash], |row| row.get(0))
        .context("read content by hash")
}

/// Run similarity search: query → find precedents → extract cited laws.
///
/// # Algorithm
///
/// 1. Hybrid query against the `precedents` collection.
/// 2. For each top result, read its full text from the `content` table.
/// 3. Run `crossref::extract_statute_refs` to find cited law articles.
/// 4. Deduplicate and count citations across all precedents.
/// 5. Optionally resolve law names to document paths in the `laws` collection.
///
/// # Errors
///
/// Returns an error if the hybrid query or content reads fail.
pub fn similarity_search(
    db: &ZmdDb,
    query: &str,
    opts: &SimilarityOptions,
) -> Result<SimilarityResult> {
    // Step 1: Find precedents
    let q_opts = QueryOptions {
        limit: opts.precedent_limit,
        collection: Some("precedents".to_string()),
        enable_vector: opts.enable_vector,
    };
    let precedents = hybrid_query(db, query, &q_opts).context("precedent search")?;

    // Step 2+3: Read content and extract statute refs
    // Key: (law_name, article) → count + detail
    let mut law_counts: HashMap<(String, String), (usize, Option<String>)> = HashMap::new();

    for hit in &precedents {
        let content = match read_content(db, &hit.hash) {
            Ok(c) => c,
            Err(_) => continue, // skip if content missing
        };
        let refs = crossref::extract_statute_refs(&content);
        for sr in &refs {
            let key = (sr.law_name.clone(), sr.article.clone());
            law_counts
                .entry(key)
                .and_modify(|(count, _)| *count += 1)
                .or_insert((1, sr.detail.clone()));
        }
    }

    // Step 4: Sort by cite count descending
    let mut cited: Vec<_> = law_counts
        .into_iter()
        .collect::<Vec<((String, String), (usize, Option<String>))>>();
    cited.sort_by_key(|item| std::cmp::Reverse(item.1.0));
    cited.truncate(opts.law_limit);

    // Step 5: Try to resolve law names to docs in laws collection
    let cited_laws: Vec<CitedLaw> = cited
        .into_iter()
        .map(|((law_name, article), (cite_count, detail))| {
            let law_doc_path = resolve_law_path(db, &law_name);
            CitedLaw {
                law_name,
                article,
                detail,
                cite_count,
                law_doc_path,
            }
        })
        .collect();

    Ok(SimilarityResult {
        precedents,
        cited_laws,
    })
}

/// Try to find a law document path by title in the laws collection.
fn resolve_law_path(db: &ZmdDb, law_name: &str) -> Option<String> {
    let sql = "SELECT path FROM documents \
               WHERE collection = 'laws' AND active = 1 AND title = ?1 \
               LIMIT 1";
    db.conn()
        .query_row(sql, params![law_name], |row| row.get::<_, String>(0))
        .ok()
}

// ── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_simple() {
        assert_eq!(build_fts5_query("hello world"), "hello world");
    }

    #[test]
    fn fts5_negation() {
        assert_eq!(build_fts5_query("auth -test"), "auth -test");
    }

    #[test]
    fn fts5_prefix() {
        assert_eq!(build_fts5_query("auth*"), "auth*");
    }

    #[test]
    fn fts5_hyphenated() {
        assert_eq!(build_fts5_query("auth-flow"), "\"auth-flow\"");
    }

    #[test]
    fn fts5_negated_prefix_hyphen() {
        // "-foo-bar*" -> negated, hyphenated → quoted, prefixed *.
        assert_eq!(build_fts5_query("-foo-bar*"), "-\"foo-bar\"*");
    }

    #[test]
    fn fts5_empty() {
        assert_eq!(build_fts5_query("   "), "");
    }

    #[test]
    fn rrf_orders_by_summed_score() {
        let a = vec![
            QueryHit {
                id: 1,
                collection: String::new(),
                path: String::new(),
                title: String::new(),
                hash: String::new(),
                score: 0.0,
            },
            QueryHit {
                id: 2,
                collection: String::new(),
                path: String::new(),
                title: String::new(),
                hash: String::new(),
                score: 0.0,
            },
        ];
        let b = vec![QueryHit {
            id: 2,
            collection: String::new(),
            path: String::new(),
            title: String::new(),
            hash: String::new(),
            score: 0.0,
        }];

        // doc 2 appears in both lists → higher fused score than doc 1.
        let fused = rrf_fuse(&[a.as_slice(), b.as_slice()]);
        assert_eq!(fused[0].id, 2);
        assert_eq!(fused[1].id, 1);
    }

    #[test]
    fn encode_embedding_compact() {
        let s = encode_embedding_json(&[0.5, -0.25, 0.0]);
        assert_eq!(s, "[0.5,-0.25,0]");
    }
}
