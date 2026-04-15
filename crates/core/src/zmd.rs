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
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use tracing::{info, warn};

// ── Spinner helper ────────────────────────────────────────────

/// Create a spinner on stderr with an elapsed-time display.
///
/// The spinner is hidden automatically when stderr is not a TTY
/// (e.g. when `--json` output is piped), so callers don't need to
/// check for JSON mode.
fn spinner(msg: &str) -> ProgressBar {
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        // Invariant: template string is a compile-time literal with valid indicatif placeholders.
        ProgressStyle::with_template("{spinner:.cyan} {msg} {elapsed}")
            .expect("valid template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    pb.set_message(msg.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));
    pb
}

// ── Configuration ─────────────────────────────────────────────

const LAWS_REPO: &str = "https://github.com/legalize-kr/legalize-kr.git";
const PRECEDENT_REPO: &str = "https://github.com/legalize-kr/precedent-kr.git";

/// Default case types to index for precedents.
const DEFAULT_CASE_TYPES: &[&str] = &["민사", "형사"];

/// Default court levels to index for precedents.
const DEFAULT_COURTS: &[&str] = &["대법원"];

/// Default batch size: number of files to stage before each `zmd update` call.
///
/// Smaller batches reduce per-update scan overhead (zmd checks every staged
/// file, not just the new ones) and give more frequent progress updates.
/// 100 files ≈ 6-10s per batch on first index.
const DEFAULT_BATCH_SIZE: usize = 100;

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
    /// Skip `git pull` when the repo already exists (avoids network round-trip).
    pub skip_pull: bool,
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
            skip_pull: false,
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

/// One precedent source scope discovered during collection.
#[derive(Debug, Clone)]
pub struct PrecedentCourtCount {
    pub label: String,
    pub total_files: usize,
}

/// Result of a full precedent indexing run.
#[derive(Debug, Clone)]
pub struct PrecedentIndexResult {
    pub courts: Vec<PrecedentCourtCount>,
    pub summary: IndexResult,
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
///
/// When `skip_pull` is true and the repo already exists, the pull is
/// skipped entirely — useful when the caller knows the repo is fresh.
fn clone_or_pull(url: &str, dir: &Path, name: &str, skip_pull: bool) -> Result<()> {
    if dir.join(".git").is_dir() {
        if skip_pull {
            info!(%name, "Repo exists — skipping pull (--skip-pull)");
            return Ok(());
        }
        info!(%name, "Pulling latest");
        let output = Command::new("git")
            .args(["-C"])
            .arg(dir)
            .args(["pull", "--ff-only", "--depth", "1"])
            .output()
            .with_context(|| format!("Failed to run git pull for {name}"))?;
        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            info!(%name, result = stdout.trim(), "Pull complete");
        } else {
            // Fast-forward failed — likely a diverged shallow clone.
            // These are read-only data repos so we can safely force-sync.
            warn!(%name, "Fast-forward pull failed; resetting to origin");
            let fetch = Command::new("git")
                .args(["-C"])
                .arg(dir)
                .args(["fetch", "--depth", "1", "origin"])
                .output()
                .with_context(|| format!("Failed to git fetch for {name}"))?;
            if fetch.status.success() {
                // Detect the default branch from the remote HEAD.
                let head_ref = Command::new("git")
                    .args(["-C"])
                    .arg(dir)
                    .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
                    .output()
                    .ok()
                    .and_then(|o| {
                        if o.status.success() {
                            String::from_utf8(o.stdout).ok().and_then(|s| {
                                s.trim()
                                    .strip_prefix("refs/remotes/origin/")
                                    .map(String::from)
                            })
                        } else {
                            None
                        }
                    })
                    .unwrap_or_else(|| "main".to_string());

                let reset = Command::new("git")
                    .args(["-C"])
                    .arg(dir)
                    .args(["reset", "--hard", &format!("origin/{head_ref}")])
                    .output()
                    .with_context(|| format!("Failed to git reset for {name}"))?;
                if reset.status.success() {
                    info!(%name, branch = %head_ref, "Reset to origin complete");
                } else {
                    let stderr = String::from_utf8_lossy(&reset.stderr);
                    warn!(%name, %stderr, "git reset --hard failed (non-fatal)");
                }
            } else {
                let stderr = String::from_utf8_lossy(&fetch.stderr);
                warn!(%name, %stderr, "git fetch failed (non-fatal)");
            }
        }
    } else {
        let sp = spinner(&format!("Cloning {name} (shallow)"));
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
        sp.finish_and_clear();
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
fn collect_md_files(src_root: &Path, pattern: &FilePattern<'_>) -> Vec<PathBuf> {
    let mut files = Vec::new();
    match *pattern {
        FilePattern::Laws => {
            let kr_dir = src_root.join("kr");
            if let Ok(entries) = std::fs::read_dir(&kr_dir) {
                for entry in entries.flatten() {
                    // Each entry is a directory like kr/<law_name>/; check for 법률.md inside.
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
                    if path.extension().and_then(|e| e.to_str()) == Some("md")
                        // Use file_type() from DirEntry (no extra stat on most platforms)
                        && entry.file_type().is_ok_and(|ft| ft.is_file())
                    {
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
///
/// Scans the stage directory once (via `read_dir`) to build a set of
/// existing filenames, then checks membership — O(1) per file instead
/// of one `stat()` syscall per file.
fn classify_files(
    files: &[PathBuf],
    src_root: &Path,
    stage_root: &Path,
) -> Result<(usize, Vec<(PathBuf, PathBuf)>)> {
    std::fs::create_dir_all(stage_root)
        .with_context(|| format!("Failed to create stage dir {}", stage_root.display()))?;

    // Build a set of all paths that already exist in the stage tree.
    // We walk the stage directory once instead of calling stat() per file.
    let existing = scan_existing_paths(stage_root);

    let mut to_link = Vec::new();
    let mut already_staged = 0usize;

    for src in files {
        let rel = src
            .strip_prefix(src_root)
            .with_context(|| format!("File {} not under {}", src.display(), src_root.display()))?;
        let dst = stage_root.join(rel);

        if existing.contains(dst.as_path()) && paths_match(src, &dst) {
            already_staged += 1;
        } else {
            to_link.push((src.clone(), dst));
        }
    }

    Ok((already_staged, to_link))
}

fn paths_match(src: &Path, dst: &Path) -> bool {
    let Ok(src_meta) = std::fs::metadata(src) else {
        return false;
    };
    let Ok(dst_meta) = std::fs::metadata(dst) else {
        return false;
    };

    src_meta.len() == dst_meta.len()
        && src_meta
            .modified()
            .ok()
            .map(crate::native_indexer::system_time_to_unix_nanos)
            == dst_meta
                .modified()
                .ok()
                .map(crate::native_indexer::system_time_to_unix_nanos)
}

/// Recursively scan a directory and collect all file paths into a `HashSet`.
fn scan_existing_paths(root: &Path) -> std::collections::HashSet<PathBuf> {
    let mut set = std::collections::HashSet::new();
    scan_existing_paths_inner(root, &mut set);
    set
}

fn scan_existing_paths_inner(dir: &Path, set: &mut std::collections::HashSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_dir() {
            scan_existing_paths_inner(&entry.path(), set);
        } else if ft.is_file() {
            set.insert(entry.path());
        }
    }
}

/// Hardlink a batch of `(src, dst)` pairs in parallel via Rayon.
///
/// Creates necessary parent directories first (sequential), then
/// hardlinks all files in parallel, replacing stale staged files when
/// needed. Returns count of successful links.
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
        .for_each(|(src, dst)| match replace_hard_link(src, dst) {
            Ok(()) => {
                linked.fetch_add(1, Ordering::Relaxed);
            }
            Err(e) => {
                warn!(src = %src.display(), dst = %dst.display(), error = %e, "Hardlink failed");
            }
        });
    linked.load(Ordering::Relaxed)
}

fn replace_hard_link(src: &Path, dst: &Path) -> std::io::Result<()> {
    match std::fs::hard_link(src, dst) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(dst)?;
            std::fs::hard_link(src, dst)
        }
        Err(e) => Err(e),
    }
}

#[derive(Default)]
struct FileEntryPlan {
    entries: Vec<crate::native_indexer::FileEntry>,
    unchanged_by_metadata: usize,
}

fn build_file_entry_plan(
    files: &[PathBuf],
    src_root: &Path,
    existing_docs: &std::collections::HashMap<String, crate::native_indexer::ExistingDoc>,
) -> FileEntryPlan {
    let mut entries = Vec::new();
    let mut unchanged_by_metadata = 0usize;

    for src in files {
        let Ok(rel) = src.strip_prefix(src_root) else {
            continue;
        };
        let Ok(metadata) = std::fs::metadata(src) else {
            continue;
        };
        let source_size = metadata.len();
        let source_mtime_ns = metadata
            .modified()
            .map(crate::native_indexer::system_time_to_unix_nanos)
            .unwrap_or_default();
        let rel_path = rel.to_string_lossy().to_string();

        if existing_docs.get(&rel_path).is_some_and(|doc| {
            doc.source_size == Some(source_size) && doc.source_mtime_ns == Some(source_mtime_ns)
        }) {
            unchanged_by_metadata += 1;
            continue;
        }

        entries.push(crate::native_indexer::FileEntry {
            path: rel_path,
            staged_path: src.to_path_buf(),
            source_size,
            source_mtime_ns,
        });
    }

    FileEntryPlan {
        entries,
        unchanged_by_metadata,
    }
}

/// Stage files and index using the native indexer.
///
/// This is the core indexing loop.  All files are hardlinked into the
/// stage directory (Rayon parallel), then the entire collection is
/// indexed in a single pass using the native Rust indexer that writes
/// directly to `.qmd/data.db`.
///
/// The native indexer:
/// - Skips already-indexed content (SHA-256 dedup, same as zmd)
/// - Processes files in parallel (Rayon) for hashing, chunking, embedding
/// - Writes everything in a single `SQLite` transaction
///
/// Already-staged files are skipped for linking (no re-linking needed).
/// Safe to interrupt and re-run — picks up where it left off.
fn stage_and_index_batched<F>(
    files: &[PathBuf],
    src_root: &Path,
    stage_root: &Path,
    collection_name: &str,
    _batch_size: usize,
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
        collection = collection_name,
        "Starting native stage+index"
    );

    // ── Stage phase: hardlink all unstaged files at once ──────────
    let newly_staged = if to_link.is_empty() {
        0
    } else {
        hardlink_batch(&to_link)
    };

    let db_path = crate::native_indexer::default_db_path();
    let mut db = crate::native_indexer::ZmdDb::open(&db_path)
        .context("Failed to open zmd database for native indexing")?;

    // Register the collection (idempotent, same as `zmd collection add`).
    db.register_collection(collection_name, stage_root)?;

    let existing_docs = db.existing_docs(collection_name)?;
    let plan = build_file_entry_plan(files, src_root, &existing_docs);
    let total_files = plan.entries.len();

    info!(
        collection = collection_name,
        candidate_files = total_files,
        unchanged_by_metadata = plan.unchanged_by_metadata,
        total_source_files = files.len(),
        "Starting native indexing"
    );

    if plan.entries.is_empty() {
        let output = format!(
            "Indexed 0 documents (0 new, {} unchanged by metadata) in 0.0s",
            plan.unchanged_by_metadata
        );
        info!(collection = collection_name, %output, "Native indexing complete");
        on_batch(&BatchProgress {
            batch_num: 1,
            batch_new: 0,
            total_staged: already_staged + newly_staged,
            total_files: files.len(),
            update_secs: 0.0,
            update_output: output,
        });
        return Ok(IndexResult {
            total_files: files.len(),
            already_staged,
            newly_staged,
            batches: 1,
            total_update_secs: 0.0,
        });
    }

    let start = Instant::now();

    // Create a progress bar.
    let pb = ProgressBar::new(total_files as u64);
    // Invariant: template string is a compile-time literal with valid indicatif placeholders.
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.cyan} Indexing {msg} [{bar:30.cyan/dim}] {pos}/{len} {elapsed}",
        )
        .expect("valid template")
        .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏")
        .progress_chars("━╸─"),
    );
    pb.set_message(collection_name.to_string());
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let stats = db.index_collection(collection_name, &plan.entries, |current, _total| {
        pb.set_position(current as u64);
    })?;

    pb.finish_and_clear();
    let elapsed = start.elapsed().as_secs_f64();

    let output = format!(
        "Indexed {} documents ({} new, {} metadata refresh, {} rehashed, {} unchanged by metadata) in {elapsed:.1}s",
        stats.indexed,
        stats.new,
        stats.metadata_refreshed,
        stats.content_rehashed,
        plan.unchanged_by_metadata
    );
    info!(collection = collection_name, %output, "Native indexing complete");

    on_batch(&BatchProgress {
        batch_num: 1,
        batch_new: stats.new,
        total_staged: already_staged + newly_staged,
        total_files: files.len(),
        update_secs: elapsed,
        update_output: output,
    });

    Ok(IndexResult {
        total_files: files.len(),
        already_staged,
        newly_staged,
        batches: 1,
        total_update_secs: elapsed,
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
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_dir() {
                count += walkdir(&entry.path());
            } else if ft.is_file()
                && entry.path().extension().and_then(|e| e.to_str()) == Some("md")
            {
                count += 1;
            }
        }
    }
    count
}

// ── zmd CLI wrappers ──────────────────────────────────────────

/// Check that `zmd` is on PATH; bail with a clear message if not.
fn ensure_zmd() -> Result<()> {
    let ok = Command::new("zmd")
        .arg("version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !ok {
        bail!("zmd is not installed or not on PATH");
    }
    Ok(())
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
///
/// Accepts a pre-fetched `zmd collection list` output.
fn remove_collection(name: &str, cached_list: &str) -> Result<()> {
    let pattern = format!("{name}:");
    if !cached_list
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
fn run_zmd_cleanup() {
    let _ = Command::new("zmd").arg("cleanup").output();
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
///
/// # Errors
///
/// Returns an error if zmd is not installed, git operations fail, or
/// staging/indexing encounters an I/O error.
pub fn index_laws<F>(cfg: &ZmdConfig, on_batch: F) -> Result<IndexResult>
where
    F: FnMut(&BatchProgress),
{
    clone_or_pull(
        LAWS_REPO,
        &cfg.laws_clone(),
        "legalize-kr (laws)",
        cfg.skip_pull,
    )?;

    let files = collect_md_files(&cfg.laws_clone(), &FilePattern::Laws);
    info!(count = files.len(), "Found 법률.md files");

    stage_and_index_batched(
        &files,
        &cfg.laws_clone(),
        &cfg.laws_stage(),
        "laws",
        cfg.batch_size,
        on_batch,
    )
}

/// Index precedent files into zmd.
///
/// 1. Clone/pull the precedent-kr repo.
/// 2. For each `case_type` × court, find `.md` files and stage in batches.
/// 3. Call `zmd update` after each batch.
///
/// `on_batch` fires after each batch. `on_court` fires when a new court starts.
///
/// # Errors
///
/// Returns an error if zmd is not installed, git operations fail, or
/// staging/indexing encounters an I/O error.
pub fn index_precedents<F, G>(
    cfg: &ZmdConfig,
    mut on_court: G,
    mut on_batch: F,
) -> Result<PrecedentIndexResult>
where
    F: FnMut(&str, &str, &BatchProgress),
    G: FnMut(&str, &str, usize),
{
    clone_or_pull(
        PRECEDENT_REPO,
        &cfg.precedent_clone(),
        "precedent-kr (precedents)",
        cfg.skip_pull,
    )?;

    // ── Phase 1: Collect files for all case_type × court combos ──
    let mut all_files: Vec<PathBuf> = Vec::new();
    let mut court_counts: Vec<(String, String, usize)> = Vec::new();

    for case_type in &cfg.case_types {
        for court in &cfg.courts {
            let files = collect_md_files(
                &cfg.precedent_clone(),
                &FilePattern::Precedents { case_type, court },
            );

            if files.is_empty() {
                warn!(%case_type, %court, "No files found — skipping");
                continue;
            }

            on_court(case_type, court, files.len());
            court_counts.push((case_type.clone(), court.clone(), files.len()));
            all_files.extend(files);
        }
    }

    if all_files.is_empty() {
        return Ok(PrecedentIndexResult {
            courts: Vec::new(),
            summary: IndexResult {
                total_files: 0,
                already_staged: 0,
                newly_staged: 0,
                batches: 0,
                total_update_secs: 0.0,
            },
        });
    }

    // ── Phase 2: Stage + index all files in a single pass ────────
    let result = stage_and_index_batched(
        &all_files,
        &cfg.precedent_clone(),
        &cfg.precedent_stage(),
        "precedents",
        cfg.batch_size,
        |bp| {
            // Report the batch to the first court (for backward compat).
            if let Some((ct, co, _)) = court_counts.first() {
                on_batch(ct, co, bp);
            }
        },
    )?;

    Ok(PrecedentIndexResult {
        courts: court_counts
            .into_iter()
            .map(|(ct, co, total_files)| PrecedentCourtCount {
                label: format!("{ct}/{co}"),
                total_files,
            })
            .collect(),
        summary: result,
    })
}

/// Run both laws and precedents indexing.
///
/// # Errors
///
/// Returns an error if git operations fail or any indexing phase fails.
pub fn index_all(cfg: &ZmdConfig) -> Result<()> {
    info!("Phase 1/2: Laws (법률 only)");
    let law_result = index_laws(cfg, |bp| {
        if bp.batch_num > 0 {
            info!(
                batch = bp.batch_num,
                new = bp.batch_new,
                staged = bp.total_staged,
                secs = format!("{:.0}", bp.update_secs),
                "laws batch complete",
            );
        }
    })?;
    info!(
        total = law_result.total_files,
        new = law_result.newly_staged,
        existing = law_result.already_staged,
        secs = format!("{:.0}", law_result.total_update_secs),
        "Laws done",
    );

    info!("Phase 2/2: Precedents");
    let prec_result = index_precedents(
        cfg,
        |ct, court, count| {
            info!(%ct, %court, count, "precedent court");
        },
        |ct, court, bp| {
            if bp.batch_num > 0 {
                info!(
                    %ct,
                    %court,
                    batch = bp.batch_num,
                    new = bp.batch_new,
                    staged = bp.total_staged,
                    secs = format!("{:.0}", bp.update_secs),
                    "precedent batch complete",
                );
            }
        },
    )?;

    let total_new = prec_result.summary.newly_staged;
    let total_files = prec_result.summary.total_files;
    let total_secs = prec_result.summary.total_update_secs;
    info!(
        total_files,
        total_new,
        secs = format!("{total_secs:.0}"),
        "Precedents done",
    );

    Ok(())
}

/// Pull latest from upstream repos and re-index.
///
/// # Errors
///
/// Returns an error if git operations fail or any indexing phase fails.
pub fn sync(cfg: &ZmdConfig) -> Result<()> {
    if cfg.laws_clone().join(".git").is_dir() {
        info!("Syncing laws...");
        let result = index_laws(cfg, |bp| {
            if bp.batch_new > 0 {
                info!(
                    batch = bp.batch_num,
                    new = bp.batch_new,
                    secs = format!("{:.0}", bp.update_secs),
                    "sync laws batch",
                );
            }
        })?;
        info!(
            new = result.newly_staged,
            secs = format!("{:.0}", result.total_update_secs),
            "Laws sync complete",
        );
    }

    if cfg.precedent_clone().join(".git").is_dir() {
        info!("Syncing precedents...");
        let result = index_precedents(
            cfg,
            |_, _, _| {},
            |ct, court, bp| {
                if bp.batch_new > 0 {
                    info!(
                        %ct,
                        %court,
                        batch = bp.batch_num,
                        new = bp.batch_new,
                        secs = format!("{:.0}", bp.update_secs),
                        "sync precedent batch",
                    );
                }
            },
        )?;
        let total_new = result.summary.newly_staged;
        let total_secs = result.summary.total_update_secs;
        info!(
            total_new,
            secs = format!("{total_secs:.0}"),
            "Precedents sync complete",
        );
    }

    Ok(())
}

/// Gather status information about repos, staged files, and zmd state.
///
/// # Errors
///
/// Returns an error if zmd CLI commands fail unexpectedly.
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
///
/// # Errors
///
/// Returns an error if zmd is not installed or cleanup/removal fails.
pub fn reset(cfg: &ZmdConfig) -> Result<()> {
    ensure_zmd()?;

    info!("Removing zmd collections and staged data");

    let collection_list = run_zmd_collection_list().unwrap_or_default();
    remove_collection("laws", &collection_list)?;
    remove_collection("precedents", &collection_list)?;

    let stage = cfg.stage_dir();
    if stage.is_dir() {
        std::fs::remove_dir_all(&stage)
            .with_context(|| format!("Failed to remove {}", stage.display()))?;
    }

    run_zmd_cleanup();

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
        let files = collect_md_files(Path::new("/nonexistent"), &FilePattern::Laws);
        assert!(files.is_empty());
    }

    #[test]
    fn test_file_pattern_precedents() {
        let files = collect_md_files(
            Path::new("/nonexistent"),
            &FilePattern::Precedents {
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

    #[test]
    fn test_paths_match_detects_changes() {
        let root = std::env::temp_dir().join(format!("zmd_paths_match_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let src = root.join("src.md");
        let dst = root.join("dst.md");
        std::fs::write(&src, "one").unwrap();
        std::fs::hard_link(&src, &dst).unwrap();
        assert!(paths_match(&src, &dst));

        std::fs::remove_file(&dst).unwrap();
        std::fs::write(&dst, "one").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(&src, "two two").unwrap();
        assert!(!paths_match(&src, &dst));

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn test_replace_hard_link_replaces_existing_file() {
        let root = std::env::temp_dir().join(format!("zmd_replace_link_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();

        let src = root.join("src.md");
        let dst = root.join("dst.md");
        std::fs::write(&src, "fresh").unwrap();
        std::fs::write(&dst, "stale").unwrap();

        replace_hard_link(&src, &dst).unwrap();

        assert_eq!(std::fs::read_to_string(&dst).unwrap(), "fresh");

        let _ = std::fs::remove_dir_all(&root);
    }
}
