use std::collections::BTreeMap;
use std::net::IpAddr;

use serde::Serialize;
use owo_config::{AuthMode, ModelDefaults, ProviderConfig};
use owo_credentials::CredentialRef;
use url::Url;

use crate::presets::{Preset, PresetCatalog};

/// A fully resolved provider: config merged over its preset.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Provider {
    pub id: String,
    pub display_name: String,
    pub adapter: String,
    #[serde(serialize_with = "ser_url")]
    pub base_url: Url,
    pub auth: AuthMode,
    pub auth_param: Option<String>,
    pub api_key: CredentialRef,
    pub headers: BTreeMap<String, String>,
    pub query: BTreeMap<String, String>,
    /// Upstream ids exposed as models under their own id (config list, else preset list).
    pub models: Vec<String>,
    /// Metadata defaults for this provider's models (config merged over preset).
    pub model_defaults: ModelDefaults,
    pub allow_direct_models: bool,
    pub enabled: bool,
    pub allow_private_network: bool,
    pub preset: Option<String>,
}

fn ser_url<S: serde::Serializer>(url: &Url, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(url.as_str())
}

impl Provider {
    pub(crate) fn resolve(id: &str, cfg: &ProviderConfig, catalog: &PresetCatalog) -> Result<Self, Vec<String>> {
        let mut errors = Vec::new();
        let preset_id = cfg.preset.as_deref().unwrap_or(id);
        let preset: Option<&Preset> = catalog.get(preset_id);
        if cfg.preset.is_some() && preset.is_none() {
            errors.push(format!("unknown preset `{preset_id}` (see `owo providers presets`)"));
        }

        let adapter = cfg.adapter.clone().or_else(|| preset.map(|p| p.adapter.clone()));
        let base_url = cfg.base_url.clone().or_else(|| preset.map(|p| p.base_url.clone()));
        if adapter.is_none() {
            errors.push("`adapter` is required (no built-in preset with this id)".into());
        }
        if base_url.is_none() {
            errors.push("`base_url` is required (no built-in preset with this id)".into());
        }

        let auth = cfg.auth.or_else(|| preset.map(|p| p.auth)).unwrap_or(AuthMode::Bearer);
        let auth_param = cfg.auth_param.clone().or_else(|| preset.and_then(|p| p.auth_param.clone()));
        let credential = match (&cfg.api_key, auth) {
            (Some(c), _) => c.clone(),
            (None, AuthMode::None) => CredentialRef::None,
            (None, _) => match preset.and_then(|p| p.api_key.clone()) {
                Some(c) => c,
                None => {
                    errors.push("`api_key` is required unless `auth = \"none\"`".into());
                    CredentialRef::None
                }
            },
        };
        let allow_private_network =
            cfg.allow_private_network.unwrap_or_else(|| preset.is_some_and(|p| p.private_network));

        let base_url = base_url.and_then(|raw| match Url::parse(&raw) {
            Ok(url) if matches!(url.scheme(), "http" | "https") => Some(url),
            _ => {
                errors.push(format!("invalid base_url `{raw}`"));
                None
            }
        });
        if let Some(url) = &base_url {
            if !url.username().is_empty() || url.password().is_some() {
                errors.push("base_url must not embed credentials".into());
            }
            if is_private_host(url) && !allow_private_network {
                errors.push(format!(
                    "base_url `{url}` points at a private/loopback network; set `allow_private_network = true` to allow it"
                ));
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }

        let mut headers = preset.map(|p| p.headers.clone()).unwrap_or_default();
        headers.extend(cfg.headers.clone());
        let models = match &cfg.models {
            Some(list) => list.clone(),
            None => preset.map(|p| p.models.clone()).unwrap_or_default(),
        };

        Ok(Provider {
            id: id.to_string(),
            display_name: cfg
                .display_name
                .clone()
                .or_else(|| preset.map(|p| p.display_name.clone()))
                .unwrap_or_else(|| id.to_string()),
            adapter: adapter.unwrap_or_default(),
            base_url: base_url.expect("checked above"),
            auth,
            auth_param,
            api_key: credential,
            headers,
            query: cfg.query.clone(),
            models,
            model_defaults: cfg
                .model_defaults
                .clone()
                .unwrap_or_default()
                .over(&preset.and_then(|p| p.model_defaults.clone()).unwrap_or_default()),
            allow_direct_models: cfg.allow_direct_models.unwrap_or(true),
            enabled: cfg.enabled.unwrap_or(true),
            allow_private_network,
            preset: preset.map(|p| p.id.clone()),
        })
    }
}

/// Literal loopback/private/link-local hosts. DNS names other than `localhost`
/// are not resolved here; that check belongs at connect time.
fn is_private_host(url: &Url) -> bool {
    match url.host() {
        Some(url::Host::Domain(d)) => {
            let d = d.to_ascii_lowercase();
            d == "localhost" || d.ends_with(".localhost") || d.ends_with(".local") || d.ends_with(".internal")
        }
        Some(url::Host::Ipv4(ip)) => is_private_ip(IpAddr::V4(ip)),
        Some(url::Host::Ipv6(ip)) => is_private_ip(IpAddr::V6(ip)),
        None => false,
    }
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified() || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64
        }
        IpAddr::V6(v6) => {
            v6.is_loopback() || v6.is_unspecified() || (v6.segments()[0] & 0xfe00) == 0xfc00 || (v6.segments()[0] & 0xffc0) == 0xfe80
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(text: &str) -> ProviderConfig {
        toml::from_str(text).unwrap()
    }

    #[test]
    fn inherits_from_preset() {
        let p = Provider::resolve("deepseek", &cfg(""), &PresetCatalog::builtin()).unwrap();
        assert_eq!(p.adapter, "openai-chat");
        assert_eq!(p.base_url.as_str(), "https://api.deepseek.com/");
        assert_eq!(p.api_key, CredentialRef::Env("DEEPSEEK_API_KEY".into()));
        assert!(p.models.contains(&"deepseek-chat".to_string()));
    }

    #[test]
    fn config_overrides_preset() {
        let p = Provider::resolve(
            "work",
            &cfg("preset = \"openrouter\"\napi_key = \"keyring:work\"\nbase_url = \"https://proxy.example.com/v1\""),
            &PresetCatalog::builtin(),
        )
        .unwrap();
        assert_eq!(p.adapter, "openai-chat");
        assert_eq!(p.api_key, CredentialRef::Keyring("work".into()));
        assert_eq!(p.base_url.host_str(), Some("proxy.example.com"));
    }

    #[test]
    fn custom_provider_requires_fields() {
        let errs = Provider::resolve("custom", &cfg(""), &PresetCatalog::builtin()).unwrap_err();
        assert!(errs.iter().any(|e| e.contains("adapter")));
        assert!(errs.iter().any(|e| e.contains("base_url")));
    }

    #[test]
    fn private_network_requires_opt_in() {
        let catalog = PresetCatalog::builtin();
        let errs = Provider::resolve(
            "lan",
            &cfg("adapter = \"openai-chat\"\nbase_url = \"http://192.168.1.5:8000/v1\"\nauth = \"none\""),
            &catalog,
        )
        .unwrap_err();
        assert!(errs[0].contains("private"));
        assert!(Provider::resolve("ollama", &cfg(""), &catalog).unwrap().allow_private_network);
        assert!(Provider::resolve(
            "lan",
            &cfg("adapter = \"openai-chat\"\nbase_url = \"http://192.168.1.5:8000/v1\"\nauth = \"none\"\nallow_private_network = true"),
            &catalog,
        )
        .is_ok());
    }
}
