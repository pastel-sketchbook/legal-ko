//! Native zmd-compatible indexer.
//!
//! Replaces the external `zmd update` subprocess with a direct `SQLite` writer
//! that produces an identical database.  All algorithms (SHA-256 hashing,
//! title extraction, text normalization, FNV-384 embedding, boundary-aware
//! chunking) are ported from the Zig reference implementation so that
//! `zmd search`, `zmd query`, `zmd vsearch`, and the MCP server all continue
//! to work transparently.

use std::{
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use anyhow::{Context, Result};
use rayon::prelude::*;
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use tracing::{debug, info};

// FFI binding for sqlite-vec (compiled via build.rs from vendored C sources).
unsafe extern "C" {
    fn sqlite3_vec_init(
        db: *mut std::ffi::c_void,
        pz_err_msg: *mut *mut std::ffi::c_char,
        p_api: *const std::ffi::c_void,
    ) -> std::ffi::c_int;
}

// ── Constants ────────────────────────────────────────────────────

/// Embedding dimensionality (must match zmd's `embedding_dim`).
const EMBEDDING_DIM: usize = 384;

/// Target chunk size in bytes.
const CHUNK_SIZE: usize = 3600;

/// Overlap window used when searching for a good split boundary.
const CHUNK_OVERLAP: usize = 540;

/// Maximum text length (bytes) fed to the embedding function.
const EMBEDDING_MAX_TEXT_LEN: usize = 2048;

/// Hardcoded timestamp used by zmd for all rows.
const TIMESTAMP: &str = "2024-01-01T00:00:00Z";

/// Model name written for fallback FNV embeddings.
const MODEL_NAME: &str = "fallback-fnv";

/// FNV offset basis (FNV-1a 32-bit).
const FNV_OFFSET_BASIS: u32 = 2_166_136_261;

/// FNV prime (FNV-1a 32-bit).
const FNV_PRIME: u32 = 16_777_619;

// ── Public types ─────────────────────────────────────────────────

/// A file to be indexed.
pub struct FileEntry {
    /// Path relative to the collection root (e.g. `"민법.md"`).
    pub path: String,
    /// Path to the staged file content.
    pub staged_path: PathBuf,
    /// Source file length in bytes.
    pub source_size: u64,
    /// Source file mtime as nanoseconds since the Unix epoch.
    pub source_mtime_ns: i64,
}

#[derive(Debug, Clone)]
pub struct ExistingDoc {
    pub hash: String,
    pub source_size: Option<u64>,
    pub source_mtime_ns: Option<i64>,
}

/// Result of processing a single file on a Rayon thread.
struct Processed {
    path: String,
    source_size: u64,
    source_mtime_ns: i64,
    hash: String,
    title: String,
    content: Vec<u8>,
    /// `None` when the hash already existed in the database (skip embedding).
    chunks: Option<Vec<Vec<u8>>>,
    /// Parallel-computed embeddings, one per chunk.
    embeddings: Option<Vec<Vec<f32>>>,
    /// `true` when the document row already maps this path to the same
    /// source metadata — no file read or SQL writes needed at all.
    doc_unchanged: bool,
    /// `true` when only stored source metadata needs to be refreshed.
    metadata_only: bool,
}

/// Statistics returned after an indexing run.
#[derive(Debug, Default)]
pub struct IndexStats {
    pub indexed: usize,
    pub new: usize,
    pub skipped: usize,
    pub metadata_refreshed: usize,
    pub content_rehashed: usize,
}

// ── Database wrapper ─────────────────────────────────────────────

/// A thin wrapper around a rusqlite [`Connection`] pointing at a zmd
/// `data.db` database.  Initialises the full schema (tables, indices,
/// FTS5, vec0, triggers) on construction so the database is always
/// ready for writes.
pub struct ZmdDb {
    conn: Connection,
}

impl ZmdDb {
    /// Open (or create) the database at `path` and initialise the schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the parent directory cannot be created, the
    /// database cannot be opened, sqlite-vec fails to initialise, or
    /// schema creation fails.
    pub fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists.
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir for {}", path.display()))?;
        }

        let conn =
            Connection::open(path).with_context(|| format!("open database {}", path.display()))?;

        // SAFETY: `sqlite3_vec_init` is compiled from vendored C sources via
        // build.rs and linked into this binary.  `conn.handle()` returns a
        // valid `*mut sqlite3` pointer that is cast to `*mut c_void` for the
        // FFI boundary.  The remaining args are null (no error message buffer
        // or API pointer needed when compiled as `SQLITE_CORE`).
        unsafe {
            let rc = sqlite3_vec_init(
                conn.handle().cast::<std::ffi::c_void>(),
                std::ptr::null_mut(),
                std::ptr::null(),
            );
            if rc != 0 {
                anyhow::bail!("sqlite-vec init failed (rc={rc})");
            }
        }

        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        init_schema(&conn)?;

        Ok(Self { conn })
    }

    /// Return a shared reference to the inner connection (useful for
    /// callers that need to run ad-hoc queries).
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    // ── Queries ──────────────────────────────────────────────────

    /// Collect the set of content hashes already present in the `content`
    /// table.  Used as a bloom filter so Rayon workers can skip
    /// chunking/embedding for unchanged files.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn existing_hashes(&self) -> Result<std::collections::HashSet<String>> {
        let mut stmt = self.conn.prepare("SELECT hash FROM content")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut set = std::collections::HashSet::new();
        for hash in rows {
            set.insert(hash?);
        }
        Ok(set)
    }

    /// Snapshot existing document metadata for a collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub fn existing_docs(
        &self,
        collection: &str,
    ) -> Result<std::collections::HashMap<String, ExistingDoc>> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT path, hash, source_size, source_mtime_ns FROM documents WHERE collection = ?1 AND active = 1",
            )?;
        let rows = stmt.query_map(params![collection], |row| {
            Ok((
                row.get::<_, String>(0)?,
                ExistingDoc {
                    hash: row.get::<_, String>(1)?,
                    source_size: row
                        .get::<_, Option<i64>>(2)?
                        .and_then(|size| u64::try_from(size).ok()),
                    source_mtime_ns: row.get::<_, Option<i64>>(3)?,
                },
            ))
        })?;
        let mut map = std::collections::HashMap::new();
        for row in rows {
            let (path, doc) = row?;
            map.insert(path, doc);
        }
        Ok(map)
    }

    /// Register (or update) a collection in `store_collections`.
    ///
    /// This is the native equivalent of `zmd collection add <name> <path>`.
    ///
    /// # Errors
    ///
    /// Returns an error if the SQL statement fails.
    pub fn register_collection(&self, name: &str, path: &Path) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO store_collections (name, path, pattern) VALUES (?1, ?2, '**/*.md')",
            params![name, path.to_string_lossy().as_ref()],
        )?;
        Ok(())
    }

    /// Count the number of active documents in a collection.
    ///
    /// # Errors
    ///
    /// Returns an error if the query fails.
    pub fn document_count(&self, collection: &str) -> Result<usize> {
        let count: i64 = self.conn.query_row(
            "SELECT count(*) FROM documents WHERE collection = ?1 AND active = 1",
            params![collection],
            |row| row.get(0),
        )?;
        // count(*) is always non-negative, safe to cast.
        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
        let result = count.min(i64::from(u32::MAX)) as usize;
        Ok(result)
    }

    // ── High-level batch pipeline ────────────────────────────────

    /// Default batch size for `index_collection`: process and commit this
    /// many files per iteration.  Large enough to amortise transaction
    /// overhead while keeping memory bounded.
    const INDEX_BATCH_SIZE: usize = 2000;

    /// Index a set of files into the given collection.
    ///
    /// For each file, computes a SHA-256 content hash, extracts a title,
    /// and (for new/changed content) chunks the text and generates FNV-384
    /// embeddings.  Unchanged documents (same path→hash mapping) are
    /// skipped entirely — no SQL writes.
    ///
    /// FTS5 triggers and vec0 index maintenance are disabled during bulk
    /// ingest — they are rebuilt once at the end for dramatically better
    /// throughput on large collections.
    ///
    /// The `progress_cb` callback is invoked after each document is
    /// processed with `(current_count, total_count)`.
    ///
    /// # Errors
    ///
    /// Returns an error if reading hashes, the transaction, or any SQL
    /// statement fails.
    #[allow(clippy::too_many_lines)]
    pub fn index_collection<F>(
        &mut self,
        collection: &str,
        files: &[FileEntry],
        progress_cb: F,
    ) -> Result<IndexStats>
    where
        F: Fn(usize, usize),
    {
        let total = files.len();
        if total == 0 {
            return Ok(IndexStats::default());
        }

        // Bulk-load PRAGMAs — only set during indexing so query-only
        // callers keep the lightweight defaults from open().
        self.conn.execute_batch(
            "PRAGMA synchronous = NORMAL;\
             PRAGMA temp_store = MEMORY;\
             PRAGMA cache_size = -200000;\
             PRAGMA mmap_size = 268435456;",
        )?;

        info!(collection, total, "starting native indexing");

        // Snapshot of known hashes (for skip optimisation).
        let mut known = self.existing_hashes()?;
        debug!(known_hashes = known.len(), "loaded existing content hashes");

        // Snapshot of existing document metadata so unchanged files can be
        // skipped before reading content from disk.
        let existing_docs = self.existing_docs(collection)?;
        debug!(
            known_docs = existing_docs.len(),
            "loaded existing document metadata"
        );

        // ── Disable FTS triggers during bulk ingest ──────────────
        self.conn.execute_batch(
            r"DROP TRIGGER IF EXISTS documents_ai;
              DROP TRIGGER IF EXISTS documents_ad;
              DROP TRIGGER IF EXISTS documents_au;",
        )?;

        let mut stats = IndexStats::default();
        let mut global_idx = 0usize;

        for batch in files.chunks(Self::INDEX_BATCH_SIZE) {
            // ── Parallel phase (CPU-bound, no DB access) ──────────
            let processed: Vec<Processed> = batch
                .par_iter()
                .map(|entry| {
                    let existing = existing_docs.get(&entry.path);
                    if existing.is_some_and(|doc| {
                        doc.source_size == Some(entry.source_size)
                            && doc.source_mtime_ns == Some(entry.source_mtime_ns)
                    }) {
                        return Processed {
                            path: entry.path.clone(),
                            source_size: entry.source_size,
                            source_mtime_ns: entry.source_mtime_ns,
                            hash: String::new(),
                            title: String::new(),
                            content: Vec::new(),
                            chunks: None,
                            embeddings: None,
                            doc_unchanged: true,
                            metadata_only: false,
                        };
                    }

                    let content = std::fs::read(&entry.staged_path).unwrap_or_default();
                    let hash = sha256_hex(&content);
                    let title = extract_title(&content);
                    let need_embed = !known.contains(&hash);
                    let metadata_only = existing.is_some_and(|doc| doc.hash == hash);
                    let (chunks, embeddings) = if need_embed {
                        let ch = chunk_document(&content);
                        let embs: Vec<Vec<f32>> = ch
                            .iter()
                            .map(|chunk| {
                                let normalized = format_doc_for_embedding(chunk);
                                fnv_embed(&normalized)
                            })
                            .collect();
                        (Some(ch), Some(embs))
                    } else {
                        (None, None)
                    };
                    Processed {
                        path: entry.path.clone(),
                        source_size: entry.source_size,
                        source_mtime_ns: entry.source_mtime_ns,
                        hash,
                        title,
                        content,
                        chunks,
                        embeddings,
                        doc_unchanged: false,
                        metadata_only,
                    }
                })
                .collect();

            // ── Sequential write phase (one transaction per batch) ─
            //
            // Prepared statements are cached for the lifetime of the
            // transaction to avoid re-parsing SQL on every row.
            let tx = self.conn.transaction()?;

            {
                let mut stmt_meta = tx.prepare_cached(
                    "UPDATE documents SET source_size = ?1, source_mtime_ns = ?2, modified_at = ?3 WHERE collection = ?4 AND path = ?5",
                )?;
                let mut stmt_content = tx.prepare_cached(
                    "INSERT OR IGNORE INTO content (hash, doc, created_at) VALUES (?1, ?2, ?3)",
                )?;
                let mut stmt_doc = tx.prepare_cached(
                    "INSERT OR REPLACE INTO documents (collection, path, title, hash, source_size, source_mtime_ns, created_at, modified_at, active) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
                )?;
                let mut stmt_vec = tx.prepare_cached(
                    "INSERT OR REPLACE INTO content_vectors (hash, seq, pos, model, embedding, embedded_at) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                )?;

                for p in &processed {
                    if p.doc_unchanged {
                        stats.skipped += 1;
                        global_idx += 1;
                        progress_cb(global_idx, total);
                        continue;
                    }

                    if p.metadata_only {
                        #[allow(clippy::cast_possible_wrap)]
                        let source_size = p.source_size as i64;
                        stmt_meta.execute(params![
                            source_size,
                            p.source_mtime_ns,
                            TIMESTAMP,
                            collection,
                            &p.path,
                        ])?;
                        stats.indexed += 1;
                        stats.metadata_refreshed += 1;
                        global_idx += 1;
                        progress_cb(global_idx, total);
                        continue;
                    }

                    // Insert content (dedup by hash).
                    stats.content_rehashed += 1;
                    let doc_str = std::str::from_utf8(&p.content).unwrap_or("");
                    stmt_content.execute(params![&p.hash, doc_str, TIMESTAMP])?;

                    // Insert document (no FTS trigger — rebuilt at end).
                    #[allow(clippy::cast_possible_wrap)]
                    let source_size = p.source_size as i64;
                    stmt_doc.execute(params![
                        collection,
                        &p.path,
                        &p.title,
                        &p.hash,
                        source_size,
                        p.source_mtime_ns,
                        TIMESTAMP,
                        TIMESTAMP,
                    ])?;

                    stats.indexed += 1;

                    // Write embeddings (no vec0 idx — rebuilt at end).
                    if let (Some(chunks), Some(embeddings)) = (&p.chunks, &p.embeddings) {
                        debug_assert_eq!(chunks.len(), embeddings.len());
                        for (seq, emb) in embeddings.iter().enumerate() {
                            let emb_json = embedding_to_json(emb);
                            #[allow(clippy::cast_possible_wrap)]
                            let seq_i64 = seq as i64;
                            stmt_vec.execute(params![
                                &p.hash, seq_i64, MODEL_NAME, &emb_json, TIMESTAMP,
                            ])?;
                        }
                        stats.new += 1;
                    }

                    global_idx += 1;
                    progress_cb(global_idx, total);
                }
            }

            tx.commit()?;

            // Update known hashes so the next batch can skip already-indexed content.
            for p in &processed {
                if !p.doc_unchanged {
                    known.insert(p.hash.clone());
                }
            }

            debug!(
                batch_size = batch.len(),
                indexed = stats.indexed,
                "batch committed"
            );
        }

        let needs_rebuild = stats.new > 0 || stats.content_rehashed > 0;

        if needs_rebuild {
            // ── Rebuild FTS5 index from base tables ──────────────────
            info!(collection, "rebuilding FTS5 index");
            self.conn.execute_batch(
                r"DELETE FROM documents_fts;
                  INSERT INTO documents_fts(rowid, filepath, title, body)
                  SELECT d.id,
                         d.collection || '/' || d.path,
                         d.title,
                         c.doc
                  FROM documents d
                  JOIN content c ON c.hash = d.hash
                  WHERE d.active = 1;",
            )?;

            // ── Rebuild vec0 index from content_vectors ──────────────
            info!(collection, "rebuilding vector index");
            self.conn.execute_batch(
                r"DROP TABLE IF EXISTS content_vectors_idx;
                  CREATE VIRTUAL TABLE content_vectors_idx USING vec0(
                      embedding float[384],
                      hash TEXT,
                      model TEXT,
                      +seq INTEGER,
                      +pos INTEGER
                  );
                  INSERT INTO content_vectors_idx(embedding, hash, model, seq, pos)
                  SELECT vec_f32(embedding), hash, model, seq, pos
                  FROM content_vectors;",
            )?;
        } else {
            info!(
                collection,
                "skipping FTS/vector rebuild (no content changes)"
            );
        }

        // ── Re-create FTS triggers for future incremental updates ─
        self.conn.execute_batch(
            r"CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents
              WHEN new.active = 1
              BEGIN
                  INSERT INTO documents_fts(rowid, filepath, title, body)
                  SELECT new.id,
                         new.collection || '/' || new.path,
                         new.title,
                         (SELECT doc FROM content WHERE hash = new.hash)
                  WHERE new.active = 1;
              END;
              CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents
              BEGIN
                  DELETE FROM documents_fts WHERE rowid = old.id;
              END;
              CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE OF collection, path, title, hash, active ON documents
              BEGIN
                  DELETE FROM documents_fts WHERE rowid = old.id AND new.active = 0;
                  INSERT OR REPLACE INTO documents_fts(rowid, filepath, title, body)
                  SELECT new.id,
                         new.collection || '/' || new.path,
                         new.title,
                         (SELECT doc FROM content WHERE hash = new.hash)
                  WHERE new.active = 1;
              END;",
        )?;

        // Reset bulk-load PRAGMAs to conservative defaults.
        self.conn.execute_batch(
            "PRAGMA synchronous = FULL;\
             PRAGMA temp_store = DEFAULT;\
             PRAGMA cache_size = -2000;\
             PRAGMA mmap_size = 0;",
        )?;

        info!(
            collection,
            indexed = stats.indexed,
            new = stats.new,
            skipped = stats.skipped,
            metadata_refreshed = stats.metadata_refreshed,
            content_rehashed = stats.content_rehashed,
            "native indexing complete"
        );
        Ok(stats)
    }
}

