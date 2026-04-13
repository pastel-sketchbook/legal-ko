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

/// Sort order for law entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortOrder {
    /// Sort by title (Korean alphabetical), then category.
    #[default]
    Title,
    /// Sort by promulgation date (newest first), then title.
    PromulgationDate,
}

impl SortOrder {
    /// Cycle to the next sort order.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            SortOrder::Title => SortOrder::PromulgationDate,
            SortOrder::PromulgationDate => SortOrder::Title,
        }
    }

    /// Human-readable label for the current sort order.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            SortOrder::Title => "title",
            SortOrder::PromulgationDate => "promulgation date",
        }
    }
}

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
    /// Promulgation date (공포일자)
    pub promulgation_date: String,
    /// Enforcement date (시행일자)
    pub enforcement_date: String,
    /// Status (시행/폐지)
    pub status: String,
    /// Raw file path in the repo
    pub path: String,
}

/// Convert a `MetadataIndex` into a sorted `Vec<LawEntry>`.
///
/// Entries are sorted by title, then by category for deterministic ordering.
#[must_use]
pub fn entries_from_index(index: MetadataIndex) -> Vec<LawEntry> {
    let mut entries: Vec<LawEntry> = index
        .into_iter()
        .map(|(id, meta)| LawEntry {
            id,
            title: meta.title,
            category: meta.category,
            departments: meta.departments,
            promulgation_date: meta.promulgation_date,
            enforcement_date: meta.enforcement_date,
            status: meta.status,
            path: meta.path,
        })
        .collect();
    sort_entries(&mut entries, SortOrder::Title);
    entries
}

/// Sort entries in-place according to the given sort order.
pub fn sort_entries(entries: &mut [LawEntry], order: SortOrder) {
    match order {
        SortOrder::Title => {
            entries.sort_by(|a, b| {
                a.title
                    .cmp(&b.title)
                    .then_with(|| a.category.cmp(&b.category))
            });
        }
        SortOrder::PromulgationDate => {
            entries.sort_by(|a, b| {
                // Descending date (newest first); empty dates sort last.
                let da = if a.promulgation_date.is_empty() {
                    ""
                } else {
                    &a.promulgation_date
                };
                let db = if b.promulgation_date.is_empty() {
                    ""
                } else {
                    &b.promulgation_date
                };
                db.cmp(da).then_with(|| a.title.cmp(&b.title))
            });
        }
    }
}

// ── Precedent (판례) types ────────────────────────────────────

/// Sort order for precedent entries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrecedentSortOrder {
    /// Sort by case name (사건명), then case number.
    #[default]
    CaseName,
    /// Sort by ruling date (선고일자, newest first), then case name.
    RulingDate,
}

impl PrecedentSortOrder {
    /// Cycle to the next sort order.
    #[must_use]
    pub fn next(self) -> Self {
        match self {
            PrecedentSortOrder::CaseName => PrecedentSortOrder::RulingDate,
            PrecedentSortOrder::RulingDate => PrecedentSortOrder::CaseName,
        }
    }

    /// Human-readable label for the current sort order.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            PrecedentSortOrder::CaseName => "case name",
            PrecedentSortOrder::RulingDate => "ruling date",
        }
    }
}

/// Raw JSON shape from `precedent-kr/metadata.json` (Korean field names).
#[derive(Debug, Clone, Deserialize)]
pub struct RawPrecedentMeta {
    pub path: String,
    #[serde(rename = "사건명", default)]
    pub case_name: String,
    #[serde(rename = "사건번호", default)]
    pub case_number: String,
    #[serde(rename = "선고일자", default)]
    pub ruling_date: String,
    #[serde(rename = "법원명", default)]
    pub court_name: String,
    #[serde(rename = "사건종류", default)]
    pub case_type: String,
    #[serde(rename = "판결유형", default)]
    pub ruling_type: String,
}

