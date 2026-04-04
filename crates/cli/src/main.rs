use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::{self, LawEntry};
use legal_ko_core::search::{self, Searcher};
#[cfg(feature = "tts")]
use legal_ko_core::tts;
use legal_ko_core::{client, parser, reqwest};

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
        /// Law ID (e.g. "kr/민법/법률")
        id: String,

        /// Output as JSON (includes raw markdown)
        #[arg(long)]
        json: bool,
    },
    /// List articles (제X조) in a law
    Articles {
        /// Law ID (e.g. "kr/민법/법률")
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
    /// Show the current TUI browsing context (for `OpenCode` integration)
    Context {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Send a navigate command to the TUI (for `OpenCode` integration).
    ///
    /// On list view the TUI scrolls to the law. On detail view it jumps
    /// to the specified article within the currently viewed law.
    Navigate {
        /// Law ID (e.g. "kr/민법/법률")
        id: String,

        /// Article label to jump to in detail view (e.g. "제3조")
        #[arg(long)]
        article: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Read a law aloud using TTS (`VibeVoice`).
    ///
    /// Build with --release for smooth playback (debug builds are 10-50x slower).
    #[cfg(feature = "tts")]
    Speak {
        /// Law ID (e.g. "kr/민법/법률")
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
    // Initialize tracing to stderr; respects RUST_LOG (default: warn).
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let cli = Cli::parse();
    let client = client::http_client()?;

    match cli.command {
        Command::List {
            category,
            department,
            bookmarks,
            json,
            limit,
        } => cmd_list(&client, category, department, bookmarks, json, limit).await,
        Command::Search { query, json, limit } => cmd_search(&client, &query, json, limit).await,
        Command::Show { id, json } => cmd_show(&client, &id, json).await,
        Command::Articles { id, json } => cmd_articles(&client, &id, json).await,
        Command::Bookmarks { json } => cmd_bookmarks(&client, json).await,
        Command::Context { json } => cmd_context(json),
        Command::Navigate { id, article, json } => cmd_navigate(&id, article, json),
        #[cfg(feature = "tts")]
        Command::Speak {
            id,
            article,
            voice,
            fast,
            json,
        } => cmd_speak(&client, &id, article, &voice, fast, json).await,
    }
}

fn cmd_navigate(id: &str, article: Option<String>, as_json: bool) -> Result<()> {
    use legal_ko_core::context::{TuiCommand, write_command};

    let cmd = TuiCommand {
        action: "navigate".to_string(),
        law_id: id.to_string(),
        article,
        timestamp: chrono::Utc::now().to_rfc3339(),
    };
    write_command(&cmd)?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&cmd)?);
    } else {
        print!("navigate → {}", cmd.law_id);
        if let Some(ref art) = cmd.article {
            print!(" (article: {art})");
        }
        println!();
    }
    Ok(())
}

async fn load_entries(client: &reqwest::Client) -> Result<Vec<LawEntry>> {
    let index = client::fetch_metadata(client)
        .await
        .context("Failed to load law metadata from GitHub")?;
    Ok(models::entries_from_index(index))
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
    client: &reqwest::Client,
    category: Option<String>,
    department: Option<String>,
    bookmarks_only: bool,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let entries = load_entries(client).await?;
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

async fn cmd_search(
    client: &reqwest::Client,
    query: &str,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let entries = load_entries(client).await?;
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

async fn cmd_show(client: &reqwest::Client, id: &str, as_json: bool) -> Result<()> {
    let path = format!("{id}.md");
    let content = client::load_law_content(client, &path).await?;

    let stripped = parser::strip_frontmatter(&content);

    if as_json {
        let mut entry = LawEntry {
            id: id.to_string(),
            title: String::new(),
            category: String::new(),
            departments: Vec::new(),
            enforcement_date: String::new(),
            status: String::new(),
            path,
        };
        parser::enrich_entry_from_frontmatter(&mut entry, &content);

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

async fn cmd_articles(client: &reqwest::Client, id: &str, as_json: bool) -> Result<()> {
    let path = format!("{id}.md");
    let content = client::load_law_content(client, &path).await?;

    let articles = parser::extract_articles(&content);

    if as_json {
        let mut entry = LawEntry {
            id: id.to_string(),
            title: String::new(),
            category: String::new(),
            departments: Vec::new(),
            enforcement_date: String::new(),
            status: String::new(),
            path,
        };
        parser::enrich_entry_from_frontmatter(&mut entry, &content);

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
        let mut title = String::new();
        let fm = parser::parse_frontmatter(&content);
        if let Some(t) = fm.get("제목") {
            title = t.as_str().to_string();
        }
        println!("# {id} — {title}");
        for a in &articles {
            println!("  L{}: {}", a.line_index, a.label);
        }
    }
    Ok(())
}

async fn cmd_bookmarks(client: &reqwest::Client, as_json: bool) -> Result<()> {
    let bm = Bookmarks::load();
    let entries = load_entries(client).await?;
    let results: Vec<&LawEntry> = entries.iter().filter(|e| bm.is_bookmarked(&e.id)).collect();
    print_entries(&results, as_json)?;
    Ok(())
}

fn cmd_context(as_json: bool) -> Result<()> {
    let ctx = legal_ko_core::context::read_context()
        .context("No TUI context found — is legal-ko running?")?;

    if as_json {
        println!("{}", serde_json::to_string_pretty(&ctx)?);
    } else {
        println!("view: {}", ctx.view);
        println!("timestamp: {}", ctx.timestamp);
        if let Some(ref law) = ctx.selected_law {
            println!("selected: {} — {}", law.id, law.title);
        }
        if let Some(ref f) = ctx.filters {
            if !f.search_query.is_empty() {
                println!("search: {}", f.search_query);
            }
            if let Some(ref c) = f.category {
                println!("category: {c}");
            }
            if let Some(ref d) = f.department {
                println!("department: {d}");
            }
            println!("laws: {}/{}", f.filtered_count, f.total_laws);
        }
        if let Some(ref d) = ctx.detail {
            println!("detail: {} — {}", d.law_id, d.law_title);
            println!("scroll: {}/{}", d.scroll_position, d.total_lines);
            if let Some(ref art) = d.current_article {
                println!("article: [{}] {}", art.index, art.label);
            }
            println!("articles: {}", d.total_articles);
        }
    }
    Ok(())
}

#[cfg(feature = "tts")]
#[allow(clippy::too_many_lines)]
async fn cmd_speak(
    client: &reqwest::Client,
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

    // Fetch law content concurrently with engine loading.
    let path = format!("{id}.md");
    let content_future = client::load_law_content(client, &path);

    // Wait for engine to finish loading before starting synthesis.
    engine_load
        .await
        .context("TTS engine loading task panicked")?
        .context("TTS engine failed to load")?;

    let content = content_future.await?;

    let mut title = String::new();
    let fm = parser::parse_frontmatter(&content);
    if let Some(t) = fm.get("제목") {
        title = t.as_str().to_string();
    }

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
                "id": id,
                "title": title,
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
                "id": id,
                "title": title,
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
