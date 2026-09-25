use std::collections::BTreeMap;

use serde::Serialize;
use owo_config::{CapabilityOverrides, ModelConfig, ModelDefaults, Price};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelOrigin {
    /// Declared in `[[models]]`.
    Config,
    /// Listed in `providers.<id>.models` (or the preset's list).
    Provider,
    /// Synthesized from a `provider/upstream-model` request.
    Direct,
}

/// A canonical model. Client-facing aliases never change this identity.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Model {
    pub id: String,
    pub display_name: String,
    pub provider: String,
    pub upstream_model: String,
    pub aliases: BTreeMap<String, String>,
    pub context_window: Option<u32>,
    pub max_output_tokens: Option<u32>,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
    /// Tri-state: `Some(false)` means the capability is known to be absent and requests
    /// needing it are rejected; `None` means unknown and is not advertised.
    pub capabilities: CapabilityOverrides,
    /// Token prices for cost estimates, when configured.
    pub price: Option<Price>,
    pub enabled: bool,
    pub origin: ModelOrigin,
}

impl Model {
    /// A `[[models]]` entry; fields it leaves unset come from the provider's model defaults.
    pub(crate) fn from_config(cfg: &ModelConfig, defaults: &ModelDefaults) -> Self {
        let own = ModelDefaults {
            context_window: cfg.context_window,
            max_output_tokens: cfg.max_output_tokens,
            reasoning_efforts: (!cfg.reasoning_efforts.is_empty()).then(|| cfg.reasoning_efforts.clone()),
            default_reasoning_effort: cfg.default_reasoning_effort.clone(),
            capabilities: cfg.capabilities.clone(),
            price: cfg.price,
        };
        Self {
            id: cfg.id.clone(),
            display_name: cfg.display_name.clone().unwrap_or_else(|| cfg.id.clone()),
            provider: cfg.provider.clone(),
            upstream_model: cfg.upstream().to_string(),
            aliases: cfg.aliases.clone(),
            enabled: cfg.enabled,
            origin: ModelOrigin::Config,
            ..Self::with_defaults(&own.over(defaults))
        }
    }

    pub(crate) fn listed(provider: &str, upstream_model: &str, defaults: &ModelDefaults) -> Self {
        Self {
            id: upstream_model.to_string(),
            display_name: upstream_model.to_string(),
            origin: ModelOrigin::Provider,
            ..Self::direct(provider, upstream_model, defaults)
        }
    }

    pub(crate) fn direct(provider: &str, upstream_model: &str, defaults: &ModelDefaults) -> Self {
        let id = format!("{provider}/{upstream_model}");
        Self {
            display_name: id.clone(),
            id,
            provider: provider.to_string(),
            upstream_model: upstream_model.to_string(),
            ..Self::with_defaults(defaults)
        }
    }

    fn with_defaults(d: &ModelDefaults) -> Self {
        Self {
            id: String::new(),
            display_name: String::new(),
            provider: String::new(),
            upstream_model: String::new(),
            aliases: BTreeMap::new(),
            context_window: d.context_window,
            max_output_tokens: d.max_output_tokens,
            reasoning_efforts: d.reasoning_efforts.clone().unwrap_or_default(),
            default_reasoning_effort: d.default_reasoning_effort.clone(),
            capabilities: d.capabilities.clone(),
            price: d.price,
            enabled: true,
            origin: ModelOrigin::Direct,
        }
    }

    /// The id a given client should see: its alias when one exists, else the canonical id.
    pub fn exposed_id(&self, client: Option<&str>) -> &str {
        client
            .and_then(|c| self.aliases.get(c))
            .or_else(|| self.aliases.get("*"))
            .map(String::as_str)
            .unwrap_or(&self.id)
    }
}
