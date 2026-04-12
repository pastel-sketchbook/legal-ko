# 0002 -- TUI Precedent Views: Design Rationale

**Status:** Implemented
**Date:** 2026-04-12

## Context

legal-ko started as a TUI/CLI for browsing Korean statutes from the
[legalize-kr](https://github.com/legalize-kr/legalize-kr) repository (~2,400
law files). In v0.4.0, the scope expanded to include 123,000+ court precedents
from the [precedent-kr](https://github.com/legalize-kr/precedent-kr) repository,
along with a 4-approach cross-reference system linking precedents to law articles.

The core crate and CLI were completed first (models, client, parser, crossref,
6 CLI subcommands). This rationale covers the design decisions for adding
precedent browsing views to the TUI.

## Decision: Parallel Data Model, Not Unified

### Option A: Unified view with a type toggle
A single list view that can show either laws or precedents, sharing `list_selected`,
`filtered_indices`, `search_query`, etc. Switch between types via a mode flag.

### Option B: Parallel fields on App struct
Separate `all_precedents`, `precedent_filtered_indices`, `precedent_list_selected`,
`precedent_search_query`, etc. alongside the existing law fields. Each view has
its own independent state.

**Chose B.** Rationale:

1. **State preservation.** Switching between law list and precedent list with Tab
   preserves each view's scroll position, search query, and active filters. A
   unified model would require saving and restoring state on every switch.

2. **No type gymnastics.** Laws and precedents have different schemas (`LawEntry`
   vs `PrecedentEntry`), different filter dimensions (category/department vs
   case_type/court), and different detail structures (articles vs sections). A
   unified model would require either trait objects or an enum wrapper, adding
   complexity with no real benefit.

3. **Consistency with the core crate.** The core already has separate types
   (`PrecedentEntry`, `PrecedentDetail`, `PrecedentSectionRef`) parallel to
   (`LawEntry`, `LawDetail`, `ArticleRef`). The TUI mirrors this structure.

4. **Independent loading.** Law metadata and precedent metadata are fetched in
   parallel from different GitHub repos. With separate fields, each loads and
   populates independently. The law list appears first (smaller repo, faster);
   the precedent list becomes available when its metadata arrives. A unified
   model would need to coordinate completion of both before showing anything.

The cost is ~19 additional fields on `App`. This is acceptable for a TUI
application where the struct is instantiated once.

## Decision: Tab Key for View Switching

Considered: number keys (1/2), dedicated letter (P), function keys, or Tab.

**Chose Tab/BackTab.** Rationale:

- Tab is the standard "switch tab" key in terminal applications (browsers, tmux
  window cycling, vim `:tabn`).
- It doesn't conflict with any existing keybinding.
- BackTab (Shift+Tab) also works, matching the bidirectional expectation.
- Simple discovery: the footer bar shows `Tab precedents` in law list view and
  `Tab laws` in precedent list view.

## Decision: Reuse n/p for Section Navigation

In law detail view, `n`/`p` navigate between articles (제X조). In precedent
detail view, the same keys navigate between sections (판시사항, 판결요지, etc.).

**Rationale:** Muscle memory transfer. The semantic meaning is "next/previous
navigable landmark in the document." The landmark type differs (articles vs
sections), but the user intent is identical. Similarly, `a` opens the article
list popup in law detail and the section list popup in precedent detail.

## Decision: c/d Filter Keys Reused Per View

In law list: `c` = category filter, `d` = department filter.
In precedent list: `c` = case type filter, `d` = court filter.

**Rationale:** The keys map to the semantic role (primary filter, secondary
filter) rather than the specific field name. Users in the precedent view never
need category/department filters, and vice versa. Context-dependent keybindings
keep the mapping compact without adding new letters.

## Decision: Parallel Metadata Fetch in start_loading()

Both `fetch_metadata` (law) and `fetch_precedent_metadata` (precedent) are
spawned as independent `tokio::spawn` tasks on startup.

**Rationale:**

- The law list view appears as soon as law metadata arrives (existing behaviour
  preserved). The precedent Tab shows "loading..." until its metadata arrives.
- If precedent metadata fails, the law list still works. The error is logged
  but not surfaced as a blocking error.

## Decision: metadata.json Instead of GitHub Trees API

The precedent-kr repository provides a pre-built `metadata.json` (123K entries)
alongside the individual `.md` files. We fetch this instead of using the GitHub
Git Trees API (`/git/trees/main?recursive=1`).

**Comparison:**

