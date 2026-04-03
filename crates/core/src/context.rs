//! Shared TUI context written to `~/.cache/legal-ko/context.json`.
//!
//! The TUI writes this file on every navigation event so that external tools
//! (e.g. `legal-ko-cli context`) can read the user's current browsing state.

use anyhow::{Context as _, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::models::{ArticleRef, LawEntry};

// ── Persisted context (serialized to JSON) ───────────────────

/// Full context snapshot written by the TUI.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiContext {
    /// Current view: `"loading"`, `"list"`, or `"detail"`.
    pub view: String,
    /// ISO 8601 timestamp of the last update.
    pub timestamp: String,
    /// The law currently highlighted in the list (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub selected_law: Option<SelectedLaw>,
    /// Active list-view filters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filters: Option<Filters>,
    /// Detail-view context (only present when viewing a law).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<DetailContext>,
}

/// Summary of the currently highlighted law entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedLaw {
    pub id: String,
    pub title: String,
    pub category: String,
    pub departments: Vec<String>,
}

/// Active filters and list statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filters {
    pub search_query: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub department: Option<String>,
    pub bookmarks_only: bool,
    pub total_laws: usize,
    pub filtered_count: usize,
}

/// Detail-view browsing state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetailContext {
    pub law_id: String,
    pub law_title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current_article: Option<CurrentArticle>,
    pub total_articles: usize,
    pub scroll_position: usize,
    pub total_lines: usize,
}

/// The article the user is currently reading.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrentArticle {
    pub index: usize,
    pub label: String,
}

// ── Snapshot input (TUI → context builder) ───────────────────

/// Plain-data snapshot the TUI passes to [`build_and_write`].
///
/// Keeps all builder logic inside `context.rs` while the TUI only
/// has to collect its own fields.
pub struct Snapshot<'a> {
    /// `"loading"`, `"list"`, or `"detail"`.
    pub view: &'a str,
    /// Currently highlighted law entry (list view selection).
    pub selected_entry: Option<&'a LawEntry>,
    /// Search query string (may be empty).
    pub search_query: &'a str,
    /// Active category filter.
    pub category_filter: Option<&'a str>,
    /// Active department filter.
    pub department_filter: Option<&'a str>,
    /// Whether only bookmarks are shown.
    pub bookmarks_only: bool,
    /// Total number of laws loaded.
    pub total_laws: usize,
    /// Number of laws after filtering.
    pub filtered_count: usize,
    /// Detail-view law entry (when in detail view).
    pub detail_entry: Option<&'a LawEntry>,
    /// All articles in the detail view.
    pub detail_articles: &'a [ArticleRef],
    /// Current scroll position (line index) in detail view.
    pub detail_scroll: usize,
    /// Total rendered lines in detail view.
    pub detail_lines_count: usize,
}

// ── Builder ──────────────────────────────────────────────────

/// Build a [`TuiContext`] from the given snapshot and write it to disk.
pub fn build_and_write(snap: &Snapshot<'_>) -> Result<()> {
    let selected_law = snap.selected_entry.map(|e| SelectedLaw {
        id: e.id.clone(),
        title: e.title.clone(),
        category: e.category.clone(),
        departments: e.departments.clone(),
    });

    let filters = if snap.view == "list" || snap.view == "detail" {
        Some(Filters {
            search_query: snap.search_query.to_string(),
            category: snap.category_filter.map(String::from),
            department: snap.department_filter.map(String::from),
            bookmarks_only: snap.bookmarks_only,
            total_laws: snap.total_laws,
            filtered_count: snap.filtered_count,
        })
    } else {
        None
    };

    let detail = if snap.view == "detail" {
        snap.detail_entry.map(|e| {
            // Find the article the user is currently reading based on scroll.
            let current_article = snap
                .detail_articles
                .iter()
                .enumerate()
                .rev()
                .find(|(_, art)| art.line_index <= snap.detail_scroll)
                .map(|(i, art)| CurrentArticle {
                    index: i,
                    label: art.label.clone(),
                });

            DetailContext {
                law_id: e.id.clone(),
                law_title: e.title.clone(),
                current_article,
                total_articles: snap.detail_articles.len(),
                scroll_position: snap.detail_scroll,
                total_lines: snap.detail_lines_count,
            }
        })
    } else {
        None
    };

    let ctx = TuiContext {
        view: snap.view.to_string(),
        timestamp: Utc::now().to_rfc3339(),
        selected_law,
        filters,
        detail,
    };

    write_context(&ctx)
}

