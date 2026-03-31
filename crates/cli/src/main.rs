use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::LawEntry;
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
    /// Read a law aloud using TTS (VibeVoice)
    Speak {
        /// Law ID (법령MST number)
        id: String,

        /// Read only a specific article (0-indexed)
        #[arg(long)]
        article: Option<usize>,

        /// Voice preset name
        #[arg(long, default_value = "kr-spk0_woman")]
        voice: String,

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
            json,
        } => cmd_speak(&id, article, &voice, json).await,
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
    category: &Option<String>,
    department: &Option<String>,
    bookmarks_only: bool,
    bookmarks: &Bookmarks,
) -> Vec<&'a LawEntry> {
    entries
        .iter()
        .filter(|e| {
            if let Some(cat) = category
                && &e.category != cat
            {
                return false;
            }
            if let Some(dept) = department
                && !e.departments.contains(dept)
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

fn print_entries(entries: &[&LawEntry], as_json: bool) {
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
        println!("{}", serde_json::to_string_pretty(&items).unwrap());
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
    let mut filtered = apply_filters(&entries, &category, &department, bookmarks_only, &bm);
    if let Some(n) = limit {
        filtered.truncate(n);
    }
    print_entries(&filtered, as_json);
    Ok(())
}

async fn cmd_search(query: &str, as_json: bool, limit: Option<usize>) -> Result<()> {
    let entries = load_entries().await?;
    let query_lower = query.to_lowercase();
    let mut results: Vec<&LawEntry> = entries
        .iter()
        .filter(|e| e.title.to_lowercase().contains(&query_lower))
        .collect();
    if let Some(n) = limit {
        results.truncate(n);
    }
    print_entries(&results, as_json);
    Ok(())
}

async fn cmd_show(id: &str, as_json: bool) -> Result<()> {
    let entries = load_entries().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("Law not found: {id}"))?;

    // Try cache first, then fetch
    let content = match cache::read_cache(&entry.path)? {
        Some(c) => c,
        None => {
            let c = client::fetch_law_content(&entry.path).await?;
            let _ = cache::write_cache(&entry.path, &c);
            c
        }
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
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
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

    let content = match cache::read_cache(&entry.path)? {
        Some(c) => c,
        None => {
            let c = client::fetch_law_content(&entry.path).await?;
            let _ = cache::write_cache(&entry.path, &c);
            c
        }
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
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
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
    print_entries(&results, as_json);
    Ok(())
}

async fn cmd_speak(id: &str, article: Option<usize>, voice: &str, as_json: bool) -> Result<()> {
    let entries = load_entries().await?;
    let entry = entries
        .iter()
        .find(|e| e.id == id)
        .ok_or_else(|| anyhow::anyhow!("Law not found: {id}"))?;

    let content = match cache::read_cache(&entry.path)? {
        Some(c) => c,
        None => {
            let c = client::fetch_law_content(&entry.path).await?;
            let _ = cache::write_cache(&entry.path, &c);
            c
        }
    };

    let text = if let Some(idx) = article {
        parser::extract_article_text(&content, idx)
            .ok_or_else(|| anyhow::anyhow!("Article index {idx} not found"))?
    } else {
        parser::extract_full_text(&content)
    };

    if text.is_empty() {
        anyhow::bail!("No text content to speak");
    }

    let voice = voice.to_string();
    let result = tokio::task::spawn_blocking(move || {
        let project_root = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());
        tts::synthesize_and_play(&project_root, &text, &voice, tts::DEFAULT_CFG_SCALE)
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
        println!("{}", serde_json::to_string_pretty(&obj).unwrap());
    } else {
        eprintln!(
            "Spoke {:.1}s of audio in {:.1}s (RTF: {:.2})",
            result.duration_secs, result.generation_time_secs, result.rtf
        );
    }

    Ok(())
}