/// Raw metadata index from `precedent-kr/metadata.json`.
///
/// Keys are serial numbers (판례일련번호); values are `RawPrecedentMeta`.
pub type RawPrecedentMetadataIndex = HashMap<String, RawPrecedentMeta>;

/// Metadata entry for a single precedent file.
///
/// Constructed from `metadata.json` in the precedent-kr repository.
/// All fields are populated at load time — no lazy frontmatter fetching needed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrecedentMetadataEntry {
    /// Raw file path in the repo (e.g. "민사/대법원/2000다10048.md")
    pub path: String,
    /// Case name (사건명)
    pub case_name: String,
    /// Case number (사건번호, e.g. "2000다10048")
    pub case_number: String,
    /// Ruling date (선고일자, e.g. "2003-11-14")
    pub ruling_date: String,
    /// Court name (법원명, e.g. "대법원")
    pub court_name: String,
    /// Case type (사건종류, e.g. "민사", "형사")
    pub case_type: String,
    /// Ruling type (판결유형)
    pub ruling_type: String,
}

/// Metadata index for precedents: stable ID → `PrecedentMetadataEntry`.
///
/// The ID is derived from the path, e.g. `민사/대법원/2000다10048`.
pub type PrecedentMetadataIndex = HashMap<String, PrecedentMetadataEntry>;

/// A single precedent entry for display in list views.
#[derive(Debug, Clone, Serialize)]
pub struct PrecedentEntry {
    /// Path-derived ID (e.g. "민사/대법원/2000다10048")
    pub id: String,
    /// Case name (사건명)
    pub case_name: String,
    /// Case number (사건번호)
    pub case_number: String,
    /// Ruling date (선고일자)
    pub ruling_date: String,
    /// Court name (법원명)
    pub court_name: String,
    /// Case type (사건종류)
    pub case_type: String,
    /// Ruling type (판결유형)
    pub ruling_type: String,
    /// Raw file path in the repo
    pub path: String,
}

/// Role of a person referenced in a precedent document.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonRole {
    /// 대법관 / 판사 — judge (presiding, associate, etc.)
    Judge,
    /// 변호사 — attorney / counsel
    Attorney,
    /// 검사 — prosecutor
    Prosecutor,
}

impl std::fmt::Display for PersonRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Judge => write!(f, "judge"),
            Self::Attorney => write!(f, "attorney"),
            Self::Prosecutor => write!(f, "prosecutor"),
        }
    }
}

/// A person referenced in a precedent document (judge, attorney, or prosecutor).
#[derive(Debug, Clone, Serialize)]
pub struct PersonRef {
    /// The person's name (Korean)
    pub name: String,
    /// Their role in the case
    pub role: PersonRole,
    /// Optional qualifier (e.g. "재판장", "주심")
    pub qualifier: Option<String>,
}

/// A reference to a named section within a precedent document.
#[derive(Debug, Clone, Serialize)]
pub struct PrecedentSectionRef {
    /// Section heading (e.g. "판시사항", "판결요지", "판례내용")
    pub label: String,
    /// Line index in the stripped content
    pub line_index: usize,
}

/// Parsed precedent detail content.
#[derive(Debug, Clone)]
pub struct PrecedentDetail {
    /// The precedent entry metadata
    pub entry: PrecedentEntry,
    /// Raw markdown content
    pub raw_markdown: String,
    /// Extracted sections for navigation
    pub sections: Vec<PrecedentSectionRef>,
}

/// Convert a `PrecedentMetadataIndex` into a sorted `Vec<PrecedentEntry>`.
///
/// Entries are sorted by case name, then by case number for deterministic ordering.
#[must_use]
pub fn precedent_entries_from_index(index: PrecedentMetadataIndex) -> Vec<PrecedentEntry> {
    let mut entries: Vec<PrecedentEntry> = index
        .into_iter()
        .map(|(id, meta)| PrecedentEntry {
            id,
            case_name: meta.case_name,
            case_number: meta.case_number,
            ruling_date: meta.ruling_date,
            court_name: meta.court_name,
            case_type: meta.case_type,
            ruling_type: meta.ruling_type,
            path: meta.path,
        })
        .collect();
    sort_precedent_entries(&mut entries, PrecedentSortOrder::CaseName);
    entries
}

