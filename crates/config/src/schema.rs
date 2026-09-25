use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use owo_credentials::{CredentialBackend, CredentialRef};

pub const CURRENT_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    #[serde(default = "default_version")]
    pub version: u32,
    /// The provider / account name connected apps show for OwO AI Gateway
    /// (`[clients.<app>] name` overrides it per app).
    #[serde(default = "default_name")]
    pub name: String,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub credentials: CredentialsConfig,
    #[serde(default)]
    pub model_discovery: ModelDiscoveryConfig,
    #[serde(default)]
    pub providers: BTreeMap<String, ProviderConfig>,
    #[serde(default)]
    pub models: Vec<ModelConfig>,
    #[serde(default)]
    pub clients: BTreeMap<String, ClientConfig>,
    /// MCP servers OwO AI Gateway writes into apps (`owo mcp`).
    #[serde(default)]
    pub mcp: BTreeMap<String, crate::mcp::McpServerConfig>,
}

/// App names as `owo connect` spells them, with the integration id each maps to.
pub const APP_NAMES: [(&str, &str); 11] = [
    ("codex", "codex"),
    ("codex-desktop", "codex_desktop"),
    ("claude", "claude_code"),
    ("claude-desktop", "claude_desktop"),
    ("cursor", "cursor"),
    ("grok", "grok_build"),
    ("opencode", "opencode"),
    ("mcode", "minimax_code"),
    ("mmx", "minimax_cli"),
    ("zcode", "zcode"),
    ("copilot", "copilot_app"),
];

/// The integration id for an app name (`claude` → `claude_code`); `None` for anything else.
pub fn app_client_id(name: &str) -> Option<&'static str> {
    APP_NAMES.iter().find(|(n, _)| *n == name).map(|(_, id)| *id)
}

impl Config {
    /// The name the app with this integration id shows for OwO AI Gateway.
    pub fn client_name(&self, client_id: &str) -> &str {
        self.clients.get(client_id).and_then(|c| c.name.as_deref()).unwrap_or(&self.name)
    }

