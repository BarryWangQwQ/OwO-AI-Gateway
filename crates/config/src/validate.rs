use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::net::SocketAddr;

use owo_credentials::CredentialRef;

use crate::schema::{AuthMode, Config, CURRENT_VERSION};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub severity: Severity,
    /// Dotted location, e.g. `providers.deepseek.base_url`.
    pub path: String,
    pub message: String,
}

impl Diagnostic {
    pub fn error(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Error, path: path.into(), message: message.into() }
    }

    pub fn warning(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self { severity: Severity::Warning, path: path.into(), message: message.into() }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let level = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        write!(f, "{level}: {}: {}", self.path, self.message)
    }
}

pub(crate) fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
}

/// Model ids may also contain `/`, because upstream ids do (`anthropic/claude-x` on
/// OpenRouter). A canonical id always wins over `provider/model` direct routing.
pub(crate) fn is_valid_model_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && !id.starts_with('/')
        && !id.ends_with('/')
        && id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':' | '/' | '@'))
}

impl Config {
    /// Structural validation. Preset/adapter checks are performed by the registry.
    pub fn validate(&self) -> Vec<Diagnostic> {
        let mut out = Vec::new();

        if self.version != CURRENT_VERSION {
            out.push(Diagnostic::error(
                "version",
                format!("unsupported config version {} (expected {CURRENT_VERSION})", self.version),
            ));
        }

        match self.server.listen.parse::<SocketAddr>() {
            Ok(addr) => {
                if !addr.ip().is_loopback() && self.server.auth_token.is_none() {
                    out.push(Diagnostic::error(
                        "server.listen",
                        "binding a non-loopback address requires `server.auth_token`",
                    ));
                }
            }
            Err(_) => out.push(Diagnostic::error(
                "server.listen",
                format!("`{}` is not a socket address (expected e.g. 127.0.0.1:8787)", self.server.listen),
            )),
        }
        if self.server.max_body_bytes == 0 || self.server.max_concurrent_requests == 0 {
            out.push(Diagnostic::error("server", "limits must be greater than zero"));
        }
        let names = std::iter::once(("name".to_string(), self.name.as_str()))
            .chain(self.clients.iter().filter_map(|(id, c)| Some((format!("clients.{id}.name"), c.name.as_deref()?))));
        for (path, name) in names {
            if name.trim().is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
                out.push(Diagnostic::error(path, "must be 1-64 characters, without control characters"));
            }
        }
        let prices = self
            .providers
            .iter()
            .filter_map(|(id, p)| Some((format!("providers.{id}.model_defaults.price"), p.model_defaults.as_ref()?.price?)))
            .chain(self.models.iter().filter_map(|m| Some((format!("models.{}.price", m.id), m.price?))));
        for (path, price) in prices {
            for (field, value) in price.fields() {
                if value.is_some_and(|v| !v.is_finite() || v < 0.0) {
                    out.push(Diagnostic::error(format!("{path}.{field}"), "must be a price of 0 or more (USD per million tokens)"));
                }
            }
        }

        for (id, p) in &self.providers {
            let path = format!("providers.{id}");
            if !is_valid_id(id) {
                out.push(Diagnostic::error(&path, "provider id may only contain [A-Za-z0-9-_.:]"));
            }
            if matches!(p.api_key, Some(CredentialRef::Inline(_))) {
                out.push(Diagnostic::warning(
                    format!("{path}.api_key"),
                    format!(
                        "the key is stored in plain text in config.toml; keep it in the OS keyring instead: \
                         `owo key set {id}`, then `api_key = \"keyring:{id}\"`"
                    ),
                ));
            }
            if matches!(p.auth, Some(AuthMode::Header | AuthMode::Query)) && p.auth_param.is_none() {
                out.push(Diagnostic::error(format!("{path}.auth_param"), "required when auth is `header` or `query`"));
            }
            if let Some(base) = &p.base_url {
                if !(base.starts_with("https://") || base.starts_with("http://")) {
                    out.push(Diagnostic::error(format!("{path}.base_url"), "must be an http(s) URL"));
                }
            }
            for name in p.headers.keys() {
                let lower = name.to_ascii_lowercase();
                if matches!(lower.as_str(), "authorization" | "x-api-key" | "api-key" | "x-goog-api-key") {
                    out.push(Diagnostic::error(
                        format!("{path}.headers.{name}"),
                        "credentials must not be set as static headers; use `api_key`",
                    ));
                }
            }
        }

