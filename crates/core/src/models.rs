use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── GitHub Trees API response types ──────────────────────────

/// A single entry from the GitHub Git Trees API response.
#[derive(Debug, Clone, Deserialize)]
pub struct GitTreeEntry {
    pub path: String,
    #[serde(rename = "type")]
    pub entry_type: String,
}

/// Response from `GET /repos/:owner/:repo/git/trees/:sha?recursive=1`.
#[derive(Debug, Clone, Deserialize)]
pub struct GitTreeResponse {
    pub tree: Vec<GitTreeEntry>,
    /// `true` when the tree has too many entries and was truncated.
    #[serde(default)]
    pub truncated: bool,
}

// ── Law metadata types ───────────────────────────────────────

/// Metadata entry for a single law file.
///
/// In the old repo layout this was deserialized directly from `metadata.json`.
/// Now it is constructed from the GitHub tree listing + frontmatter parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetadataEntry {
    pub path: String,
    pub title: String,
    pub category: String,
    pub departments: Vec<String>,
    pub promulgation_date: String,
    pub enforcement_date: String,
    pub status: String,
}

/// Metadata index: law ID → `MetadataEntry`.
///
/// The ID is a path-derived key such as `kr/민법/법률` (the `.md` extension is
/// stripped so the ID remains stable).
pub type MetadataIndex = HashMap<String, MetadataEntry>;

/// A single law entry for display in the list view
#[derive(Debug, Clone, Serialize)]
pub struct LawEntry {
    /// 법령MST identifier
    pub id: String,
    /// Law title (제목)
    pub title: String,
    /// Category (법률, 대통령령, 부령, etc.)
    pub category: String,
    /// Departments (소관부처)
    pub departments: Vec<String>,
    /// Enforcement date
    pub enforcement_date: String,
    /// Status (시행/폐지)
    pub status: String,
    /// Raw file path in the repo
    pub path: String,
}

/// A reference to an article (제X조) within a law document
#[derive(Debug, Clone, Serialize)]
pub struct ArticleRef {
    /// Display label, e.g. "제1조 (목적)"
    pub label: String,
    /// Line index in the parsed content
    pub line_index: usize,
}

/// Parsed law detail content
#[derive(Debug, Clone)]
pub struct LawDetail {
    /// The law entry metadata
    pub entry: LawEntry,
    /// Raw markdown content
    pub raw_markdown: String,
    /// Extracted articles for navigation
    pub articles: Vec<ArticleRef>,
}
