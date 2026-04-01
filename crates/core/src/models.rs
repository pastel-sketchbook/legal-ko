use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw metadata entry from metadata.json
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct MetadataEntry {
    pub path: String,
    #[serde(rename = "제목")]
    pub title: String,
    #[serde(rename = "법령구분")]
    pub category: String,
    #[serde(rename = "소관부처")]
    pub departments: Vec<String>,
    #[serde(rename = "공포일자")]
    pub promulgation_date: String,
    #[serde(rename = "시행일자")]
    pub enforcement_date: String,
    #[serde(rename = "상태")]
    pub status: String,
}

/// Metadata index: 법령MST → `MetadataEntry`
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
