use anyhow::{Context, Result};
use tracing::{debug, info};

use super::models::MetadataIndex;

const BASE_URL: &str = "https://raw.githubusercontent.com/9bow/legalize-kr/main";

/// Fetch the metadata index from GitHub
pub async fn fetch_metadata() -> Result<MetadataIndex> {
    let url = format!("{BASE_URL}/metadata.json");
    info!("Fetching metadata from {url}");

    let resp = reqwest::get(&url)
        .await
        .context("Failed to fetch metadata.json")?;

    let status = resp.status();
    if !status.is_success() {
        anyhow::bail!("metadata.json returned HTTP {status}");
    }

    let index: MetadataIndex = resp.json().await.context("Failed to parse metadata.json")?;

    info!("Loaded {} law entries from metadata", index.len());
    Ok(index)
}

/// Fetch a single law file's raw markdown content from GitHub
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
