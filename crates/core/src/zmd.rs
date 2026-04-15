//! zmd collection management: clone repos, stage files via hardlinks, and
//! invoke `zmd update` for full-text + vector indexing.
//!
//! Replaces the shell script `scripts/zmd-collections.sh` with a native Rust
//! implementation that is faster and resumable:
//!
//! - Collects all source files up front, then stages in batches of
//!   `batch_size` (default 300) with Rayon-parallel hardlinking.
//! - Calls `zmd update` after each batch so progress is visible and the
//!   process can be safely interrupted (re-run to resume).
//! - zmd skips already-indexed documents (SHA-256 content check), so
//!   re-runs are near-instant for unchanged files.
//!
//! # Directory layout
//!
//! ```text
//! ~/.cache/legal-ko/zmd/
//!   repos/
//!     legalize-kr/    ← shallow clone of the laws repo
//!     precedent-kr/   ← shallow clone of the precedents repo
//!   stage/
//!     laws/           ← hardlinks to legalize-kr .md files (registered with zmd)
//!     precedents/     ← hardlinks to precedent-kr .md files (registered with zmd)
//! ```

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use rayon::prelude::*;
use tracing::{info, warn};

// ── Configuration ─────────────────────────────────────────────

const LAWS_REPO: &str = "https://github.com/legalize-kr/legalize-kr.git";
const PRECEDENT_REPO: &str = "https://github.com/legalize-kr/precedent-kr.git";

/// Default case types to index for precedents.
const DEFAULT_CASE_TYPES: &[&str] = &["민사", "형사"];

/// Default court levels to index for precedents.
const DEFAULT_COURTS: &[&str] = &["대법원"];

/// Default batch size: number of files to stage before each `zmd update` call.
///
/// zmd indexes at ~60-100ms per *new* file (FTS + chunk + embed), so 300 files
/// ≈ 20-30s per batch — enough for visible progress and safe interruption.
const DEFAULT_BATCH_SIZE: usize = 300;

// ── Public types ──────────────────────────────────────────────

/// Configuration for a zmd indexing run.
#[derive(Debug, Clone)]
pub struct ZmdConfig {
    /// Root cache directory (default: `~/.cache/legal-ko/zmd`).
    pub cache_dir: PathBuf,
    /// Case types to index for precedents.
    pub case_types: Vec<String>,
    /// Court levels to index for precedents.
    pub courts: Vec<String>,
    /// Files to stage per `zmd update` call.
    pub batch_size: usize,
}