        let mut model_ids = HashSet::new();
        for (i, m) in self.models.iter().enumerate() {
            let path = format!("models[{i}]");
            if !is_valid_model_id(&m.id) {
                out.push(Diagnostic::error(format!("{path}.id"), "model id may only contain [A-Za-z0-9-_.:/@]"));
            }
            if !model_ids.insert(m.id.as_str()) {
                out.push(Diagnostic::error(format!("{path}.id"), format!("duplicate model id `{}`", m.id)));
            }
            if m.upstream_model.as_deref().is_some_and(|u| u.trim().is_empty()) {
                out.push(Diagnostic::error(format!("{path}.upstream_model"), "must not be empty (omit it to use `id`)"));
            }
            if !self.providers.contains_key(&m.provider) {
                out.push(Diagnostic::error(
                    format!("{path}.provider"),
                    format!("unknown provider `{}`; declare it under [providers.{}]", m.provider, m.provider),
                ));
            }
            if let Some(default) = &m.default_reasoning_effort {
                if !m.reasoning_efforts.is_empty() && !m.reasoning_efforts.contains(default) {
                    out.push(Diagnostic::error(
                        format!("{path}.default_reasoning_effort"),
                        format!("`{default}` is not listed in reasoning_efforts"),
                    ));
                }
            }
        }

        // Models listed on providers become canonical ids of their own.
        let mut listed: BTreeMap<&str, &str> = BTreeMap::new();
        for (pid, p) in &self.providers {
            let Some(list) = &p.models else { continue };
            for (j, id) in list.iter().enumerate() {
                let path = format!("providers.{pid}.models[{j}]");
                if !is_valid_model_id(id) {
                    out.push(Diagnostic::error(&path, format!("invalid model id `{id}`")));
                    continue;
                }
                if let Some(other) = listed.insert(id.as_str(), pid.as_str()) {
                    let msg = if other == pid {
                        format!("`{id}` is listed twice")
                    } else {
                        format!(
                            "`{id}` is also listed by providers.{other}; give one of them a distinct id with [[models]]"
                        )
                    };
                    out.push(Diagnostic::error(&path, msg));
                }
                // A [[models]] entry for the same provider and upstream id refines the listing.
                if let Some(m) = self.models.iter().find(|m| m.id == *id) {
                    if m.provider != *pid || m.upstream() != id {
                        out.push(Diagnostic::error(
                            &path,
                            format!("`{id}` is also a [[models]] id for a different model; rename one of them"),
                        ));
                    }
                }
            }
        }
        model_ids.extend(listed.keys().copied());

        // An alias may not shadow a canonical id or collide with another alias for the same client.
        let mut aliases: BTreeMap<(&str, &str), &str> = BTreeMap::new();
        for m in &self.models {
            for (client, alias) in &m.aliases {
                let path = format!("models.{}.aliases.{client}", m.id);
                if model_ids.contains(alias.as_str()) && alias != &m.id {
                    out.push(Diagnostic::error(&path, format!("alias `{alias}` shadows canonical model `{alias}`")));
                }
                if let Some(other) = aliases.insert((client.as_str(), alias.as_str()), m.id.as_str()) {
                    out.push(Diagnostic::error(&path, format!("alias `{alias}` is already used by model `{other}`")));
                }
            }
        }
        for ((client, alias), owner) in &aliases {
            if *client == "*" {
                continue;
            }
            if let Some(other) = aliases.get(&("*", alias)) {
                if other != owner {
                    out.push(Diagnostic::error(
                        format!("models.{owner}.aliases.{client}"),
                        format!("alias `{alias}` collides with the global alias of model `{other}`"),
                    ));
                }
            }
        }

        crate::mcp::validate(&self.mcp, &mut out);

