//! TTS engine wrapper around `vibe-rust` `RealtimeTts` with `rodio` playback.
//!
//! Design:
//! - Model loading is expensive (~3-5s), so we load once and hold in an `Arc<Mutex<..>>`.
//! - `RealtimeTts::synthesize()` is synchronous — callers must use
//!   `tokio::task::spawn_blocking` from async contexts.
//! - Audio playback uses `rodio` for in-process playback from memory (24 kHz mono f32).
//! - vibe-rust uses `println!` internally; we suppress stdout/stderr via fd
//!   redirection so the ratatui TUI is not corrupted.

use std::fmt;
use std::num::NonZero;
use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player};
pub use vibe_rust::realtime::OUTPUT_SR;
use vibe_rust::realtime::{RealtimeConfig, RealtimeTts, SynthesisResult};

/// Default Korean voice preset (man).
pub const DEFAULT_KOREAN_VOICE: &str = "kr-Spk1_man";

/// TTS quality/speed profile.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub enum TtsProfile {
    /// Fast mode: `cfg_scale=1.0` for ~2x diffusion speedup, shorter prebuffer (1s).
    Fast,
    /// Balanced mode: `cfg_scale=1.5` (original quality), longer prebuffer (5s).
    #[default]
    Balanced,
}

impl TtsProfile {
    /// Get the CFG scale for this profile.
    #[must_use]
    pub fn cfg_scale(self) -> f32 {
        match self {
            Self::Fast => 1.0,
            Self::Balanced => 1.5,
        }
    }

    /// Get the prebuffer duration in seconds for streaming playback.
    #[must_use]
    pub fn prebuffer_secs(self) -> f64 {
        match self {
            Self::Fast => 1.0,
            Self::Balanced => 5.0,
        }
    }
}

impl fmt::Display for TtsProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fast => write!(f, "Fast"),
            Self::Balanced => write!(f, "Balanced"),
        }
    }
}

/// Mono channel count for rodio.
// SAFETY (const): `1` is non-zero, so `unwrap` will never panic.
const CHANNELS: NonZero<u16> = NonZero::new(1).unwrap();

/// Sample rate for rodio (must match `OUTPUT_SR` = 24000).
// SAFETY (const): `OUTPUT_SR` is 24000, which is non-zero, so `unwrap` will never panic.
const SAMPLE_RATE: NonZero<u32> = NonZero::new(OUTPUT_SR).unwrap();

// ── stdout/stderr suppression ───────────────────────────────

/// Temporarily redirect stdout and stderr to `/dev/null`, run the closure,
/// then restore the original file descriptors.
///
/// This prevents vibe-rust's `println!` calls (and ONNX Runtime's C++ logger)
/// from corrupting the ratatui terminal.  Uses raw fd-level redirection
/// (`dup`/`dup2`) so it catches *all* writes on fd 1 and fd 2, regardless of
/// buffering.
///
/// Callers should wrap the **entire** blocking task (not just individual calls)
/// so that deferred output from background threads is also suppressed.
pub fn with_suppressed_output<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    // SAFETY: dup/dup2/close are well-defined POSIX calls.  We save the
    // original fds, redirect to /dev/null, call the closure, then restore.
    unsafe {
        let stdout_backup = libc::dup(libc::STDOUT_FILENO);
        let stderr_backup = libc::dup(libc::STDERR_FILENO);

        if let Ok(devnull) = std::fs::OpenOptions::new().write(true).open("/dev/null") {
            let null_fd = devnull.as_raw_fd();
            libc::dup2(null_fd, libc::STDOUT_FILENO);
            libc::dup2(null_fd, libc::STDERR_FILENO);
            // devnull is dropped here, but the dup2'd fds remain open
        }

        let result = f();

        // Restore originals
        if stdout_backup >= 0 {
            libc::dup2(stdout_backup, libc::STDOUT_FILENO);
            libc::close(stdout_backup);
        }
        if stderr_backup >= 0 {
            libc::dup2(stderr_backup, libc::STDERR_FILENO);
            libc::close(stderr_backup);
        }

        result
    }
}

/// TTS engine state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TtsState {
    /// No TTS engine loaded yet.
    Unloaded,
    /// Model is currently loading.
    Loading,
    /// Model loaded, ready to synthesize.
    Ready,
    /// Currently synthesizing speech.
    Synthesizing,
    /// Playing back audio.
    Playing,
    /// An error occurred.
    Error,
}

