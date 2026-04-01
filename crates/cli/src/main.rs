use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::LawEntry;
use legal_ko_core::search::{self, Searcher};
use legal_ko_core::tts;
use legal_ko_core::{cache, client, parser};

#[derive(Parser)]
#[command(
    name = "legal-ko-cli",
    about = "CLI for Korean law lookup (LLM-friendly)"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List all laws (optionally filtered)
    List {
        /// Filter by category (법령구분)
        #[arg(long)]
        category: Option<String>,

        /// Filter by department (소관부처)
        #[arg(long)]
        department: Option<String>,

        /// Only show bookmarked laws
        #[arg(long)]
        bookmarks: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Maximum number of results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Search laws by title
    Search {
        /// Search query
        query: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Maximum number of results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show a law's full content
    Show {
        /// Law ID (법령MST number)
        id: String,

        /// Output as JSON (includes raw markdown)
        #[arg(long)]
        json: bool,
    },
    /// List articles (제X조) in a law
    Articles {
        /// Law ID (법령MST number)
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List bookmarked laws
    Bookmarks {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read a law aloud using TTS (`VibeVoice`).
    ///
    /// Build with --release for smooth playback (debug builds are 10-50x slower).
    Speak {
        /// Law ID (법령MST number)
        id: String,

        /// Read only a specific article (0-indexed)
        #[arg(long)]
        article: Option<usize>,

        /// Voice preset name
        #[arg(long, default_value = "kr-spk0_woman")]
        voice: String,

        /// Use fast synthesis profile (`cfg_scale`=1.0, 1s prebuffer)
        #[arg(long)]
        fast: bool,

        /// Output synthesis stats as JSON (no playback)
        #[arg(long)]
        json: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Command::List {
            category,
            department,
            bookmarks,
            json,
            limit,
        } => cmd_list(category, department, bookmarks, json, limit).await,
        Command::Search { query, json, limit } => cmd_search(&query, json, limit).await,
        Command::Show { id, json } => cmd_show(&id, json).await,
        Command::Articles { id, json } => cmd_articles(&id, json).await,
        Command::Bookmarks { json } => cmd_bookmarks(json).await,
        Command::Speak {
            id,
            article,
            voice,
            fast,
            json,
        } => cmd_speak(&id, article, &voice, fast, json).await,
    }
}

async fn load_entries() -> Result<Vec<LawEntry>> {
    let index = client::fetch_metadata().await?;
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
    entries.sort_by(|a, b| a.title.cmp(&b.title));
    Ok(entries)
}

fn apply_filters<'a>(
    entries: &'a [LawEntry],
    category: Option<&str>,
    department: Option<&str>,
    bookmarks_only: bool,
    bookmarks: &Bookmarks,
) -> Vec<&'a LawEntry> {
    entries
        .iter()
        .filter(|e| {
            if let Some(cat) = category
                && e.category != cat
            {
                return false;
            }
            if let Some(dept) = department
                && !e.departments.iter().any(|d| d == dept)
            {
                return false;
            }
            if bookmarks_only && !bookmarks.is_bookmarked(&e.id) {
                return false;
            }
            true
        })
        .collect()
}

fn print_entries(entries: &[&LawEntry], as_json: bool) -> Result<()> {
    if as_json {
        let items: Vec<_> = entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "title": e.title,
                    "category": e.category,
                    "departments": e.departments,
                    "enforcement_date": e.enforcement_date,
                    "status": e.status,
                    "path": e.path,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        for e in entries {
            println!(
                "{}\t{}\t[{}]\t{}",
                e.id,
                e.title,
                e.category,
                e.departments.join(", ")
            );
        }
    }
    Ok(())
}

async fn cmd_list(
    category: Option<String>,
    department: Option<String>,
    bookmarks_only: bool,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let entries = load_entries().await?;
    let bm = Bookmarks::load();
    let mut filtered = apply_filters(
        &entries,
        category.as_deref(),
        department.as_deref(),
        bookmarks_only,
        &bm,
    );
    if let Some(n) = limit {
        filtered.truncate(n);
    }
    print_entries(&filtered, as_json)?;
    Ok(())
}

async fn cmd_search(query: &str, as_json: bool, limit: Option<usize>) -> Result<()> {
    let entries = load_entries().await?;
    let n = limit.unwrap_or(50);

    let searcher = Searcher::from_env();
    let ids = if searcher.is_enabled() {
        match searcher.warmup(&entries).await {
            Ok(()) => searcher
                .search_ids(query, n)
                .await
                .unwrap_or_else(|_| search::naive_search_ids(&entries, query, n)),
            Err(_) => search::naive_search_ids(&entries, query, n),
        }
    } else {
        search::naive_search_ids(&entries, query, n)
    };

    let by_id: std::collections::HashMap<&str, &LawEntry> =
        entries.iter().map(|e| (e.id.as_str(), e)).collect();
    let results: Vec<&LawEntry> = ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).copied())
        .collect();

    print_entries(&results, as_json)?;
    Ok(())
}