        if self.providers.is_empty() {
            out.push(Diagnostic::warning("providers", "no providers configured"));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use owo_credentials::CredentialRef;

    use crate::{Config, ConfigError, Severity, SAMPLE_CONFIG};

    fn errors(text: &str) -> String {
        match Config::from_toml_str(text, "test") {
            Err(ConfigError::Invalid(msg)) => msg,
            Err(e) => e.to_string(),
            Ok(_) => String::new(),
        }
    }

    #[test]
    fn client_names_default_to_owo_and_can_be_overridden() {
        let (c, _) = Config::from_toml_str("[providers.p]\nmodels = [\"m\"]\n", "t").unwrap();
        assert_eq!(c.client_name("codex_desktop"), "OwO");
        let text = "name = \"My AI\"\n[clients.claude-desktop]\nname = \"Team Claude\"\n[providers.p]\nmodels = [\"m\"]\n";
        let (c, _) = Config::from_toml_str(text, "t").unwrap();
        assert_eq!((c.client_name("codex_desktop"), c.client_name("claude_desktop")), ("My AI", "Team Claude"));
        assert!(errors("name = \"  \"\n").contains("name: must be 1-64 characters"));
        assert!(errors("[clients.codex]\nname = \"a\\nb\"\n").contains("clients.codex.name"));
    }

    #[test]
    fn prices_parse_merge_and_validate() {
        let text = "[providers.p]\nmodels = [\"m\"]\nmodel_defaults = { price = { input = 1, output = 2 } }\n[[models]]\nid = \"n\"\nprovider = \"p\"\nprice = { input = 3, output = 15, cache_read = 0.3 }\n";
        let (c, _) = Config::from_toml_str(text, "t").unwrap();
        let price = c.models[0].price.unwrap();
        assert_eq!((price.input, price.cache_read, price.cache_write), (3.0, Some(0.3), None));
        // 1M input with 400K cached, 100K written, plus 200K output.
        let cost = price.cost(1_000_000, 400_000, 100_000, 200_000);
        assert!((cost - (0.5 * 3.0 + 0.4 * 0.3 + 0.1 * 3.0 + 0.2 * 15.0)).abs() < 1e-9, "{cost}");
        assert!(errors("[[models]]\nid = \"m\"\nprovider = \"p\"\nprice = { input = -1, output = 1 }\n[providers.p]\nmodels = []\n").contains("models.m.price.input"));
    }

    #[test]
    fn sample_config_is_valid() {
        let (config, _) = Config::from_toml_str(SAMPLE_CONFIG, "sample").unwrap();
        assert_eq!(config.version, 1);
        assert!(config.models.is_empty());
        assert_eq!(config.providers["ollama"].models.as_deref(), Some(&["qwen3:8b".to_string()][..]));
    }

    #[test]
    fn minimal_config_needs_no_boilerplate() {
        let (config, _) = Config::from_toml_str(
            "[providers.p]\nmodels = [\"a\"]\n[[models]]\nid = \"b\"\nprovider = \"p\"\n",
            "t",
        )
        .unwrap();
        assert_eq!(config.version, 1);
        assert_eq!(config.models[0].upstream(), "b");
    }

    #[test]
    fn listed_model_collisions() {
        let msg = errors("[providers.a]\nmodels = [\"x\"]\n[providers.b]\nmodels = [\"x\"]\n");
        assert!(msg.contains("also listed by providers.a"), "{msg}");
        let msg = errors("[providers.a]\nmodels = [\"x\", \"x\"]\n");
        assert!(msg.contains("listed twice"), "{msg}");
        let msg = errors("[providers.a]\nmodels = [\"x\"]\n[[models]]\nid = \"x\"\nprovider = \"a\"\nupstream_model = \"y\"\n");
        assert!(msg.contains("different model"), "{msg}");
        // Same provider and upstream: the [[models]] entry refines the listing.
        assert!(errors("[providers.a]\nmodels = [\"x\"]\n[[models]]\nid = \"x\"\nprovider = \"a\"\ndisplay_name = \"X\"\n").is_empty());
        // Upstream ids with slashes are fine.
        assert!(errors("[providers.openrouter]\nmodels = [\"anthropic/claude-x\"]\n").is_empty());
    }

    #[test]
    fn inline_api_key_is_accepted_with_a_warning() {
        let (config, diagnostics) =
            Config::from_toml_str("[providers.x]\nadapter = \"openai-chat\"\nbase_url = \"https://x.example/v1\"\napi_key = \"sk-abc123\"\nmodels = [\"m\"]\n", "t")
                .unwrap();
        assert!(matches!(config.providers["x"].api_key, Some(CredentialRef::Inline(_))));
        let warning = diagnostics.iter().find(|d| d.path == "providers.x.api_key").expect("a warning");
        assert_eq!(warning.severity, Severity::Warning);
        assert!(warning.message.contains("owo key set x"), "{warning}");
        assert!(!format!("{diagnostics:?}").contains("sk-abc123"));
    }

    #[test]
    fn rejects_unknown_fields() {
        let msg = errors("version = 1\n[server]\nlisten = \"127.0.0.1:1\"\nlistn = 3\n");
        assert!(msg.contains("unknown field"), "{msg}");
    }

    #[test]
    fn public_bind_requires_auth() {
        let msg = errors("version = 1\n[server]\nlisten = \"0.0.0.0:8787\"\n");
        assert!(msg.contains("auth_token"), "{msg}");
        assert!(errors("version = 1\n[server]\nlisten = \"0.0.0.0:8787\"\nauth_token = \"env:OWO_TOKEN\"\n").is_empty());
    }

    #[test]
    fn model_must_reference_declared_provider() {
        let msg = errors(
            "version = 1\n[[models]]\nid = \"a\"\nprovider = \"nope\"\nupstream_model = \"x\"\n",
        );
        assert!(msg.contains("unknown provider `nope`"), "{msg}");
    }

    #[test]
    fn detects_duplicate_models_and_alias_collisions() {
        let msg = errors(
            r#"version = 1
[providers.p]
[[models]]
id = "a"
provider = "p"
upstream_model = "x"
aliases = { codex = "b" }
[[models]]
id = "b"
provider = "p"
upstream_model = "y"
[[models]]
id = "a"
provider = "p"
upstream_model = "z"
"#,
        );
        assert!(msg.contains("duplicate model id `a`"), "{msg}");
        assert!(msg.contains("shadows canonical model `b`"), "{msg}");
    }

    #[test]
    fn client_tables_cannot_hold_providers() {
        let msg = errors("version = 1\n[clients.codex.providers.x]\nadapter = \"a\"\n");
        assert!(msg.contains("unknown field `providers`"), "{msg}");
    }

    #[test]
    fn credentials_cannot_be_static_headers() {
        let msg = errors("version = 1\n[providers.p]\nheaders = { Authorization = \"Bearer x\" }\n");
        assert!(msg.contains("must not be set as static headers"), "{msg}");
    }
}