// ── Schema initialisation ────────────────────────────────────────

fn init_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r"
        CREATE TABLE IF NOT EXISTS content (
            hash       TEXT PRIMARY KEY,
            doc        TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS documents (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            collection  TEXT NOT NULL,
            path        TEXT NOT NULL,
            title       TEXT NOT NULL,
            hash        TEXT NOT NULL,
            source_size INTEGER,
            source_mtime_ns INTEGER,
            created_at  TEXT NOT NULL,
            modified_at TEXT NOT NULL,
            active      INTEGER NOT NULL DEFAULT 1,
            FOREIGN KEY (hash) REFERENCES content(hash) ON DELETE CASCADE,
            UNIQUE(collection, path)
        );
        CREATE INDEX IF NOT EXISTS idx_documents_collection ON documents(collection, active);
        CREATE INDEX IF NOT EXISTS idx_documents_hash ON documents(hash);
        CREATE INDEX IF NOT EXISTS idx_documents_path ON documents(path, active);

        CREATE TABLE IF NOT EXISTS llm_cache (
            hash       TEXT PRIMARY KEY,
            result     TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS content_vectors (
            hash        TEXT NOT NULL,
            seq         INTEGER NOT NULL DEFAULT 0,
            pos         INTEGER NOT NULL DEFAULT 0,
            model       TEXT NOT NULL,
            embedding   TEXT NOT NULL,
            embedded_at TEXT NOT NULL,
            PRIMARY KEY (hash, seq, pos)
        );

        CREATE TABLE IF NOT EXISTS store_collections (
            name                TEXT PRIMARY KEY,
            path                TEXT NOT NULL,
            pattern             TEXT NOT NULL DEFAULT '**/*.md',
            ignore_patterns     TEXT,
            include_by_default  INTEGER DEFAULT 1,
            update_command      TEXT,
            context             TEXT
        );

        CREATE TABLE IF NOT EXISTS store_config (
            key   TEXT PRIMARY KEY,
            value TEXT
        );

        CREATE VIRTUAL TABLE IF NOT EXISTS documents_fts USING fts5(
            filepath, title, body,
            tokenize='porter unicode61'
        );

        -- FTS sync triggers (drop old documents_au to upgrade to column-scoped version)
        DROP TRIGGER IF EXISTS documents_au;
        CREATE TRIGGER IF NOT EXISTS documents_ai AFTER INSERT ON documents
        WHEN new.active = 1
        BEGIN
            INSERT INTO documents_fts(rowid, filepath, title, body)
            SELECT new.id,
                   new.collection || '/' || new.path,
                   new.title,
                   (SELECT doc FROM content WHERE hash = new.hash)
            WHERE new.active = 1;
        END;

        CREATE TRIGGER IF NOT EXISTS documents_ad AFTER DELETE ON documents
        BEGIN
            DELETE FROM documents_fts WHERE rowid = old.id;
        END;

        CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE OF collection, path, title, hash, active ON documents
        BEGIN
            DELETE FROM documents_fts WHERE rowid = old.id AND new.active = 0;
            INSERT OR REPLACE INTO documents_fts(rowid, filepath, title, body)
            SELECT new.id,
                   new.collection || '/' || new.path,
                   new.title,
                   (SELECT doc FROM content WHERE hash = new.hash)
            WHERE new.active = 1;
        END;
        ",
    )?;

    // vec0 virtual table must be created separately (not in execute_batch
    // because some SQLite wrappers split on `;` and fail on virtual table DDL).
    conn.execute_batch(
        r"
        CREATE VIRTUAL TABLE IF NOT EXISTS content_vectors_idx USING vec0(
            embedding float[384],
            hash TEXT,
            model TEXT,
            +seq INTEGER,
            +pos INTEGER
        );
        ",
    )?;

    ensure_documents_column(conn, "source_size", "INTEGER")?;
    ensure_documents_column(conn, "source_mtime_ns", "INTEGER")?;

    Ok(())
}