    /// Rewrites the app names in `[clients.*]` and `models[].aliases` to integration ids.
    /// Returns an error message for every name that is not an app.
    pub fn normalize_client_ids(&mut self) -> Vec<String> {
        let apps = APP_NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
        let mut errors = Vec::new();
        let mut clients = BTreeMap::new();
        for (name, cfg) in std::mem::take(&mut self.clients) {
            match app_client_id(&name) {
                None => errors.push(format!("`[clients.{name}]`: unknown app (apps: {apps})")),
                Some(id) => {
                    clients.insert(id.to_string(), cfg);
                }
            }
        }
        self.clients = clients;
        for m in &mut self.models {
            let mut aliases = BTreeMap::new();
            for (name, alias) in std::mem::take(&mut m.aliases) {
                match if name == "*" { Some("*") } else { app_client_id(&name) } {
                    None => errors.push(format!("model `{}`: alias for unknown app `{name}` (apps: {apps}, or \"*\")", m.id)),
                    Some(id) => {
                        aliases.insert(id.to_string(), alias);
                    }
                }
            }
            m.aliases = aliases;
        }
        errors
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ServerConfig {
    pub listen: String,
    /// Token clients must present (`Authorization: Bearer` or `x-api-key`).
    /// Required when `listen` is not a loopback address.
    pub auth_token: Option<CredentialRef>,
    pub max_body_bytes: usize,
    pub request_timeout_secs: u64,
    pub max_concurrent_requests: usize,
    pub control_api: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen: "127.0.0.1:8787".into(),
            auth_token: None,
            max_body_bytes: 32 * 1024 * 1024,
            request_timeout_secs: 900,
            max_concurrent_requests: 256,
            control_api: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CredentialsConfig {
    pub backend: CredentialBackend,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModelDiscoveryConfig {
    pub new_model_policy: NewModelPolicy,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NewModelPolicy {
    #[default]
    Off,
    On,
    Notify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMode {
    /// `Authorization: Bearer <secret>`.
    Bearer,
    /// Secret in the header named by `auth_param`.
    Header,
    /// Secret in the query parameter named by `auth_param`.
    Query,
    /// No credential is sent.
    None,
}

/// A provider definition. Fields left unset inherit from the built-in preset with
/// the same id (or the one named by `preset`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub adapter: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<CredentialRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth: Option<AuthMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_param: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub query: BTreeMap<String, String>,
    /// Upstream model ids to expose as models, each under its own id. When omitted, the
    /// preset's model list is used. Use `[[models]]` to rename or add details.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub models: Option<Vec<String>>,
    /// Metadata every model of this provider starts from (merged over the preset's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_defaults: Option<ModelDefaults>,
    /// Allow `provider/upstream-model` requests for ids not declared in `[[models]]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_direct_models: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    /// Allow a base URL on a private/loopback network (local runtimes such as Ollama).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_private_network: Option<bool>,
}

/// Per-model capability overrides. `None` means "use the provider/adapter default".
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct CapabilityOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel_tools: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vision: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_output: Option<bool>,
}

/// Token prices in US dollars per million tokens, used to estimate what calls cost.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Price {
    /// Uncached input.
    pub input: f64,
    /// Output, reasoning included.
    pub output: f64,
    /// Input served from the prompt cache; defaults to `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,
    /// Input written to the prompt cache; defaults to `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl Price {
    /// Dollars for one call. `input` counts every input token, cached ones included.
    pub fn cost(&self, input: u64, cache_read: u64, cache_write: u64, output: u64) -> f64 {
        let uncached = input.saturating_sub(cache_read).saturating_sub(cache_write);
        (uncached as f64 * self.input
            + cache_read as f64 * self.cache_read.unwrap_or(self.input)
            + cache_write as f64 * self.cache_write.unwrap_or(self.input)
            + output as f64 * self.output)
            / 1e6
    }

    pub(crate) fn fields(&self) -> [(&'static str, Option<f64>); 4] {
        [("input", Some(self.input)), ("output", Some(self.output)), ("cache_read", self.cache_read), ("cache_write", self.cache_write)]
    }
}

/// Per-provider model metadata defaults. A model's own `[[models]]` fields win.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ModelDefaults {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_efforts: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<String>,
    pub capabilities: CapabilityOverrides,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
}

impl ModelDefaults {
    /// Field-by-field merge; `self` wins where set.
    pub fn over(&self, base: &ModelDefaults) -> ModelDefaults {
        let c = &self.capabilities;
        let b = &base.capabilities;
        ModelDefaults {
            context_window: self.context_window.or(base.context_window),
            max_output_tokens: self.max_output_tokens.or(base.max_output_tokens),
            reasoning_efforts: self.reasoning_efforts.clone().or_else(|| base.reasoning_efforts.clone()),
            default_reasoning_effort: self.default_reasoning_effort.clone().or_else(|| base.default_reasoning_effort.clone()),
            capabilities: CapabilityOverrides {
                streaming: c.streaming.or(b.streaming),
                tools: c.tools.or(b.tools),
                parallel_tools: c.parallel_tools.or(b.parallel_tools),
                vision: c.vision.or(b.vision),
                reasoning: c.reasoning.or(b.reasoning),
                structured_output: c.structured_output.or(b.structured_output),
            },
            price: self.price.or(base.price),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub provider: String,
    /// The provider's id for this model; defaults to `id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_model: Option<String>,
    /// Client-facing aliases keyed by app (`codex`, `claude`, `cursor`, ... — the names
    /// `owo connect` uses), or `"*"` for an alias every app may use.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub aliases: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasoning_efforts: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_reasoning_effort: Option<String>,
    #[serde(default)]
    pub capabilities: CapabilityOverrides,
    /// Token prices for cost estimates (USD per million tokens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price: Option<Price>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl ModelConfig {
    pub fn upstream(&self) -> &str {
        self.upstream_model.as_deref().unwrap_or(&self.id)
    }
}

/// Client integration settings. Only client-specific options belong here; providers,
/// models, and credentials are shared and must never be redefined per client.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientConfig {
    /// Model the app starts with (`owo connect` uses it when `--model` is not given).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The provider / account name this app shows, instead of the top-level `name`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

fn default_true() -> bool {
    true
}

fn default_version() -> u32 {
    CURRENT_VERSION
}

pub const DEFAULT_NAME: &str = "OwO";

fn default_name() -> String {
    DEFAULT_NAME.to_string()
}

/// Starter config written by `owo init`.
pub const SAMPLE_CONFIG: &str = r#"# OwO AI Gateway — one local endpoint for every AI coding app
#
# Define each provider once. Every client (Codex, Claude, Cursor, ...) shares
# these providers, models, and credentials.

# The provider / account name apps show (per app: `name` under [clients.<app>]).
# name = "OwO"

# A provider named after a built-in preset needs nothing but its models
# (`owo providers presets` lists them). Prefer a key reference over the key itself:
#   api_key = "keyring:NAME"  (owo key set NAME)  or  api_key = "env:NAME"
[providers.deepseek]
api_key = "env:DEEPSEEK_API_KEY"
models = ["deepseek-chat", "deepseek-reasoner"]

[providers.ollama]
models = ["qwen3:8b"]

# Any other OpenAI-compatible endpoint:
# [providers.my-gateway]
# adapter = "openai-chat"
# base_url = "https://llm.example.com/v1"
# api_key = "keyring:my-gateway"
# models = ["some-model"]

# Optional: rename a model, give it a display name, or per-client aliases.
# [[models]]
# id = "fast"
# provider = "deepseek"
# upstream_model = "deepseek-chat"
# display_name = "Fast (DeepSeek)"
# aliases = { codex = "gpt-fast" }
# price = { input = 0.27, output = 1.1, cache_read = 0.07 }   # USD per million tokens, for `owo usage` cost estimates

# Optional server settings (defaults shown).
# [server]
# listen = "127.0.0.1:8787"
"#;