async fn cmd_show(id: &str, as_json: bool) -> Result<()> {
    let entries = load_entries().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("Law not found: {id}"))?;

    // Try cache first, then fetch
    let content = if let Some(c) = cache::read_cache(&entry.path)? {
        c
    } else {
        let c = client::fetch_law_content(&entry.path).await?;
        let _ = cache::write_cache(&entry.path, &c);
        c
    };

    let stripped = parser::strip_frontmatter(&content);

    if as_json {
        let obj = json!({
            "id": entry.id,
            "title": entry.title,
            "category": entry.category,
            "departments": entry.departments,
            "content": stripped,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("{stripped}");
    }
    Ok(())
}

async fn cmd_articles(id: &str, as_json: bool) -> Result<()> {
    let entries = load_entries().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("Law not found: {id}"))?;

    let content = if let Some(c) = cache::read_cache(&entry.path)? {
        c
    } else {
        let c = client::fetch_law_content(&entry.path).await?;
        let _ = cache::write_cache(&entry.path, &c);
        c
    };

    let articles = parser::extract_articles(&content);

    if as_json {
        let items: Vec<_> = articles
            .iter()
            .map(|a| {
                json!({
                    "label": a.label,
                    "line_index": a.line_index,
                })
            })
            .collect();
        let obj = json!({
            "id": entry.id,
            "title": entry.title,
            "articles": items,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("# {} — {}", entry.id, entry.title);
        for a in &articles {
            println!("  L{}: {}", a.line_index, a.label);
        }
    }
    Ok(())
}

async fn cmd_bookmarks(as_json: bool) -> Result<()> {
    let bm = Bookmarks::load();
    let entries = load_entries().await?;
    let results: Vec<&LawEntry> = entries.iter().filter(|e| bm.is_bookmarked(&e.id)).collect();
    print_entries(&results, as_json)?;
    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn cmd_speak(
    id: &str,
    article: Option<usize>,
    voice: &str,
    fast: bool,
    as_json: bool,
) -> Result<()> {
    // Start engine loading immediately in a background thread so it overlaps
    // with the async network I/O below (metadata fetch, law content fetch).
    // This hides ~3-5s of model loading latency.
    let engine_handle = tts::new_engine_handle();
    let engine_handle_bg = engine_handle.clone();
    let engine_load = tokio::task::spawn_blocking(move || {
        let project_root = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());
        tts::with_suppressed_output(|| tts::load_engine(&engine_handle_bg, &project_root))
    });

    // Fetch metadata and law content concurrently with engine loading.
    let entries = load_entries().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("Law not found: {id}"))?;

    let content = if let Some(c) = cache::read_cache(&entry.path)? {
        c
    } else {
        let c = client::fetch_law_content(&entry.path).await?;
        let _ = cache::write_cache(&entry.path, &c);
        c
    };

    // Wait for engine to finish loading before starting synthesis.
    engine_load.await??;

    let voice = voice.to_string();
    let profile = if fast {
        tts::TtsProfile::Fast
    } else {
        tts::TtsProfile::Balanced
    };

    if let Some(idx) = article {
        // Single article — use streaming (one segment, no gaps)
        let text = parser::extract_article_text(&content, idx)
            .ok_or_else(|| anyhow::anyhow!("Article index {idx} not found"))?;
        if text.is_empty() {
            anyhow::bail!("No text content to speak");
        }

        let result = tokio::task::spawn_blocking(move || {
            tts::with_suppressed_output(|| {
                tts::synthesize_and_play_with_handle(&engine_handle, &text, &voice, profile)
            })
        })
        .await??;

        if as_json {
            let obj = json!({
                "id": entry.id,
                "title": entry.title,
                "article_index": article,
                "duration_secs": result.duration_secs,
                "generation_time_secs": result.generation_time_secs,
                "rtf": result.rtf,
            });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!(
                "Spoke {:.1}s of audio in {:.1}s (RTF: {:.2})",
                result.duration_secs, result.generation_time_secs, result.rtf
            );
        }
    } else {
        // Full text — article-level batch synthesis for smooth playback.
        // Each article is fully synthesized before being appended to the
        // player as one large buffer (no micro-chunk gaps).  Playback of
        // earlier articles overlaps with synthesis of later ones.
        let articles = parser::extract_articles(&content);
        let segments: Vec<String> = if articles.is_empty() {
            // No articles found — fall back to full text as one segment
            let full = parser::extract_full_text(&content);
            if full.is_empty() {
                anyhow::bail!("No text content to speak");
            }
            vec![full]
        } else {
            articles
                .iter()
                .enumerate()
                .filter_map(|(i, _)| parser::extract_article_text(&content, i))
                .filter(|t| !t.is_empty())
                .collect()
        };

        if segments.is_empty() {
            anyhow::bail!("No text content to speak");
        }

        let n_segments = segments.len();
        eprintln!("Synthesizing {n_segments} article(s)...");

        let stats = tokio::task::spawn_blocking(move || {
            tts::with_suppressed_output(|| {
                tts::synthesize_and_play_segments_with_handle(
                    &engine_handle,
                    &segments,
                    &voice,
                    profile.cfg_scale(),
                )
            })
        })
        .await??;

        if as_json {
            let obj = json!({
                "id": entry.id,
                "title": entry.title,
                "segments": stats.segments,
                "duration_secs": stats.duration_secs,
                "generation_time_secs": stats.generation_time_secs,
                "rtf": stats.rtf,
            });
            println!("{}", serde_json::to_string_pretty(&obj)?);
        } else {
            eprintln!(
                "Spoke {:.1}s of audio ({} articles) in {:.1}s (RTF: {:.2})",
                stats.duration_secs, stats.segments, stats.generation_time_secs, stats.rtf
            );
        }
    }

    Ok(())
}
