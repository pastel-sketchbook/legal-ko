use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::{
    self, LawEntry, PersonRole, PrecedentEntry, PrecedentSortOrder, SortOrder,
};
use legal_ko_core::search::{self, Searcher};
#[cfg(feature = "tts")]
use legal_ko_core::tts;
use legal_ko_core::{client, crossref, enrichment, parser, person_index, reqwest};

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

        /// Sort order: "title" (default) or "date" (promulgation date, newest first)
        #[arg(long, default_value = "title")]
        sort: String,

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

    // ── Precedent (판례) subcommands ────────────────────────
    /// List court precedents (optionally filtered)
    #[command(name = "precedent-list")]
    PrecedentList {
        /// Filter by case type (사건종류: 민사, 형사, 일반행정, etc.)
        #[arg(long)]
        case_type: Option<String>,

        /// Filter by court name (법원명: 대법원, 하급심)
        #[arg(long)]
        court: Option<String>,

        /// Sort order: "name" (default) or "date" (ruling date, newest first)
        #[arg(long, default_value = "name")]
        sort: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Maximum number of results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Search precedents by case name or case number
    #[command(name = "precedent-search")]
    PrecedentSearch {
        /// Search query
        query: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Maximum number of results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Show a precedent's full content
    #[command(name = "precedent-show")]
    PrecedentShow {
        /// Precedent ID (e.g. "민사/대법원/2000다10048")
        id: String,

        /// Output as JSON (includes raw markdown)
        #[arg(long)]
        json: bool,
    },
    /// List sections in a precedent (판시사항, 판결요지, etc.)
    #[command(name = "precedent-sections")]
    PrecedentSections {
        /// Precedent ID (e.g. "민사/대법원/2000다10048")
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Cross-reference: find laws cited by a precedent (4-approach fallback)
    #[command(name = "precedent-laws")]
    PrecedentLaws {
        /// Precedent ID (e.g. "민사/대법원/2000다10048")
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reverse cross-reference: find precedents that cite a given law article
    #[command(name = "law-precedents")]
    LawPrecedents {
        /// Law name to search for (e.g. "민법")
        law_name: String,

        /// Article to filter by (e.g. "제393조")
        #[arg(long)]
        article: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Maximum number of results
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Extract persons (judges, attorneys, prosecutors) from a precedent
    #[command(name = "precedent-persons")]
    PrecedentPersons {
        /// Precedent ID (e.g. "민사/대법원/2000다10048")
        id: String,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Search precedents by attorney, judge, or prosecutor name.
    ///
    /// Fetches content for each matching precedent (pre-filtered by --case-type
    /// / --court) and extracts person names. Requires network access per
    /// candidate; use filters and --limit to keep it fast.
    #[command(name = "precedent-search-person")]
    PrecedentSearchPerson {
        /// Person name to search for (Korean, e.g. "김길찬")
        name: String,

        /// Filter by role: judge, attorney, prosecutor (all if omitted)
        #[arg(long)]
        role: Option<String>,

        /// Pre-filter by case type (사건종류) to narrow search
        #[arg(long)]
        case_type: Option<String>,

        /// Pre-filter by court name (법원명) to narrow search
        #[arg(long)]
        court: Option<String>,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Maximum number of results to return (default 20)
        #[arg(long, default_value = "20")]
        limit: usize,
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
            sort,
            json,
            limit,
        } => cmd_list(&client, category, department, bookmarks, &sort, json, limit).await,
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

        // ── Precedent commands ──────────────────────────────
        Command::PrecedentList {
            case_type,
            court,
            sort,
            json,
            limit,
        } => cmd_precedent_list(&client, case_type, court, &sort, json, limit).await,
        Command::PrecedentSearch { query, json, limit } => {
            cmd_precedent_search(&client, &query, json, limit).await
        }
        Command::PrecedentShow { id, json } => cmd_precedent_show(&client, &id, json).await,
        Command::PrecedentSections { id, json } => cmd_precedent_sections(&client, &id, json).await,
        Command::PrecedentLaws { id, json } => cmd_precedent_laws(&client, &id, json).await,
        Command::LawPrecedents {
            law_name,
            article,
            json,
            limit,
        } => cmd_law_precedents(&client, &law_name, article, json, limit).await,
        Command::PrecedentPersons { id, json } => cmd_precedent_persons(&client, &id, json).await,
        Command::PrecedentSearchPerson {
            name,
            role,
            case_type,
            court,
            json,
            limit,
        } => cmd_precedent_search_person(&client, &name, role, case_type, court, json, limit).await,
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
    let mut entries = models::entries_from_index(index);

    // Apply cached enrichment, then batch-fetch any missing entries
    let cache = enrichment::load_cache();
    let _ = enrichment::apply_cache(&mut entries, &cache);
    let final_cache = enrichment::fetch_and_enrich(client, &entries, cache, |_batch| {}).await;
    // Apply the freshly fetched data to entries
    let _ = enrichment::apply_cache(&mut entries, &final_cache);
    // Save cache to disk (best-effort)
    tokio::task::spawn_blocking(move || enrichment::save_cache(&final_cache));

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
                    "promulgation_date": e.promulgation_date,
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
    sort: &str,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let mut entries = load_entries(client).await?;

    let order = match sort {
        "date" | "promulgation" => SortOrder::PromulgationDate,
        _ => SortOrder::Title,
    };
    models::sort_entries(&mut entries, order);

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
            promulgation_date: String::new(),
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
            "promulgation_date": entry.promulgation_date,
            "enforcement_date": entry.enforcement_date,
            "status": entry.status,
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
            promulgation_date: String::new(),
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

// ── Precedent (판례) command handlers ────────────────────────

async fn load_precedent_entries(client: &reqwest::Client) -> Result<Vec<PrecedentEntry>> {
    let index = client::fetch_precedent_metadata(client)
        .await
        .context("Failed to load precedent metadata from GitHub")?;
    Ok(models::precedent_entries_from_index(index))
}

fn print_precedent_entries(entries: &[&PrecedentEntry], as_json: bool) -> Result<()> {
    if as_json {
        let items: Vec<_> = entries
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "case_name": e.case_name,
                    "case_number": e.case_number,
                    "ruling_date": e.ruling_date,
                    "court_name": e.court_name,
                    "case_type": e.case_type,
                    "ruling_type": e.ruling_type,
                    "path": e.path,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&items)?);
    } else {
        for e in entries {
            let name = if e.case_name.is_empty() {
                &e.case_number
            } else {
                &e.case_name
            };
            println!(
                "{}\t{}\t[{}]\t{}\t{}",
                e.id, name, e.case_type, e.court_name, e.ruling_date
            );
        }
    }
    Ok(())
}

async fn cmd_precedent_list(
    client: &reqwest::Client,
    case_type: Option<String>,
    court: Option<String>,
    sort: &str,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let mut entries = load_precedent_entries(client).await?;

    let order = match sort {
        "date" | "ruling" => PrecedentSortOrder::RulingDate,
        _ => PrecedentSortOrder::CaseName,
    };
    models::sort_precedent_entries(&mut entries, order);

    let mut filtered: Vec<&PrecedentEntry> = entries
        .iter()
        .filter(|e| {
            if let Some(ref ct) = case_type
                && e.case_type != *ct
            {
                return false;
            }
            if let Some(ref c) = court
                && e.court_name != *c
            {
                return false;
            }
            true
        })
        .collect();
    if let Some(n) = limit {
        filtered.truncate(n);
    }
    print_precedent_entries(&filtered, as_json)?;
    Ok(())
}

async fn cmd_precedent_search(
    client: &reqwest::Client,
    query: &str,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    let entries = load_precedent_entries(client).await?;
    let n = limit.unwrap_or(50);
    let query_lower = query.to_lowercase();

    // Naive search: match against case_name and case_number
    let results: Vec<&PrecedentEntry> = entries
        .iter()
        .filter(|e| {
            e.case_name.to_lowercase().contains(&query_lower)
                || e.case_number.to_lowercase().contains(&query_lower)
        })
        .take(n)
        .collect();

    if !results.is_empty() {
        print_precedent_entries(&results, as_json)?;
        return Ok(());
    }

    // No metadata matches — if the query looks like a Korean name, fall back
    // to 법조인 (legal professional) search across documents.
    if !parser::is_korean_name(query) {
        // Not a name-shaped query; just print empty results.
        print_precedent_entries(&results, as_json)?;
        return Ok(());
    }

    let max_results = limit.unwrap_or(20);
    if !as_json {
        eprintln!("No metadata matches for \"{query}\". Trying 법조인 search…");
    }

    search_persons_indexed(client, query, None, &entries, as_json, max_results).await
}

async fn cmd_precedent_show(client: &reqwest::Client, id: &str, as_json: bool) -> Result<()> {
    let path = format!("{id}.md");
    let content = client::load_precedent_content(client, &path).await?;

    let stripped = parser::strip_frontmatter(&content);

    if as_json {
        let mut entry = PrecedentEntry {
            id: id.to_string(),
            case_name: String::new(),
            case_number: String::new(),
            ruling_date: String::new(),
            court_name: String::new(),
            case_type: String::new(),
            ruling_type: String::new(),
            path,
        };
        parser::enrich_precedent_from_frontmatter(&mut entry, &content);

        let sections = parser::extract_precedent_sections(&content);
        let section_labels: Vec<&str> = sections.iter().map(|s| s.label.as_str()).collect();

        let obj = json!({
            "id": entry.id,
            "case_name": entry.case_name,
            "case_number": entry.case_number,
            "ruling_date": entry.ruling_date,
            "court_name": entry.court_name,
            "case_type": entry.case_type,
            "ruling_type": entry.ruling_type,
            "sections": section_labels,
            "content": stripped,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        println!("{stripped}");
    }
    Ok(())
}

async fn cmd_precedent_sections(client: &reqwest::Client, id: &str, as_json: bool) -> Result<()> {
    let path = format!("{id}.md");
    let content = client::load_precedent_content(client, &path).await?;

    let sections = parser::extract_precedent_sections(&content);

    if as_json {
        let mut entry = PrecedentEntry {
            id: id.to_string(),
            case_name: String::new(),
            case_number: String::new(),
            ruling_date: String::new(),
            court_name: String::new(),
            case_type: String::new(),
            ruling_type: String::new(),
            path,
        };
        parser::enrich_precedent_from_frontmatter(&mut entry, &content);

        let items: Vec<_> = sections
            .iter()
            .map(|s| {
                json!({
                    "label": s.label,
                    "line_index": s.line_index,
                })
            })
            .collect();
        let obj = json!({
            "id": entry.id,
            "case_name": entry.case_name,
            "sections": items,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        let mut case_name = String::new();
        let fm = parser::parse_frontmatter(&content);
        if let Some(t) = fm.get("사건명") {
            case_name = t.as_str().to_string();
        }
        println!("# {id} — {case_name}");
        for s in &sections {
            println!("  L{}: {}", s.line_index, s.label);
        }
    }
    Ok(())
}

// ── Cross-reference command handlers ─────────────────────────

async fn cmd_precedent_laws(client: &reqwest::Client, id: &str, as_json: bool) -> Result<()> {
    // Fetch precedent content
    let path = format!("{id}.md");
    let content = client::load_precedent_content(client, &path).await?;

    // Extract case_type from frontmatter
    let fm = parser::parse_frontmatter(&content);
    let case_type = fm.get("사건종류").map_or("", |v| v.as_str());

    // Fetch law metadata to get known law names for Approach 3 matching
    let law_index = client::fetch_metadata(client)
        .await
        .context("Failed to load law metadata from GitHub")?;
    let law_entries = models::entries_from_index(law_index);
    let known_laws: Vec<String> = law_entries.iter().map(|e| e.title.clone()).collect();

    // Run full 4-approach cross-reference
    let xref = crossref::cross_reference(&content, case_type, &known_laws);

    if as_json {
        println!("{}", serde_json::to_string_pretty(&xref)?);
    } else {
        let case_name = fm.get("사건명").map_or("", |v| v.as_str());
        println!("# {id} — {case_name}");
        println!("Resolution: {:?}", xref.resolution);
        println!();

        if !xref.statute_refs.is_empty() {
            println!("## 참조조문 (Statute References)");
            for sr in &xref.statute_refs {
                let detail = sr.detail.as_deref().unwrap_or("");
                let group = sr.group.map_or(String::new(), |g| format!("[{g}] "));
                println!("  {group}{} {} {detail}", sr.law_name, sr.article);
            }
            println!();
        }

        if !xref.law_matches.is_empty() {
            let matched: Vec<_> = xref
                .law_matches
                .iter()
                .filter(|m| m.law_id.is_some())
                .collect();
            if !matched.is_empty() {
                println!("## Law Matches");
                for m in &matched {
                    println!(
                        "  {} {} → {} ({:?})",
                        m.statute_ref.law_name,
                        m.statute_ref.article,
                        m.law_id.as_deref().unwrap_or("?"),
                        m.match_type
                    );
                }
                println!();
            }
        }

        if !xref.case_refs.is_empty() {
            println!("## 참조판례 (Case References)");
            for cr in &xref.case_refs {
                let groups = if cr.groups.is_empty() {
                    String::new()
                } else {
                    let g: Vec<String> = cr.groups.iter().map(|n| format!("[{n}]")).collect();
                    format!("{} ", g.join(""))
                };
                println!(
                    "  {groups}{} {} ({})",
                    cr.court, cr.case_number, cr.ruling_date
                );
            }
            println!();
        }

        if xref.resolution == crossref::Resolution::AffinityFallback {
            println!("## Affinity Suggestions (case type: {case_type})");
            for a in &xref.affinity {
                println!("  {} — {}", a.search_term, a.reason);
            }
        }
    }
    Ok(())
}

async fn cmd_law_precedents(
    client: &reqwest::Client,
    law_name: &str,
    article_filter: Option<String>,
    as_json: bool,
    limit: Option<usize>,
) -> Result<()> {
    // Fetch all precedent entries
    let precedent_entries = load_precedent_entries(client).await?;
    let n = limit.unwrap_or(50);

    // For each precedent, fetch content and check if it cites the given law.
    // This is expensive for large datasets, so we first filter by naive text
    // matching on the precedent ID / metadata, then confirm with parsing.
    //
    // Since we can't download all 123K precedent files, we use a heuristic:
    // search for the law_name in the case_name field, and also check any
    // precedents whose case_type matches the law's typical domain.
    //
    // For a production system this would use a pre-built index, but for now
    // we report this as a best-effort scan with a practical limit.

    let mut matches = Vec::new();

    // Naive pre-filter: check case_name for the law name
    let candidates: Vec<&PrecedentEntry> = precedent_entries
        .iter()
        .filter(|e| e.case_name.contains(law_name))
        .take(n * 5) // fetch extra candidates to account for false positives
        .collect();

    for entry in &candidates {
        if matches.len() >= n {
            break;
        }

        // Fetch content and parse statute refs
        let path = format!("{}.md", entry.id);
        let Ok(content) = client::load_precedent_content(client, &path).await else {
            continue;
        };

        let refs = crossref::extract_statute_refs(&content);
        let is_match = refs.iter().any(|sr| {
            if sr.law_name != law_name {
                return false;
            }
            if let Some(ref art) = article_filter {
                sr.article == *art
            } else {
                true
            }
        });

        if is_match {
            matches.push(entry);
        }
    }

    if as_json {
        let items: Vec<_> = matches
            .iter()
            .map(|e| {
                json!({
                    "id": e.id,
                    "case_name": e.case_name,
                    "case_number": e.case_number,
                    "ruling_date": e.ruling_date,
                    "court_name": e.court_name,
                    "case_type": e.case_type,
                })
            })
            .collect();
        let obj = json!({
            "law_name": law_name,
            "article": article_filter,
            "matches": items,
            "count": matches.len(),
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        let art_label = article_filter.as_deref().unwrap_or("(all articles)");
        println!("# Precedents citing {law_name} {art_label}");
        println!("Found {} match(es):", matches.len());
        println!();
        for e in &matches {
            let name = if e.case_name.is_empty() {
                &e.case_number
            } else {
                &e.case_name
            };
            println!(
                "  {} — {} [{}] {} ({})",
                e.id, name, e.case_type, e.court_name, e.ruling_date
            );
        }
    }
    Ok(())
}

async fn cmd_precedent_persons(client: &reqwest::Client, id: &str, as_json: bool) -> Result<()> {
    let path = format!("{id}.md");
    let content = client::load_precedent_content(client, &path).await?;

    let persons = parser::extract_persons(&content);

    if as_json {
        let mut entry = PrecedentEntry {
            id: id.to_string(),
            case_name: String::new(),
            case_number: String::new(),
            ruling_date: String::new(),
            court_name: String::new(),
            case_type: String::new(),
            ruling_type: String::new(),
            path,
        };
        parser::enrich_precedent_from_frontmatter(&mut entry, &content);

        let items: Vec<_> = persons
            .iter()
            .map(|p| {
                json!({
                    "name": p.name,
                    "role": p.role,
                    "qualifier": p.qualifier,
                })
            })
            .collect();
        let obj = json!({
            "id": entry.id,
            "case_name": entry.case_name,
            "persons": items,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        let fm = parser::parse_frontmatter(&content);
        let case_name = fm.get("사건명").map_or("(unknown)", |v| v.as_str());
        println!("# {id}");
        println!("  {case_name}");
        println!();
        if persons.is_empty() {
            println!("  (no persons found)");
        } else {
            for p in &persons {
                let qual = p
                    .qualifier
                    .as_deref()
                    .map_or(String::new(), |q| format!(" ({q})"));
                println!("  [{role}] {name}{qual}", role = p.role, name = p.name);
            }
        }
    }
    Ok(())
}

async fn cmd_precedent_search_person(
    client: &reqwest::Client,
    name: &str,
    role_filter: Option<String>,
    case_type_filter: Option<String>,
    court_filter: Option<String>,
    as_json: bool,
    limit: usize,
) -> Result<()> {
    // Parse optional role filter.
    let role: Option<PersonRole> = match role_filter.as_deref() {
        Some("judge") => Some(PersonRole::Judge),
        Some("attorney") => Some(PersonRole::Attorney),
        Some("prosecutor") => Some(PersonRole::Prosecutor),
        Some(other) => anyhow::bail!("Unknown role '{other}'. Use: judge, attorney, or prosecutor"),
        None => None,
    };

    // Load metadata.
    let entries = load_precedent_entries(client).await?;

    if !as_json {
        eprintln!(
            "Searching for \"{name}\" across {} precedent(s)…",
            entries.len()
        );
    }

    let mut results =
        person_index::search_persons(client, name, role.as_ref(), &entries, |scanned, total| {
            if !as_json {
                eprint!("\rBuilding person index: {scanned}/{total}");
            }
        })
        .await;

    if !as_json {
        eprint!("\r\x1b[K");
    }

    // Post-filter by case type / court.
    if case_type_filter.is_some() || court_filter.is_some() {
        results.retain(|r| {
            if let Some(ref ct) = case_type_filter
                && &r.entry.case_type != ct
            {
                return false;
            }
            if let Some(ref court) = court_filter
                && &r.entry.court_name != court
            {
                return false;
            }
            true
        });
    }

    print_person_results(name, role.as_ref(), &results, as_json, limit)
}

/// Search for a person using the cached person index.
///
/// If no index exists, builds one concurrently (with progress output to
/// stderr), caches it, and then searches. Subsequent calls are instant.
async fn search_persons_indexed(
    client: &reqwest::Client,
    name: &str,
    role: Option<&PersonRole>,
    all_entries: &[PrecedentEntry],
    as_json: bool,
    max_results: usize,
) -> Result<()> {
    let results =
        person_index::search_persons(client, name, role, all_entries, |scanned, total| {
            if !as_json {
                eprint!("\rBuilding person index: {scanned}/{total}");
            }
        })
        .await;

    if !as_json {
        // Clear progress line
        eprint!("\r\x1b[K");
    }

    print_person_results(name, role, &results, as_json, max_results)
}

/// Format and print person search results (shared by direct and fallback paths).
fn print_person_results(
    name: &str,
    role: Option<&PersonRole>,
    results: &[person_index::PersonSearchResult],
    as_json: bool,
    max_results: usize,
) -> Result<()> {
    let capped: Vec<_> = results.iter().take(max_results).collect();

    if as_json {
        let matches: Vec<serde_json::Value> = capped
            .iter()
            .map(|r| {
                json!({
                    "id": r.entry.id,
                    "case_name": r.entry.case_name,
                    "case_number": r.entry.case_number,
                    "ruling_date": r.entry.ruling_date,
                    "court_name": r.entry.court_name,
                    "case_type": r.entry.case_type,
                    "matched_roles": [{
                        "role": r.role,
                        "qualifier": r.qualifier,
                    }],
                })
            })
            .collect();
        let obj = json!({
            "query": name,
            "role_filter": role.map(std::string::ToString::to_string),
            "total_matches": results.len(),
            "matches": matches,
        });
        println!("{}", serde_json::to_string_pretty(&obj)?);
    } else {
        for r in &capped {
            let q = r
                .qualifier
                .as_deref()
                .map_or(String::new(), |q| format!("/{q}"));
            let case = if r.entry.case_name.is_empty() {
                &r.entry.case_number
            } else {
                &r.entry.case_name
            };
            println!(
                "  {} — {} [{}] {} ({}) [{}{}]",
                r.entry.id,
                case,
                r.entry.case_type,
                r.entry.court_name,
                r.entry.ruling_date,
                r.role,
                q,
            );
        }
        eprintln!("Found {} match(es).", results.len());
    }

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
            tracing::info!(
                duration_secs = result.duration_secs,
                generation_time_secs = result.generation_time_secs,
                rtf = result.rtf,
                "TTS complete"
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
        tracing::info!(segments = n_segments, "Synthesizing articles");

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
            tracing::info!(
                segments = stats.segments,
                duration_secs = stats.duration_secs,
                generation_time_secs = stats.generation_time_secs,
                rtf = stats.rtf,
                "TTS complete"
            );
        }
    }

    Ok(())
}
