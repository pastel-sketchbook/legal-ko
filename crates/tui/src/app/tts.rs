use std::num::NonZero;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rodio::{DeviceSinkBuilder, Player};
use tokio::sync::mpsc;
use tracing::{error, info};

use legal_ko_core::parser;
use legal_ko_core::tts::{self, OUTPUT_SR, TtsState};

use super::{App, Message};

// SAFETY (const): literal 1 is always non-zero.
pub const CHANNELS: NonZero<u16> = NonZero::new(1).unwrap();
// SAFETY (const): OUTPUT_SR is a non-zero compile-time constant (24 000).
pub const SAMPLE_RATE: NonZero<u32> = NonZero::new(tts::OUTPUT_SR).unwrap();

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
        let prebuffer_val = f64::from(OUTPUT_SR) * prebuffer_secs;
        debug_assert!(
            prebuffer_val >= 0.0 && prebuffer_val.is_finite(),
            "prebuffer must be non-negative and finite, got {prebuffer_val}"
        );
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let prebuffer_threshold = prebuffer_val as usize;
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
        if self.flushed {
            let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, chunk.to_vec());
            self.player.append(source);
        } else {
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

/// Action deferred until the TTS engine finishes loading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingTtsAction {
    None,
    /// Read the article at the current scroll position.
    SpeakArticle,
    /// Read all articles from the current scroll position.
    SpeakFull,
}

impl App {
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
    /// Unlike `ensure_tts_loaded()`, this doesn't show loading messages to the user.
    pub(super) fn ensure_tts_prewarmed(&mut self) {
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
                            Ok(()) => {
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

        let Some(text) = parser::extract_article_text(&detail.raw_markdown, article_idx) else {
            self.status_message = Some("Could not extract article text".to_string());
            return;
        };

        // Single article — no queue
        self.tts_article_queue.clear();
        self.tts_current_article = Some(article_idx);
        self.detail_scroll = self.detail_articles[article_idx].line_index;

        let label = self.detail_articles[article_idx].label.clone();
        self.start_synthesis(text, &label);
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
            self.start_synthesis(text, &title);
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
    /// Audio playback begins after a short prebuffer (determined by `tts_profile`),
    /// then chunks are appended as they arrive. Much better perceived latency
    /// than waiting for full synthesis.
    fn start_synthesis(&mut self, text: String, label: &str) {
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
    #[allow(clippy::too_many_lines)]
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

                                    info!(
                                        segment = 1,
                                        total, "Batch article synthesized (streaming)"
                                    );
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

                                    info!(segment = i + 1, total, "Batch article synthesized");
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
                error!(error = %e, "Failed to open audio output");
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
            .map_or(self.detail_lines_count, |a| a.line_index);
        Some((start, end))
    }
}
