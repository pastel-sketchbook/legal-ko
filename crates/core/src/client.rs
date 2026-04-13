use std::time::Duration;

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use crate::cache;
use crate::models::{
    GitTreeResponse, MetadataEntry, MetadataIndex, PrecedentMetadataEntry, PrecedentMetadataIndex,
    RawPrecedentMetadataIndex,
};

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
///
/// # Errors
///
/// Returns an error if the HTTP client cannot be constructed.
pub fn http_client() -> Result<reqwest::Client> {
    let mut headers = reqwest::header::HeaderMap::new();
    if let Ok(token) = std::env::var("GITHUB_TOKEN")
        && let Ok(auth_val) = reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))
    {
        headers.insert(reqwest::header::AUTHORIZATION, auth_val);
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
                warn!(error = %e, "GitHub API fetch failed, retrying in 2s");
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

    info!(count = index.len(), "Built metadata index from tree");
    Ok(index)
}

/// Fetch a single law file's raw markdown content from GitHub.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response body cannot be read.
pub async fn fetch_law_content(client: &reqwest::Client, path: &str) -> Result<String> {
    let url = format!("{BASE_URL}/{path}");
    debug!(url, "Fetching law content");

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

/// Fetch only the YAML frontmatter from a law file (first 1024 bytes).
///
/// Uses an HTTP `Range` header to minimize bandwidth. Returns the raw bytes
/// as a UTF-8 string (possibly truncated mid-line, which is fine for our
/// frontmatter parser).
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response body cannot be read.
pub async fn fetch_frontmatter(client: &reqwest::Client, path: &str) -> Result<String> {
    let url = format!("{BASE_URL}/{path}");

    let resp = client
        .get(&url)
        .header("Range", "bytes=0-1023")
        .send()
        .await
        .with_context(|| format!("Failed to fetch frontmatter for {path}"))?;

    let status = resp.status();
    // 200 (full content) or 206 (partial) are both acceptable.
    if !status.is_success() && status.as_u16() != 206 {
        anyhow::bail!("{path} frontmatter returned HTTP {status}");
    }

    resp.text()
        .await
        .with_context(|| format!("Failed to read frontmatter body of {path}"))
}

/// Load law content: try cache first, then fetch from GitHub and cache the result.
///
/// # Errors
///
/// Returns an error if both cache read and network fetch fail.
pub async fn load_law_content(client: &reqwest::Client, path: &str) -> Result<String> {
    // Try cache first (blocking I/O — run off the async executor)
    let cache_path = path.to_string();
    let cached = tokio::task::spawn_blocking(move || cache::read_cache(&cache_path))
        .await
        .unwrap_or_else(|_| Ok(None));
    match cached {
        Ok(Some(content)) => {
            debug!(path, "Loaded from cache");
            return Ok(content);
        }
        Ok(None) => {} // cache miss
        Err(e) => {
            warn!(path, error = %e, "Cache read error");
        }
    }

    // Fetch from network
    let content = fetch_law_content(client, path).await?;

    // Cache the result (best-effort, blocking I/O)
    let cache_path = path.to_string();
    let cache_content = content.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = cache::write_cache(&cache_path, &cache_content) {
            warn!(path = %cache_path, error = %e, "Failed to cache");
        }
    });

    Ok(content)
}

// ── Precedent (판례) client ──────────────────────────────────

/// Raw-content base URL for the precedent-kr repo.
const PRECEDENT_BASE_URL: &str = "https://raw.githubusercontent.com/legalize-kr/precedent-kr/main";

/// URL for the pre-built metadata.json in the precedent-kr repo.
///
/// This ~35 MB file (< 3 MB gzip) contains all 123K+ precedent entries with
/// fully populated fields (case name, case number, date, court, case type),
/// which is much faster and richer than the GitHub Trees API approach.
const PRECEDENT_METADATA_URL: &str =
    "https://raw.githubusercontent.com/legalize-kr/precedent-kr/main/metadata.json";