// ── I/O ──────────────────────────────────────────────────────

/// Path to the context file: `~/.cache/legal-ko/context.json`.
fn context_path() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("Cannot determine cache directory")?
        .join("legal-ko");
    Ok(dir.join("context.json"))
}

/// Write the context snapshot to disk (synchronous, atomic via rename).
pub fn write_context(ctx: &TuiContext) -> Result<()> {
    let path = context_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(ctx).context("Failed to serialize context")?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &json).with_context(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("Failed to rename to {}", path.display()))?;
    Ok(())
}

/// Read the current context from disk.
pub fn read_context() -> Result<TuiContext> {
    let path = context_path()?;
    let json = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read {}", path.display()))?;
    let ctx: TuiContext = serde_json::from_str(&json).context("Failed to parse context.json")?;
    Ok(ctx)
}

// ── Commands (OpenCode → TUI) ────────────────────────────────

/// A command written by an external tool (e.g. `legal-ko-cli navigate`)
/// and consumed by the TUI on the next event-loop tick.
///
/// Behaviour is context-aware:
/// - **List view**: scrolls to and highlights the law matching `law_id`.
/// - **Detail view (same law)**: jumps to the article matching `article` (if set).
/// - **Detail view (different law)**: returns to list and highlights `law_id`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TuiCommand {
    /// The action to perform: `"navigate"`.
    pub action: String,
    /// Target law ID (e.g. `"kr/민법/법률"`).
    pub law_id: String,
    /// Optional article label to jump to when in detail view
    /// (e.g. `"제3조"` or `"제3조 (대항력 등)"`).
    /// Matched as a prefix against article labels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub article: Option<String>,
    /// ISO 8601 timestamp when the command was written.
    pub timestamp: String,
}

/// Path to the command file: `~/.cache/legal-ko/command.json`.
fn command_path() -> Result<PathBuf> {
    let dir = dirs::cache_dir()
        .context("Cannot determine cache directory")?
        .join("legal-ko");
    Ok(dir.join("command.json"))
}

/// Write a command for the TUI to consume (atomic via rename).
pub fn write_command(cmd: &TuiCommand) -> Result<()> {
    let path = command_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cmd).context("Failed to serialize command")?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, &json).with_context(|| format!("Failed to write {}", tmp.display()))?;
    std::fs::rename(&tmp, &path)
        .with_context(|| format!("Failed to rename to {}", path.display()))?;
    debug!(
        action = cmd.action,
        law_id = cmd.law_id,
        path = %path.display(),
        "write_command: wrote command file"
    );
    Ok(())
}

/// Read and remove the pending command file (returns `None` if absent).
pub fn take_command() -> Option<TuiCommand> {
    let path = command_path().ok()?;
    let json = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return None, // No command file — normal case, don't log
    };
    // Remove first so a crash doesn't re-process the same command.
    let _ = std::fs::remove_file(&path);
    match serde_json::from_str::<TuiCommand>(&json) {
        Ok(cmd) => {
            debug!(
                action = cmd.action,
                law_id = cmd.law_id,
                article = ?cmd.article,
                "take_command: consumed command file"
            );
            Some(cmd)
        }
        Err(e) => {
            warn!(error = %e, "take_command: failed to parse command.json");
            None
        }
    }
}