impl ZmdConfig {
    /// Create a config with default paths and scope.
    ///
    /// # Errors
    ///
    /// Returns an error if the cache directory cannot be determined.
    pub fn default_config() -> Result<Self> {
        let cache_dir = if let Ok(v) = std::env::var("ZMD_CACHE_DIR") {
            PathBuf::from(v)
        } else {
            // Use ~/.cache/legal-ko/zmd to match the original shell script layout.
            // We intentionally avoid `dirs::cache_dir()` (which returns ~/Library/Caches
            // on macOS) for compatibility with existing staged data and zmd registrations.
            let home = std::env::var("HOME")
                .or_else(|_| std::env::var("USERPROFILE"))
                .context("Cannot determine home directory")?;
            PathBuf::from(home).join(".cache/legal-ko/zmd")
        };
        let batch_size = std::env::var("ZMD_BATCH_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_BATCH_SIZE);
        Ok(Self {
            cache_dir,
            case_types: DEFAULT_CASE_TYPES
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            courts: DEFAULT_COURTS.iter().map(|s| (*s).to_string()).collect(),
            batch_size,
        })
    }

    /// Path to the repos directory (for display in CLI output).
    #[must_use]
    pub fn repos_dir(&self) -> PathBuf {
        self.cache_dir.join("repos")
    }

    fn stage_dir(&self) -> PathBuf {
        self.cache_dir.join("stage")
    }

    /// Path to the cloned laws repo (for checking if cloned).
    #[must_use]
    pub fn laws_clone(&self) -> PathBuf {
        self.repos_dir().join("legalize-kr")
    }

    /// Path to the cloned precedent repo (for checking if cloned).
    #[must_use]
    pub fn precedent_clone(&self) -> PathBuf {
        self.repos_dir().join("precedent-kr")
    }

    fn laws_stage(&self) -> PathBuf {
        self.stage_dir().join("laws")
    }

    fn precedent_stage(&self) -> PathBuf {
        self.stage_dir().join("precedents")
    }
}

/// Progress information reported after each batch.
#[derive(Debug, Clone)]
pub struct BatchProgress {
    /// Batch number (1-indexed).
    pub batch_num: usize,
    /// Files newly staged in this batch.
    pub batch_new: usize,
    /// Cumulative staged count (including previously staged).
    pub total_staged: usize,
    /// Total source files.
    pub total_files: usize,
    /// Elapsed seconds for `zmd update` in this batch.
    pub update_secs: f64,
    /// Raw output from `zmd update`.
    pub update_output: String,
}

/// Result of a full indexing run (all batches combined).
#[derive(Debug, Clone)]
pub struct IndexResult {
    /// Total source files found.
    pub total_files: usize,
    /// Files that were already staged before this run.
    pub already_staged: usize,
    /// Files newly staged in this run.
    pub newly_staged: usize,
    /// Number of batches that ran `zmd update`.
    pub batches: usize,
    /// Total wall-clock time for all `zmd update` calls.
    pub total_update_secs: f64,
}

/// Full status snapshot.
#[derive(Debug, Clone, Default)]
pub struct ZmdStatus {
    /// Laws repo state.
    pub laws_repo: Option<String>,
    /// Precedent repo state.
    pub precedent_repo: Option<String>,
    /// Staged law file count.
    pub laws_staged: usize,
    /// Staged precedent file count by `case_type/court`.
    pub precedent_staged: Vec<(String, usize)>,
    /// Precedent total staged.
    pub precedent_total: usize,
    /// Raw `zmd collection list` output.
    pub collections: String,
    /// Raw `zmd status` output.
    pub zmd_status: String,
}

// ── Git helpers ───────────────────────────────────────────────

/// Clone a repo (shallow, depth=1) or fast-forward pull if already cloned.
fn clone_or_pull(url: &str, dir: &Path, name: &str) -> Result<()> {
    if dir.join(".git").is_dir() {
        info!(%name, "Pulling latest");
        let output = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .args(["pull", "--ff-only", "--depth", "1"])
            .output()
            .with_context(|| format!("Failed to run git pull for {name}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!(%name, %stderr, "git pull failed (non-fatal)");
        } else {
            let stdout = String::from_utf8_lossy(&output.stdout);
            info!(%name, result = stdout.trim(), "Pull complete");
        }
    } else {
        info!(%name, %url, "Cloning (shallow)");
        if let Some(parent) = dir.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create dir {}", parent.display()))?;
        }
        let output = Command::new("git")
            .args(["clone", "--depth", "1", url])
            .arg(dir)
            .output()
            .with_context(|| format!("Failed to clone {name}"))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clone failed for {name}: {stderr}");
        }
        info!(%name, "Clone complete");
    }
    Ok(())
}

/// Get the latest commit summary from a local repo.
fn repo_commit_summary(dir: &Path) -> Option<String> {
    if !dir.join(".git").is_dir() {
        return None;
    }
    let output = Command::new("git")
        .args(["-C"])
        .arg(dir)
        .args(["log", "-1", "--format=%h %s (%ci)"])
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        None
    }
}

// ── File collection ──────────────────────────────────────────