/// Shared handle to the TTS engine.
///
/// Wraps `RealtimeTts` in `Arc<Mutex<..>>` so it can be shared across
/// the main thread and background tasks.
pub type TtsEngineHandle = Arc<Mutex<Option<RealtimeTts>>>;

/// Create an unloaded engine handle.
#[must_use]
pub fn new_engine_handle() -> TtsEngineHandle {
    Arc::new(Mutex::new(None))
}

/// Environment variable to override ONNX intra-op thread count.
///
/// Controls how many CPU threads each ONNX operator can use internally.
/// The upstream default is 4. Apple Silicon M2/M3 Pro/Max often benefit
/// from 6–8 threads. Set to benchmark on your hardware:
///
/// ```sh
/// LEGAL_KO_ONNX_THREADS=6 legal-ko-cli speak 123456
/// ```
pub(crate) const ONNX_THREADS_ENV: &str = "LEGAL_KO_ONNX_THREADS";

/// Load the TTS engine synchronously (call from `spawn_blocking`).
///
/// The `project_root` is passed to `RealtimeTts::load()` for resolving
/// voice presets and model files (typically `std::env::current_dir()`).
///
/// Reads [`ONNX_THREADS_ENV`] to configure ONNX intra-op thread count.
///
/// # Errors
///
/// Returns an error if model loading fails or the engine mutex is poisoned.
pub fn load_engine(handle: &TtsEngineHandle, project_root: &Path) -> Result<()> {
    let intra_threads = std::env::var(ONNX_THREADS_ENV)
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .filter(|&n| n > 0);

    let config = RealtimeConfig {
        intra_threads,
        ..RealtimeConfig::default()
    };

    info!(
        device = %config.device,
        attn = %config.attn_impl,
        threads = %intra_threads.map_or("default".to_string(), |n| n.to_string()),
        "Loading VibeVoice TTS",
    );

    let tts =
        RealtimeTts::load(config, project_root).context("Failed to load VibeVoice TTS model")?;

    let mut guard = handle
        .lock()
        .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;
    *guard = Some(tts);

    info!("TTS engine loaded successfully");
    Ok(())
}

/// Synthesize speech from text synchronously (call from `spawn_blocking`).
///
/// Returns the raw `SynthesisResult` containing audio samples.
///
/// # Errors
///
/// Returns an error if the engine is not loaded or synthesis fails.
pub fn synthesize(
    handle: &TtsEngineHandle,
    text: &str,
    speaker: &str,
    cfg_scale: f32,
) -> Result<SynthesisResult> {
    let mut guard = handle
        .lock()
        .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;

    let tts = guard
        .as_mut()
        .context("TTS engine not loaded — call load_engine first")?;

    debug!(chars = text.len(), speaker, "Synthesizing");

    let result = tts
        .synthesize(text, speaker, cfg_scale, None)
        .context("TTS synthesis failed")?;

    info!(
        duration_secs = format!("{:.1}", result.duration_secs),
        generation_secs = format!("{:.1}", result.generation_time_secs),
        rtf = format!("{:.2}", result.rtf),
        "Synthesized audio",
    );

    Ok(result)
}

/// Synthesize speech with streaming: each decoded audio chunk is passed to
/// `on_chunk` as it becomes available, so playback can begin while generation
/// continues.
///
/// Call from `spawn_blocking`.  Returns the full `SynthesisResult` when done.
///
/// # Errors
///
/// Returns an error if the engine is not loaded or synthesis fails.
pub fn synthesize_streaming<F>(
    handle: &TtsEngineHandle,
    text: &str,
    speaker: &str,
    cfg_scale: f32,
    on_chunk: F,
) -> Result<SynthesisResult>
where
    F: FnMut(&[f32]),
{
    let mut guard = handle
        .lock()
        .map_err(|e| anyhow::anyhow!("Lock poisoned: {e}"))?;

    let tts = guard
        .as_mut()
        .context("TTS engine not loaded — call load_engine first")?;

    debug!(chars = text.len(), speaker, "Synthesizing (streaming)",);

    let result = tts
        .synthesize_streaming(text, speaker, cfg_scale, None, on_chunk)
        .context("TTS streaming synthesis failed")?;

    info!(
        duration_secs = format!("{:.1}", result.duration_secs),
        generation_secs = format!("{:.1}", result.generation_time_secs),
        rtf = format!("{:.2}", result.rtf),
        "Synthesized audio (streaming)",
    );

    Ok(result)
}

