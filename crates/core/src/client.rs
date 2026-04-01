use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::models::{GitTreeResponse, MetadataEntry, MetadataIndex};

/// Raw-content base URL for the new repo location.
const BASE_URL: &str = "https://raw.githubusercontent.com/legalize-kr/legalize-kr/main";

/// GitHub API endpoint for the recursive tree listing.
const TREE_API_URL: &str =
    "https://api.github.com/repos/legalize-kr/legalize-kr/git/trees/main?recursive=1";

/// Known law-type filenames (without `.md`) and their Korean category labels.
///
/// The upstream repo stores each law type as a separate markdown file:
/// `법률.md`, `시행령.md`, `시행규칙.md`, `대통령령.md`.
const LAW_TYPES: &[(&str, &str)] = &[
    ("법률", "법률"),
    ("시행령", "대통령령"),
    ("시행규칙", "부령"),
    ("대통령령", "대통령령"),
];

/// Fetch the law metadata index by listing the repository tree via the GitHub
/// Git Trees API and constructing a `MetadataIndex` from the file paths.
///
/// Each `.md` file under `kr/` becomes one entry. The directory name provides
/// the law title, and the filename determines the category (법률, 대통령령, etc.).
///
/// Full metadata (departments, dates, status) is left with default/placeholder
/// values here — it is populated lazily from YAML frontmatter when a specific
/// law is opened.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response cannot be parsed.
pub async fn fetch_metadata() -> Result<MetadataIndex> {
    info!("Fetching repository tree from GitHub API");

    let client = reqwest::Client::new();
    let resp = client
        .get(TREE_API_URL)
        .header("User-Agent", "legal-ko")
        .header("Accept", "application/vnd.github+json")
        .send()
        .await
        .context("Failed to fetch GitHub tree API")?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("GitHub Trees API returned HTTP {status}");
    }

    let tree_resp: GitTreeResponse = resp
        .json()
        .await
        .context("Failed to parse GitHub Trees API response")?;

    if tree_resp.truncated {
        warn!("GitHub tree response was truncated — some laws may be missing");
    }

    let mut index = MetadataIndex::new();

    for entry in &tree_resp.tree {
        // Only process blob entries under kr/ that end in .md
        if entry.entry_type != "blob" {
            continue;
        }
        let Some(rest) = entry.path.strip_prefix("kr/") else {
            continue;
        };
        if !rest.ends_with(".md") {
            continue;
        }

        // Expected format: {법령명}/{type}.md  e.g. "민법/법률.md"
        let Some((dir_name, file_name)) = rest.rsplit_once('/') else {
            continue;
        };
        let stem = file_name.strip_suffix(".md").unwrap_or(file_name);

        // Map filename stem to category
        let category = LAW_TYPES
            .iter()
            .find(|(name, _)| *name == stem)
            .map_or_else(|| stem.to_string(), |(_, cat)| (*cat).to_string());

        // Build a stable ID from the path (without .md)
        let id = format!("kr/{dir_name}/{stem}");

        let meta = MetadataEntry {
            path: entry.path.clone(),
            title: dir_name.to_string(),
            category,
            departments: Vec::new(),
            promulgation_date: String::new(),
            enforcement_date: String::new(),
            status: "시행".to_string(),
        };

        index.insert(id, meta);
    }

    info!(
        "Built metadata index with {} law entries from tree",
        index.len()
    );
    Ok(index)
}

/// Fetch a single law file's raw markdown content from GitHub.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response body cannot be read.
pub async fn fetch_law_content(path: &str) -> Result<String> {
    let url = format!("{BASE_URL}/{path}");
    debug!("Fetching law content from {url}");

    let resp = reqwest::get(&url)
        .await
        .with_context(|| format!("Failed to fetch {path}"))?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("{path} returned HTTP {status}");
    }

    let content = resp
        .text()
        .await
        .with_context(|| format!("Failed to read body of {path}"))?;

    Ok(content)
}