fn ensure_documents_column(conn: &Connection, column: &str, definition: &str) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(documents)")?;
    let columns = stmt.query_map([], |row| row.get::<_, String>(1))?;
    for existing in columns {
        if existing? == column {
            return Ok(());
        }
    }

    conn.execute(
        &format!("ALTER TABLE documents ADD COLUMN {column} {definition}"),
        [],
    )?;
    Ok(())
}

#[must_use]
pub fn system_time_to_unix_nanos(time: std::time::SystemTime) -> i64 {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_nanos()).ok())
        .unwrap_or_default()
}

// ── SHA-256 ──────────────────────────────────────────────────────

/// Compute the SHA-256 hex digest of `data` (64 lowercase hex chars).
#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let hash = Sha256::digest(data);
    hex_encode(&hash)
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

// ── Title extraction ─────────────────────────────────────────────

/// Extract a document title using the same priority as zmd:
/// 1. YAML frontmatter `title:` field
/// 2. First markdown `# heading`
/// 3. First non-blank line
/// 4. `"Untitled"`
#[must_use]
pub fn extract_title(content: &[u8]) -> String {
    let text = std::str::from_utf8(content).unwrap_or("");
    let mut lines = text.lines();

    let mut in_frontmatter = false;
    let mut frontmatter_started = false;
    let mut frontmatter_title: Option<&str> = None;

    for line in &mut lines {
        let trimmed = line.trim();

        if !frontmatter_started && trimmed == "---" {
            frontmatter_started = true;
            in_frontmatter = true;
            continue;
        }

        if in_frontmatter {
            if let Some(rest) = trimmed.strip_prefix("title:") {
                let v = rest.trim();
                // Strip surrounding quotes.
                let v = if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                    &v[1..v.len() - 1]
                } else {
                    v
                };
                if !v.is_empty() {
                    frontmatter_title = Some(v);
                }
            }
            if trimmed == "---" {
                in_frontmatter = false;
                if let Some(t) = frontmatter_title {
                    return t.to_string();
                }
            }
            continue;
        }

        if trimmed.is_empty() {
            continue;
        }

        // Markdown heading.
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if hashes > 0 && trimmed.as_bytes().get(hashes) == Some(&b' ') {
            let heading = trimmed[hashes + 1..].trim();
            if !heading.is_empty() {
                return heading.to_string();
            }
            continue;
        }

        // First non-blank, non-frontmatter line.
        return trimmed.to_string();
    }

    "Untitled".to_string()
}

