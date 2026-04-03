use std::collections::HashSet;

pub mod filters;
pub mod navigation;
#[cfg(feature = "tts")]
pub mod tts;
#[cfg(feature = "tts")]
pub use tts::PendingTtsAction;

#[cfg(feature = "tts")]
use std::collections::VecDeque;
use std::sync::Arc;

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::{self, ArticleRef, LawDetail, LawEntry, MetadataIndex};
use legal_ko_core::preferences::Preferences;
use legal_ko_core::search::Searcher;
#[cfg(feature = "tts")]
use legal_ko_core::tts::{TtsEngineHandle, TtsProfile, TtsState, new_engine_handle};
use legal_ko_core::{client, parser, reqwest};

use ratatui::text::Line;

use crate::theme::{self, Theme};

// Constants and TTS imports moved to tts.rs
#[cfg(feature = "tts")]
use rodio::{MixerDeviceSink, Player};
use tokio::sync::mpsc;
#[cfg(feature = "tts")]
use tracing::debug;
use tracing::{error, info, warn};

// ── View / Mode enums ─────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum View {
    Loading,
    List,
    Detail,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    Search,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Popup {
    None,
    Help,
    CategoryFilter,
    DepartmentFilter,
    ArticleList,
}

// ── Messages (background → main) ─────────────────────────────

pub enum Message {
    MetadataLoaded(MetadataIndex),
    MetadataError(String),
    LawContentLoaded {
        id: String,
        content: String,
    },
    LawContentError {
        id: String,
        error: String,
    },
    #[cfg(feature = "tts")]
    TtsEngineLoaded,
    #[cfg(feature = "tts")]
    TtsEngineError(String),
    /// Streaming playback started (prebuffer flushed).
    #[cfg(feature = "tts")]
    TtsPlaybackStarted,
    /// Streaming synthesis completed successfully.
    #[cfg(feature = "tts")]
    TtsSynthesisComplete,
    /// Meilisearch warmup completed.
    MeiliReady,
    /// Meilisearch warmup failed.
    MeiliError(String),
    /// Ranked search results from Meilisearch.
    MeiliSearchResults {
        seq: u64,
        ids: Vec<String>,
    },
    /// All synthesis and playback for a batch session is done.
    #[cfg(feature = "tts")]
    TtsSynthesisDone,
    /// The background thread has advanced to the next article in read-all mode.
    #[cfg(feature = "tts")]
    TtsArticleAdvanced {
        article_idx: usize,
    },
    #[cfg(feature = "tts")]
    TtsSynthesisError(String),
}

// ── App state ─────────────────────────────────────────────────

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub view: View,
    pub input_mode: InputMode,
    pub popup: Popup,
    pub should_quit: bool,

    // HTTP client
    pub client: reqwest::Client,

    // Data
    pub all_laws: Vec<LawEntry>,
    pub filtered_indices: Vec<usize>,

    // List view state
    pub list_selected: usize,
    pub list_offset: usize,
    pub search_query: String,
    pub category_filter: Option<String>,
    pub department_filter: Option<String>,
    pub bookmarks_only: bool,

    // Available filter options
    pub categories: Vec<String>,
    pub departments: Vec<String>,

    // Popup selection index
    pub popup_selected: usize,

    // Detail view state
    pub detail: Option<LawDetail>,
    pub detail_scroll: usize,
    pub detail_lines_count: usize,
    pub detail_articles: Vec<ArticleRef>,
    pub detail_loading: bool,
    /// The law ID we're currently waiting on; used to discard stale async responses.
    pub pending_detail_id: Option<String>,
    /// Cached rendered lines from `parse_law_markdown`; invalidated on content/theme change.
    pub detail_rendered_lines: Vec<Line<'static>>,

    // Bookmarks
    pub bookmarks: Bookmarks,

    // Status message
    pub status_message: Option<String>,

    // Channel for sending messages from background tasks
    pub msg_tx: mpsc::UnboundedSender<Message>,
    pub msg_rx: mpsc::UnboundedReceiver<Message>,

    // Theme
    pub theme_index: usize,

    // TTS
    #[cfg(feature = "tts")]
    pub tts_state: TtsState,
    #[cfg(feature = "tts")]
    pub tts_engine: TtsEngineHandle,
    /// TTS quality/speed profile (Fast=cfg 1.0, Balanced=cfg 1.5).
    #[cfg(feature = "tts")]
    pub tts_profile: TtsProfile,
    /// Index of the article currently being spoken (into `detail_articles`).
    #[cfg(feature = "tts")]
    pub tts_current_article: Option<usize>,
    /// Queue of article indices remaining to be spoken (for `R` read-all mode).
    #[cfg(feature = "tts")]
    tts_article_queue: VecDeque<usize>,
    /// Keeps the audio device sink alive for the duration of playback.
    #[cfg(feature = "tts")]
    tts_device_sink: Option<MixerDeviceSink>,
    /// Player handle for controlling playback (stop/pause).
    #[cfg(feature = "tts")]
    tts_player: Option<Arc<Player>>,
    /// Action to execute once the TTS engine finishes loading.
    #[cfg(feature = "tts")]
    pending_tts_action: PendingTtsAction,
    /// True while buffering initial audio before unpausing the player.
    #[cfg(feature = "tts")]
    tts_buffering: bool,

    /// Tick counter incremented every event-loop iteration (~50ms).
    /// Used for UI animations (e.g. TTS loading indicator).
    pub tick: usize,

    // Meilisearch
    pub searcher: Arc<Searcher>,
    /// True once warmup completed successfully.
    pub meili_ready: bool,
    /// Monotonic counter to discard stale search results.
    pub search_seq: u64,
    /// Ranked IDs from the last Meilisearch query (if any).
    pub meili_search_ids: Option<Vec<String>>,
    /// The query string that produced `meili_search_ids`.
    pub meili_search_query: Option<String>,
}

