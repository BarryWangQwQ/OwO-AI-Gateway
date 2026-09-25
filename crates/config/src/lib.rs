//! `config.toml` schema, on-disk layout, and structural validation.
//!
//! Registry-level checks (preset existence, adapter availability) live in
//! `owo-registry`, which owns the built-in provider presets.

mod mcp;
mod paths;
mod schema;
mod validate;

pub use mcp::{is_valid_mcp_name, looks_secret, mcp_keyring_name, mcp_value_ref, McpServerConfig, McpTransport};
pub use paths::{LayoutMode, OwoPaths};
pub use schema::{
    app_client_id, APP_NAMES, DEFAULT_NAME, AuthMode, CapabilityOverrides, ClientConfig, Config, ModelConfig, ModelDefaults,
    ModelDiscoveryConfig, NewModelPolicy, Price, ProviderConfig, ServerConfig, SAMPLE_CONFIG,
};

#[cfg(test)]
mod client_id_tests {
    use super::*;

    #[test]
    fn app_names_map_to_integration_ids() {
        let text = "[clients.claude]\nmodel = \"m\"\n[clients.grok]\n[[models]]\nid = \"m\"\nprovider = \"p\"\naliases = { claude = \"x\", codex = \"y\", \"*\" = \"z\" }\n[providers.p]\nadapter = \"openai-chat\"\nbase_url = \"https://e.example/v1\"\n";
        let (c, _) = Config::from_toml_str(text, "t").unwrap();
        assert_eq!(c.clients["claude_code"].model.as_deref(), Some("m"));
        assert!(c.clients.contains_key("grok_build"));
        assert_eq!(c.models[0].aliases.get("claude_code").map(String::as_str), Some("x"));
        assert!(c.models[0].aliases.contains_key("*"));
    }

    #[test]
    fn only_app_names_are_accepted() {
        for bad in ["[clients.claude_code]\n", "[clients.grok-build]\n", "[clients.nope]\n", "[clients.codex]\nenabled = true\n"] {
            assert!(Config::from_toml_str(bad, "t").is_err(), "{bad}");
        }
    }

    #[test]
    fn codex_and_codex_desktop_are_separate_apps() {
        let text = "[clients.codex]\nmodel = \"m\"\n[clients.codex-desktop]\nmodel = \"m\"\n[[models]]\nid = \"m\"\nprovider = \"p\"\naliases = { codex = \"x\", codex-desktop = \"y\" }\n[providers.p]\nadapter = \"openai-chat\"\nbase_url = \"https://e.example/v1\"\n";
        let (c, _) = Config::from_toml_str(text, "t").unwrap();
        assert!(c.clients.contains_key("codex") && c.clients.contains_key("codex_desktop"));
        assert_eq!(c.models[0].aliases.get("codex_desktop").map(String::as_str), Some("y"));
    }
}
pub use validate::{Diagnostic, Severity};

use std::path::Path;

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config file not found at {0} (create one with `owo init`)")]
    NotFound(String),
    #[error("failed to read {path}: {source}")]
    Io { path: String, source: std::io::Error },
    #[error("failed to parse {path}: {message}")]
    Parse { path: String, message: String },
    #[error("config is invalid:\n{0}")]
    Invalid(String),
}

impl Config {
    /// Parses and validates a config file. Warnings are returned; errors fail the load.
    pub fn load(path: &Path) -> Result<(Config, Vec<Diagnostic>), ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NotFound(path.display().to_string())
            } else {
                ConfigError::Io { path: path.display().to_string(), source }
            }
        })?;
        Self::from_toml_str(&text, &path.display().to_string())
    }

    pub fn from_toml_str(text: &str, origin: &str) -> Result<(Config, Vec<Diagnostic>), ConfigError> {
        let mut config: Config = toml::from_str(text)
            .map_err(|e| ConfigError::Parse { path: origin.to_string(), message: e.to_string() })?;
        let clashes = config.normalize_client_ids();
        if !clashes.is_empty() {
            return Err(ConfigError::Invalid(clashes.iter().map(|c| format!("  - {c}")).collect::<Vec<_>>().join("\n")));
        }
        let diagnostics = config.validate();
        let errors: Vec<_> = diagnostics.iter().filter(|d| d.severity == Severity::Error).collect();
        if !errors.is_empty() {
            let text = errors.iter().map(|d| format!("  - {d}")).collect::<Vec<_>>().join("\n");
            return Err(ConfigError::Invalid(text));
        }
        Ok((config, diagnostics))
    }
}
