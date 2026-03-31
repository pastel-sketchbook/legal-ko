use std::collections::{HashSet, VecDeque};

use legal_ko_core::bookmarks::Bookmarks;
use legal_ko_core::models::{ArticleRef, LawDetail, LawEntry, MetadataIndex};
use legal_ko_core::preferences::Preferences;
use legal_ko_core::tts::{self, TtsEngineHandle, TtsState};
use legal_ko_core::{cache, client, parser};

use crate::theme::{self, Theme};

use legal_ko_core::tts::OUTPUT_SR;
use rodio::{OutputStream, Sink};
use tokio::sync::mpsc;
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
    /// A streamed audio chunk ready to be appended to the sink.
    TtsChunk {
        audio: Vec<f32>,
    },
    /// Streaming synthesis finished (all chunks sent).
    TtsSynthesisDone,
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
    /// Index of the article currently being spoken (into `detail_articles`).
    pub tts_current_article: Option<usize>,
    /// Queue of article indices remaining to be spoken (for `R` read-all mode).
    tts_article_queue: VecDeque<usize>,
    /// Keeps the audio OutputStream alive for the duration of playback.
    tts_stream: Option<OutputStream>,
    /// Sink handle for controlling playback (stop/pause).
    tts_sink: Option<Sink>,

    /// Tick counter incremented every event-loop iteration (~50ms).
    /// Used for UI animations (e.g. TTS loading indicator).
    pub tick: usize,
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
            bookmarks,
            status_message: None,
            msg_tx,
            msg_rx,
            theme_index,
            tts_state: TtsState::Unloaded,
            tts_engine: tts::new_engine_handle(),
            tts_current_article: None,
            tts_article_queue: VecDeque::new(),
            tts_stream: None,
            tts_sink: None,
            tick: 0,
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
            }
            Message::MetadataError(err) => {
                self.status_message = Some(format!("Error: {err}"));
                error!("Failed to load metadata: {err}");
            }
            Message::LawContentLoaded { id, content } => {
                self.on_law_content_loaded(&id, &content);
            }
            Message::LawContentError { id, error } => {
                self.detail_loading = false;
                self.status_message = Some(format!("Error loading {id}: {error}"));
                error!("Failed to load law {id}: {error}");
            }
            Message::TtsEngineLoaded => {
                self.tts_state = TtsState::Ready;
                self.status_message = Some("TTS engine loaded".to_string());
                info!("TTS engine loaded successfully");
            }
            Message::TtsEngineError(err) => {
                self.tts_state = TtsState::Error;
                self.status_message = Some(format!("TTS error: {err}"));
                error!("TTS engine load failed: {err}");
            }
            Message::TtsChunk { audio } => {
                // Append streamed audio chunk to the active sink
                if let Some(ref sink) = self.tts_sink {
                    let source = rodio::buffer::SamplesBuffer::new(1, OUTPUT_SR, audio);
                    sink.append(source);
                }
            }
            Message::TtsSynthesisDone => {
                // Synthesis finished; state stays Playing until sink drains
                if self.tts_state == TtsState::Synthesizing {
                    self.tts_state = TtsState::Playing;
                    self.status_message = Some("Playing...".to_string());
                }
            }
            Message::TtsSynthesisError(err) => {
                self.tts_state = TtsState::Ready;
                self.tts_sink = None;
                self.tts_stream = None;
                self.tts_current_article = None;
                self.tts_article_queue.clear();
                self.status_message = Some(format!("TTS error: {err}"));
                error!("TTS synthesis failed: {err}");
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

    /// Apply search + category + department + bookmarks filters
    pub fn apply_filters(&mut self) {
        let query_lower = self.search_query.to_lowercase();

        self.filtered_indices = self
            .all_laws
            .iter()
            .enumerate()
            .filter(|(_, entry)| {
                // Search filter
                if !query_lower.is_empty() && !entry.title.to_lowercase().contains(&query_lower) {
                    return false;
                }
                // Category filter
                if let Some(ref cat) = self.category_filter
                    && &entry.category != cat
                {
                    return false;
                }
                // Department filter
                if let Some(ref dept) = self.department_filter
                    && !entry.departments.contains(dept)
                {
                    return false;
                }
                // Bookmarks filter
                if self.bookmarks_only && !self.bookmarks.is_bookmarked(&entry.id) {
                    return false;
                }
                true
            })
            .map(|(i, _)| i)
            .collect();

        // Clamp selection
        if self.filtered_indices.is_empty() {
            self.list_selected = 0;
        } else if self.list_selected >= self.filtered_indices.len() {
            self.list_selected = self.filtered_indices.len() - 1;
        }
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

    /// Speak the current article (제X조 + its paragraphs).
    /// Auto-scrolls to the article and highlights it.
    pub fn speak_article(&mut self) {
        self.stop_tts();

        if self.tts_state == TtsState::Unloaded || self.tts_state == TtsState::Error {
            self.ensure_tts_loaded();
            self.status_message =
                Some("Loading TTS engine... press r again when ready".to_string());
            return;
        }

        if self.tts_state == TtsState::Loading {
            self.status_message = Some("TTS engine still loading... please wait".to_string());
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
            self.ensure_tts_loaded();
            self.status_message =
                Some("Loading TTS engine... press R again when ready".to_string());
            return;
        }

        if self.tts_state == TtsState::Loading {
            self.status_message = Some("TTS engine still loading... please wait".to_string());
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

        // Queue all articles from start_idx onward
        self.tts_article_queue = (start_idx..self.detail_articles.len()).collect::<VecDeque<_>>();

        // Start with the first queued article
        self.advance_tts_queue();
    }

    /// Pop the next article from the queue and start synthesizing it.
    /// Auto-scrolls to that article.
    fn advance_tts_queue(&mut self) {
        let Some(article_idx) = self.tts_article_queue.pop_front() else {
            // Queue exhausted
            self.tts_current_article = None;
            self.tts_state = TtsState::Ready;
            self.status_message = Some("Read-all finished".to_string());
            return;
        };

        let Some(ref detail) = self.detail else {
            return;
        };

        let text = match parser::extract_article_text(&detail.raw_markdown, article_idx) {
            Some(t) => t,
            None => {
                // Skip this article, try next
                self.advance_tts_queue();
                return;
            }
        };

        self.tts_current_article = Some(article_idx);
        self.detail_scroll = self.detail_articles[article_idx].line_index;

        let remaining = self.tts_article_queue.len();
        let label = format!(
            "{} ({}/{})",
            self.detail_articles[article_idx].label,
            self.detail_articles.len() - remaining,
            self.detail_articles.len(),
        );
        self.start_synthesis(text, label);
    }

    /// Start streaming synthesis in a background thread.
    ///
    /// Creates the audio sink immediately so playback begins as soon as the
    /// first chunk is decoded.
    fn start_synthesis(&mut self, text: String, label: String) {
        self.tts_state = TtsState::Synthesizing;
        self.status_message = Some(format!("Synthesizing: {label}..."));

        // Open audio output now so chunks can be appended as they arrive
        match OutputStream::try_default() {
            Ok((stream, stream_handle)) => match Sink::try_new(&stream_handle) {
                Ok(sink) => {
                    self.tts_stream = Some(stream);
                    self.tts_sink = Some(sink);
                }
                Err(e) => {
                    self.tts_state = TtsState::Ready;
                    self.status_message = Some(format!("Audio error: {e:#}"));
                    error!("Failed to create sink: {e:#}");
                    return;
                }
            },
            Err(e) => {
                self.tts_state = TtsState::Ready;
                self.status_message = Some(format!("Audio error: {e:#}"));
                error!("Failed to open audio output: {e:#}");
                return;
            }
        }

        let handle = self.tts_engine.clone();
        let tx = self.msg_tx.clone();
        tokio::task::spawn_blocking(move || {
            match tts::synthesize_streaming(
                &handle,
                &text,
                tts::DEFAULT_KOREAN_VOICE,
                tts::DEFAULT_CFG_SCALE,
                |chunk| {
                    let _ = tx.send(Message::TtsChunk {
                        audio: chunk.to_vec(),
                    });
                },
            ) {
                Ok(_result) => {
                    let _ = tx.send(Message::TtsSynthesisDone);
                }
                Err(e) => {
                    let _ = tx.send(Message::TtsSynthesisError(format!("{e:#}")));
                }
            }
        });
    }

    /// Stop any ongoing TTS synthesis or playback.
    pub fn stop_tts(&mut self) {
        if let Some(sink) = self.tts_sink.take() {
            sink.stop();
        }
        self.tts_stream = None;
        self.tts_article_queue.clear();
        self.tts_current_article = None;

        if self.tts_state == TtsState::Playing || self.tts_state == TtsState::Synthesizing {
            self.tts_state = TtsState::Ready;
            self.status_message = Some("Stopped".to_string());
        }
    }

    /// Check if TTS playback finished; if there are queued articles, advance.
    pub fn check_tts_playback(&mut self) {
        if self.tts_state == TtsState::Playing
            && let Some(ref sink) = self.tts_sink
            && sink.empty()
        {
            self.tts_sink = None;
            self.tts_stream = None;

            if self.tts_article_queue.is_empty() {
                // All done
                self.tts_state = TtsState::Ready;
                self.tts_current_article = None;
                self.status_message = Some("Playback finished".to_string());
            } else {
                // Advance to next article
                self.advance_tts_queue();
            }
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