// ── Text normalisation ───────────────────────────────────────────

/// Collapse whitespace, truncate at `EMBEDDING_MAX_TEXT_LEN` bytes,
/// and prepend `"passage: "`.
#[must_use]
pub fn format_doc_for_embedding(text: &[u8]) -> Vec<u8> {
    let normalized = normalize_embedding_text(text, EMBEDDING_MAX_TEXT_LEN);
    let mut out = Vec::with_capacity(b"passage: ".len() + normalized.len());
    out.extend_from_slice(b"passage: ");
    out.extend_from_slice(&normalized);
    out
}

/// Collapse whitespace, truncate at `EMBEDDING_MAX_TEXT_LEN` bytes,
/// and prepend `"query: "`.  Mirrors zmd's `formatQueryForEmbedding`.
#[must_use]
pub fn format_query_for_embedding(text: &[u8]) -> Vec<u8> {
    let normalized = normalize_embedding_text(text, EMBEDDING_MAX_TEXT_LEN);
    let mut out = Vec::with_capacity(b"query: ".len() + normalized.len());
    out.extend_from_slice(b"query: ");
    out.extend_from_slice(&normalized);
    out
}

fn normalize_embedding_text(text: &[u8], max_len: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len().min(max_len));
    let mut prev_space = true;

    for &ch in text {
        if out.len() >= max_len {
            break;
        }
        let is_space = ch == b' ' || ch == b'\n' || ch == b'\t' || ch == b'\r';
        if is_space {
            if !prev_space && out.len() < max_len {
                out.push(b' ');
                prev_space = true;
            }
            continue;
        }
        out.push(ch);
        prev_space = false;
    }

    // Strip trailing space.
    if out.last() == Some(&b' ') {
        out.pop();
    }
    out
}

