# 0003: Native zmd Indexer

## Status

Implemented (Phase 2)

## Context

The `legal-ko` project uses [zmd](https://github.com/nicholasgasior/zig-qmd) for
full-text search (FTS5) and vector search (sqlite-vec) over Korean law and
court precedent documents. The original pipeline:

1. Hardlinks `.md` files from cloned GitHub repos into a stage directory.
2. Registers the stage directory as a zmd collection (`zmd collection add`).
3. Calls `zmd update` as a subprocess to scan the collection and index into
   `.qmd/data.db`.

Phase 1 optimised the subprocess orchestration (batching, resume, progress
bars, dedup). Even so, `zmd update` remained the bottleneck:

- **O(total_collection) per invocation** -- zmd rescans every staged file on
  every call, not just newly added ones. With 1,711 law files and batches of
  100, each batch still walks all previously staged files.
- **Serial processing** -- zmd processes files sequentially with no
  parallelism.
- **Subprocess overhead** -- each batch spawns a new `zmd update` process,
  re-opens the database, and re-initialises the schema.
- **3+ hours for a full index** of ~1,700 law documents on first run.

## Decision

Replace `zmd update` with a native Rust indexer that writes directly to zmd's
SQLite database, producing byte-identical output so that `zmd search`,
`zmd query`, `zmd vsearch`, and the zmd MCP server continue to work
transparently.

### What the native indexer does

1. Opens `.qmd/data.db` via `rusqlite` (bundled SQLite with FTS5) and loads
   `sqlite-vec` (compiled from vendored C sources via `build.rs`).
2. Initialises the full zmd schema: `content`, `documents`, `content_vectors`,
   `content_vectors_idx` (vec0), `documents_fts` (FTS5 with porter/unicode61),
   `store_collections`, `store_config`, `llm_cache`, and three FTS sync
   triggers.
3. Snapshots existing content hashes from the `content` table.
4. **Parallel phase (Rayon):** For each staged file, computes SHA-256, extracts
   the title, and -- only if the hash is new -- chunks the document and
   generates FNV-384 embeddings. All of this runs on the Rayon thread pool
   with zero database access.
5. **Sequential write phase:** Inserts all results in a single SQLite
   transaction: `content` (INSERT OR IGNORE), `documents` (INSERT OR REPLACE,
   which fires FTS5 sync triggers), `content_vectors`, and
   `content_vectors_idx`.

### Algorithms ported from zmd (Zig)

Every algorithm was ported to produce identical output:

| Algorithm | zmd source | Rust port |
|-----------|-----------|-----------|
| SHA-256 content hash | `store.zig:hashContent` | `native_indexer::sha256_hex` |
| Title extraction (frontmatter / heading / first line) | `store.zig:extractTitle` | `native_indexer::extract_title` |
| Text normalisation (whitespace collapse, 2048-byte truncation, "passage: " prefix) | `llm.zig:normalizeEmbeddingText` + `formatDocForEmbedding` | `native_indexer::normalize_embedding_text` + `format_doc_for_embedding` |
| FNV-384 embedding (deterministic hash, L2 normalised) | `llm.zig:LlamaCpp.embed` | `native_indexer::fnv_embed` |
| Boundary-aware chunking (3600-byte chunks, 540-byte overlap window, heading boost) | `chunker.zig:chunkDocument` + `findBestCutoff` | `native_indexer::chunk_document` + `find_best_cutoff` |
| Hardcoded timestamp `"2024-01-01T00:00:00Z"` | `store.zig` | `native_indexer::TIMESTAMP` |
| Model name `"fallback-fnv"` | `main.zig` | `native_indexer::MODEL_NAME` |

### Why not patch zmd instead?

- zmd is a Zig project with its own release cadence; patching it for
  incremental indexing would be a significant upstream contribution.
- The native indexer lets us control parallelism, transaction boundaries, and
  progress reporting without IPC overhead.
- Embedding the indexer in `legal-ko-core` eliminates the runtime dependency
  on the `zmd` binary for the indexing path (`zmd` is still used for
  `status` and `reset` commands).

## Consequences

### Positive

- **Dramatically faster indexing.** The parallel Rayon pipeline with a single
  transaction eliminates the O(n * total) scanning overhead and subprocess
  spawn cost.
- **No `zmd` binary required for indexing.** `zmd` is only needed for
  secondary commands (`status`, `reset`).
- **Resumable.** SHA-256 dedup in the `content` table means re-runs skip
  unchanged files instantly, same as zmd.
- **Full compatibility.** `zmd search`, `zmd query`, `zmd vsearch`, and the
  MCP server all work against the natively-indexed database without
  modification.

### Negative

- **Vendored C code.** `sqlite-vec` is compiled from C sources via `build.rs`
  because the Rust crate (`sqlite-vec 0.1.10-alpha.3`) has a build failure
  (missing `sqlite-vec-diskann.c` include). This adds ~150 KB of C to the
  repo.
- **Schema coupling.** If zmd changes its schema, the native indexer must be
  updated to match. The schema is versioned implicitly by the `CREATE TABLE
  IF NOT EXISTS` statements.
- **FNV embeddings are not semantic.** The fallback FNV hash produces
  deterministic but non-semantic vectors. This is the same as zmd's default
  behaviour when no LLM model is configured. Real semantic search requires
  configuring an embedding model in zmd separately.

## Files

| File | Role |
|------|------|
| `crates/core/src/native_indexer.rs` | All indexing logic: `ZmdDb`, schema, SHA-256, title extraction, chunking, FNV embedding, parallel pipeline |
| `crates/core/build.rs` | Compiles vendored `sqlite-vec.c` against bundled SQLite headers |
| `crates/core/sqlite-vec/` | Vendored sqlite-vec C sources (`.c`, `.h`) |
| `crates/core/src/zmd.rs` | Updated `stage_and_index_batched` to call native indexer instead of `zmd update` subprocess |
