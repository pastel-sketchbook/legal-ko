# 0004: Native Query (FTS5 + vec0 Hybrid)

## Status

Implemented

## Context

After implementing the native indexer (0003), the project still relied on the
`zmd` binary (v0.4.1) as a subprocess for every search query. Each invocation
of `zmd query` costs ~1.5 seconds on Apple Silicon — acceptable for interactive
use, but unacceptable for LLM tool-call chains that may issue 5–10 queries per
turn.

The database schema and scoring algorithms used by zmd are well-documented and
deterministic. The native indexer (0003) already opens the same
`.qmd/data.db` SQLite database. Extending it to also *query* eliminates the
subprocess entirely.

## Decision

Implement a native query module (`native_query.rs`) that performs FTS5 + vec0
hybrid search in-process against the zmd database, with algorithm parity:

| Component | zmd (v0.4.1) | Native |
|-----------|-------------|--------|
| FTS5 BM25 weights | `(1.5, 4.0, 1.0)` | Same |
| FTS5 score normalisation | `|s| / (1 + |s|)` | Same |
| vec0 kNN k | 200 | Same |
| Vector score | `1 / (1 + distance)` | Same |
| Fusion | RRF k=60 | Same |
| Query embedding | `"query: "` prefix + FNV-384 fallback | Same |
| Reranking | llama.cpp (optional) | Not implemented |

Two CLI subcommands expose this:

- `legal-ko-cli zmd search <query>` — FTS-only (fastest path)
- `legal-ko-cli zmd query <query>` — FTS + vector + RRF (default)

## Benchmark

Query: `"전세 보증금 반환"`, limit 10, warm cache, Apple Silicon (M-series)

| Method | Median | Speedup |
|--------|--------|---------|
| Native FTS-only (`zmd search`) | **10 ms** | 150× |
| Native hybrid (`zmd query`) | **410 ms** | 3.7× |
| zmd subprocess (`zmd query`) | 1,510 ms | baseline |

Cold start (first invocation after boot): ~0.8–6 s due to SQLite page cache
warming and vec0 index load. All subsequent calls are fast.

## Trade-offs

1. **No reranking** — zmd's `--rerank` flag loads a llama.cpp model for
   cross-encoder scoring. The native path intentionally skips this; 99% of
   queries are keyword-centric and don't benefit from reranking.

2. **FNV-384 vs real embeddings** — Both the native indexer and native query use
   FNV hashing as a vector fallback. This means vector search is effectively a
   locality-sensitive hash lookup, not true semantic search. For Korean legal
   text where queries use exact legal terms, FTS dominates relevance anyway.

3. **Collection filter overhead** — Observed ~800 ms penalty when using
   `--collection` with FTS on large indices. Likely caused by the join pattern;
   a covering index on `documents(collection, id)` may help. Tracked for future
   optimisation.

## Recommendation for Agents

| Scenario | Preferred command |
|----------|-----------------|
| Keyword search (exact law name, case number) | `legal-ko-cli zmd search <q>` |
| General topic discovery | `legal-ko-cli zmd query <q>` |
| Ambiguous semantic query needing best precision | `zmd query <q> --rerank` (subprocess, only when native results are poor) |

Default for LLM agents: **always use native** (`legal-ko-cli zmd search` or
`legal-ko-cli zmd query`). Fall back to subprocess `zmd query --rerank` only
when results seem irrelevant.

## References

- zmd v0.4.1: https://github.com/nicholasgasior/zig-qmd
- 0003: Native zmd Indexer (prerequisite)
- `crates/core/src/native_query.rs`
- `crates/core/src/native_indexer.rs` (`format_query_for_embedding`)
