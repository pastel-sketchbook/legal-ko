use std::collections::{HashSet, VecDeque};
use std::num::NonZero;
use std::sync::Arc;
use std::time::{Duration, Instant};

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::{ArticleRef, LawDetail, LawEntry, MetadataIndex};
use legal_ko_core::preferences::Preferences;
use legal_ko_core::search::Searcher;
use legal_ko_core::tts::{self, TtsEngineHandle, TtsState};
use legal_ko_core::{cache, client, parser};

use ratatui::text::Line;

use crate::theme::{self, Theme};

use legal_ko_core::tts::OUTPUT_SR;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Mono channel count for rodio.
const CHANNELS: NonZero<u16> = NonZero::new(1).unwrap();

/// Sample rate for rodio (must match OUTPUT_SR = 24000).
const SAMPLE_RATE: NonZero<u32> = NonZero::new(OUTPUT_SR).unwrap();

// ── Prebuffer helper ──────────────────────────────────────────

/// Accumulates streaming audio chunks until a threshold is reached, then
/// flushes to the player and feeds subsequent chunks directly.
///
/// This eliminates the play-pause-play stutter caused by the player
/// draining faster than the next chunk arrives.
struct PrebufferStreamer {
    player: Arc<Player>,
    tx: mpsc::UnboundedSender<Message>,
    prebuffer: Vec<f32>,
    prebuffer_threshold: usize,
    flushed: bool,
}

impl PrebufferStreamer {
    fn new(player: Arc<Player>, tx: mpsc::UnboundedSender<Message>, prebuffer_secs: f64) -> Self {
        let prebuffer_threshold = (OUTPUT_SR as f64 * prebuffer_secs) as usize;
        Self {
            player,
            tx,
            prebuffer: Vec::with_capacity(prebuffer_threshold + 48_000),
            prebuffer_threshold,
            flushed: false,
        }
    }

    /// Feed a chunk of audio samples. Returns `true` if this chunk triggered
    /// the initial flush (caller can perform additional actions like setting
    /// playback start time).
    fn feed(&mut self, chunk: &[f32]) -> bool {
        if !self.flushed {
            self.prebuffer.extend_from_slice(chunk);
            if self.prebuffer.len() >= self.prebuffer_threshold {
                let drained = std::mem::take(&mut self.prebuffer);
                let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, drained);
                self.player.append(source);
                self.player.play();
                self.flushed = true;
                let _ = self.tx.send(Message::TtsPlaybackStarted);
                return true;
            }
        } else {
            let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, chunk.to_vec());
            self.player.append(source);
        }
        false
    }

    /// Flush any remaining prebuffer (for short texts that never hit the
    /// threshold). Returns `true` if audio was actually flushed.
    fn flush_remaining(&mut self) -> bool {
        if !self.flushed && !self.prebuffer.is_empty() {
            let source = rodio::buffer::SamplesBuffer::new(
                CHANNELS,
                SAMPLE_RATE,
                std::mem::take(&mut self.prebuffer),
            );
            self.player.append(source);
            self.player.play();
            self.flushed = true;
            let _ = self.tx.send(Message::TtsPlaybackStarted);
            true
        } else {
            false
        }
    }
}

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

/// Action deferred until the TTS engine finishes loading.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingTtsAction {
    None,
    /// Read the article at the current scroll position.
    SpeakArticle,
    /// Read all articles from the current scroll position.
    SpeakFull,
}

// ── Messages (background → main) ─────────────────────────────

#[allow(dead_code)]
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
    TtsEngineLoaded,
    TtsEngineError(String),
    /// Batch synthesis produced audio ready for playback.
    TtsBatchReady {
        articles_audio: Vec<Vec<f32>>,
        article_indices: Vec<usize>,
    },
    /// Streaming playback started (prebuffer flushed).
    TtsPlaybackStarted,
    /// Streaming synthesis completed successfully.
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
    TtsSynthesisDone,
    /// The background thread has advanced to the next article in read-all mode.
    TtsArticleAdvanced {
        article_idx: usize,
    },
    TtsSynthesisError(String),
}

// ── App state ─────────────────────────────────────────────────