/// Collect all `.md` files matching the given criteria under `src_root`.
///
/// For laws: finds `**/법률.md` up to depth 2 under `kr/`.
/// For precedents: finds `*.md` at depth 1 under each `case_type/court/`.
fn collect_md_files(src_root: &Path, pattern: FilePattern<'_>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    match pattern {
        FilePattern::Laws => {
            let kr_dir = src_root.join("kr");
            if let Ok(entries) = std::fs::read_dir(&kr_dir) {
                for entry in entries.flatten() {
                    let law_file = entry.path().join("법률.md");
                    if law_file.is_file() {
                        files.push(law_file);
                    }
                }
            }
        }
        FilePattern::Precedents { case_type, court } => {
            let dir = src_root.join(case_type).join(court);
            if dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&dir)
            {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("md") && path.is_file() {
                        files.push(path);
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// File-finding patterns.
enum FilePattern<'a> {
    Laws,
    Precedents { case_type: &'a str, court: &'a str },
}

// ── Batched stage + index ────────────────────────────────────

/// Classify source files into already-staged vs needs-staging.
///
/// Returns `(already_staged_count, to_link)` where `to_link` is a vec
/// of `(src, dst)` pairs for files that need hardlinking.
fn classify_files(
    files: &[PathBuf],
    src_root: &Path,
    stage_root: &Path,
) -> Result<(usize, Vec<(PathBuf, PathBuf)>)> {
    std::fs::create_dir_all(stage_root)
        .with_context(|| format!("Failed to create stage dir {}", stage_root.display()))?;

    let mut to_link = Vec::new();
    let mut already_staged = 0usize;

    for src in files {
        let rel = src
            .strip_prefix(src_root)
            .with_context(|| format!("File {} not under {}", src.display(), src_root.display()))?;
        let dst = stage_root.join(rel);

        if dst.exists() {
            already_staged += 1;
        } else {
            to_link.push((src.clone(), dst));
        }
    }

    Ok((already_staged, to_link))
}

/// Hardlink a batch of `(src, dst)` pairs in parallel via Rayon.
///
/// Creates necessary parent directories first (sequential), then
/// hardlinks all files in parallel.  Returns count of successful links.
fn hardlink_batch(batch: &[(PathBuf, PathBuf)]) -> usize {
    // Collect unique parent dirs needed.
    let mut dirs_needed = std::collections::HashSet::new();
    for (_, dst) in batch {
        if let Some(parent) = dst.parent() {
            dirs_needed.insert(parent.to_path_buf());
        }
    }
    for dir in &dirs_needed {
        if let Err(e) = std::fs::create_dir_all(dir) {
            warn!(dir = %dir.display(), error = %e, "Failed to create dir");
        }
    }

    let linked = AtomicUsize::new(0);
    batch
        .par_iter()
        .for_each(|(src, dst)| match std::fs::hard_link(src, dst) {
            Ok(()) => {
                linked.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                warn!(src = %src.display(), dst = %dst.display(), error = %e, "Hardlink failed");
            }
        });
    linked.load(Ordering::Relaxed)
}

/// Stage files in batches and call `zmd update` after each batch.
///
/// This is the core indexing loop.  For each batch of `batch_size` files:
/// 1. Hardlink the batch into the stage directory (Rayon parallel).
/// 2. Call `zmd update` (indexes only the newly staged files).
/// 3. Report progress via `on_batch` callback.
///
/// Already-staged files are skipped entirely (no re-linking, no re-indexing).
/// Safe to interrupt and re-run — picks up where it left off.
fn stage_and_index_batched<F>(
    files: &[PathBuf],
    src_root: &Path,
    stage_root: &Path,
    batch_size: usize,
    mut on_batch: F,
) -> Result<IndexResult>
where
    F: FnMut(&BatchProgress),
{
    let (already_staged, to_link) = classify_files(files, src_root, stage_root)?;

    info!(
        total = files.len(),
        already_staged,
        to_stage = to_link.len(),
        batch_size,
        "Starting batched stage+index"
    );

    if to_link.is_empty() {
        info!("All files already staged — nothing to do");
        // Still call zmd update once to catch any staged-but-not-indexed files.
        let update = run_zmd_update()?;
        on_batch(&BatchProgress {
            batch_num: 0,
            batch_new: 0,
            total_staged: already_staged,
            total_files: files.len(),
            update_secs: update.elapsed_secs,
            update_output: update.output,
        });
        return Ok(IndexResult {
            total_files: files.len(),
            already_staged,
            newly_staged: 0,
            batches: 1,
            total_update_secs: update.elapsed_secs,
        });
    }

    let mut total_newly_staged = 0usize;
    let mut total_update_secs = 0.0f64;
    let mut batch_num = 0usize;

    for chunk in to_link.chunks(batch_size) {
        batch_num += 1;
        let linked = hardlink_batch(chunk);
        total_newly_staged += linked;

        let update = run_zmd_update()?;
        total_update_secs += update.elapsed_secs;

        on_batch(&BatchProgress {
            batch_num,
            batch_new: linked,
            total_staged: already_staged + total_newly_staged,
            total_files: files.len(),
            update_secs: update.elapsed_secs,
            update_output: update.output,
        });
    }

    Ok(IndexResult {
        total_files: files.len(),
        already_staged,
        newly_staged: total_newly_staged,
        batches: batch_num,
        total_update_secs,
    })
}

/// Count `.md` files recursively under a directory.
fn count_md_files(dir: &Path) -> usize {
    if !dir.is_dir() {
        return 0;
    }
    walkdir(dir)
}

fn walkdir(dir: &Path) -> usize {
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                count += walkdir(&path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("md") {
                count += 1;
            }
        }
    }
    count
}

// ── zmd CLI wrappers ──────────────────────────────────────────

/// Check if `zmd` is available on PATH.
fn zmd_available() -> bool {
    Command::new("zmd")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Register a collection with zmd (idempotent: skips if already registered).
fn register_collection(name: &str, path: &Path) -> Result<()> {
    let list_output = Command::new("zmd")
        .args(["collection", "list"])
        .output()
        .context("Failed to run zmd collection list")?;
    let list_text = String::from_utf8_lossy(&list_output.stdout);

    let pattern = format!("{name}:");
    if list_text
        .lines()
        .any(|line| line.trim_start().starts_with(&pattern))
    {
        info!(%name, "Collection already registered");
        return Ok(());
    }

    info!(%name, path = %path.display(), "Registering collection");
    let output = Command::new("zmd")
        .args(["collection", "add", name])
        .arg(path)
        .output()
        .context("Failed to run zmd collection add")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("zmd collection add failed: {stderr}");
    }
    Ok(())
}

/// Run `zmd update` and return timing + output.
fn run_zmd_update() -> Result<UpdateResult> {
    let start = Instant::now();
    let output = Command::new("zmd")
        .arg("update")
        .output()
        .context("Failed to run zmd update")?;

    let elapsed = start.elapsed().as_secs_f64();
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        warn!(stderr = %stderr, "zmd update reported errors (non-fatal)");
    }

    let combined = if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{stdout}\n{stderr}")
    };

    info!(
        elapsed_secs = format!("{elapsed:.1}"),
        "zmd update complete"
    );
    Ok(UpdateResult {
        elapsed_secs: elapsed,
        output: combined,
    })
}