/// Play f32 PCM audio at 24 kHz mono through `rodio`.
///
/// This blocks until playback completes.
///
/// # Errors
///
/// Returns an error if the audio output device cannot be opened.
pub fn play_audio(samples: &[f32]) -> Result<()> {
    if samples.is_empty() {
        warn!("No audio samples to play");
        return Ok(());
    }

    let sink =
        DeviceSinkBuilder::open_default_sink().context("Failed to open audio output device")?;
    let player = Player::connect_new(sink.mixer());

    let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, samples.to_vec());
    player.append(source);
    player.sleep_until_end();

    debug!("Audio playback finished");
    Ok(())
}

/// Play f32 PCM audio non-blocking. Returns the device sink and player handle
/// so the caller can stop/pause playback.
///
/// **Important**: The caller must keep the returned `MixerDeviceSink` alive
/// for the duration of playback (dropping it stops audio).
///
/// # Errors
///
/// Returns an error if samples are empty or the audio device cannot be opened.
pub fn play_audio_async(samples: &[f32]) -> Result<(MixerDeviceSink, Player)> {
    if samples.is_empty() {
        anyhow::bail!("No audio samples to play");
    }

    let sink =
        DeviceSinkBuilder::open_default_sink().context("Failed to open audio output device")?;
    let player = Player::connect_new(sink.mixer());

    let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, samples.to_vec());
    player.append(source);

    Ok((sink, player))
}

/// Aggregated stats from multi-segment synthesis and playback.
#[derive(Debug, Clone)]
pub struct PlaybackStats {
    /// Total audio duration in seconds.
    pub duration_secs: f64,
    /// Total wall-clock time spent generating audio.
    pub generation_time_secs: f64,
    /// Overall real-time factor (`generation_time` / duration).
    pub rtf: f64,
    /// Number of segments synthesized.
    pub segments: usize,
}

/// Synthesize multiple text segments using **batch** mode and play them
/// back-to-back with natural pipelining.
///
/// Each segment is fully synthesized via [`synthesize`] before being appended
/// to the rodio player as **one large `SamplesBuffer`**.  This eliminates the
/// micro-chunk source-boundary gaps that plague streaming mode.
///
/// Playback begins as soon as the first segment is ready, so later segments
/// are synthesized while earlier ones are already playing — the same proven
/// pipeline the TUI uses for `R` (read all).
///
/// # Errors
///
/// Returns an error if the engine cannot be loaded, no segments are provided,
/// the audio device cannot be opened, or any segment synthesis fails.
pub fn synthesize_and_play_segments(
    project_root: &Path,
    segments: &[String],
    speaker: &str,
    cfg_scale: f32,
) -> Result<PlaybackStats> {
    let handle = new_engine_handle();
    load_engine(&handle, project_root)?;
    synthesize_and_play_segments_with_handle(&handle, segments, speaker, cfg_scale)
}

/// Like [`synthesize_and_play_segments`] but uses a pre-loaded engine handle.
///
/// This allows the caller to overlap engine loading with other work (e.g.,
/// network I/O) and then pass the ready handle for synthesis.
///
/// # Errors
///
/// Returns an error if no segments are provided, the audio device cannot be
/// opened, or any segment synthesis fails.
pub fn synthesize_and_play_segments_with_handle(
    handle: &TtsEngineHandle,
    segments: &[String],
    speaker: &str,
    cfg_scale: f32,
) -> Result<PlaybackStats> {
    if segments.is_empty() {
        anyhow::bail!("No text segments to speak");
    }

    let device_sink =
        DeviceSinkBuilder::open_default_sink().context("Failed to open audio output device")?;
    let player = Player::connect_new(device_sink.mixer());

    let mut total_duration = 0.0_f64;
    let mut total_gen_time = 0.0_f64;
    let mut synthesized = 0_usize;
    let total = segments.len();

    for (i, segment) in segments.iter().enumerate() {
        if segment.trim().is_empty() {
            continue;
        }

        let result = synthesize(handle, segment, speaker, cfg_scale)?;

        let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, result.audio);
        player.append(source);

        total_duration += result.duration_secs;
        total_gen_time += result.generation_time_secs;
        synthesized += 1;

        info!(
            segment = i + 1,
            total,
            duration_secs = format!("{:.1}", result.duration_secs),
            generation_secs = format!("{:.1}", result.generation_time_secs),
            "Segment synthesized",
        );
    }

    player.sleep_until_end();
    drop(device_sink);

    Ok(PlaybackStats {
        duration_secs: total_duration,
        generation_time_secs: total_gen_time,
        rtf: if total_duration > 0.0 {
            total_gen_time / total_duration
        } else {
            0.0
        },
        segments: synthesized,
    })
}