pub struct App {
    pub view: View,
    pub input_mode: InputMode,
    pub popup: Popup,
    pub should_quit: bool,

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
    /// Cached rendered lines from parse_law_markdown; invalidated on content/theme change.
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
    pub tts_state: TtsState,
    pub tts_engine: TtsEngineHandle,
    /// TTS quality/speed profile (Fast=cfg 1.0, Balanced=cfg 1.5).
    pub tts_profile: tts::TtsProfile,
    /// Index of the article currently being spoken (into `detail_articles`).
    pub tts_current_article: Option<usize>,
    /// Queue of article indices remaining to be spoken (for `R` read-all mode).
    tts_article_queue: VecDeque<usize>,
    /// Keeps the audio device sink alive for the duration of playback.
    tts_device_sink: Option<MixerDeviceSink>,
    /// Player handle for controlling playback (stop/pause).
    tts_player: Option<Arc<Player>>,
    /// Action to execute once the TTS engine finishes loading.
    pending_tts_action: PendingTtsAction,
    /// True while buffering initial audio before unpausing the player.
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

        Self {
            view: View::Loading,
            input_mode: InputMode::Normal,
            popup: Popup::None,
            should_quit: false,
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
            detail_rendered_lines: Vec::new(),
            bookmarks,
            status_message: None,
            msg_tx,
            msg_rx,
            theme_index,
            tts_state: TtsState::Unloaded,
            tts_engine: tts::new_engine_handle(),
            tts_profile: tts::TtsProfile::default(),
            tts_current_article: None,
            tts_article_queue: VecDeque::new(),
            tts_device_sink: None,
            tts_player: None,
            pending_tts_action: PendingTtsAction::None,
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
        if let Err(e) = prefs.save() {
            warn!("Failed to save theme preference: {e}");
        }
        // Re-render cached lines with the new theme
        if let Some(ref detail) = self.detail {
            let (lines, _) = crate::parser::parse_law_markdown(&detail.raw_markdown, self.theme());
            self.detail_lines_count = lines.len();
            self.detail_rendered_lines = lines;
        }
    }

    /// Toggle TTS profile between Fast (cfg=1.0, 1s prebuffer) and Balanced (cfg=1.5, 5s prebuffer).
    pub fn toggle_tts_profile(&mut self) {
        self.tts_profile = match self.tts_profile {
            tts::TtsProfile::Fast => tts::TtsProfile::Balanced,
            tts::TtsProfile::Balanced => tts::TtsProfile::Fast,
        };
        self.status_message = Some(format!("TTS profile: {}", self.tts_profile));
    }

