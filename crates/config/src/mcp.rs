//! `[mcp.<name>]`: MCP servers OwO AI Gateway writes into the apps listed in `apps`.
//!
//! Values in `env` and `headers` may be credential references (`keyring:NAME`, `env:NAME`),
//! resolved only when a server is written into an app's config file.

use std::collections::BTreeMap;

use owo_credentials::{CredentialError, CredentialRef};
use serde::{Deserialize, Serialize};

use crate::schema::APP_NAMES;
use crate::validate::Diagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum McpTransport {
    /// A local process speaking MCP over stdin/stdout.
    Stdio,
    /// Streamable HTTP.
    Http,
    /// The older HTTP + server-sent events transport.
    Sse,
}

impl McpTransport {
    pub fn as_str(self) -> &'static str {
        match self {
            McpTransport::Stdio => "stdio",
            McpTransport::Http => "http",
            McpTransport::Sse => "sse",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct McpServerConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Defaults to `stdio` with `command` and `http` with `url`; only `sse` needs saying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<McpTransport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// Apps the server is written into, by `owo connect` name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub apps: Vec<String>,
}

impl McpServerConfig {
    pub fn transport(&self) -> McpTransport {
        match (self.transport, &self.url) {
            (Some(t), _) => t,
            (None, Some(_)) => McpTransport::Http,
            (None, None) => McpTransport::Stdio,
        }
    }

