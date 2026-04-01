# 0001 -- Parallel Synthesis for Read-All Mode: Evaluation

**Status:** Deferred
**Date:** 2026-04-01

## Context

legal-ko's read-all mode (`R` key in TUI, `legal-ko-cli speak <id>` without
`--article`) synthesizes all articles of a law sequentially using a single
`RealtimeTts` instance behind `Arc<Mutex<..>>`. The current pipeline:

1. First article: streamed (playback begins after ~1-2s prebuffer)
2. Remaining articles: batch-synthesized one at a time, appended to player
   as each completes (playback of earlier articles overlaps with synthesis
   of later ones)

vibe-rust's `korean_folktale` example demonstrates a multi-worker parallel
synthesis pattern using `Vec<Mutex<RealtimeTts>>` + `rayon::par_iter`. With
3 workers on M4 Pro, the user measured:

- 248.9s total audio from 20 segments
- 388.6s wall-clock (vs 1085.7s sum of generation times)
- **2.79x parallel speedup**, 4.36x average RTF

This ADR evaluates whether adopting that pattern would benefit legal-ko.

## Analysis

### What the folktale pattern does well

The pattern is simple and effective for **batch-to-file** synthesis:

- N independent `RealtimeTts` instances, each with its own 5 ONNX sessions
- `rayon::par_iter` distributes segments across workers with round-robin
  mutex assignment (`pool[i % pool.len()]`)
- Results collected in order, concatenated into a single WAV file
- Near-linear speedup up to 2-3 workers (diminishing returns beyond that
  due to memory bandwidth contention on shared L2/L3 cache)

### Why it doesn't directly apply to legal-ko

**1. legal-ko is streaming-first, not batch-to-file.**

The folktale example synthesizes all segments to disk, then plays the
combined file. legal-ko streams the first article to the audio device
within 1-2 seconds. Adopting a fully parallel batch approach would
*increase* time-to-first-audio — the opposite of what the enhancement
plan optimizes for.

**2. Memory cost is significant.**

Each `RealtimeTts` instance uses ~1.5-2 GB on M4 Pro with the fp16 model.
2 workers = 3-4 GB for TTS alone. legal-ko is a TUI app that should be
lightweight. The folktale demo is a one-shot script where temporary high
memory usage is acceptable.

**3. Playback is inherently sequential.**

Even if articles are synthesized in parallel, they must play back in order.
The bottleneck is not total synthesis throughput — it's that the user can
only listen to one article at a time. The current pipeline already overlaps
synthesis of article N+1 with playback of article N.

**4. Diminishing returns with current architecture.**

The sequential pipeline already gets implicit overlap: while article 3
plays, article 4 synthesizes. Parallel synthesis would help primarily when
synthesis is slower than playback (RTF > 1.0). With the `Fast` profile
(cfg_scale=1.0), RTF approaches 1.0 on Apple Silicon release builds,
narrowing the window where parallelism helps.

**5. Voice preset caching (A1) isn't done yet.**

The enhancement plan explicitly warns: "if you start splitting articles
into smaller segments in legal-ko, repeated preset loads will erase part
of the gain." While A1 (voice_cache) has been implemented in vibe-rust,
multiplying engine instances multiplies the initial voice preset load
cost N-fold (one per worker). This is now cached per-engine, but N
engines still means N independent caches to warm up.

### Where it could theoretically help

- **Very long laws (50+ articles)** where the synthesis pipeline falls far
  behind playback. The user would reach the end of available audio and wait.
- **CLI batch export** (not currently implemented) where writing all articles
  to a single WAV file mirrors the folktale use case exactly.
- **If RTF remains consistently > 1.5** after all other optimizations, meaning
  the sequential pipeline can never keep up with playback.

## Decision

**Defer parallel synthesis for read-all mode.** The enhancement plan's
priority order is correct:

1. ~~A1: Voice preset caching (upstream)~~ Done
2. ~~A2: Fast/Balanced profiles~~ Done
3. ~~B3: Streaming single-article reads~~ Done
4. ~~B4: Hybrid streaming+batch for read-all~~ Done
5. ~~C5: Prewarm engine~~ Done
6. ~~C6: CLI engine load overlap~~ Done
7. ~~D7: ONNX thread tuning~~ Done
8. ~~E8/E9: Prebuffer + clone cleanup~~ Done

All plan items are now complete. Parallel synthesis should only be
reconsidered if:

- Release-build RTF is consistently > 1.0 on target Apple Silicon after
  D7 thread tuning
- CPU utilization stays surprisingly low (suggesting a single ONNX instance
  isn't saturating available cores)
- A CLI batch-export feature is added where the folktale pattern maps
  directly

## Consequences

- legal-ko stays with a single `RealtimeTts` instance
- `rayon` is not added as a dependency
- The streaming-first architecture is preserved (low time-to-first-audio)
- Memory footprint stays at ~1.5-2 GB (one engine instance)
- If parallel synthesis is later desired, the `_with_handle` API introduced
  in C6 makes it straightforward: create N handles, load N engines, and
  distribute segments across them