    /// Start fetching metadata in background
    pub fn start_loading(&self) {
        let tx = self.msg_tx.clone();
        tokio::spawn(async move {
            match client::fetch_metadata().await {
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
                error!("Failed to load metadata: {err}");
            }
            Message::LawContentLoaded { id, content } => {
                self.on_law_content_loaded(&id, &content);
                // Prewarm TTS engine in background so it's ready when user wants to speak
                self.ensure_tts_prewarmed();
            }
            Message::LawContentError { id, error } => {
                self.detail_loading = false;
                self.status_message = Some(format!("Error loading {id}: {error}"));
                error!("Failed to load law {id}: {error}");
            }
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
            Message::TtsEngineError(err) => {
                self.tts_state = TtsState::Error;
                self.status_message = Some(format!("TTS error: {err}"));
                error!("TTS engine load failed: {err}");
            }
            Message::TtsSynthesisDone => {
                self.tts_buffering = false;
                if self.tts_state == TtsState::Synthesizing {
                    self.tts_state = TtsState::Playing;
                    self.status_message = Some("Playing...".to_string());
                }
            }
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
            Message::TtsBatchReady {
                articles_audio,
                article_indices,
            } => {
                // Enqueue batch-synthesized articles for gapless playback
                debug!("TTS batch ready: {} articles", articles_audio.len());
                for idx in &article_indices {
                    self.tts_article_queue.push_back(*idx);
                }
                // If we have a player, append audio; otherwise start fresh
                if let Some(ref player) = self.tts_player {
                    for samples in articles_audio {
                        let source =
                            rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, samples);
                        player.append(source);
                    }
                } else if let Ok(sink) = DeviceSinkBuilder::open_default_sink() {
                    let player = Player::connect_new(sink.mixer());
                    for samples in &articles_audio {
                        let source = rodio::buffer::SamplesBuffer::new(
                            CHANNELS,
                            SAMPLE_RATE,
                            samples.clone(),
                        );
                        player.append(source);
                    }
                    self.tts_device_sink = Some(sink);
                    self.tts_player = Some(Arc::new(player));
                }
                self.tts_state = TtsState::Playing;
                self.status_message = Some("Playing...".to_string());
            }
            Message::TtsSynthesisError(err) => {
                self.tts_state = TtsState::Ready;
                self.tts_player = None;
                self.tts_device_sink = None;
                self.tts_current_article = None;
                self.tts_article_queue.clear();
                self.tts_buffering = false;
                self.status_message = Some(format!("TTS error: {err}"));
                error!("TTS synthesis failed: {err}");
            }
            Message::TtsPlaybackStarted => {
                self.tts_buffering = false;
                self.tts_state = TtsState::Playing;
                self.status_message = Some("Playing...".to_string());
                debug!("Streaming playback started");
            }
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
                warn!("Meilisearch warmup failed: {err}");
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

        // Sort by title
        entries.sort_by(|a, b| a.title.cmp(&b.title));

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

    /// Apply search + category + department + bookmarks filters.
    ///
    /// When Meilisearch ranked results are available for the current query,
    /// they are used for ordering and filtering. Otherwise falls back to naive
    /// substring matching on the title.
    pub fn apply_filters(&mut self) {
        let query = &self.search_query;
        let use_meili = !query.is_empty()
            && self
                .meili_search_query
                .as_deref()
                .is_some_and(|q| q == query);

        if use_meili {
            // Build index lookup: id → position in all_laws
            let id_to_idx: std::collections::HashMap<&str, usize> = self
                .all_laws
                .iter()
                .enumerate()
                .map(|(i, e)| (e.id.as_str(), i))
                .collect();

            let meili_ids = self.meili_search_ids.as_deref().unwrap_or(&[]);

            self.filtered_indices = meili_ids
                .iter()
                .filter_map(|id| id_to_idx.get(id.as_str()).copied())
                .filter(|&i| {
                    let entry = &self.all_laws[i];
                    self.passes_non_search_filters(entry)
                })
                .collect();
        } else {
            let query_lower = query.to_lowercase();

            self.filtered_indices = self
                .all_laws
                .iter()
                .enumerate()
                .filter(|(_, entry)| {
                    // Search filter
                    if !query_lower.is_empty() && !entry.title.to_lowercase().contains(&query_lower)
                    {
                        return false;
                    }
                    self.passes_non_search_filters(entry)
                })
                .map(|(i, _)| i)
                .collect();

            // Dispatch async Meilisearch query if ready and query is non-empty
            if !query.is_empty() && self.meili_ready {
                self.dispatch_meili_search();
            }
        }

        // Clamp selection
        if self.filtered_indices.is_empty() {
            self.list_selected = 0;
        } else if self.list_selected >= self.filtered_indices.len() {
            self.list_selected = self.filtered_indices.len() - 1;
        }
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
        self.status_message = Some(format!("Loading {}...", entry.title));

        // Check cache first
        match cache::read_cache(&entry.path) {
            Ok(Some(content)) => {
                info!("Loaded {} from cache", entry.path);
                self.on_law_content_loaded(&entry.id, &content);
                return;
            }
            Ok(None) => {} // cache miss
            Err(e) => warn!("Cache read error: {e}"),
        }

        // Fetch from network
        let tx = self.msg_tx.clone();
        let path = entry.path.clone();
        let id = entry.id.clone();
        tokio::spawn(async move {
            match client::fetch_law_content(&path).await {
                Ok(content) => {
                    // Cache the result (ignore errors)
                    if let Err(e) = cache::write_cache(&path, &content) {
                        warn!("Failed to cache {path}: {e}");
                    }
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
        let entry = self.all_laws.iter().find(|e| e.id == id).cloned();
        let Some(entry) = entry else {
            warn!("Law {id} not found in entries");
            self.detail_loading = false;
            return;
        };

        let (lines, articles) = crate::parser::parse_law_markdown(content, self.theme());
        self.detail_lines_count = lines.len();
        self.detail_rendered_lines = lines;
        self.detail_articles = articles.clone();
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

    // ── List navigation ───────────────────────────────────────

    pub fn list_move_down(&mut self) {
        if !self.filtered_indices.is_empty() && self.list_selected < self.filtered_indices.len() - 1
        {
            self.list_selected += 1;
        }
    }

    pub fn list_move_up(&mut self) {
        if self.list_selected > 0 {
            self.list_selected -= 1;
        }
    }

    pub fn list_page_down(&mut self, page_size: usize) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.list_selected = (self.list_selected + page_size).min(self.filtered_indices.len() - 1);
    }

    pub fn list_page_up(&mut self, page_size: usize) {
        self.list_selected = self.list_selected.saturating_sub(page_size);
    }

    pub fn list_top(&mut self) {
        self.list_selected = 0;
    }

    pub fn list_bottom(&mut self) {
        if !self.filtered_indices.is_empty() {
            self.list_selected = self.filtered_indices.len() - 1;
        }
    }

    // ── Detail navigation ─────────────────────────────────────

    pub fn detail_scroll_down(&mut self, amount: usize) {
        self.detail_scroll =
            (self.detail_scroll + amount).min(self.detail_lines_count.saturating_sub(1));
    }

    pub fn detail_scroll_up(&mut self, amount: usize) {
        self.detail_scroll = self.detail_scroll.saturating_sub(amount);
    }

    pub fn detail_top(&mut self) {
        self.detail_scroll = 0;
    }

    pub fn detail_bottom(&mut self) {
        self.detail_scroll = self.detail_lines_count.saturating_sub(1);
    }

    pub fn next_article(&mut self) {
        if self.detail_articles.is_empty() {
            return;
        }
        for art in &self.detail_articles {
            if art.line_index > self.detail_scroll {
                self.detail_scroll = art.line_index;
                return;
            }
        }
        self.detail_scroll = self.detail_articles[0].line_index;
    }

    pub fn prev_article(&mut self) {
        if self.detail_articles.is_empty() {
            return;
        }
        for art in self.detail_articles.iter().rev() {
            if art.line_index < self.detail_scroll {
                self.detail_scroll = art.line_index;
                return;
            }
        }
        self.detail_scroll = self.detail_articles.last().unwrap().line_index;
    }

    /// Jump to a specific article by index in the articles list
    pub fn jump_to_article(&mut self, article_index: usize) {
        if let Some(art) = self.detail_articles.get(article_index) {
            self.detail_scroll = art.line_index;
        }
    }

    // ── Bookmarks ─────────────────────────────────────────────

    pub fn toggle_bookmark(&mut self) {
        let id = match self.view {
            View::List => self.selected_entry().map(|e| e.id.clone()),
            View::Detail => self.detail.as_ref().map(|d| d.entry.id.clone()),
            _ => None,
        };

        if let Some(id) = id {
            let added = self.bookmarks.toggle(&id);
            if let Err(e) = self.bookmarks.save() {
                warn!("Failed to save bookmarks: {e}");
            }
            self.status_message = Some(if added {
                "Bookmarked".to_string()
            } else {
                "Bookmark removed".to_string()
            });
        }
    }

    pub fn toggle_bookmarks_only(&mut self) {
        self.bookmarks_only = !self.bookmarks_only;
        self.apply_filters();
        self.status_message = Some(if self.bookmarks_only {
            "Showing bookmarks only".to_string()
        } else {
            "Showing all laws".to_string()
        });
    }

    // ── Search ────────────────────────────────────────────────

    pub fn start_search(&mut self) {
        self.input_mode = InputMode::Search;
    }

    pub fn search_push_char(&mut self, c: char) {
        self.search_query.push(c);
        self.apply_filters();
    }

    pub fn search_pop_char(&mut self) {
        self.search_query.pop();
        self.apply_filters();
    }

    pub fn finish_search(&mut self) {
        self.input_mode = InputMode::Normal;
    }

    pub fn clear_search(&mut self) {
        self.search_query.clear();
        self.input_mode = InputMode::Normal;
        self.apply_filters();
    }

    // ── Filter popups ─────────────────────────────────────────

    pub fn open_category_filter(&mut self) {
        self.popup = Popup::CategoryFilter;
        self.popup_selected = 0;
    }

    pub fn open_department_filter(&mut self) {
        self.popup = Popup::DepartmentFilter;
        self.popup_selected = 0;
    }

    pub fn open_article_list(&mut self) {
        if !self.detail_articles.is_empty() {
            self.popup = Popup::ArticleList;
            self.popup_selected = 0;
        }
    }

    pub fn close_popup(&mut self) {
        self.popup = Popup::None;
    }

    pub fn popup_move_down(&mut self) {
        let max = self.popup_items_count();
        if max > 0 && self.popup_selected < max - 1 {
            self.popup_selected += 1;
        }
    }

    pub fn popup_move_up(&mut self) {
        if self.popup_selected > 0 {
            self.popup_selected -= 1;
        }
    }

    pub fn popup_select(&mut self) {
        match self.popup {
            Popup::CategoryFilter => {
                if self.popup_selected == 0 {
                    self.category_filter = None;
                } else {
                    self.category_filter = self.categories.get(self.popup_selected - 1).cloned();
                }
                self.apply_filters();
                self.close_popup();
            }
            Popup::DepartmentFilter => {
                if self.popup_selected == 0 {
                    self.department_filter = None;
                } else {
                    self.department_filter = self.departments.get(self.popup_selected - 1).cloned();
                }
                self.apply_filters();
                self.close_popup();
            }
            Popup::ArticleList => {
                self.jump_to_article(self.popup_selected);
                self.close_popup();
            }
            _ => {}
        }
    }

    fn popup_items_count(&self) -> usize {
        match self.popup {
            Popup::CategoryFilter => self.categories.len() + 1,
            Popup::DepartmentFilter => self.departments.len() + 1,
            Popup::ArticleList => self.detail_articles.len(),
            _ => 0,
        }
    }

    // ── Back navigation ───────────────────────────────────────

    pub fn go_back(&mut self) {
        match self.view {
            View::Detail => {
                self.stop_tts();
                self.view = View::List;
                self.detail = None;
                self.detail_scroll = 0;
                self.detail_rendered_lines.clear();
            }
            View::List => {
                self.should_quit = true;
            }
            View::Loading => {
                self.should_quit = true;
            }
        }
    }

    // ── TTS ───────────────────────────────────────────────────

    /// Ensure the TTS engine is loaded (starts background load if needed).
    fn ensure_tts_loaded(&mut self) {
        match self.tts_state {
            TtsState::Unloaded | TtsState::Error => {
                self.tts_state = TtsState::Loading;
                self.status_message = Some("Loading TTS engine...".to_string());

                let handle = self.tts_engine.clone();
                let tx = self.msg_tx.clone();
                tokio::task::spawn_blocking(move || {
                    let project_root = std::env::current_dir().unwrap_or_else(|_| "/tmp".into());
                    match tts::load_engine(&handle, &project_root) {
                        Ok(()) => {
                            let _ = tx.send(Message::TtsEngineLoaded);
                        }
                        Err(e) => {
                            let _ = tx.send(Message::TtsEngineError(format!("{e:#}")));
                        }
                    }
                });
            }
            _ => {} // Loading, Ready, Synthesizing, Playing — don't restart
        }
    }

    /// Silently preload the TTS engine in the background if not already loaded.
    /// Unlike ensure_tts_loaded(), this doesn't show loading messages to the user.
    fn ensure_tts_prewarmed(&mut self) {
        match self.tts_state {
            TtsState::Unloaded | TtsState::Error => {
                self.tts_state = TtsState::Loading;
                let handle = self.tts_engine.clone();
                let tx = self.msg_tx.clone();
                tokio::task::spawn_blocking(move || {
                    tts::with_suppressed_output(|| {
                        match tts::load_engine(
                            &handle,
                            &std::env::current_dir().unwrap_or("/tmp".into()),
                        ) {
                            Ok(_) => {
                                let _ = tx.send(Message::TtsEngineLoaded);
                            }
                            Err(e) => {
                                let _ = tx.send(Message::TtsEngineError(format!("{e:#}")));
                            }
                        }
                    });
                });
            }
            _ => {} // Already loading, ready, synthesizing, or playing
        }
    }

    /// Speak the current article (제X조 + its paragraphs).
    /// Auto-scrolls to the article and highlights it.
    pub fn speak_article(&mut self) {
        self.stop_tts();

        if self.tts_state == TtsState::Unloaded || self.tts_state == TtsState::Error {
            self.pending_tts_action = PendingTtsAction::SpeakArticle;
            self.ensure_tts_loaded();
            return;
        }

        if self.tts_state == TtsState::Loading {
            self.pending_tts_action = PendingTtsAction::SpeakArticle;
            self.status_message = Some("TTS engine still loading...".to_string());
            return;
        }

        let Some(ref detail) = self.detail else {
            return;
        };

        if self.detail_articles.is_empty() {
            self.status_message = Some("No articles found in this law".to_string());
            return;
        }

        // Find which article is at the current scroll position
        let article_idx = self
            .detail_articles
            .iter()
            .rposition(|a| a.line_index <= self.detail_scroll)
            .unwrap_or(0);

        let text = match parser::extract_article_text(&detail.raw_markdown, article_idx) {
            Some(t) => t,
            None => {
                self.status_message = Some("Could not extract article text".to_string());
                return;
            }
        };

        // Single article — no queue
        self.tts_article_queue.clear();
        self.tts_current_article = Some(article_idx);
        self.detail_scroll = self.detail_articles[article_idx].line_index;

        let label = self.detail_articles[article_idx].label.clone();
        self.start_synthesis(text, label);
    }

    /// Speak all articles starting from the current scroll position.
    /// Reads article-by-article, auto-scrolling and highlighting each one.
    pub fn speak_full(&mut self) {
        self.stop_tts();

        if self.tts_state == TtsState::Unloaded || self.tts_state == TtsState::Error {
            self.pending_tts_action = PendingTtsAction::SpeakFull;
            self.ensure_tts_loaded();
            return;
        }

        if self.tts_state == TtsState::Loading {
            self.pending_tts_action = PendingTtsAction::SpeakFull;
            self.status_message = Some("TTS engine still loading...".to_string());
            return;
        }

        let Some(ref detail) = self.detail else {
            return;
        };

        if self.detail_articles.is_empty() {
            // No articles — try reading the full text as a single block
            let text = parser::extract_full_text(&detail.raw_markdown);
            if text.is_empty() {
                self.status_message = Some("No text content to read".to_string());
                return;
            }
            self.tts_article_queue.clear();
            self.tts_current_article = None;
            let title = detail.entry.title.clone();
            self.start_synthesis(text, title);
            return;
        }

        // Find the first article at or after the current scroll position
        let start_idx = self
            .detail_articles
            .iter()
            .position(|a| a.line_index >= self.detail_scroll)
            .unwrap_or(0);

        // Extract all article texts upfront so the background thread can
        // synthesize them back-to-back into the same player without gaps.
        let articles: Vec<(usize, String)> = (start_idx..self.detail_articles.len())
            .filter_map(|idx| {
                parser::extract_article_text(&detail.raw_markdown, idx).map(|t| (idx, t))
            })
            .collect();

        if articles.is_empty() {
            self.status_message = Some("No article text to read".to_string());
            return;
        }

        self.tts_article_queue.clear();
        self.tts_current_article = Some(articles[0].0);
        self.detail_scroll = self.detail_articles[articles[0].0].line_index;
        self.start_synthesis_batch(articles);
    }

    /// Synthesize a single text using streaming mode for immediate playback.
    ///
    /// Audio playback begins after a short prebuffer (determined by tts_profile),
    /// then chunks are appended as they arrive. Much better perceived latency
    /// than waiting for full synthesis.
    fn start_synthesis(&mut self, text: String, label: String) {
        self.tts_state = TtsState::Synthesizing;
        self.status_message = Some(format!("Synthesizing: {label}..."));
        self.tts_buffering = true;

        // Open audio device and create player
        match rodio::DeviceSinkBuilder::open_default_sink() {
            Ok(device_sink) => {
                let player = std::sync::Arc::new(rodio::Player::connect_new(device_sink.mixer()));
                self.tts_device_sink = Some(device_sink);
                self.tts_player = Some(player.clone());

                let handle = self.tts_engine.clone();
                let tx = self.msg_tx.clone();
                let cfg_scale = self.tts_profile.cfg_scale();
                let prebuffer_secs = self.tts_profile.prebuffer_secs();

                tokio::task::spawn_blocking(move || {
                    let mut streamer = PrebufferStreamer::new(player, tx.clone(), prebuffer_secs);

                    let result = tts::synthesize_streaming(
                        &handle,
                        &text,
                        tts::DEFAULT_KOREAN_VOICE,
                        cfg_scale,
                        |chunk| {
                            streamer.feed(chunk);
                        },
                    );

                    match result {
                        Ok(_) => {
                            streamer.flush_remaining();
                            let _ = tx.send(Message::TtsSynthesisComplete);
                        }
                        Err(e) => {
                            let _ = tx.send(Message::TtsSynthesisError(format!("{e:#}")));
                        }
                    }
                });
            }
            Err(e) => {
                self.status_message = Some(format!("Audio device error: {e:#}"));
                self.tts_state = TtsState::Ready;
            }
        }
    }

    /// Synthesize multiple articles using hybrid streaming+batch mode.
    ///
    /// The FIRST article uses streaming synthesis (quick initial playback),
    /// then remaining articles use batch synthesis for gapless transitions.
    ///
    /// Scroll / highlight advances are **timed to actual playback**, not to
    /// synthesis completion.  After each article is synthesized we know its
    /// audio duration, so we spawn a lightweight timer thread that sleeps
    /// until the cumulative playback clock reaches the boundary, then sends
    /// `TtsArticleAdvanced`.  This keeps the scroll in sync with what the
    /// user actually hears, even when synthesis runs faster than real-time.
    fn start_synthesis_batch(&mut self, articles: Vec<(usize, String)>) {
        self.tts_state = TtsState::Synthesizing;
        self.status_message = Some("Synthesizing...".to_string());
        self.tts_buffering = true;

        match DeviceSinkBuilder::open_default_sink() {
            Ok(device_sink) => {
                let player = Arc::new(Player::connect_new(device_sink.mixer()));
                self.tts_device_sink = Some(device_sink);
                self.tts_player = Some(player.clone());

                let handle = self.tts_engine.clone();
                let tx = self.msg_tx.clone();
                let cfg_scale = self.tts_profile.cfg_scale();
                let prebuffer_secs = self.tts_profile.prebuffer_secs();

                tokio::task::spawn_blocking(move || {
                    let total = articles.len();
                    let article_indices: Vec<usize> =
                        articles.iter().map(|(idx, _)| *idx).collect();

                    let mut playback_start: Option<Instant> = None;
                    let mut accumulated_secs = 0.0_f64;

                    for (i, (_article_idx, text)) in articles.into_iter().enumerate() {
                        if i == 0 {
                            // FIRST article: stream with prebuffer for fast initial playback
                            let mut streamer =
                                PrebufferStreamer::new(player.clone(), tx.clone(), prebuffer_secs);

                            match tts::synthesize_streaming(
                                &handle,
                                &text,
                                tts::DEFAULT_KOREAN_VOICE,
                                cfg_scale,
                                |chunk| {
                                    if streamer.feed(chunk) {
                                        // Initial flush just happened — record playback start
                                        playback_start = Some(Instant::now());
                                        let _ = tx.send(Message::TtsArticleAdvanced {
                                            article_idx: article_indices[0],
                                        });
                                    }
                                },
                            ) {
                                Ok(result) => {
                                    if streamer.flush_remaining() {
                                        // Short first article that never hit threshold
                                        playback_start = Some(Instant::now());
                                        let _ = tx.send(Message::TtsArticleAdvanced {
                                            article_idx: article_indices[0],
                                        });
                                    }

                                    accumulated_secs += result.duration_secs;

                                    // Schedule scroll to second article
                                    if let Some(start) = playback_start
                                        && total > 1
                                    {
                                        let next_article_idx = article_indices[1];
                                        let target_secs = accumulated_secs;
                                        let tx_timer = tx.clone();
                                        std::thread::spawn(move || {
                                            let target =
                                                start + Duration::from_secs_f64(target_secs);
                                            let now = Instant::now();
                                            if target > now {
                                                std::thread::sleep(target - now);
                                            }
                                            let _ = tx_timer.send(Message::TtsArticleAdvanced {
                                                article_idx: next_article_idx,
                                            });
                                        });
                                    }

                                    info!("Batch article 1/{total} synthesized (streaming)");
                                }
                                Err(e) => {
                                    let _ = tx.send(Message::TtsSynthesisError(format!("{e:#}")));
                                    return;
                                }
                            }
                        } else {
                            // Remaining articles: batch synthesis
                            match tts::synthesize(
                                &handle,
                                &text,
                                tts::DEFAULT_KOREAN_VOICE,
                                cfg_scale,
                            ) {
                                Ok(result) => {
                                    let article_duration = result.duration_secs;
                                    let source = rodio::buffer::SamplesBuffer::new(
                                        CHANNELS,
                                        SAMPLE_RATE,
                                        result.audio,
                                    );
                                    player.append(source);

                                    accumulated_secs += article_duration;

                                    // Schedule scroll to the NEXT article
                                    if let Some(start) = playback_start
                                        && i + 1 < total
                                    {
                                        let next_article_idx = article_indices[i + 1];
                                        let target_secs = accumulated_secs;
                                        let tx_timer = tx.clone();
                                        std::thread::spawn(move || {
                                            let target =
                                                start + Duration::from_secs_f64(target_secs);
                                            let now = Instant::now();
                                            if target > now {
                                                std::thread::sleep(target - now);
                                            }
                                            let _ = tx_timer.send(Message::TtsArticleAdvanced {
                                                article_idx: next_article_idx,
                                            });
                                        });
                                    }

                                    info!("Batch article {}/{total} synthesized", i + 1);
                                }
                                Err(e) => {
                                    let _ = tx.send(Message::TtsSynthesisError(format!("{e:#}")));
                                    return;
                                }
                            }
                        }
                    }

                    let _ = tx.send(Message::TtsSynthesisDone);
                });
            }
            Err(e) => {
                self.tts_state = TtsState::Ready;
                self.status_message = Some(format!("Audio error: {e:#}"));
                error!("Failed to open audio output: {e:#}");
            }
        }
    }

    /// Stop any ongoing TTS synthesis or playback.
    pub fn stop_tts(&mut self) {
        if let Some(player) = self.tts_player.take() {
            player.stop();
        }
        self.tts_device_sink = None;
        self.tts_article_queue.clear();
        self.tts_current_article = None;
        self.tts_buffering = false;

        if self.tts_state == TtsState::Playing || self.tts_state == TtsState::Synthesizing {
            self.tts_state = TtsState::Ready;
            self.status_message = Some("Stopped".to_string());
        }
    }

    /// Check if TTS playback finished (all audio drained from player).
    pub fn check_tts_playback(&mut self) {
        if self.tts_state == TtsState::Playing
            && let Some(ref player) = self.tts_player
            && player.empty()
        {
            self.tts_player = None;
            self.tts_device_sink = None;
            self.tts_state = TtsState::Ready;
            self.tts_current_article = None;
            self.status_message = Some("Playback finished".to_string());
        }
    }

    /// Return the line range (start..end) of the currently-playing article,
    /// for the renderer to apply highlight styling.
    pub fn tts_highlight_lines(&self) -> Option<(usize, usize)> {
        let article_idx = self.tts_current_article?;
        let start = self.detail_articles.get(article_idx)?.line_index;
        let end = self
            .detail_articles
            .get(article_idx + 1)
            .map(|a| a.line_index)
            .unwrap_or(self.detail_lines_count);
        Some((start, end))
    }
}