// ── FNV-384 embedding ────────────────────────────────────────────

/// Deterministic FNV-hash embedding (384 dimensions), matching zmd's
/// `LlamaCpp.embed()` fallback exactly.
///
/// Algorithm per dimension *i*:
/// ```text
/// h = 2166136261
/// for byte in text:
///     h = h.wrapping_add(byte as u32).wrapping_mul(16777619)
/// h = h.wrapping_add(i as u32).wrapping_mul(16777619)    // zmd uses +%=, *%=
/// value = (h & 0xFFFF) as f32 / 65535.0 - 0.5
/// ```
/// Then L2-normalise the entire vector.
#[must_use]
pub fn fnv_embed(text: &[u8]) -> Vec<f32> {
    let mut embedding = vec![0.0f32; EMBEDDING_DIM];

    // Compute the text-dependent base hash once instead of repeating the
    // full text scan for each of the 384 dimensions.
    let mut base: u32 = FNV_OFFSET_BASIS;
    for &c in text {
        base = base.wrapping_add(u32::from(c)).wrapping_mul(FNV_PRIME);
    }

    for (i, slot) in embedding.iter_mut().enumerate() {
        // Dimension index is always < 384, fits in u32.
        #[allow(clippy::cast_possible_truncation)]
        let dim = i as u32;
        let h = base.wrapping_add(dim).wrapping_mul(FNV_PRIME);
        // Only the low 16 bits are used; max value is 65535 which fits
        // in f32 without precision loss.
        #[allow(clippy::cast_precision_loss)]
        let val = (h & 0xFFFF) as f32 / 65535.0 - 0.5;
        *slot = val;
    }

    // L2 normalise.
    let norm: f32 = embedding.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in &mut embedding {
            *x /= norm;
        }
    }

    embedding
}

