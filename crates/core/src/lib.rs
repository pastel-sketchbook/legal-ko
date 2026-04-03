pub mod bookmarks;
pub mod cache;
pub mod client;
pub mod config;
pub mod context;
pub mod models;
pub mod parser;
pub mod preferences;
pub mod search;
#[cfg(feature = "tts")]
pub mod tts;

// Re-export reqwest::Client so downstream crates don't need a direct dependency.
pub use reqwest;