impl App {
    pub fn new() -> Self {
        let (msg_tx, msg_rx) = mpsc::unbounded_channel();
        let bookmarks = Bookmarks::load();
        let prefs = Preferences::load();
        let theme_index = theme::theme_index_by_name(&prefs.theme);
        // Invariant: http_client() only fails if TLS backend or system config is
        // broken — the application cannot function without an HTTP client.
        let client = client::http_client().expect("Failed to build HTTP client");

        Self {
            view: View::Loading,
            input_mode: InputMode::Normal,
            popup: Popup::None,
            should_quit: false,
            client,
            all_laws: Vec::new(),
            filtered_indices: Vec::new(),
            list_selected: 0,
            list_offset: 0,
            search_query: String::new(),
            category_filter: None,
            department_filter: None,
            bookmarks_only: false,
            categories: Vec::new(),
            departments: Vec::new(),
            popup_selected: 0,
            detail: None,
            detail_scroll: 0,
            detail_lines_count: 0,
            detail_articles: Vec::new(),
            detail_loading: false,
            pending_detail_id: None,
            detail_rendered_lines: Vec::new(),
            bookmarks,
            status_message: None,
            msg_tx,
            msg_rx,
            theme_index,
            #[cfg(feature = "tts")]
            tts_state: TtsState::Unloaded,
            #[cfg(feature = "tts")]
            tts_engine: new_engine_handle(),
            #[cfg(feature = "tts")]
            tts_profile: TtsProfile::default(),
            #[cfg(feature = "tts")]
            tts_current_article: None,
            #[cfg(feature = "tts")]
            tts_article_queue: VecDeque::new(),
            #[cfg(feature = "tts")]
            tts_device_sink: None,
            #[cfg(feature = "tts")]
            tts_player: None,
            #[cfg(feature = "tts")]
            pending_tts_action: PendingTtsAction::None,
            #[cfg(feature = "tts")]
            tts_buffering: false,
            tick: 0,
            searcher: Arc::new(Searcher::from_env()),
            meili_ready: false,
            search_seq: 0,
            meili_search_ids: None,
            meili_search_query: None,
        }
    }