    /// The same server, ignoring `description` and `apps`.
    pub fn same_server(&self, other: &McpServerConfig) -> bool {
        let strip = |s: &McpServerConfig| McpServerConfig { description: None, apps: Vec::new(), transport: Some(s.transport()), ..s.clone() };
        strip(self) == strip(other)
    }
}

/// Server names are used as keys in every app's config (Codex requires `[A-Za-z0-9_-]`).
pub fn is_valid_mcp_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 64 && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// `Some` when `value` is a credential reference rather than a literal.
pub fn mcp_value_ref(value: &str) -> Option<Result<CredentialRef, CredentialError>> {
    let v = value.trim();
    (v.starts_with("keyring:") || v.starts_with("env:")).then(|| v.parse())
}

/// Whether an `env` / header name usually holds a secret, so its literal value is hidden
/// in every output and offered to go into the keyring.
pub fn looks_secret(key: &str) -> bool {
    let k = key.to_ascii_uppercase().replace('-', "_");
    ["TOKEN", "SECRET", "PASSWORD", "PASSWD", "API_KEY", "APIKEY", "ACCESS_KEY", "PRIVATE_KEY", "CREDENTIAL", "AUTH", "COOKIE", "SESSION"]
        .iter()
        .any(|m| k.contains(m))
        || k.ends_with("_KEY")
        || k.ends_with("_PAT")
        || k == "KEY"
}

/// The keyring entry OwO AI Gateway suggests for one secret of a server.
pub fn mcp_keyring_name(server: &str, key: &str) -> String {
    let key: String = key.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.') { c } else { '_' }).collect();
    format!("mcp-{server}-{key}")
}

fn is_valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

fn is_valid_env_name(name: &str) -> bool {
    !name.is_empty() && !name.contains('=') && !name.chars().any(char::is_control)
}

pub(crate) fn validate(servers: &BTreeMap<String, McpServerConfig>, out: &mut Vec<Diagnostic>) {
    for (name, s) in servers {
        let path = format!("mcp.{name}");
        if !is_valid_mcp_name(name) {
            out.push(Diagnostic::error(&path, "MCP server names may only contain [A-Za-z0-9-_] (at most 64)"));
        }
        let transport = s.transport();
        match transport {
            McpTransport::Stdio => {
                if s.command.as_deref().is_none_or(|c| c.trim().is_empty()) {
                    out.push(Diagnostic::error(format!("{path}.command"), "a stdio server needs `command`"));
                }
                if s.url.is_some() || !s.headers.is_empty() {
                    out.push(Diagnostic::error(&path, "`url` and `headers` belong to http/sse servers, not stdio"));
                }
            }
            McpTransport::Http | McpTransport::Sse => {
                match s.url.as_deref() {
                    None => out.push(Diagnostic::error(format!("{path}.url"), format!("an {} server needs `url`", transport.as_str()))),
                    Some(u) if !(u.starts_with("https://") || u.starts_with("http://")) => {
                        out.push(Diagnostic::error(format!("{path}.url"), "must be an http(s) URL"))
                    }
                    Some(_) => {}
                }
                if s.command.is_some() || !s.args.is_empty() || !s.env.is_empty() || s.cwd.is_some() {
                    out.push(Diagnostic::error(&path, "`command`, `args`, `env` and `cwd` belong to stdio servers"));
                }
            }
        }
        for (field, map, valid) in [("env", &s.env, is_valid_env_name as fn(&str) -> bool), ("headers", &s.headers, is_valid_header_name)] {
            for (key, value) in map {
                let at = format!("{path}.{field}.{key}");
                if !valid(key) {
                    out.push(Diagnostic::error(&at, "not a valid name"));
                }
                match mcp_value_ref(value) {
                    Some(Err(e)) => out.push(Diagnostic::error(&at, e.to_string())),
                    Some(Ok(_)) => {}
                    None if looks_secret(key) && !value.is_empty() => out.push(Diagnostic::warning(
                        &at,
                        format!(
                            "the value is stored in plain text in config.toml; keep it in the OS keyring instead: \
                             `owo key set {k}`, then `\"keyring:{k}\"`",
                            k = mcp_keyring_name(name, key)
                        ),
                    )),
                    None => {}
                }
            }
        }
        let mut seen = Vec::new();
        for app in &s.apps {
            if !APP_NAMES.iter().any(|(n, _)| n == app) {
                let apps = APP_NAMES.iter().map(|(n, _)| *n).collect::<Vec<_>>().join(", ");
                out.push(Diagnostic::error(format!("{path}.apps"), format!("unknown app `{app}` (apps: {apps})")));
            } else if seen.contains(&app) {
                out.push(Diagnostic::warning(format!("{path}.apps"), format!("`{app}` is listed twice")));
            }
            seen.push(app);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Config, ConfigError, McpTransport, Severity};

    fn load(text: &str) -> Result<Config, String> {
        match Config::from_toml_str(text, "t") {
            Ok((c, _)) => Ok(c),
            Err(ConfigError::Invalid(m)) => Err(m),
            Err(e) => Err(e.to_string()),
        }
    }

    #[test]
    fn servers_parse_with_inferred_transports() {
        let c = load(
            "[mcp.fs]\ncommand = \"npx\"\nargs = [\"-y\", \"fs\"]\nenv = { TOKEN = \"keyring:mcp-fs-TOKEN\" }\napps = [\"claude\", \"codex\"]\n\
             [mcp.linear]\nurl = \"https://mcp.linear.app/mcp\"\n[mcp.old]\ntransport = \"sse\"\nurl = \"http://127.0.0.1:9000/sse\"\n",
        )
        .unwrap();
        assert_eq!(c.mcp["fs"].transport(), McpTransport::Stdio);
        assert_eq!(c.mcp["linear"].transport(), McpTransport::Http);
        assert_eq!(c.mcp["old"].transport(), McpTransport::Sse);
        assert_eq!(c.mcp["fs"].apps, ["claude", "codex"]);
    }

    #[test]
    fn invalid_servers_are_rejected() {
        for (text, needle) in [
            ("[mcp.\"a b\"]\ncommand = \"x\"\n", "may only contain"),
            ("[mcp.x]\nargs = [\"a\"]\n", "needs `command`"),
            ("[mcp.x]\ntransport = \"sse\"\n", "needs `url`"),
            ("[mcp.x]\nurl = \"ftp://x\"\n", "http(s) URL"),
            ("[mcp.x]\ncommand = \"a\"\nurl = \"https://x\"\ntransport = \"stdio\"\n", "belong to http/sse"),
            ("[mcp.x]\nurl = \"https://x\"\nenv = { A = \"b\" }\n", "belong to stdio"),
            ("[mcp.x]\ncommand = \"a\"\napps = [\"nope\"]\n", "unknown app `nope`"),
            ("[mcp.x]\ncommand = \"a\"\nenv = { A = \"keyring:bad name\" }\n", "invalid key reference"),
            ("[mcp.x]\ncommand = \"a\"\nfoo = 1\n", "unknown field"),
        ] {
            let err = load(text).unwrap_err();
            assert!(err.contains(needle), "{text}: {err}");
        }
    }

    #[test]
    fn inline_secrets_warn_without_echoing() {
        let (_, diags) = Config::from_toml_str("[mcp.gh]\ncommand = \"npx\"\nenv = { GITHUB_TOKEN = \"ghp_abc123\", MODE = \"x\" }\n", "t").unwrap();
        let w: Vec<_> = diags.iter().filter(|d| d.path.starts_with("mcp.")).collect();
        assert_eq!(w.len(), 1, "{w:?}");
        assert_eq!(w[0].severity, Severity::Warning);
        assert!(w[0].message.contains("keyring:mcp-gh-GITHUB_TOKEN"));
        assert!(!format!("{diags:?}").contains("ghp_abc123"));
    }

    #[test]
    fn secret_names() {
        for k in ["GITHUB_PERSONAL_ACCESS_TOKEN", "Authorization", "x-api-key", "OPENAI_API_KEY", "GH_PAT", "DB_PASSWORD"] {
            assert!(super::looks_secret(k), "{k}");
        }
        for k in ["NODE_ENV", "MODE", "Z_AI_MODE", "PATH", "Accept"] {
            assert!(!super::looks_secret(k), "{k}");
        }
    }
}
