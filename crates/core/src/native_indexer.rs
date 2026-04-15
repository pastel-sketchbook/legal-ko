//! Native zmd-compatible indexer.
//!
//! Replaces the external `zmd update` subprocess with a direct `SQLite` writer
//! that produces an identical database.  All algorithms (SHA-256 hashing,
//! title extraction, text normalization, FNV-384 embedding, boundary-aware
//! chunking) are ported from the Zig reference implementation so that
//! `zmd search`, `zmd query`, `zmd vsearch`, and the MCP server all continue
//! to work transparently.

use std::path::Path;

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

/// A file to be indexed: its collection-relative path and raw content.
pub struct FileEntry {
    /// Path relative to the collection root (e.g. `"민법.md"`).
    pub path: String,
    /// Raw file content bytes.
    pub content: Vec<u8>,
}

/// Result of processing a single file on a Rayon thread.
struct Processed {
    path: String,
    hash: String,
    title: String,
    content: Vec<u8>,
    /// `None` when the hash already existed in the database (skip embedding).
    chunks: Option<Vec<Vec<u8>>>,
    /// Parallel-computed embeddings, one per chunk.
    embeddings: Option<Vec<Vec<f32>>>,
}

/// Statistics returned after an indexing run.
#[derive(Debug, Default)]
pub struct IndexStats {
    pub indexed: usize,
    pub new: usize,
    pub skipped: usize,
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

    // ── Writes (single-thread, called inside a transaction) ──────

    /// Insert content blob if not already present.  Returns `true` when
    /// a new row was inserted.
    #[allow(dead_code)]
    fn insert_content(&self, hash: &str, doc: &[u8]) -> Result<bool> {
        let doc_str = std::str::from_utf8(doc).unwrap_or("");
        let changed = self.conn.execute(
            "INSERT OR IGNORE INTO content (hash, doc, created_at) VALUES (?1, ?2, ?3)",
            params![hash, doc_str, TIMESTAMP],
        )?;
        Ok(changed > 0)
    }