    /// Get the current theme
    pub fn theme(&self) -> &'static Theme {
        &theme::THEMES[self.theme_index]
    }

    /// Cycle to the next theme. Saves preference to disk.
    pub fn next_theme(&mut self) {
        self.theme_index = (self.theme_index + 1) % theme::THEMES.len();
        let prefs = Preferences {
            theme: self.theme().name.to_string(),
        };
        tokio::task::spawn_blocking(move || {
            if let Err(e) = prefs.save() {
                warn!(error = %e, "Failed to save theme preference");
            }
        });
        // Re-render cached lines with the new theme
        if let Some(ref detail) = self.detail {
            let (lines, _) = crate::parser::parse_law_markdown(&detail.raw_markdown, self.theme());
            self.detail_lines_count = lines.len();
            self.detail_rendered_lines = lines;
        }
    }

    /// Toggle TTS profile between Fast (cfg=1.0, 1s prebuffer) and Balanced (cfg=1.5, 5s prebuffer).
    #[cfg(feature = "tts")]
    pub fn toggle_tts_profile(&mut self) {
        self.tts_profile = match self.tts_profile {
            TtsProfile::Fast => TtsProfile::Balanced,
            TtsProfile::Balanced => TtsProfile::Fast,
        };
        self.status_message = Some(format!("TTS profile: {}", self.tts_profile));
    }

    /// Start fetching metadata in background
    pub fn start_loading(&self) {
        let tx = self.msg_tx.clone();
        let client = self.client.clone();
        tokio::spawn(async move {
            match client::fetch_metadata(&client).await {
                Ok(index) => {
                    let _ = tx.send(Message::MetadataLoaded(index));
                }
                Err(e) => {
                    let _ = tx.send(Message::MetadataError(format!("{e:#}")));
                }
            }
        });
    }

    /// Process a message from background tasks
    #[allow(clippy::too_many_lines)]
    pub fn handle_message(&mut self, msg: Message) {
        match msg {
            Message::MetadataLoaded(index) => {
                self.load_metadata(index);
                self.view = View::List;
                self.status_message = Some(format!("Loaded {} laws", self.all_laws.len()));
                // Start Meilisearch warmup in background
                if self.searcher.is_enabled() {
                    let searcher = Arc::clone(&self.searcher);
                    let entries = self.all_laws.clone();
                    let tx = self.msg_tx.clone();
                    tokio::spawn(async move {
                        match searcher.warmup(&entries).await {
                            Ok(()) => {
                                let _ = tx.send(Message::MeiliReady);
                            }
                            Err(e) => {
                                let _ = tx.send(Message::MeiliError(format!("{e:#}")));
                            }
                        }
                    });
                }
            }
            Message::MetadataError(err) => {
                self.status_message = Some(format!("Error: {err}"));
                error!(error = %err, "Failed to load metadata");
            }
            Message::LawContentLoaded { id, content } => {
                // Discard stale responses from a previous selection
                if self.pending_detail_id.as_deref() != Some(&id) {
                    info!(id, "Discarding stale law content");
                    return;
                }
                self.on_law_content_loaded(&id, &content);
                // Prewarm TTS engine in background so it's ready when user wants to speak
                #[cfg(feature = "tts")]
                self.ensure_tts_prewarmed();
            }
            Message::LawContentError { id, error } => {
                // Discard stale errors from a previous selection
                if self.pending_detail_id.as_deref() != Some(&id) {
                    return;
                }
                self.detail_loading = false;
                self.status_message = Some(format!("Error loading {id}: {error}"));
                error!(id, error, "Failed to load law");
            }
            #[cfg(feature = "tts")]
            Message::TtsEngineLoaded => {
                self.tts_state = TtsState::Ready;
                info!("TTS engine loaded successfully");

                // Auto-execute the pending action the user requested before load
                match std::mem::replace(&mut self.pending_tts_action, PendingTtsAction::None) {
                    PendingTtsAction::SpeakArticle => self.speak_article(),
                    PendingTtsAction::SpeakFull => self.speak_full(),
                    PendingTtsAction::None => {
                        self.status_message = Some("TTS engine loaded".to_string());
                    }
                }
            }
            #[cfg(feature = "tts")]
            Message::TtsEngineError(err) => {
                self.tts_state = TtsState::Error;
                self.status_message = Some(format!("TTS error: {err}"));
                error!(error = %err, "TTS engine load failed");
            }
            #[cfg(feature = "tts")]
            Message::TtsSynthesisDone => {
                self.tts_buffering = false;
                if self.tts_state == TtsState::Synthesizing {
                    self.tts_state = TtsState::Playing;
                    self.status_message = Some("Playing...".to_string());
                }
            }
            #[cfg(feature = "tts")]
            Message::TtsArticleAdvanced { article_idx } => {
                // Only advance if TTS is still active (timers may fire after stop)
                if self.tts_state == TtsState::Synthesizing || self.tts_state == TtsState::Playing {
                    self.tts_current_article = Some(article_idx);
                    if let Some(art) = self.detail_articles.get(article_idx) {
                        self.detail_scroll = art.line_index;
                        self.status_message = Some(format!("Playing: {}", art.label));
                    }
                }
            }
            #[cfg(feature = "tts")]
            Message::TtsSynthesisError(err) => {
                self.tts_state = TtsState::Ready;
                self.tts_player = None;
                self.tts_device_sink = None;
                self.tts_current_article = None;
                self.tts_article_queue.clear();
                self.tts_buffering = false;
                self.status_message = Some(format!("TTS error: {err}"));
                error!(error = %err, "TTS synthesis failed");
            }
            #[cfg(feature = "tts")]
            Message::TtsPlaybackStarted => {
                self.tts_buffering = false;
                self.tts_state = TtsState::Playing;
                self.status_message = Some("Playing...".to_string());
                debug!("Streaming playback started");
            }
            #[cfg(feature = "tts")]
            Message::TtsSynthesisComplete => {
                // Streaming synthesis finished; transition to Playing so
                // check_tts_playback() can detect when the player drains.
                self.tts_buffering = false;
                if self.tts_state == TtsState::Synthesizing {
                    self.tts_state = TtsState::Playing;
                    self.status_message = Some("Playing...".to_string());
                }
                debug!("Streaming synthesis complete");
            }
            Message::MeiliReady => {
                self.meili_ready = true;
                info!("Meilisearch index ready");
                // Re-run search if there is an active query
                if !self.search_query.is_empty() {
                    self.dispatch_meili_search();
                }
            }
            Message::MeiliError(err) => {
                warn!(error = %err, "Meilisearch warmup failed");
            }
            Message::MeiliSearchResults { seq, ids } => {
                if seq == self.search_seq {
                    self.meili_search_ids = Some(ids);
                    self.meili_search_query = Some(self.search_query.clone());
                    self.apply_filters();
                }
            }
        }
    }

    fn load_metadata(&mut self, index: MetadataIndex) {
        let entries = models::entries_from_index(index);

        // Extract unique categories and departments
        let mut cat_set: HashSet<String> = HashSet::new();
        let mut dept_set: HashSet<String> = HashSet::new();
        for entry in &entries {
            cat_set.insert(entry.category.clone());
            for dept in &entry.departments {
                dept_set.insert(dept.clone());
            }
        }

        let mut categories: Vec<String> = cat_set.into_iter().collect();
        categories.sort();
        let mut departments: Vec<String> = dept_set.into_iter().collect();
        departments.sort();

        self.all_laws = entries;
        self.categories = categories;
        self.departments = departments;
        self.apply_filters();
    }

    /// Check category, department, and bookmark filters (excludes search query).
    fn passes_non_search_filters(&self, entry: &LawEntry) -> bool {
        if let Some(ref cat) = self.category_filter
            && &entry.category != cat
        {
            return false;
        }
        if let Some(ref dept) = self.department_filter
            && !entry.departments.contains(dept)
        {
            return false;
        }
        if self.bookmarks_only && !self.bookmarks.is_bookmarked(&entry.id) {
            return false;
        }
        true
    }

    /// Dispatch an async Meilisearch search for the current query.
    fn dispatch_meili_search(&mut self) {
        self.search_seq += 1;
        let seq = self.search_seq;
        let query = self.search_query.clone();
        let limit = self.all_laws.len();
        let searcher = Arc::clone(&self.searcher);
        let tx = self.msg_tx.clone();
        tokio::spawn(async move {
            if let Ok(ids) = searcher.search_ids(&query, limit).await {
                let _ = tx.send(Message::MeiliSearchResults { seq, ids });
            }
        });
    }

    /// Get the currently selected law entry (if any)
    pub fn selected_entry(&self) -> Option<&LawEntry> {
        self.filtered_indices
            .get(self.list_selected)
            .map(|&i| &self.all_laws[i])
    }

    /// Open the selected law: fetch or load from cache
    pub fn open_selected(&mut self) {
        let Some(entry) = self.selected_entry().cloned() else {
            return;
        };

        self.detail_loading = true;
        self.detail_scroll = 0;
        self.pending_detail_id = Some(entry.id.clone());
        self.status_message = Some(format!("Loading {}...", entry.title));

        let client = self.client.clone();
        let tx = self.msg_tx.clone();
        let path = entry.path.clone();
        let id = entry.id.clone();
        tokio::spawn(async move {
            match client::load_law_content(&client, &path).await {
                Ok(content) => {
                    let _ = tx.send(Message::LawContentLoaded { id, content });
                }
                Err(e) => {
                    let _ = tx.send(Message::LawContentError {
                        id,
                        error: format!("{e:#}"),
                    });
                }
            }
        });
    }

    fn on_law_content_loaded(&mut self, id: &str, content: &str) {
        self.pending_detail_id = None;

        let entry = self.all_laws.iter().find(|e| e.id == id).cloned();
        let Some(mut entry) = entry else {
            warn!(id, "Law not found in entries");
            self.detail_loading = false;
            return;
        };

        // Enrich entry metadata from frontmatter (departments, dates, etc.)
        parser::enrich_entry_from_frontmatter(&mut entry, content);
        // Update the master list so the list view also reflects enriched data
        if let Some(master) = self.all_laws.iter_mut().find(|e| e.id == id) {
            master.clone_from(&entry);
        }

        let (lines, articles) = crate::parser::parse_law_markdown(content, self.theme());
        self.detail_lines_count = lines.len();
        self.detail_rendered_lines = lines;
        self.detail_articles.clone_from(&articles);
        self.detail = Some(LawDetail {
            entry,
            raw_markdown: content.to_string(),
            articles,
        });
        self.detail_loading = false;
        self.detail_scroll = 0;
        self.view = View::Detail;
        self.status_message = None;
    }
}
