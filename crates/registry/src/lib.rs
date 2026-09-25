//! The single shared provider/model plane (spec §7, §10, §12).
//!
//! Every client adapter resolves models through [`Registry::resolve`]; no client
//! keeps its own provider, model, or credential store.

mod model;
mod presets;
mod provider;

use std::collections::BTreeMap;
use std::sync::Arc;

use owo_config::{Config, Diagnostic};
use owo_core::{ErrorKind, ModelError};

pub use model::{Model, ModelOrigin};
pub use presets::{Preset, PresetCatalog};
pub use provider::Provider;

/// Adapter kinds OwO AI Gateway knows about, with whether this build implements them.
/// Unknown kinds are rejected; known-but-unimplemented kinds are parity gaps.
pub const KNOWN_ADAPTERS: &[&str] = &[
    "openai-chat",
    "openai-responses",
    "anthropic",
    "google",
    "ollama-native",
    "cursor",
];

/// A resolved routing target.
#[derive(Debug, Clone)]
pub struct Target {
    pub model: Arc<Model>,
    pub provider: Arc<Provider>,
}

#[derive(Debug, Clone)]
pub struct Registry {
    providers: BTreeMap<String, Arc<Provider>>,
    models: Vec<Arc<Model>>,
    catalog: PresetCatalog,
}

impl Registry {
    /// Builds the registry from a structurally valid config. `implemented_adapters`
    /// is the set of adapter kinds the running binary can execute.
    pub fn build(
        config: &Config,
        catalog: PresetCatalog,
        implemented_adapters: &[&str],
    ) -> Result<(Self, Vec<Diagnostic>), Vec<Diagnostic>> {
        let mut errors = Vec::new();
        let mut warnings = Vec::new();
        let mut providers = BTreeMap::new();

        for (id, cfg) in &config.providers {
            match Provider::resolve(id, cfg, &catalog) {
                Ok(p) => {
                    let path = format!("providers.{id}.adapter");
                    if !KNOWN_ADAPTERS.contains(&p.adapter.as_str()) {
                        errors.push(Diagnostic::error(path, format!("unknown adapter `{}`", p.adapter)));
                    } else if !implemented_adapters.contains(&p.adapter.as_str()) {
                        let msg = format!(
                            "adapter `{}` is not implemented in this build yet; provider is disabled",
                            p.adapter
                        );
                        if p.enabled {
                            warnings.push(Diagnostic::warning(path, msg));
                        }
                        let mut p = p;
                        p.enabled = false;
                        providers.insert(id.clone(), Arc::new(p));
                        continue;
                    }
                    providers.insert(id.clone(), Arc::new(p));
                }
                Err(msgs) => {
                    for m in msgs {
                        errors.push(Diagnostic::error(format!("providers.{id}"), m));
                    }
                }
            }
        }

        let no_defaults = owo_config::ModelDefaults::default();
        let mut models: Vec<Arc<Model>> = config
            .models
            .iter()
            .map(|m| {
                let defaults = providers.get(&m.provider).map(|p| &p.model_defaults).unwrap_or(&no_defaults);
                Arc::new(Model::from_config(m, defaults))
            })
            .collect();
        for m in &models {
            if let Some(p) = providers.get(&m.provider) {
                if !p.enabled && m.enabled {
                    warnings.push(Diagnostic::warning(
                        format!("models.{}", m.id),
                        format!("provider `{}` is disabled; model is unavailable", m.provider),
                    ));
                }
            }
        }
        // Provider-listed models, unless a [[models]] entry already claims the id
        // (config validation rejects explicit conflicts; preset lists yield silently).
        for p in providers.values() {
            for upstream in &p.models {
                if !models.iter().any(|m| m.id == *upstream) {
                    models.push(Arc::new(Model::listed(&p.id, upstream, &p.model_defaults)));
                }
            }
        }

        if !errors.is_empty() {
            return Err(errors);
        }
        Ok((Self { providers, models, catalog }, warnings))
    }