    /// Insert or replace a document row (triggers auto-sync FTS5).
    #[allow(dead_code)]
    fn insert_document(&self, collection: &str, path: &str, title: &str, hash: &str) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO documents (collection, path, title, hash, created_at, modified_at, active) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
            params![collection, path, title, hash, TIMESTAMP, TIMESTAMP],
        )?;
        Ok(())
    }

    /// Upsert a single embedding vector (`content_vectors` + vec0 index).
    #[allow(dead_code)]
    fn upsert_vector(&self, hash: &str, seq: i64, embedding: &[f32]) -> Result<()> {
        let emb_json = embedding_to_json(embedding);

        // content_vectors table
        self.conn.execute(
            "INSERT OR REPLACE INTO content_vectors (hash, seq, pos, model, embedding, embedded_at) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
            params![hash, seq, MODEL_NAME, emb_json, TIMESTAMP],
        )?;

        // vec0 index: delete then insert (no upsert for virtual tables)
        self.conn.execute(
            "DELETE FROM content_vectors_idx WHERE hash = ?1 AND seq = ?2 AND pos = 0",
            params![hash, seq],
        )?;
        self.conn.execute(
            "INSERT INTO content_vectors_idx(embedding, hash, model, seq, pos) VALUES(vec_f32(?1), ?2, ?3, ?4, 0)",
            params![emb_json, hash, MODEL_NAME, seq],
        )?;

        Ok(())
    }

    // ── High-level batch pipeline ────────────────────────────────

    /// Index a collection of files.
    ///
    /// 1. Reads existing hashes so Rayon workers can skip unchanged files.
    /// 2. On Rayon threads: hash, extract title, optionally chunk + embed.
    /// 3. On the main thread: write everything inside a single transaction.
    ///
    /// The `progress_cb` callback is invoked after each document is written
    /// with `(current_count, total_count)`.
    ///
    /// # Errors
    ///
    /// Returns an error if reading hashes, the transaction, or any SQL
    /// statement fails.
    /// Default batch size for `index_collection`: process and commit this
    /// many files per iteration.  Small enough to keep memory bounded and
    /// give frequent progress updates; large enough to amortise transaction
    /// overhead.
    const INDEX_BATCH_SIZE: usize = 500;

    pub fn index_collection<F>(
        &mut self,
        collection: &str,
        files: Vec<FileEntry>,
        progress_cb: F,
    ) -> Result<IndexStats>
    where
        F: Fn(usize, usize),
    {
        let total = files.len();
        if total == 0 {
            return Ok(IndexStats::default());
        }

        info!(collection, total, "starting native indexing");

        // Snapshot of known hashes (for skip optimisation).
        let mut known = self.existing_hashes()?;
        debug!(known_hashes = known.len(), "loaded existing content hashes");

        let mut stats = IndexStats::default();
        let mut global_idx = 0usize;

        for batch in files.chunks(Self::INDEX_BATCH_SIZE) {
            // ── Parallel phase (CPU-bound, no DB access) ──────────
            let processed: Vec<Processed> = batch
                .par_iter()
                .map(|entry| {
                    let hash = sha256_hex(&entry.content);
                    let title = extract_title(&entry.content);
                    let need_embed = !known.contains(&hash);
                    let (chunks, embeddings) = if need_embed {
                        let ch = chunk_document(&entry.content);
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
                        hash,
                        title,
                        content: entry.content.clone(),
                        chunks,
                        embeddings,
                    }
                })
                .collect();

            // ── Sequential write phase (one transaction per batch) ─
            let tx = self.conn.transaction()?;

            for p in &processed {
                // Insert content (dedup by hash).
                let doc_str = std::str::from_utf8(&p.content).unwrap_or("");
                tx.execute(
                    "INSERT OR IGNORE INTO content (hash, doc, created_at) VALUES (?1, ?2, ?3)",
                    params![&p.hash, doc_str, TIMESTAMP],
                )?;

                // Insert document (triggers FTS5 sync).
                tx.execute(
                    "INSERT OR REPLACE INTO documents (collection, path, title, hash, created_at, modified_at, active) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1)",
                    params![collection, &p.path, &p.title, &p.hash, TIMESTAMP, TIMESTAMP],
                )?;

                stats.indexed += 1;

                // Write embeddings only for new/changed content.
                if let (Some(chunks), Some(embeddings)) = (&p.chunks, &p.embeddings) {
                    debug_assert_eq!(chunks.len(), embeddings.len());
                    for (seq, emb) in embeddings.iter().enumerate() {
                        let emb_json = embedding_to_json(emb);
                        #[allow(clippy::cast_possible_wrap)]
                        let seq_i64 = seq as i64;
                        tx.execute(
                            "INSERT OR REPLACE INTO content_vectors (hash, seq, pos, model, embedding, embedded_at) VALUES (?1, ?2, 0, ?3, ?4, ?5)",
                            params![&p.hash, seq_i64, MODEL_NAME, &emb_json, TIMESTAMP],
                        )?;
                        tx.execute(
                            "DELETE FROM content_vectors_idx WHERE hash = ?1 AND seq = ?2 AND pos = 0",
                            params![&p.hash, seq_i64],
                        )?;
                        tx.execute(
                            "INSERT INTO content_vectors_idx(embedding, hash, model, seq, pos) VALUES(vec_f32(?1), ?2, ?3, ?4, 0)",
                            params![&emb_json, &p.hash, MODEL_NAME, seq_i64],
                        )?;
                    }
                    stats.new += 1;
                } else {
                    stats.skipped += 1;
                }

                global_idx += 1;
                progress_cb(global_idx, total);
            }

            tx.commit()?;

            // Update known hashes so the next batch can skip already-indexed content.
            for p in &processed {
                known.insert(p.hash.clone());
            }

            debug!(
                batch_size = batch.len(),
                indexed = stats.indexed,
                "batch committed"
            );
        }

        info!(
            collection,
            indexed = stats.indexed,
            new = stats.new,
            skipped = stats.skipped,
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

        -- FTS sync triggers
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

        CREATE TRIGGER IF NOT EXISTS documents_au AFTER UPDATE ON documents
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

    Ok(())
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

    for (i, slot) in embedding.iter_mut().enumerate() {
        let mut h: u32 = FNV_OFFSET_BASIS;
        for &c in text {
            h = h.wrapping_add(u32::from(c)).wrapping_mul(FNV_PRIME);
        }
        // Dimension index is always < 384, fits in u32.
        #[allow(clippy::cast_possible_truncation)]
        let dim = i as u32;
        h = h.wrapping_add(dim).wrapping_mul(FNV_PRIME);
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
        let content = std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
        entries.push(FileEntry {
            path: name,
            content,
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
            let content =
                std::fs::read(&path).with_context(|| format!("read {}", path.display()))?;
            entries.push(FileEntry { path: rel, content });
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