/// Result of a single `zmd update` call.
#[derive(Debug, Clone)]
struct UpdateResult {
    elapsed_secs: f64,
    output: String,
}

/// Run `zmd status` and return the raw output.
fn run_zmd_status() -> Result<String> {
    let output = Command::new("zmd")
        .arg("status")
        .output()
        .context("Failed to run zmd status")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Run `zmd collection list` and return the raw output.
fn run_zmd_collection_list() -> Result<String> {
    let output = Command::new("zmd")
        .args(["collection", "list"])
        .output()
        .context("Failed to run zmd collection list")?;
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

/// Remove a zmd collection by name (idempotent).
fn remove_collection(name: &str) -> Result<()> {
    let list = run_zmd_collection_list().unwrap_or_default();
    let pattern = format!("{name}:");
    if !list
        .lines()
        .any(|line| line.trim_start().starts_with(&pattern))
    {
        return Ok(());
    }

    let output = Command::new("zmd")
        .args(["collection", "remove", name])
        .output()
        .context("Failed to run zmd collection remove")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(%name, %stderr, "zmd collection remove failed (non-fatal)");
    }
    Ok(())
}

/// Run `zmd cleanup` (remove orphaned entries).
fn run_zmd_cleanup() -> Result<()> {
    let _ = Command::new("zmd").arg("cleanup").output();
    Ok(())
}

// ── Public API ────────────────────────────────────────────────

/// Index law files (법률 only) into zmd.
///
/// 1. Clone/pull the legalize-kr repo.
/// 2. Find all `법률.md` files.
/// 3. Stage in batches with Rayon-parallel hardlinks.
/// 4. Call `zmd update` after each batch.
///
/// The `on_batch` callback fires after each batch completes.
pub fn index_laws<F>(cfg: &ZmdConfig, on_batch: F) -> Result<IndexResult>
where
    F: FnMut(&BatchProgress),
{
    if !zmd_available() {
        bail!("zmd is not installed or not on PATH");
    }

    clone_or_pull(LAWS_REPO, &cfg.laws_clone(), "legalize-kr (laws)")?;

    let files = collect_md_files(&cfg.laws_clone(), FilePattern::Laws);
    info!(count = files.len(), "Found 법률.md files");

    register_collection("laws", &cfg.laws_stage())?;

    stage_and_index_batched(
        &files,
        &cfg.laws_clone(),
        &cfg.laws_stage(),
        cfg.batch_size,
        on_batch,
    )
}

/// Index precedent files into zmd.
///
/// 1. Clone/pull the precedent-kr repo.
/// 2. For each case_type × court, find `.md` files and stage in batches.
/// 3. Call `zmd update` after each batch.
///
/// `on_batch` fires after each batch. `on_court` fires when a new court starts.
pub fn index_precedents<F, G>(
    cfg: &ZmdConfig,
    mut on_court: G,
    mut on_batch: F,
) -> Result<Vec<(String, IndexResult)>>
where
    F: FnMut(&str, &str, &BatchProgress),
    G: FnMut(&str, &str, usize),
{
    if !zmd_available() {
        bail!("zmd is not installed or not on PATH");
    }

    clone_or_pull(
        PRECEDENT_REPO,
        &cfg.precedent_clone(),
        "precedent-kr (precedents)",
    )?;

    register_collection("precedents", &cfg.precedent_stage())?;

    let mut results = Vec::new();

    for case_type in &cfg.case_types {
        for court in &cfg.courts {
            let files = collect_md_files(
                &cfg.precedent_clone(),
                FilePattern::Precedents { case_type, court },
            );

            if files.is_empty() {
                warn!(%case_type, %court, "No files found — skipping");
                continue;
            }

            on_court(case_type, court, files.len());

            let ct = case_type.clone();
            let co = court.clone();
            let result = stage_and_index_batched(
                &files,
                &cfg.precedent_clone(),
                &cfg.precedent_stage(),
                cfg.batch_size,
                |bp| on_batch(&ct, &co, bp),
            )?;

            let label = format!("{case_type}/{court}");
            results.push((label, result));
        }
    }

    Ok(results)
}

/// Run both laws and precedents indexing.
pub fn index_all(cfg: &ZmdConfig) -> Result<()> {
    if !zmd_available() {
        bail!("zmd is not installed or not on PATH");
    }

    eprintln!("Phase 1/2: Laws (법률 only)");
    let law_result = index_laws(cfg, |bp| {
        if bp.batch_num > 0 {
            eprintln!(
                "  batch {}: +{} files ({} staged) — {:.0}s",
                bp.batch_num, bp.batch_new, bp.total_staged, bp.update_secs,
            );
        }
    })?;
    eprintln!(
        "  Laws done: {} total, {} new, {} existing — {:.0}s",
        law_result.total_files,
        law_result.newly_staged,
        law_result.already_staged,
        law_result.total_update_secs,
    );

    eprintln!("\nPhase 2/2: Precedents");
    let prec_results = index_precedents(
        cfg,
        |ct, court, count| {
            eprintln!("  {ct}/{court}: {count} files");
        },
        |ct, court, bp| {
            if bp.batch_num > 0 {
                eprintln!(
                    "    {ct}/{court} batch {}: +{} files ({} staged) — {:.0}s",
                    bp.batch_num, bp.batch_new, bp.total_staged, bp.update_secs,
                );
            }
        },
    )?;

    let total_new: usize = prec_results.iter().map(|(_, r)| r.newly_staged).sum();
    let total_files: usize = prec_results.iter().map(|(_, r)| r.total_files).sum();
    let total_secs: f64 = prec_results.iter().map(|(_, r)| r.total_update_secs).sum();
    eprintln!("  Precedents done: {total_files} files, {total_new} new — {total_secs:.0}s",);

    Ok(())
}

/// Pull latest from upstream repos and re-index.
pub fn sync(cfg: &ZmdConfig) -> Result<()> {
    if !zmd_available() {
        bail!("zmd is not installed or not on PATH");
    }

    if cfg.laws_clone().join(".git").is_dir() {
        eprintln!("Syncing laws...");
        let result = index_laws(cfg, |bp| {
            if bp.batch_new > 0 {
                eprintln!(
                    "  batch {}: +{} new — {:.0}s",
                    bp.batch_num, bp.batch_new, bp.update_secs,
                );
            }
        })?;
        eprintln!(
            "  Laws: {} new — {:.0}s",
            result.newly_staged, result.total_update_secs
        );
    }

    if cfg.precedent_clone().join(".git").is_dir() {
        eprintln!("Syncing precedents...");
        let results = index_precedents(
            cfg,
            |_, _, _| {},
            |ct, court, bp| {
                if bp.batch_new > 0 {
                    eprintln!(
                        "  {ct}/{court} batch {}: +{} new — {:.0}s",
                        bp.batch_num, bp.batch_new, bp.update_secs,
                    );
                }
            },
        )?;
        let total_new: usize = results.iter().map(|(_, r)| r.newly_staged).sum();
        let total_secs: f64 = results.iter().map(|(_, r)| r.total_update_secs).sum();
        eprintln!("  Precedents: {total_new} new — {total_secs:.0}s");
    }

    Ok(())
}

/// Gather status information about repos, staged files, and zmd state.
pub fn status(cfg: &ZmdConfig) -> Result<ZmdStatus> {
    let mut precedent_staged = Vec::new();
    let mut precedent_total = 0usize;

    for case_type in &cfg.case_types {
        for court in &cfg.courts {
            let dir = cfg.precedent_stage().join(case_type).join(court);
            let count = count_md_files(&dir);
            if count > 0 {
                precedent_staged.push((format!("{case_type}/{court}"), count));
                precedent_total += count;
            }
        }
    }

    Ok(ZmdStatus {
        laws_repo: repo_commit_summary(&cfg.laws_clone()),
        precedent_repo: repo_commit_summary(&cfg.precedent_clone()),
        laws_staged: count_md_files(&cfg.laws_stage()),
        precedent_staged,
        precedent_total,
        collections: run_zmd_collection_list().unwrap_or_else(|_| "No collections".to_string()),
        zmd_status: run_zmd_status().unwrap_or_else(|_| "Database not initialized".to_string()),
    })
}

/// Remove all zmd collections and staged data.  Preserves repo clones.
pub fn reset(cfg: &ZmdConfig) -> Result<()> {
    if !zmd_available() {
        bail!("zmd is not installed or not on PATH");
    }

    info!("Removing zmd collections and staged data");

    remove_collection("laws")?;
    remove_collection("precedents")?;

    let stage = cfg.stage_dir();
    if stage.is_dir() {
        std::fs::remove_dir_all(&stage)
            .with_context(|| format!("Failed to remove {}", stage.display()))?;
    }

    run_zmd_cleanup()?;
    let _ = run_zmd_update();

    info!(
        repos = %cfg.repos_dir().display(),
        "Reset complete. Repo clones preserved"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_pattern_laws() {
        let files = collect_md_files(Path::new("/nonexistent"), FilePattern::Laws);
        assert!(files.is_empty());
    }

    #[test]
    fn test_file_pattern_precedents() {
        let files = collect_md_files(
            Path::new("/nonexistent"),
            FilePattern::Precedents {
                case_type: "민사",
                court: "대법원",
            },
        );
        assert!(files.is_empty());
    }

    #[test]
    fn test_classify_empty() {
        // Non-existent source root should produce zero files to link.
        let files: Vec<PathBuf> = vec![];
        let tmp = std::env::temp_dir().join("zmd_test_classify");
        let _ = std::fs::create_dir_all(&tmp);
        let (already, to_link) = classify_files(&files, Path::new("/nonexistent"), &tmp).unwrap();
        assert_eq!(already, 0);
        assert!(to_link.is_empty());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn test_default_batch_size() {
        // Verify the default config has a reasonable batch size.
        let cfg = ZmdConfig::default_config().unwrap();
        assert!(cfg.batch_size > 0);
        assert!(cfg.batch_size <= 1000);
    }
}
