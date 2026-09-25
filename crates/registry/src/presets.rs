use std::collections::BTreeMap;

use serde::Deserialize;
use owo_config::{AuthMode, ModelDefaults};
use owo_credentials::CredentialRef;

const BUILTIN_PRESETS: &str = include_str!("../../../registry/providers.toml");

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    #[serde(skip)]
    pub id: String,
    pub display_name: String,
    pub adapter: String,
    pub base_url: String,
    #[serde(default = "default_auth")]
    pub auth: AuthMode,
    #[serde(default)]
    pub auth_param: Option<String>,
    #[serde(default)]
    pub api_key: Option<CredentialRef>,
    #[serde(default)]
    pub private_network: bool,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub opencodex_id: Option<String>,
    #[serde(default)]
    pub model_defaults: Option<ModelDefaults>,
}

fn default_auth() -> AuthMode {
    AuthMode::Bearer
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PresetFile {
    presets: BTreeMap<String, Preset>,
}

/// The preset catalog compiled into the binary.
#[derive(Debug, Clone)]
pub struct PresetCatalog {
    presets: BTreeMap<String, Preset>,
}

impl PresetCatalog {
    pub fn builtin() -> Self {
        Self::parse(BUILTIN_PRESETS).expect("built-in registry/providers.toml must be valid")
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let file: PresetFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let presets = file
            .presets
            .into_iter()
            .map(|(id, mut p)| {
                p.id = id.clone();
                (id, p)
            })
            .collect();
        Ok(Self { presets })
    }

    pub fn get(&self, id: &str) -> Option<&Preset> {
        self.presets.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &Preset> {
        self.presets.values()
    }

    pub fn len(&self) -> usize {
        self.presets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.presets.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_catalog_parses_and_is_consistent() {
        let catalog = PresetCatalog::builtin();
        assert!(catalog.len() >= 20);
        for p in catalog.iter() {
            assert!(p.base_url.starts_with("http"), "{}", p.id);
            // Display names are tile labels (one line, ~12 chars) and provider card titles: brand only, no qualifiers.
            assert!(p.display_name.len() <= 12 && !p.display_name.contains('('), "{} display_name too long", p.id);
            if matches!(p.auth, AuthMode::Header | AuthMode::Query) {
                assert!(p.auth_param.is_some(), "{} needs auth_param", p.id);
            }
            if p.auth != AuthMode::None {
                assert!(p.api_key.is_some(), "{} should suggest a credential", p.id);
            }
        }
        assert_eq!(catalog.get("ollama").unwrap().auth, AuthMode::None);
    }

    /// The `owo` build implements `openai-chat` and `anthropic`; the first-party
    /// vendors must stay on those, or `owo add openai` yields a disabled provider
    /// and the desktop wizard hides the tile.
    #[test]
    fn first_party_presets_use_implemented_adapters() {
        let catalog = PresetCatalog::builtin();
        let openai = catalog.get("openai").unwrap();
        assert_eq!(openai.adapter, "openai-chat");
        assert_eq!(openai.base_url, "https://api.openai.com/v1");
        assert_eq!(openai.display_name, "OpenAI");
        assert_eq!(catalog.get("anthropic").unwrap().adapter, "anthropic");
        assert_eq!(catalog.get("deepseek").unwrap().adapter, "openai-chat");
    }
}
