use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::cache;
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

/// Build a configured HTTP client with timeouts and User-Agent.
pub fn http_client() -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if let Ok(auth_val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}")) {
            headers.insert(reqwest::header::AUTHORIZATION, auth_val);
        }
    }

    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .default_headers(headers)
        .user_agent("legal-ko")
        .build()
        .context("Failed to build HTTP client")
}

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
pub async fn fetch_metadata(client: &reqwest::Client) -> Result<MetadataIndex> {
    info!("Fetching repository tree from GitHub API");

    let mut retries = 1;
    let resp = loop {
        match client
            .get(TREE_API_URL)
            .header("Accept", "application/vnd.github+json")
            .send()
            .await
        {
            Ok(r) => break r,
            Err(e) if retries > 0 => {
                warn!("GitHub API fetch failed, retrying in 2s: {e}");
                tokio::time::sleep(Duration::from_secs(2)).await;
                retries -= 1;
            }
            Err(e) => return Err(e).context("Failed to fetch GitHub tree API"),
        }
    };

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("GitHub Trees API returned HTTP {status}");
    }

    let tree_resp: GitTreeResponse = resp
        .json()
        .await
        .context("Failed to parse GitHub Trees API response")?;

    if tree_resp.truncated {
        anyhow::bail!("GitHub tree response was truncated — repository may have too many entries");
    }

    let mut index = MetadataIndex::new();

    for entry in &tree_resp.tree {
        // Only process blob entries under kr/ that end in .md
        if entry.entry_type != "blob" {
            continue;
        }
        let Some(rel_path) = entry.path.strip_prefix("kr/") else {
            continue;
        };
        if std::path::Path::new(rel_path)
            .extension()
            .and_then(|e| e.to_str())
            != Some("md")
        {
            continue;
        }

        // Expected format: {법령명}/{type}.md  e.g. "민법/법률.md"
        let Some((dir_name, file_name)) = rel_path.rsplit_once('/') else {
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
pub async fn fetch_law_content(client: &reqwest::Client, path: &str) -> Result<String> {
    let url = format!("{BASE_URL}/{path}");
    debug!("Fetching law content from {url}");

    let resp = client
        .get(&url)
        .send()
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

/// Load law content: try cache first, then fetch from GitHub and cache the result.
///
/// # Errors
///
/// Returns an error if both cache read and network fetch fail.
pub async fn load_law_content(client: &reqwest::Client, path: &str) -> Result<String> {
    // Try cache first
    match cache::read_cache(path) {
        Ok(Some(content)) => {
            debug!("Loaded {path} from cache");
            return Ok(content);
        }
        Ok(None) => {} // cache miss
        Err(e) => {
            warn!("Cache read error for {path}: {e}");
        }
    }

    // Fetch from network
    let content = fetch_law_content(client, path).await?;

    // Cache the result (best-effort)
    if let Err(e) = cache::write_cache(path, &content) {
        warn!("Failed to cache {path}: {e}");
    }

    Ok(content)
}