/// Load the TTS engine and synthesize + play in one call (for CLI).
///
/// Uses streaming synthesis with a **pre-buffer**: the first ~N seconds of audio
/// are accumulated in memory before anything is sent to the audio device.
/// Once the buffer is full it is flushed as a single large source, and
/// subsequent chunks are appended immediately.  This eliminates the play-pause-play
/// stutter caused by the player draining faster than the next chunk arrives.
///
/// The prebuffer duration is determined by `profile`.
///
/// # Errors
///
/// Returns an error if the engine cannot be loaded, the audio device cannot be
/// opened, or synthesis fails.
pub fn synthesize_and_play(
    project_root: &Path,
    text: &str,
    speaker: &str,
    profile: TtsProfile,
) -> Result<SynthesisResult> {
    let handle = new_engine_handle();
    load_engine(&handle, project_root)?;
    synthesize_and_play_with_handle(&handle, text, speaker, profile)
}

/// Like [`synthesize_and_play`] but uses a pre-loaded engine handle.
///
/// This allows the caller to overlap engine loading with other work (e.g.,
/// network I/O) and then pass the ready handle for synthesis.
///
/// # Errors
///
/// Returns an error if the audio device cannot be opened or synthesis fails.
pub fn synthesize_and_play_with_handle(
    handle: &TtsEngineHandle,
    text: &str,
    speaker: &str,
    profile: TtsProfile,
) -> Result<SynthesisResult> {
    let device_sink =
        DeviceSinkBuilder::open_default_sink().context("Failed to open audio output device")?;
    let player = Player::connect_new(device_sink.mixer());

    let prebuffer_secs = profile.prebuffer_secs();
    let cfg_scale = profile.cfg_scale();
    let prebuffer_val = f64::from(OUTPUT_SR) * prebuffer_secs;
    debug_assert!(
        prebuffer_val >= 0.0 && prebuffer_val.is_finite(),
        "prebuffer must be non-negative and finite, got {prebuffer_val}"
    );
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let prebuffer_threshold = prebuffer_val as usize;
    let mut prebuffer: Vec<f32> = Vec::with_capacity(prebuffer_threshold + 48_000);
    let mut flushed = false;

    let result = synthesize_streaming(handle, text, speaker, cfg_scale, |chunk| {
        if flushed {
            // Already playing — feed chunks directly
            let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, chunk.to_vec());
            player.append(source);
        } else {
            // Accumulate until we have enough runway
            prebuffer.extend_from_slice(chunk);
            if prebuffer.len() >= prebuffer_threshold {
                let drained = std::mem::take(&mut prebuffer);
                let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, drained);
                player.append(source);
                flushed = true;
                #[allow(clippy::cast_precision_loss)]
                let threshold_secs = prebuffer_threshold as f64 / f64::from(OUTPUT_SR);
                debug!(
                    threshold_secs = format!("{threshold_secs:.1}"),
                    "Pre-buffer flushed, playback started"
                );
            }
        }
    })?;

    // Short text that never hit the threshold — flush whatever we collected
    if !flushed && !prebuffer.is_empty() {
        let source = rodio::buffer::SamplesBuffer::new(CHANNELS, SAMPLE_RATE, prebuffer);
        player.append(source);
    }

    // Wait for playback to finish
    player.sleep_until_end();
    drop(device_sink);

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_engine_handle_is_none() {
        let handle = new_engine_handle();
        let guard = handle.lock().unwrap();
        assert!(guard.is_none());
    }

    #[test]
    fn test_synthesize_without_load_fails() {
        let handle = new_engine_handle();
        let result = synthesize(
            &handle,
            "hello",
            DEFAULT_KOREAN_VOICE,
            TtsProfile::default().cfg_scale(),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_play_empty_audio() {
        let result = play_audio(&[]);
        assert!(result.is_ok());
    }

    #[test]
    fn test_play_audio_async_empty_fails() {
        let result = play_audio_async(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_state() {
        assert_eq!(TtsState::Unloaded, TtsState::Unloaded);
    }

    #[test]
    fn test_suppressed_output_returns_value() {
        let result = with_suppressed_output(|| 42);
        assert_eq!(result, 42);
    }
}
