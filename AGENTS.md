---
description: Rust project conventions for legal-ko.
globs: "*.rs, Cargo.toml, Cargo.lock"
alwaysApply: true
---

# Rust — legal-ko

Cargo workspace to browse, search, and read all Korean laws from the
[legalize-kr](https://github.com/legalize-kr/legalize-kr) GitHub repository.
This is a Rust 2024-edition workspace with three crates.
Use `cargo` for all build/test/run tasks.

## Workspace Crates

| Crate | Type | Binary | Purpose |
|-------|------|--------|---------|
| `legal-ko-core` | lib | — | Shared logic: models, HTTP client, caching, parser, crossref, person index, bookmarks, context, preferences, zmd collection management, AI agent definitions |
| `legal-ko-tui` | bin | `legal-ko` | Human-facing ratatui TUI |
| `legal-ko-cli` | bin | `legal-ko-cli` | LLM-facing CLI with `--json` output |

## Build & Run

- `cargo build --workspace` to compile all crates.
- `cargo build -p legal-ko-tui` to build only the TUI.
- `cargo build -p legal-ko-cli` to build only the CLI.
- `cargo test --workspace` to run all tests.
- `cargo clippy --workspace` for lints.
- `cargo fmt --all` to format code.
- `task run` to build **release** and run TUI.
- `task run:cli -- <subcommand>` to build **release** and run CLI.
- `task run:dev` / `task run:cli:dev` for debug builds (fast compile, slow TTS).

## Install (macOS / Apple Silicon)

```sh
cargo build --workspace --release
cp target/release/legal-ko ~/bin/legal-ko
cp target/release/legal-ko-cli ~/bin/legal-ko-cli
```

## Dependencies

Workspace deps: `ratatui`, `crossterm`, `tokio` (full), `reqwest` (json),
`serde`/`serde_json`, `clap` (derive), `anyhow`, `tracing`,
`tracing-subscriber`, `dirs`, `sha2`, `unicode-width`, `futures`, `rayon`,
`indicatif`, `meilisearch-sdk` (optional, behind `meilisearch` feature).

## Architecture

```
crates/
  core/src/
    lib.rs          — re-exports all modules, AiAgent struct + AGENTS constant
    models.rs       — LawEntry, LawDetail, ArticleRef, MetadataIndex, MetadataEntry, PrecedentEntry, PrecedentMetadataEntry, PrecedentSortOrder, PersonRole, PersonRef
    client.rs       — HTTP client (fetch metadata.json, fetch law/precedent markdown files)
    parser.rs       — YAML frontmatter stripping, article extraction, precedent section extraction, 법조인 extraction (no ratatui dep)
    crossref.rs     — 4-approach cross-reference: statute refs, case refs, fuzzy law-name matching, case-type affinity
    cache.rs        — Disk cache at ~/.cache/legal-ko/ (SHA256 keyed)
    person_index.rs — Persistent person (법조인) index: concurrent build via buffer_unordered(50), cached to ~/.cache/legal-ko/person_index.json, instant repeat lookups
    bookmarks.rs    — Persist bookmarks to ~/.config/legal-ko/bookmarks.json
    context.rs      — TUI↔Agent context (TuiContext, TuiCommand, read/write/take)
    preferences.rs  — Theme & agent preference persistence to ~/.config/legal-ko/preferences.json
    search.rs       — Meilisearch integration (feature-gated), naive fallback search
    zmd.rs          — zmd collection management: clone repos, stage files via hardlinks (Rayon parallel), invoke zmd CLI for indexing
  tui/src/
    main.rs         — entry point, terminal setup, tokio runtime, event loop
    app/
      mod.rs        — App state machine (View, InputMode, Popup), Message handling, agent split, context sync
      navigation.rs — List/detail/article navigation methods
      filters.rs    — Category/department/bookmark filter logic
    theme.rs        — 14 semantic themes (7 dark + 7 light), Theme struct, THEMES array
    parser.rs       — markdown→ratatui Lines with theme colors, inline bold parsing
    ui/
      mod.rs        — Main render dispatcher, filter popups, agent picker popup
      law_list.rs   — Searchable list view with bookmark indicators, unicode column alignment
      law_detail.rs — Scrollable rendered markdown with article navigation
      styles.rs     — Badge-style key hints, status bar helpers
      help.rs       — Keybinding overlay popup
  cli/src/
    main.rs         — clap subcommands: list, search, show, articles, bookmarks, context, navigate, speak, precedent-list, precedent-search, precedent-show, precedent-sections, precedent-persons, precedent-search-person, precedent-laws, law-precedents, zmd (all with --json)
```

## CLI Subcommands

- `legal-ko-cli list [--category X] [--department X] [--bookmarks] [--json] [--limit N]`
- `legal-ko-cli search <query> [--json] [--limit N]`
- `legal-ko-cli show <id> [--json]`
- `legal-ko-cli articles <id> [--json]`
- `legal-ko-cli bookmarks [--json]`
- `legal-ko-cli context [--json]`
- `legal-ko-cli navigate <id> [--article X] [--json]`
- `legal-ko-cli speak <id> [--article N] [--voice X] [--json]`
- `legal-ko-cli precedent-list [--case-type X] [--court X] [--sort name|date] [--json] [--limit N]`
- `legal-ko-cli precedent-search <query> [--json] [--limit N]` — search by case name/number; auto-falls back to 법조인 search if query looks like a Korean name and no metadata matches
- `legal-ko-cli precedent-show <id> [--json]`
- `legal-ko-cli precedent-sections <id> [--json]`
- `legal-ko-cli precedent-laws <id> [--json]` — cross-reference: find laws cited by a precedent (4-approach fallback)
- `legal-ko-cli law-precedents <law_name> [--article X] [--json] [--limit N]` — reverse: find precedents citing a law
- `legal-ko-cli precedent-persons <id> [--json]` — extract 법조인 (judges, attorneys, prosecutors) from a precedent
- `legal-ko-cli precedent-search-person <name> [--role judge|attorney|prosecutor] [--case-type X] [--court X] [--json] [--limit N]` — search precedents by 법조인 name; uses cached person index (~/.cache/legal-ko/person_index.json) for instant repeat lookups, builds index concurrently on first run
- `legal-ko-cli zmd laws [--json]` — clone/pull legalize-kr, stage 법률.md files via hardlinks, run `zmd update`
- `legal-ko-cli zmd precedents [--case-type X] [--court X] [--json]` — clone/pull precedent-kr, stage precedent files, run `zmd update`
- `legal-ko-cli zmd all [--json]` — run both laws and precedents phases
- `legal-ko-cli zmd sync [--json]` — pull latest from upstream repos and re-index
- `legal-ko-cli zmd status [--json]` — show repos, staged files, zmd collections
- `legal-ko-cli zmd reset [--json]` — remove collections and staged data (keeps repo clones)
- `legal-ko-cli zmd query <query> [--collection X] [--no-vector] [--limit N] [--json]` — native hybrid search (FTS5 + vec0 + RRF), ~410 ms
- `legal-ko-cli zmd search <query> [--collection X] [--limit N] [--json]` — native FTS-only search, ~10 ms
- `legal-ko-cli zmd similar <query> [--precedent-limit N] [--law-limit N] [--no-vector] [--json]` — query → precedents → cited laws in one call

## Key Design Decisions

- **Data source**: Fetches from raw.githubusercontent.com (legalize-kr repo), not a local clone.
- **Async pattern**: `#[tokio::main]`, background tasks for HTTP, `mpsc` channel for messages.
- **Caching**: Individual law files cached to disk; metadata fetched fresh on startup. Person index cached to `~/.cache/legal-ko/person_index.json` (7-day TTL, rebuilt if precedent count grows >5%).
- **Person search**: Uses `futures::stream::buffer_unordered(50)` for concurrent document fetching during index build. First run scans all 123K+ precedents (~3 min); subsequent searches are instant (HashMap lookup from cached index).
- **Vim keybindings**: j/k navigate, `/` search, Enter open, Esc back, n/p article nav, etc.
- **Theme system**: 14 themes with persistence, `t` key cycles, semantic color fields.
- **Core/TUI split**: Parser split — core has `strip_frontmatter` + `extract_articles` (pure text), TUI has `parse_law_markdown` (ratatui Lines with theme colors).
- **Search**: Optional Meilisearch backend (feature `meilisearch`), configured via `LEGAL_KO_MEILI_URL`, `LEGAL_KO_MEILI_KEY`, `LEGAL_KO_MEILI_INDEX` env vars. Falls back to naive title substring search when Meilisearch is unavailable.

## Conventions

- Use `anyhow::Result` for all fallible functions.
- Use `tracing::{info, warn, error, debug}` instead of `println!` / `eprintln!`.
- Keep modules small and focused; one responsibility per file.
- Do NOT use `serde_yaml` — it's deprecated. Frontmatter stripping uses plain string slicing.
- All workspace commands use `--workspace` flag.
