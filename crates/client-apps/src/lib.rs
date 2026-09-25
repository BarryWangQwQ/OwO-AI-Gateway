//! File-based client integrations (spec §22–26).
//!
//! Each client module turns the shared OwO AI Gateway model registry into that client's own
//! provider configuration and hands the result to [`managed`], which owns the
//! reversible file edits. No client module duplicates providers, models, or credentials:
//! every client reaches OwO AI Gateway under `/c/<client-id>` and names OwO AI Gateway models by id.

pub mod copilot_app;
pub mod grok_build;
pub mod managed;
pub mod minimax;
pub mod opencode;
pub mod zcode;

use std::path::PathBuf;

/// Provider id every integration uses inside the client's config.
pub const PROVIDER_ID: &str = "owo";
/// Sent when the gateway needs no token; clients that insist on a key get this.
pub const PLACEHOLDER_KEY: &str = "owo-local";

/// An OwO AI Gateway model as the client integrations see it.
#[derive(Debug, Clone)]
pub struct AppModel {
    /// Id the client sends back (the model's exposed id for that client).
    pub id: String,
    pub display_name: String,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
    pub vision: bool,
}

/// Where a client reaches OwO AI Gateway.
#[derive(Debug, Clone)]
pub struct Target {
    /// The provider name the client shows.
    pub name: String,
    /// Gateway root for this client, e.g. `http://127.0.0.1:8787/c/opencode` (no `/v1`).
    pub root: String,
    /// The gateway token, when the gateway requires one.
    pub token: Option<String>,
}

impl Target {
    pub fn v1(&self) -> String {
        format!("{}/v1", self.root)
    }

    pub fn key(&self) -> &str {
        self.token.as_deref().unwrap_or(PLACEHOLDER_KEY)
    }
}

pub(crate) fn home() -> Option<PathBuf> {
    dirs::home_dir()
}

pub(crate) fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Efforts in canonical order, restricted to `allowed`.
pub(crate) fn efforts(model: &AppModel, allowed: &[&str]) -> Vec<String> {
    allowed.iter().filter(|e| model.reasoning_efforts.iter().any(|m| m == *e)).map(|e| e.to_string()).collect()
}

pub(crate) fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}
