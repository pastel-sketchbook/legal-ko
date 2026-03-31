//! TTS engine wrapper around `vibe-rust` RealtimeTts with `rodio` playback.
//!
//! Design:
//! - Model loading is expensive (~3-5s), so we load once and hold in an `Arc<Mutex<..>>`.
//! - `RealtimeTts::synthesize()` is synchronous — callers must use
//!   `tokio::task::spawn_blocking` from async contexts.
//! - Audio playback uses `rodio` for in-process playback from memory (24 kHz mono f32).
//! - vibe-rust uses `println!` internally; we suppress stdout/stderr via fd
//!   redirection so the ratatui TUI is not corrupted.

use std::os::unix::io::AsRawFd;
use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use tracing::{debug, info, warn};

use rodio::{OutputStream, Sink};
use vibe_rust::realtime::{RealtimeConfig, RealtimeTts, SynthesisResult, OUTPUT_SR};

/// Default Korean voice preset (woman).
pub const DEFAULT_KOREAN_VOICE: &str = "kr-spk0_woman";

/// Default CFG scale for synthesis.
pub const DEFAULT_CFG_SCALE: f32 = 1.5;

// ── stdout/stderr suppression ───────────────────────────────

/// Temporarily redirect stdout and stderr to `/dev/null`, run the closure,
/// then restore the original file descriptors.
///
/// This prevents vibe-rust's `println!` calls from corrupting the ratatui
/// terminal.  Uses raw fd-level redirection (`dup`/`dup2`) so it catches
/// *all* writes on fd 1 and fd 2, regardless of buffering.
fn with_suppressed_output<F, R>(f: F) -> R
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
pub fn new_engine_handle() -> TtsEngineHandle {
    Arc::new(Mutex::new(None))
}

/// Load the TTS engine synchronously (call from `spawn_blocking`).
///
/// The `project_root` is passed to `RealtimeTts::load()` for resolving
/// voice presets and model files (typically `std::env::current_dir()`).
pub fn load_engine(handle: &TtsEngineHandle, project_root: &Path) -> Result<()> {
    let config = RealtimeConfig::default();
    info!(
        "Loading VibeVoice TTS (device={}, attn={})",
        config.device, config.attn_impl
    );

    let tts = with_suppressed_output(|| RealtimeTts::load(config, project_root))
        .context("Failed to load VibeVoice TTS model")?;

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

    debug!("Synthesizing {} chars with voice '{speaker}'", text.len());

    let result = with_suppressed_output(|| tts.synthesize(text, speaker, cfg_scale, None))?;

    info!(
        "Synthesized {:.1}s audio in {:.1}s (RTF: {:.2})",
        result.duration_secs, result.generation_time_secs, result.rtf
    );

    Ok(result)
}

/// Play f32 PCM audio at 24 kHz mono through `rodio`.
///
/// This blocks until playback completes.
pub fn play_audio(samples: &[f32]) -> Result<()> {
    if samples.is_empty() {
        warn!("No audio samples to play");
        return Ok(());
    }

    let (_stream, stream_handle) =
        OutputStream::try_default().context("Failed to open audio output device")?;

    let sink = Sink::try_new(&stream_handle).context("Failed to create audio sink")?;

    // rodio's SamplesBuffer takes the sample rate and channel count
    let source = rodio::buffer::SamplesBuffer::new(1, OUTPUT_SR, samples.to_vec());

    sink.append(source);
    sink.sleep_until_end();

    debug!("Audio playback finished");
    Ok(())
}

/// Play f32 PCM audio non-blocking. Returns the Sink handle so the caller
/// can stop/pause playback.
///
/// **Important**: The caller must keep the returned `OutputStream` alive
/// for the duration of playback (dropping it stops audio).
pub fn play_audio_async(samples: &[f32]) -> Result<(OutputStream, Sink)> {
    if samples.is_empty() {
        anyhow::bail!("No audio samples to play");
    }

    let (stream, stream_handle) =
        OutputStream::try_default().context("Failed to open audio output device")?;

    let sink = Sink::try_new(&stream_handle).context("Failed to create audio sink")?;

    let source = rodio::buffer::SamplesBuffer::new(1, OUTPUT_SR, samples.to_vec());
    sink.append(source);

    Ok((stream, sink))
}

/// Load the TTS engine and synthesize + play in one call (for CLI).
///
/// This is a convenience wrapper that blocks the entire time.
pub fn synthesize_and_play(
    project_root: &Path,
    text: &str,
    speaker: &str,
    cfg_scale: f32,
) -> Result<SynthesisResult> {
    let handle = new_engine_handle();
    load_engine(&handle, project_root)?;

    let result = synthesize(&handle, text, speaker, cfg_scale)?;
    play_audio(&result.audio)?;

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
        let result = synthesize(&handle, "hello", DEFAULT_KOREAN_VOICE, DEFAULT_CFG_SCALE);
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