// ── Chunking ─────────────────────────────────────────────────────

/// Split content into chunks of approximately `CHUNK_SIZE` bytes,
/// using `find_best_cutoff` to find natural line boundaries.
/// Matches zmd's `chunker.chunkDocument` exactly.
#[must_use]
pub fn chunk_document(content: &[u8]) -> Vec<Vec<u8>> {
    if content.len() <= CHUNK_SIZE {
        return vec![content.to_vec()];
    }

    let mut chunks = Vec::new();
    let mut pos = 0;

    while pos < content.len() {
        let chunk_end = pos + CHUNK_SIZE;
        if chunk_end >= content.len() {
            chunks.push(content[pos..].to_vec());
            break;
        }

        let window_start = if pos == 0 {
            pos
        } else {
            pos + CHUNK_SIZE - CHUNK_OVERLAP
        };
        let window_end = chunk_end.min(content.len());

        let mut cutoff = find_best_cutoff(content, window_start, window_end);
        if cutoff <= pos {
            cutoff = chunk_end;
        }

        chunks.push(content[pos..cutoff].to_vec());
        pos = cutoff;
    }

    chunks
}

/// Score newline positions within `[window_start, window_end)` for
/// proximity to the midpoint, with a 3x boost for heading lines.
fn find_best_cutoff(content: &[u8], window_start: usize, window_end: usize) -> usize {
    if window_end <= window_start {
        return window_start;
    }
    if window_end - window_start < 20 {
        return window_end;
    }

    let mut best_pos = window_start;
    let mut best_score: f64 = -1.0;

    let mut pos = window_start;
    while pos < window_end && pos < content.len() {
        if content[pos] == b'\n' {
            // Walk back to find the start of this line.
            let mut line_start = pos;
            while line_start > window_start && content[line_start - 1] != b'\n' {
                line_start -= 1;
            }
            let line_len = pos - line_start;
            if !(3..=200).contains(&line_len) {
                pos += 1;
                continue;
            }

            // Check if the line is a heading (starts with '#').
            let is_heading = line_start + 1 < content.len() && content[line_start + 1] == b'#';

            // Positions are always within a single document chunk (< 4 KiB),
            // so usize→f64 is lossless on all practical architectures.
            #[allow(clippy::cast_precision_loss)]
            let dist_from_mid = (pos as f64 - window_start as f64)
                - (window_end as f64 - window_start as f64) / 2.0;
            let mut score = 1.0 / (1.0 + dist_from_mid * dist_from_mid / 100.0);
            if is_heading {
                score *= 3.0;
            }

            if score > best_score {
                best_score = score;
                best_pos = pos;
            }
        }
        pos += 1;
    }

    best_pos
}