/// Fetch the precedent metadata index from the pre-built `metadata.json`
/// in the precedent-kr repository.
///
/// This replaces the former GitHub Trees API approach. The metadata.json
/// file contains all fields pre-extracted from YAML frontmatter, so entries
/// have full case names, dates, court names, etc. immediately — no lazy
/// enrichment needed.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response cannot be parsed.
pub async fn fetch_precedent_metadata(client: &reqwest::Client) -> Result<PrecedentMetadataIndex> {
    info!("Fetching precedent metadata.json from GitHub");

    let mut retries = 1;
    let resp = loop {
        match client.get(PRECEDENT_METADATA_URL).send().await {
            Ok(r) => break r,
            Err(e) if retries > 0 => {
                warn!(error = %e, "Precedent metadata fetch failed, retrying in 2s");
                tokio::time::sleep(Duration::from_secs(2)).await;
                retries -= 1;
            }
            Err(e) => return Err(e).context("Failed to fetch precedent metadata.json"),
        }
    };

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("Precedent metadata.json returned HTTP {status}");
    }

    // Deserialize the Korean-keyed JSON into raw structs, then convert.
    let raw: RawPrecedentMetadataIndex = resp
        .json()
        .await
        .context("Failed to parse precedent metadata.json")?;

    let mut index = PrecedentMetadataIndex::with_capacity(raw.len());
    for (_serial, meta) in raw {
        // Derive stable ID from path: strip .md extension
        let id = meta
            .path
            .strip_suffix(".md")
            .unwrap_or(&meta.path)
            .to_string();

        index.insert(
            id,
            PrecedentMetadataEntry {
                path: meta.path,
                case_name: sanitize_case_name(&meta.case_name),
                case_number: meta.case_number,
                ruling_date: meta.ruling_date,
                court_name: meta.court_name.trim().to_string(),
                case_type: meta.case_type,
                ruling_type: meta.ruling_type,
            },
        );
    }

    info!(
        count = index.len(),
        "Built precedent metadata index from metadata.json"
    );
    Ok(index)
}

/// Clean up a case name from metadata: strip HTML tags (`<br/>`, `<br>`, etc.),
/// collapse newlines into spaces, and trim leading/trailing whitespace.
fn sanitize_case_name(raw: &str) -> String {
    raw.replace("<br/>", " ")
        .replace("<br>", " ")
        .replace("<BR/>", " ")
        .replace("<BR>", " ")
        .replace('\n', " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Fetch a single precedent file's raw markdown content from GitHub.
///
/// # Errors
///
/// Returns an error if the HTTP request fails or the response body cannot be read.
pub async fn fetch_precedent_content(client: &reqwest::Client, path: &str) -> Result<String> {
    let url = format!("{PRECEDENT_BASE_URL}/{path}");
    debug!(url, "Fetching precedent content");

    let resp = client
        .get(&url)
        .send()
        .await
        .with_context(|| format!("Failed to fetch precedent {path}"))?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("Precedent {path} returned HTTP {status}");
    }

    let content = resp
        .text()
        .await
        .with_context(|| format!("Failed to read body of precedent {path}"))?;

    Ok(content)
}

/// Load precedent content: try cache first, then fetch from GitHub and cache.
///
/// Cache keys are prefixed with `precedent/` to avoid collisions with law
/// content.
///
/// # Errors
///
/// Returns an error if both cache read and network fetch fail.
pub async fn load_precedent_content(client: &reqwest::Client, path: &str) -> Result<String> {
    let cache_key = format!("precedent/{path}");

    // Try cache first
    let ck = cache_key.clone();
    let cached = tokio::task::spawn_blocking(move || cache::read_cache(&ck))
        .await
        .unwrap_or_else(|_| Ok(None));
    match cached {
        Ok(Some(content)) => {
            debug!(path, "Loaded precedent from cache");
            return Ok(content);
        }
        Ok(None) => {}
        Err(e) => {
            warn!(path, error = %e, "Precedent cache read error");
        }
    }

    // Fetch from network
    let content = fetch_precedent_content(client, path).await?;

    // Cache the result (best-effort)
    let ck = cache_key;
    let cc = content.clone();
    tokio::task::spawn_blocking(move || {
        if let Err(e) = cache::write_cache(&ck, &cc) {
            warn!(path = %ck, error = %e, "Failed to cache precedent");
        }
    });

    Ok(content)
}
