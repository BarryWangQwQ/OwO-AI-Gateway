//! The `owo` Codex profile layer (`<codex_home>/owo.config.toml`) and the provider table.

use std::path::Path;

use toml_edit::{value, DocumentMut, Item, Table};

pub const HEADER: &str = "\
# Managed by OwO AI Gateway (`owo connect codex`). Changes here are overwritten.
# Start Codex through OwO AI Gateway with:  codex -p owo
# Undo with:                     owo disconnect codex
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderSettings {
    /// The provider name Codex shows.
    pub name: String,
    pub base_url: String,
    /// Environment variable holding the OwO AI Gateway access token, when the gateway requires one.
    pub env_key: Option<String>,
}

pub fn provider_table(settings: &ProviderSettings) -> Table {
    let mut t = Table::new();
    t.insert("name", value(&settings.name));
    t.insert("base_url", value(&settings.base_url));
    t.insert("wire_api", value("responses"));
    match &settings.env_key {
        Some(key) => {
            t.insert("env_key", value(key));
        }
        None => {
            t.insert("requires_openai_auth", value(false));
        }
    }
    t.insert("supports_websockets", value(false));
    t
}

/// Stable fingerprint of a provider table's contents, independent of surrounding layout.
pub fn table_fingerprint(table: &Table) -> String {
    table.iter().map(|(k, v)| format!("{k}={}", v.to_string().trim())).collect::<Vec<_>>().join("\n")
}

pub fn render(model: &str, catalog: &Path, settings: &ProviderSettings) -> String {
    let mut doc = DocumentMut::new();
    doc["model_provider"] = value(crate::PROVIDER_ID);
    doc["model"] = value(model);
    doc["model_catalog_json"] = value(catalog.display().to_string());
    let mut providers = Table::new();
    providers.set_implicit(true);
    providers.insert(crate::PROVIDER_ID, Item::Table(provider_table(settings)));
    doc.insert("model_providers", Item::Table(providers));
    format!("{HEADER}\n{doc}")
}

/// The profile as `codex -c key=value` overrides, for a launch that writes no Codex file.
pub fn overrides(model: &str, catalog: &Path, settings: &ProviderSettings) -> Vec<String> {
    let q = |s: &str| value(s).to_string().trim().to_string();
    let provider: Vec<String> = provider_table(settings).iter().map(|(k, v)| format!("{k} = {}", v.to_string().trim())).collect();
    vec![
        format!("model_provider={}", q(crate::PROVIDER_ID)),
        format!("model={}", q(model)),
        format!("model_catalog_json={}", q(&catalog.display().to_string())),
        format!("model_providers.{}={{ {} }}", crate::PROVIDER_ID, provider.join(", ")),
    ]
}

#[cfg(test)]
mod tests {
    #[test]
    fn overrides_parse_as_toml() {
        let o = super::overrides(
            "claude-sonnet-5",
            std::path::Path::new(r"C:\x\models.json"),
            &super::ProviderSettings { name: "OwO".into(), base_url: "http://127.0.0.1:8787/c/codex/v1".into(), env_key: None },
        );
        for kv in &o {
            let (k, v) = kv.split_once('=').unwrap();
            let doc: toml_edit::DocumentMut = format!("{k} = {v}").parse().unwrap_or_else(|e| panic!("{kv}: {e}"));
            assert!(doc.len() == 1 || k.contains('.'), "{kv}");
        }
        assert!(o[3].contains("requires_openai_auth = false"), "{}", o[3]);
    }

    use super::*;

    #[test]
    fn renders_profile_layer() {
        let text = render(
            "deepseek-chat",
            Path::new(r"C:\Users\me\.owo\state\codex\models.json"),
            &ProviderSettings { name: "My AI".into(), base_url: "http://127.0.0.1:8787/c/codex/v1".into(), env_key: None },
        );
        let doc: DocumentMut = text.parse().unwrap();
        assert_eq!(doc["model_providers"]["owo"]["name"].as_str(), Some("My AI"));
        assert_eq!(doc["model_provider"].as_str(), Some("owo"));
        assert_eq!(doc["model"].as_str(), Some("deepseek-chat"));
        assert_eq!(doc["model_catalog_json"].as_str(), Some(r"C:\Users\me\.owo\state\codex\models.json"));
        let p = &doc["model_providers"]["owo"];
        assert_eq!(p["wire_api"].as_str(), Some("responses"));
        assert_eq!(p["requires_openai_auth"].as_bool(), Some(false));
        assert_eq!(p["supports_websockets"].as_bool(), Some(false));
        assert!(text.starts_with("# Managed by OwO AI Gateway"));
        assert!(!text.contains("[model_providers]\n"), "parent table stays implicit:\n{text}");
    }

    #[test]
    fn token_uses_env_key() {
        let t = provider_table(&ProviderSettings { name: "OwO".into(), base_url: "http://x".into(), env_key: Some("OWO_API_KEY".into()) });
        assert_eq!(t["env_key"].as_str(), Some("OWO_API_KEY"));
        assert!(t.get("requires_openai_auth").is_none());
    }
}