| | Trees API | metadata.json |
|---|---|---|
| Response size | 19 MB raw JSON | ~3 MB (gzip transfer) |
| Network time | ~5 seconds | ~2 seconds |
| Fields populated | path only | all (case name, date, court, case type) |
| Case names in list | Empty until opened | Shown immediately |
| API rate limits | Yes (60/hr unauthed) | No (CDN, no rate limit) |

**Rationale:**

- **Speed.** The Trees API response is 19 MB of JSON with 123K tree entries,
  most of which aren't even `.md` files (directories, LICENSE, etc.). The
  metadata.json file is 35 MB uncompressed but only ~3 MB over the wire
  (gzip), and the CDN serves it faster than the API endpoint.

- **Richness.** The Trees API only provides file paths. Case names, case
  numbers, ruling dates, court names, and case types are absent — they had
  to be populated lazily from YAML frontmatter when each precedent was opened.
  The metadata.json provides all fields pre-extracted, so the precedent list
  view shows full case names and dates immediately.

- **No API rate limits.** `raw.githubusercontent.com` serves static files from
  a CDN with no GitHub API rate limiting. The Trees API is rate-limited to
  60 requests/hour for unauthenticated clients, which matters for users who
  don't set `GITHUB_TOKEN`.

- **Simpler parsing.** No need to filter blobs, validate directory structure,
  or reconstruct metadata from path segments. The JSON maps directly to our
  `PrecedentMetadataEntry` struct via a `RawPrecedentMeta` intermediary with
  `#[serde(rename)]` for Korean field names.

## Decision: Cache-First Loading for Precedent Metadata

On startup, precedent metadata follows a cache-first strategy:

1. A `spawn_blocking` task reads `~/.cache/legal-ko/precedent_meta.json`
   (blocking I/O, ~10ms for 123K entries) and sends `PrecedentMetadataCached`
   if the cache exists and hasn't expired (24-hour TTL).

2. Regardless of cache hit/miss, a parallel async task fetches fresh
   metadata.json from GitHub and sends `PrecedentMetadataLoaded`.

3. On receiving `PrecedentMetadataCached`: if fresh data hasn't arrived yet,
   the cached index is loaded into `App` state, making Tab work instantly.

4. On receiving `PrecedentMetadataLoaded`: the fresh data replaces whatever
   was loaded (cache or nothing), and is written back to disk cache via
   `spawn_blocking`.

**Rationale:**

- **Instant second launch.** Without caching, every launch waits ~2 seconds
  for the metadata.json download. With caching, the precedent list is
  populated within milliseconds of startup, and the background refresh
  ensures data stays current.

- **Same pattern as enrichment cache.** The existing law enrichment cache
  (`~/.cache/legal-ko/enriched.json`) follows the same read/write/TTL
  pattern. Reusing the approach keeps the codebase consistent.

- **Graceful degradation.** If the cache is missing or expired, the user
  sees "loading..." until the network fetch completes — same as before
  caching was added. If the network fetch fails but cache exists, the
  stale data still works (within TTL).

## Decision: No Bookmarks for Precedents (Yet)

The existing bookmark system (`~/.config/legal-ko/bookmarks.json`) stores law
IDs. Precedent bookmarks were deliberately omitted from this implementation.

**Rationale:**

- The bookmark file format would need to distinguish law IDs from precedent IDs,
  or use separate storage. Adding this without careful design could break
  existing bookmarks.
- The primary precedent use case is search-and-read via the cross-reference
  system, not curating a personal collection.
- Can be added later as a non-breaking enhancement if demand emerges.

## Decision: Context Sync for Precedent Views

`sync_context()` now writes `"precedent_list"` and `"precedent_detail"` as view
strings. The existing `context.json` schema (used by `legal-ko-cli context` and
OpenCode integration) includes these new values.

The `Snapshot` struct was not extended with precedent-specific fields. The
context file currently only includes law-focused data (selected_law, filters,
detail). Extending it for precedent context would require schema changes to
`legal-ko-core/context.rs` and is deferred until an agent actually needs to
read precedent browsing state.

## Consequences

- The TUI now has 5 views: Loading, List, Detail, PrecedentList, PrecedentDetail
- 3 new popup types: SectionList, CaseTypeFilter, CourtFilter
- Tab switches between law list and precedent list (state preserved both ways)
- Precedent metadata loads in parallel; the law list is not delayed
- Precedent list shows full case names and dates immediately (no lazy enrichment)
- First launch: ~2s for metadata.json download; second launch: instant from cache
- The keybinding surface is compact: same keys, different semantics per view
- `App` struct grows by 19 fields (~152 bytes stack + heap allocations)
- All 65 tests pass, zero clippy warnings, clean build
