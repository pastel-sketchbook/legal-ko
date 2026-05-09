use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rayon::prelude::*;
use tracing::{debug, info, warn};

use crate::cache;
use crate::models::{
    GitTreeResponse, MetadataEntry, MetadataIndex, PrecedentMetadataEntry, PrecedentMetadataIndex,
    RawPrecedentMetadataIndex,
};
use crate::parser;

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

/// Fetch the precedent metadata index.
///
/// Tries three sources in order:
/// 1. Cached local metadata (`~/.cache/legal-ko/precedent_metadata.json`)
/// 2. Remote `metadata.json` from GitHub (may 404 if upstream removed it)
/// 3. Build from local zmd clone by scanning `.md` frontmatter (Rayon parallel)
///
/// The result is always cached to disk for fast subsequent loads.
///
/// # Errors
///
/// Returns an error if all sources fail.
pub async fn fetch_precedent_metadata(client: &reqwest::Client) -> Result<PrecedentMetadataIndex> {
    // 1. Try cached local metadata
    let cache_path = local_metadata_cache_path()?;
    if let Some(index) = load_cached_metadata(&cache_path) {
        info!(count = index.len(), "Loaded precedent metadata from cache");
        return Ok(index);
    }

    // 2. Try remote metadata.json
    match fetch_remote_precedent_metadata(client).await {
        Ok(index) => {
            save_metadata_cache(&cache_path, &index);
            return Ok(index);
        }
        Err(e) => {
            warn!(error = %e, "Remote metadata.json unavailable, falling back to local clone");
        }
    }

    // 3. Build from local zmd clone
    let clone_dir = zmd_precedent_clone_dir()?;
    if !clone_dir.join(".git").is_dir() {
        anyhow::bail!(
            "No precedent metadata available: remote metadata.json is gone and \
             local clone not found at {}. Run `legal-ko-cli zmd precedents` first.",
            clone_dir.display()
        );
    }

    info!(path = %clone_dir.display(), "Building precedent metadata from local clone");
    let index = tokio::task::spawn_blocking(move || build_precedent_metadata_from_clone(&clone_dir))
        .await
        .context("Metadata build task panicked")??;

    save_metadata_cache(&cache_path, &index);
    Ok(index)
}

/// Path to the cached precedent metadata file.
fn local_metadata_cache_path() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Cannot determine home directory")?;
    Ok(std::path::PathBuf::from(home).join(".cache/legal-ko/precedent_metadata.json"))
}

/// Path to the zmd precedent-kr clone.
fn zmd_precedent_clone_dir() -> Result<std::path::PathBuf> {
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .context("Cannot determine home directory")?;
    Ok(std::path::PathBuf::from(home).join(".cache/legal-ko/zmd/repos/precedent-kr"))
}

/// Load cached metadata from disk. Returns `None` if the file doesn't exist
/// or is older than 7 days.
fn load_cached_metadata(path: &Path) -> Option<PrecedentMetadataIndex> {
    let meta = std::fs::metadata(path).ok()?;
    let age = meta.modified().ok()?.elapsed().ok()?;
    if age > Duration::from_secs(7 * 24 * 3600) {
        info!("Cached precedent metadata is older than 7 days, rebuilding");
        return None;
    }
    let data = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Save metadata index to disk cache (best-effort).
fn save_metadata_cache(path: &Path, index: &PrecedentMetadataIndex) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string(index) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                warn!(error = %e, "Failed to cache precedent metadata");
            } else {
                info!(path = %path.display(), count = index.len(), "Cached precedent metadata to disk");
            }
        }
        Err(e) => warn!(error = %e, "Failed to serialize precedent metadata"),
    }
}

/// Try fetching metadata.json from the remote GitHub repo.
async fn fetch_remote_precedent_metadata(
    client: &reqwest::Client,
) -> Result<PrecedentMetadataIndex> {
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

    let raw: RawPrecedentMetadataIndex = resp
        .json()
        .await
        .context("Failed to parse precedent metadata.json")?;

    let mut index = PrecedentMetadataIndex::with_capacity(raw.len());
    for (_serial, meta) in raw {
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

/// Build a `PrecedentMetadataIndex` by scanning all `.md` files in the local
/// clone of `precedent-kr` and parsing their YAML frontmatter.
///
/// Uses Rayon for parallel file I/O across 123K+ files.
fn build_precedent_metadata_from_clone(repo_dir: &Path) -> Result<PrecedentMetadataIndex> {
    // Collect all .md file paths (exclude README.md at root)
    let mut paths: Vec<std::path::PathBuf> = Vec::new();
    collect_md_files(repo_dir, &mut paths)?;

    info!(count = paths.len(), "Scanning precedent frontmatter");

    let entries: Vec<(String, PrecedentMetadataEntry)> = paths
        .par_iter()
        .filter_map(|path| {
            let content = std::fs::read_to_string(path).ok()?;
            let fm = parser::parse_frontmatter(&content);

            let rel = path
                .strip_prefix(repo_dir)
                .ok()?
                .to_string_lossy()
                .to_string();

            let id = rel.strip_suffix(".md").unwrap_or(&rel).to_string();

            let case_name = fm.get("사건명").map_or(String::new(), |v| {
                sanitize_case_name(v.as_str())
            });
            let case_number = fm
                .get("사건번호")
                .map_or(String::new(), |v| v.as_str().to_string());
            let ruling_date = fm
                .get("선고일자")
                .map_or(String::new(), |v| v.as_str().to_string());
            let court_name = fm
                .get("법원명")
                .map_or(String::new(), |v| v.as_str().trim().to_string());
            let case_type = fm
                .get("사건종류")
                .map_or(String::new(), |v| v.as_str().to_string());
            let ruling_type = fm
                .get("판결유형")
                .map_or(String::new(), |v| v.as_str().to_string());

            Some((
                id,
                PrecedentMetadataEntry {
                    path: rel,
                    case_name,
                    case_number,
                    ruling_date,
                    court_name,
                    case_type,
                    ruling_type,
                },
            ))
        })
        .collect();

    let mut index = PrecedentMetadataIndex::with_capacity(entries.len());
    for (id, entry) in entries {
        index.insert(id, entry);
    }

    info!(
        count = index.len(),
        "Built precedent metadata index from local clone"
    );
    Ok(index)
}

/// Recursively collect `.md` files, skipping README.md at any level.
fn collect_md_files(
    dir: &Path,
    out: &mut Vec<std::path::PathBuf>,
) -> Result<()> {
    let entries = std::fs::read_dir(dir)
        .with_context(|| format!("Failed to read directory {}", dir.display()))?;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            // Skip .git
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            collect_md_files(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "md") {
            // Skip README files
            if path.file_name().is_some_and(|n| n == "README.md") {
                continue;
            }
            out.push(path);
        }
    }
    Ok(())
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
