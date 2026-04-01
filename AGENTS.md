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
| `legal-ko-core` | lib | — | Shared logic: models, HTTP client, caching, parser, bookmarks, preferences |
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
`tracing-subscriber`, `dirs`, `sha2`, `unicode-width`,
`meilisearch-sdk` (optional, behind `meilisearch` feature).

## Architecture

```
crates/
  core/src/
    lib.rs          — re-exports all modules
    models.rs       — LawEntry, LawDetail, ArticleRef, MetadataIndex, MetadataEntry
    client.rs       — HTTP client (fetch metadata.json, fetch law markdown files)
    parser.rs       — YAML frontmatter stripping, article extraction (no ratatui dep)
    cache.rs        — Disk cache at ~/.cache/legal-ko/ (SHA256 keyed)
    bookmarks.rs    — Persist bookmarks to ~/.config/legal-ko/bookmarks.json
    preferences.rs  — Theme preference persistence to ~/.config/legal-ko/preferences.json
    search.rs       — Meilisearch integration (feature-gated), naive fallback search
  tui/src/
    main.rs         — entry point, terminal setup, tokio runtime, event loop
    app.rs          — App state machine (View, InputMode, Popup), Message handling
    theme.rs        — 14 semantic themes (7 dark + 7 light), Theme struct, THEMES array
    parser.rs       — markdown→ratatui Lines with theme colors, inline bold parsing
    ui/
      mod.rs        — Main render dispatcher, filter popups
      law_list.rs   — Searchable list view with bookmark indicators, unicode column alignment
      law_detail.rs — Scrollable rendered markdown with article navigation
      styles.rs     — Badge-style key hints, status bar helpers
      help.rs       — Keybinding overlay popup
  cli/src/
    main.rs         — clap subcommands: list, search, show, articles, bookmarks, speak (all with --json)
```

## CLI Subcommands

- `legal-ko-cli list [--category X] [--department X] [--bookmarks] [--json] [--limit N]`
- `legal-ko-cli search <query> [--json] [--limit N]`
- `legal-ko-cli show <id> [--json]`
- `legal-ko-cli articles <id> [--json]`
- `legal-ko-cli bookmarks [--json]`
- `legal-ko-cli speak <id> [--article N] [--voice X] [--json]`

## Key Design Decisions

- **Data source**: Fetches from raw.githubusercontent.com (legalize-kr repo), not a local clone.
- **Async pattern**: `#[tokio::main]`, background tasks for HTTP, `mpsc` channel for messages.
- **Caching**: Individual law files cached to disk; metadata fetched fresh on startup.
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
