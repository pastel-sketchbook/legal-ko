pub mod bookmarks;
pub mod cache;
pub mod client;
pub mod config;
pub mod context;
pub mod enrichment;
pub mod models;
pub mod parser;
pub mod preferences;
pub mod search;
#[cfg(feature = "tts")]
pub mod tts;

// Re-export reqwest::Client so downstream crates don't need a direct dependency.
pub use reqwest;

// ── AI Agent definitions ──────────────────────────────────────

/// An AI coding agent that can be opened in a terminal split pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AiAgent {
    /// Human-readable display name (e.g. `"OpenCode"`).
    pub name: &'static str,
    /// Binary name on `$PATH` (e.g. "opencode").
    pub binary: &'static str,
}

/// All supported AI agents.  Order matters: first agent is the default.
pub const AGENTS: &[AiAgent] = &[
    AiAgent {
        name: "OpenCode",
        binary: "opencode",
    },
    AiAgent {
        name: "Gemini CLI",
        binary: "gemini",
    },
    AiAgent {
        name: "GitHub Copilot CLI",
        binary: "copilot",
    },
    AiAgent {
        name: "Amp",
        binary: "amp",
    },
];