    pub fn providers(&self) -> impl Iterator<Item = &Arc<Provider>> {
        self.providers.values()
    }

    pub fn provider(&self, id: &str) -> Option<&Arc<Provider>> {
        self.providers.get(id)
    }

    pub fn models(&self) -> impl Iterator<Item = &Arc<Model>> {
        self.models.iter()
    }

    pub fn presets(&self) -> &PresetCatalog {
        &self.catalog
    }

    /// Models routable right now (model and provider enabled).
    pub fn available_models(&self) -> impl Iterator<Item = &Arc<Model>> {
        self.models
            .iter()
            .filter(|m| m.enabled && self.providers.get(&m.provider).is_some_and(|p| p.enabled))
    }

    /// Resolves a client-supplied model id: canonical id, then the client's alias,
    /// then a global (`*`) alias, then `provider/upstream-model`.
    pub fn resolve(&self, requested: &str, client: Option<&str>) -> Result<Target, ModelError> {
        let found = self
            .models
            .iter()
            .find(|m| m.id == requested)
            .or_else(|| {
                client.and_then(|c| self.models.iter().find(|m| m.aliases.get(c).is_some_and(|a| a == requested)))
            })
            .or_else(|| self.models.iter().find(|m| m.aliases.get("*").is_some_and(|a| a == requested)));

        if let Some(model) = found {
            if !model.enabled {
                return Err(ModelError::new(ErrorKind::ModelNotFound, format!("model `{requested}` is disabled")));
            }
            let provider = self.enabled_provider(&model.provider)?;
            return Ok(Target { model: model.clone(), provider });
        }

        if let Some((provider_id, upstream)) = requested.split_once('/') {
            if let Some(provider) = self.providers.get(provider_id) {
                if !upstream.is_empty()
                    && (provider.allow_direct_models || provider.models.iter().any(|m| m == upstream))
                {
                    let provider = self.enabled_provider(provider_id)?;
                    let model = Model::direct(provider_id, upstream, &provider.model_defaults);
                    return Ok(Target { model: Arc::new(model), provider });
                }
            }
        }

        Err(ModelError::new(ErrorKind::ModelNotFound, format!("model `{requested}` is not configured")))
    }