// ── Embedding JSON serialisation ─────────────────────────────────

/// Serialise a float slice as a JSON array string `[0.1,0.2,...]`.
fn embedding_to_json(embedding: &[f32]) -> String {
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

// ── Helpers ──────────────────────────────────────────────────────

/// Default zmd database path: `.qmd/data.db` under the current directory.
#[must_use]
pub fn default_db_path() -> std::path::PathBuf {
    std::path::PathBuf::from(".qmd/data.db")
}

/// Read all `.md` files from a directory (non-recursively for stage dirs,
/// or recursively for repo dirs) and return `FileEntry` items.
///
/// # Errors
///
/// Returns an error if the directory cannot be read or a file cannot be read.
pub fn read_staged_files(dir: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let ft = entry.file_type()?;
        if !ft.is_file() && !ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        let metadata =
            std::fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
        #[allow(clippy::cast_possible_wrap)]
        let source_size = metadata.len() as i64;
        let source_mtime_ns = metadata
            .modified()
            .map(system_time_to_unix_nanos)
            .unwrap_or_default();
        entries.push(FileEntry {
            path: name,
            staged_path: path,
            source_size: u64::try_from(source_size).unwrap_or_default(),
            source_mtime_ns,
        });
    }
    Ok(entries)
}

/// Recursively read all `.md` files from a directory tree.
///
/// # Errors
///
/// Returns an error if a directory cannot be read or a file cannot be read.
pub fn read_staged_files_recursive(dir: &Path) -> Result<Vec<FileEntry>> {
    let mut entries = Vec::new();
    walk_dir_recursive(dir, dir, &mut entries)?;
    Ok(entries)
}