/// Sort precedent entries in-place according to the given sort order.
pub fn sort_precedent_entries(entries: &mut [PrecedentEntry], order: PrecedentSortOrder) {
    match order {
        PrecedentSortOrder::CaseName => {
            entries.sort_by(|a, b| {
                a.case_name
                    .cmp(&b.case_name)
                    .then_with(|| a.case_number.cmp(&b.case_number))
            });
        }
        PrecedentSortOrder::RulingDate => {
            entries.sort_by(|a, b| {
                // Descending date (newest first); empty dates sort last.
                let da = if a.ruling_date.is_empty() {
                    ""
                } else {
                    &a.ruling_date
                };
                let db = if b.ruling_date.is_empty() {
                    ""
                } else {
                    &b.ruling_date
                };
                db.cmp(da).then_with(|| a.case_name.cmp(&b.case_name))
            });
        }
    }
}

// ── Law article types ────────────────────────────────────────

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

    #[test]
    fn test_precedent_entries_from_index_sorted() {
        let mut index = PrecedentMetadataIndex::new();
        index.insert(
            "형사/대법원/2020도1234".to_string(),
            PrecedentMetadataEntry {
                path: "형사/대법원/2020도1234.md".to_string(),
                case_name: "사기".to_string(),
                case_number: "2020도1234".to_string(),
                ruling_date: "2021-03-15".to_string(),
                court_name: "대법원".to_string(),
                case_type: "형사".to_string(),
                ruling_type: String::new(),
            },
        );
        index.insert(
            "민사/대법원/2000다10048".to_string(),
            PrecedentMetadataEntry {
                path: "민사/대법원/2000다10048.md".to_string(),
                case_name: "소유권이전등기등".to_string(),
                case_number: "2000다10048".to_string(),
                ruling_date: "2002-09-27".to_string(),
                court_name: "대법원".to_string(),
                case_type: "민사".to_string(),
                ruling_type: String::new(),
            },
        );

        let entries = precedent_entries_from_index(index);
        assert_eq!(entries.len(), 2);
        // Sorted by case name: 사기 < 소유권이전등기등
        assert_eq!(entries[0].case_name, "사기");
        assert_eq!(entries[1].case_name, "소유권이전등기등");
    }

    #[test]
    fn test_sort_precedent_entries_by_date() {
        let mut entries = vec![
            PrecedentEntry {
                id: "민사/대법원/2000다10048".to_string(),
                case_name: "소유권이전등기등".to_string(),
                case_number: "2000다10048".to_string(),
                ruling_date: "2002-09-27".to_string(),
                court_name: "대법원".to_string(),
                case_type: "민사".to_string(),
                ruling_type: String::new(),
                path: "민사/대법원/2000다10048.md".to_string(),
            },
            PrecedentEntry {
                id: "형사/대법원/2020도1234".to_string(),
                case_name: "사기".to_string(),
                case_number: "2020도1234".to_string(),
                ruling_date: "2021-03-15".to_string(),
                court_name: "대법원".to_string(),
                case_type: "형사".to_string(),
                ruling_type: String::new(),
                path: "형사/대법원/2020도1234.md".to_string(),
            },
        ];
        sort_precedent_entries(&mut entries, PrecedentSortOrder::RulingDate);
        // Newest first
        assert_eq!(entries[0].ruling_date, "2021-03-15");
        assert_eq!(entries[1].ruling_date, "2002-09-27");
    }

    #[test]
    fn test_precedent_entries_from_empty_index() {
        let index = PrecedentMetadataIndex::new();
        let entries = precedent_entries_from_index(index);
        assert!(entries.is_empty());
    }
}
