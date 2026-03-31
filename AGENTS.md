---
description: Rust project conventions for legal-ko.
globs: "*.rs, Cargo.toml, Cargo.lock"
alwaysApply: true
---

# Rust — legal-ko

TUI application to browse, search, and read all Korean laws from the
[legalize-kr](https://github.com/9bow/legalize-kr) GitHub repository.
This is a Rust 2024-edition project. Use `cargo` for all build/test/run tasks.

## Build & Run

- `cargo build` to compile.
- `cargo run` to launch the TUI.
- `cargo test` to run unit tests.
- `cargo clippy` for lints.
- `cargo fmt` to format code.
- `task run` to build debug and run.

## Install (macOS / Apple Silicon)

```sh
cargo build --release
cp target/release/legal-ko ~/bin/legal-ko
```

## Dependencies

Core crates: `ratatui`, `crossterm`, `tokio` (full), `reqwest` (json),
`serde`/`serde_json`/`serde_yaml`, `clap` (derive), `anyhow`, `tracing`,
`tracing-subscriber`, `dirs`, `sha2`, `unicode-width`.

## Architecture

```
src/
  main.rs         — entry point, terminal setup, tokio runtime, event loop
  app.rs          — App state machine (View, InputMode, Popup), Message handling, key dispatch
  bookmarks.rs    — Persist bookmarks to ~/.config/legal-ko/bookmarks.json
  data/
    mod.rs        — re-exports
    models.rs     — LawEntry, LawDetail, ArticleRef, MetadataIndex
    client.rs     — HTTP client (fetch metadata.json, fetch law markdown files)
    parser.rs     — YAML frontmatter stripping, markdown→ratatui Lines, article extraction
    cache.rs      — Disk cache at ~/.cache/legal-ko/ (SHA256 keyed)
  ui/
    mod.rs        — Main render dispatcher, filter popups
    law_list.rs   — Searchable list view with bookmark indicators
    law_detail.rs — Scrollable rendered markdown with article navigation
    styles.rs     — Color constants and style helpers
    help.rs       — Keybinding overlay popup
```

## Key Design Decisions

- **Data source**: Fetches from raw.githubusercontent.com (legalize-kr repo), not a local clone.
- **Async pattern**: `#[tokio::main]`, background tasks for HTTP, `mpsc` channel for messages.
- **Caching**: Individual law files cached to disk; metadata fetched fresh on startup.
- **Vim keybindings**: j/k navigate, `/` search, Enter open, Esc back, n/p article nav, etc.

## Conventions

- Use `anyhow::Result` for all fallible functions.
- Use `tracing::{info, warn, error, debug}` instead of `println!` / `eprintln!`.
- Keep modules small and focused; one responsibility per file.