fn walk_dir_recursive(root: &Path, dir: &Path, entries: &mut Vec<FileEntry>) -> Result<()> {
    for entry in std::fs::read_dir(dir).with_context(|| format!("read dir {}", dir.display()))? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let path = entry.path();
        if ft.is_dir() {
            walk_dir_recursive(root, &path, entries)?;
        } else if (ft.is_file() || ft.is_symlink())
            && path.extension().and_then(|e| e.to_str()) == Some("md")
        {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let metadata =
                std::fs::metadata(&path).with_context(|| format!("stat {}", path.display()))?;
            #[allow(clippy::cast_possible_wrap)]
            let source_size = metadata.len() as i64;
            let source_mtime_ns = metadata
                .modified()
                .map(system_time_to_unix_nanos)
                .unwrap_or_default();
            entries.push(FileEntry {
                path: rel,
                staged_path: path,
                source_size: u64::try_from(source_size).unwrap_or_default(),
                source_mtime_ns,
            });
        }
    }
    Ok(())
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex() {
        let hash = sha256_hex(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_extract_title_frontmatter() {
        let content = b"---\ntitle: \"My Law\"\n---\n# Heading\nBody";
        assert_eq!(extract_title(content), "My Law");
    }

    #[test]
    fn test_extract_title_heading() {
        let content = b"# My Heading\nBody text";
        assert_eq!(extract_title(content), "My Heading");
    }

    #[test]
    fn test_extract_title_first_line() {
        let content = b"Some plain text\nMore text";
        assert_eq!(extract_title(content), "Some plain text");
    }

    #[test]
    fn test_extract_title_untitled() {
        let content = b"";
        assert_eq!(extract_title(content), "Untitled");
    }

    #[test]
    fn test_normalize_text() {
        let input = b"  hello   world  \n  foo  ";
        let out = normalize_embedding_text(input, 2048);
        assert_eq!(out, b"hello world foo");
    }

    #[test]
    fn test_normalize_truncate() {
        let input = b"abcdefghij";
        let out = normalize_embedding_text(input, 5);
        assert_eq!(out, b"abcde");
    }

    #[test]
    fn test_fnv_embed_dimensions() {
        let emb = fnv_embed(b"test text");
        assert_eq!(emb.len(), EMBEDDING_DIM);
        // Should be L2-normalised (length ≈ 1.0).
        let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "norm = {norm}");
    }

    #[test]
    fn test_chunk_small_document() {
        let content = vec![b'a'; 100];
        let chunks = chunk_document(&content);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 100);
    }

    #[test]
    fn test_chunk_large_document() {
        // 10000 bytes → should produce multiple chunks.
        let content = vec![b'a'; 10000];
        let chunks = chunk_document(&content);
        assert!(chunks.len() > 1, "got {} chunks", chunks.len());
        // Total bytes should equal the original.
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, 10000);
    }

    #[test]
    fn test_format_doc_for_embedding() {
        let input = b"hello  world";
        let out = format_doc_for_embedding(input);
        assert_eq!(out, b"passage: hello world");
    }

    #[test]
    fn test_embedding_to_json() {
        let emb = vec![0.5, -0.25, 0.0];
        let json = embedding_to_json(&emb);
        assert_eq!(json, "[0.5,-0.25,0]");
    }

    #[test]
    fn test_find_best_cutoff_heading_boost() {
        // Construct content where a heading line should be preferred as cutoff.
        let content = b"aaa\nbbb\n# heading\nccc\nddd\neee\n";
        // Window covering the heading area.
        let cutoff = find_best_cutoff(content, 0, content.len());
        // The cutoff should land on a \n boundary.
        assert_eq!(content[cutoff], b'\n');
    }
}
