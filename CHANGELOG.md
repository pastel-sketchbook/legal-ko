# Changelog

All notable changes to this project are documented in this file.
Generated from conventional commits via `task changelog`.

## [0.4.3] — 2026-04-12

### Added
- add 법조인 search with persistent cached person index
- add 법조인 extraction from precedent documents

### Fixed
- resolve clippy pedantic warnings and structured tracing fields

### Chore
- add CHANGELOG.md generation from git tags via task changelog

## [0.4.2] — 2026-04-12

### Refactored
- extract clear_area_for_popup helper, trim court_name whitespace

### Fixed
- clear CJK artifacts on filter popup borders
- scroll filter popups to selected item

## [0.4.1] — 2026-04-12

### Refactored
- fix clippy pedantic warnings and formatting

## [0.4.0] — 2026-04-12

### Added
- move sort order and theme name from footer to title bar
- inter-link precedent 참조조문 to law articles (r key)
- add TUI precedent views with cache-first loading
- add precedent support with 4-approach cross-reference system

### Changed
- switch precedent metadata from Trees API to metadata.json

### Fixed
- render theme name in accent color in title bar
- widen court column from 6 to 14 in precedent list
- sanitize case names and truncate with ellipsis in precedent views
- truncate precedent detail title to 70 display-width columns
- shorten 'case type' label to 'case' in UI and docs

### Docs
- update rationale with metadata.json and cache-first decisions

## [0.3.5] — 2026-04-10

### Added
- promulgation date enrichment, sort toggle, and detail view metadata

### Fixed
- code quality audit — remove unwrap, eliminate clone, add #[must_use]

## [0.3.4] — 2026-04-04

### Added
- export law markdown (E key), version in title bar, shared UI helpers

## [0.3.3] — 2026-04-04

### Added
- navigate auto-opens law and jumps to article directly
- suspend-and-resume fallback for terminals without split support
- multi-agent facade — support OpenCode, Gemini, Copilot, Amp via picker popup
- OpenCode integration — context sync, navigate command, terminal split

### Fixed
- code quality audit — clippy pedantic, panic hook, Ctrl+C, saturating arithmetic
- clear filters on navigate, enable mouse capture, and fix wrapped-line scroll
- add navigate IPC tracing and sync context after poll_command

### Docs
- update skill to use task install, document suspend fallback

## [0.3.2] — 2026-04-02

### Refactored
- structured tracing, borrow-based rendering, async fs saves & cast guards
- split monolithic app.rs into app/ module directory

### Fixed
- code quality audit — clippy pedantic, arithmetic safety, blocking-in-async, tracing

## [0.3.1] — 2026-04-01

### Fixed
- clear CJK artifacts on article popup border and add search navigation
- adaptive list columns and enhanced footer shortcuts

## [0.3.0] — 2026-04-01

### Added
- migrate to new upstream repo and gate TTS behind feature flag

### Fixed
- code quality audit — clippy pedantic, safety comments, error context

### Docs
- add README, LICENSE (MIT), and legal-ko-search skill

## [0.2.0] — 2026-04-01

### Added
- CLI engine load overlap (C6), ONNX thread tuning (D7), parallel synthesis ADR
- TTS performance enhancements (A2, B3, B4, C5, E8, E9)

### Fixed
- complete code quality audit (clippy pedantic, safety, cleanup)
- TTS code quality cleanup (dead code, state bug, dedup)

## [0.1.0] — 2026-04-01

### Added
- add Meilisearch integration for typo-tolerant ranked search
- article-level batch TTS for smooth full-text playback
- buffer TTS chunks before playback for seamless streaming audio
- streaming TTS playback — audio begins while synthesis continues
- add TTS (text-to-speech) for Korean law reading via vibe-rust
- initial legal-ko TUI app

### Refactored
- migrate to rodio 0.22 and updated vibe-rust API
- migrate to cargo workspace with core, tui, and cli crates

### Fixed
- permanent stdout/stderr suppression for ONNX Runtime C++ threads
- wrap entire spawn_blocking closures with output suppression
- suppress vibe-rust println! output that corrupts ratatui TUI

### Chore
- alphabetize workspace dependencies
- add VERSION file and update version tasks to use it as source of truth
- cargo fmt formatting and gitignore cleanup