    fn enabled_provider(&self, id: &str) -> Result<Arc<Provider>, ModelError> {
        match self.providers.get(id) {
            Some(p) if p.enabled => Ok(p.clone()),
            Some(_) => Err(ModelError::new(ErrorKind::ProviderUnavailable, format!("provider `{id}` is disabled"))),
            None => Err(ModelError::new(ErrorKind::ConfigurationError, format!("provider `{id}` is not configured"))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(text: &str) -> Registry {
        let (config, _) = Config::from_toml_str(text, "test").unwrap();
        Registry::build(&config, PresetCatalog::builtin(), &["openai-chat", "anthropic"]).unwrap().0
    }

    const CONFIG: &str = r#"
version = 1
[providers.deepseek]
[providers.anthropic]
[[models]]
id = "ds"
display_name = "DeepSeek"
provider = "deepseek"
upstream_model = "deepseek-chat"
aliases = { codex = "gpt-ds", "*" = "deepseek-any" }
[[models]]
id = "claude-sonnet"
provider = "anthropic"
upstream_model = "claude-sonnet-x"
"#;

    #[test]
    fn resolves_canonical_and_aliases() {
        let r = registry(CONFIG);
        assert_eq!(r.resolve("ds", None).unwrap().model.upstream_model, "deepseek-chat");
        assert_eq!(r.resolve("gpt-ds", Some("codex")).unwrap().model.id, "ds");
        assert!(r.resolve("gpt-ds", Some("claude_code")).is_err());
        assert_eq!(r.resolve("deepseek-any", Some("cursor")).unwrap().model.id, "ds");
    }

    #[test]
    fn exposed_id_prefers_client_alias() {
        let r = registry(CONFIG);
        let m = r.resolve("ds", None).unwrap().model;
        assert_eq!(m.exposed_id(Some("codex")), "gpt-ds");
        assert_eq!(m.exposed_id(Some("cursor")), "deepseek-any");
    }

    #[test]
    fn direct_provider_routing() {
        let r = registry(CONFIG);
        let t = r.resolve("deepseek/deepseek-reasoner", None).unwrap();
        assert_eq!(t.model.origin, ModelOrigin::Direct);
        assert_eq!(t.model.upstream_model, "deepseek-reasoner");
        assert_eq!(r.resolve("nope/x", None).unwrap_err().kind, ErrorKind::ModelNotFound);
    }

    #[test]
    fn unimplemented_adapter_disables_provider() {
        let (config, _) = Config::from_toml_str(CONFIG, "test").unwrap();
        let (r, warnings) = Registry::build(&config, PresetCatalog::builtin(), &["openai-chat"]).unwrap();
        assert!(warnings.iter().any(|w| w.message.contains("`anthropic` is not implemented")));
        assert_eq!(r.resolve("claude-sonnet", None).unwrap_err().kind, ErrorKind::ProviderUnavailable);
        // `ds` plus the two models of the deepseek preset.
        let ids: Vec<_> = r.available_models().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["ds", "deepseek-chat", "deepseek-reasoner"]);
    }

    #[test]
    fn model_defaults_flow_from_preset_and_provider() {
        let r = registry(
            r#"
[providers.anthropic]
models = ["claude-sonnet-5"]
model_defaults = { context_window = 500000, price = { input = 3, output = 15 } }
[[models]]
id = "fast"
provider = "anthropic"
upstream_model = "claude-haiku-4-5"
reasoning_efforts = ["low"]
price = { input = 1, output = 5 }
"#,
        );
        let sonnet = r.resolve("claude-sonnet-5", None).unwrap().model;
        assert_eq!(sonnet.context_window, Some(500_000), "provider override");
        assert_eq!(sonnet.price.map(|p| p.output), Some(15.0), "provider price");
        assert_eq!(r.resolve("fast", None).unwrap().model.price.map(|p| p.output), Some(5.0), "model price wins");
        assert_eq!(sonnet.capabilities.vision, Some(true), "preset default");
        assert!(sonnet.reasoning_efforts.contains(&"max".to_string()));
        let fast = r.resolve("fast", None).unwrap().model;
        assert_eq!(fast.reasoning_efforts, vec!["low"], "[[models]] field wins");
        assert_eq!(fast.max_output_tokens, Some(64_000), "unset field inherits");
    }

    #[test]
    fn provider_listed_models() {
        let r = registry(
            r#"
[providers.deepseek]
models = ["deepseek-chat"]
[providers.openrouter]
models = ["anthropic/claude-x"]
[[models]]
id = "deepseek-chat"
provider = "deepseek"
display_name = "DeepSeek Chat"
"#,
        );
        // An explicit list replaces the preset list; [[models]] refines a listed id in place.
        let ids: Vec<_> = r.models().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, vec!["deepseek-chat", "anthropic/claude-x"]);
        assert_eq!(r.resolve("deepseek-chat", None).unwrap().model.display_name, "DeepSeek Chat");
        // A canonical id containing `/` wins over provider/model routing.
        let t = r.resolve("anthropic/claude-x", None).unwrap();
        assert_eq!((t.provider.id.as_str(), t.model.origin), ("openrouter", ModelOrigin::Provider));
        assert_eq!(t.model.upstream_model, "anthropic/claude-x");
    }

    #[test]
    fn unknown_adapter_is_an_error() {
        let (config, _) = Config::from_toml_str(
            "version = 1\n[providers.x]\nadapter = \"magic\"\nbase_url = \"https://x.example\"\nauth = \"none\"\n",
            "test",
        )
        .unwrap();
        let errs = Registry::build(&config, PresetCatalog::builtin(), &["openai-chat"]).unwrap_err();
        assert!(errs[0].message.contains("unknown adapter `magic`"));
    }
}
