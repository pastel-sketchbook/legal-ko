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
    /// Path-derived law ID (e.g. "kr/민법/법률")
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

/// Convert a `MetadataIndex` into a sorted `Vec<LawEntry>`.
///
/// Entries are sorted by title, then by category for deterministic ordering.
pub fn entries_from_index(index: MetadataIndex) -> Vec<LawEntry> {
    let mut entries: Vec<LawEntry> = index
        .into_iter()
        .map(|(id, meta)| LawEntry {
            id,
            title: meta.title,
            category: meta.category,
            departments: meta.departments,
            enforcement_date: meta.enforcement_date,
            status: meta.status,
            path: meta.path,
        })
        .collect();
    entries.sort_by(|a, b| {
        a.title
            .cmp(&b.title)
            .then_with(|| a.category.cmp(&b.category))
    });
    entries
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entries_from_index_sorted() {
        let mut index = MetadataIndex::new();
        index.insert(
            "kr/형법/법률".to_string(),
            MetadataEntry {
                path: "kr/형법/법률.md".to_string(),
                title: "형법".to_string(),
                category: "법률".to_string(),
                departments: Vec::new(),
                promulgation_date: String::new(),
                enforcement_date: String::new(),
                status: "시행".to_string(),
            },
        );
        index.insert(
            "kr/민법/법률".to_string(),
            MetadataEntry {
                path: "kr/민법/법률.md".to_string(),
                title: "민법".to_string(),
                category: "법률".to_string(),
                departments: Vec::new(),
                promulgation_date: String::new(),
                enforcement_date: String::new(),
                status: "시행".to_string(),
            },
        );

        let entries = entries_from_index(index);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "민법");
        assert_eq!(entries[1].title, "형법");
    }

    #[test]
    fn test_entries_from_index_stable_sort_by_category() {
        let mut index = MetadataIndex::new();
        index.insert(
            "kr/민법/법률".to_string(),
            MetadataEntry {
                path: "kr/민법/법률.md".to_string(),
                title: "민법".to_string(),
                category: "법률".to_string(),
                departments: Vec::new(),
                promulgation_date: String::new(),
                enforcement_date: String::new(),
                status: "시행".to_string(),
            },
        );
        index.insert(
            "kr/민법/대통령령".to_string(),
            MetadataEntry {
                path: "kr/민법/시행령.md".to_string(),
                title: "민법".to_string(),
                category: "대통령령".to_string(),
                departments: Vec::new(),
                promulgation_date: String::new(),
                enforcement_date: String::new(),
                status: "시행".to_string(),
            },
        );

        let entries = entries_from_index(index);
        assert_eq!(entries.len(), 2);
        // Same title, sorted by category
        assert_eq!(entries[0].category, "대통령령");
        assert_eq!(entries[1].category, "법률");
    }

    #[test]
    fn test_entries_from_empty_index() {
        let index = MetadataIndex::new();
        let entries = entries_from_index(index);
        assert!(entries.is_empty());
    }
}
