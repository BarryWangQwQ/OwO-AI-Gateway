//! One server in each app's own shape, and back (for importing what an app already has).

use std::collections::BTreeMap;

use owo_config::{McpServerConfig, McpTransport};
use serde_json::{json, Map, Value};

use crate::apps::McpApp;

/// A server with every credential reference replaced by its value; written into app files,
/// never printed.
#[derive(Clone, PartialEq)]
pub struct Resolved {
    pub transport: McpTransport,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub cwd: Option<String>,
    pub url: String,
    pub headers: BTreeMap<String, String>,
}

/// Why `app` cannot run `server`, if it cannot.
pub fn unsupported_reason(app: McpApp, server: &McpServerConfig) -> Option<String> {
    let t = server.transport();
    if !app.transports().contains(&t) {
        return Some(format!("{} does not support {} MCP servers", app.label(), t.as_str()));
    }
    if server.cwd.is_some() && !app.supports_cwd() {
        return Some(format!("{} has no working-directory setting for MCP servers (`cwd`)", app.label()));
    }
    None
}

fn map(m: &BTreeMap<String, String>) -> Value {
    Value::Object(m.iter().map(|(k, v)| (k.clone(), Value::String(v.clone()))).collect())
}

/// Adds `key` unless `value` is empty.
fn put(o: &mut Map<String, Value>, key: &str, value: Value) {
    let empty = match &value {
        Value::Array(a) => a.is_empty(),
        Value::Object(m) => m.is_empty(),
        Value::Null => true,
        _ => false,
    };
    if !empty {
        o.insert(key.to_string(), value);
    }
}

fn stdio(o: &mut Map<String, Value>, r: &Resolved, cwd: bool) {
    o.insert("command".into(), json!(r.command));
    put(o, "args", json!(r.args));
    put(o, "env", map(&r.env));
    if cwd {
        put(o, "cwd", json!(r.cwd));
    }
}

fn remote(o: &mut Map<String, Value>, r: &Resolved, headers_key: &str) {
    o.insert("url".into(), json!(r.url));
    put(o, headers_key, map(&r.headers));
}

/// Standard `mcpServers` entries tagged with `type` (Claude Code, ZCode).
fn typed(r: &Resolved, stdio_type: &str) -> Map<String, Value> {
    let mut o = Map::new();
    match r.transport {
        McpTransport::Stdio => {
            o.insert("type".into(), json!(stdio_type));
            stdio(&mut o, r, false);
        }
        t => {
            o.insert("type".into(), json!(t.as_str()));
            remote(&mut o, r, "headers");
        }
    }
    o
}

/// The app's entry for `r`. Callers check [`unsupported_reason`] first.
pub fn render(app: McpApp, r: &Resolved) -> Value {
    let is_stdio = r.transport == McpTransport::Stdio;
    let mut o = Map::new();
    match app {
        McpApp::Codex if is_stdio => stdio(&mut o, r, true),
        McpApp::Codex => remote(&mut o, r, "http_headers"),
        McpApp::Grok if is_stdio => stdio(&mut o, r, true),
        // Grok tells streamable HTTP from SSE by itself.
        McpApp::Grok => remote(&mut o, r, "headers"),
        McpApp::Claude | McpApp::Zcode => o = typed(r, "stdio"),
        McpApp::ClaudeDesktop => stdio(&mut o, r, false),
        McpApp::Cursor if is_stdio => stdio(&mut o, r, false),
        McpApp::Cursor => remote(&mut o, r, "headers"),
        McpApp::Mcode if is_stdio => stdio(&mut o, r, false),
        McpApp::Mcode => o = typed(r, "stdio"),
        McpApp::Opencode if is_stdio => {
            let mut command = vec![r.command.clone()];
            command.extend(r.args.iter().cloned());
            o.insert("type".into(), json!("local"));
            o.insert("command".into(), json!(command));
            put(&mut o, "environment", map(&r.env));
            o.insert("enabled".into(), json!(true));
        }
        McpApp::Opencode => {
            o.insert("type".into(), json!("remote"));
            remote(&mut o, r, "headers");
            o.insert("enabled".into(), json!(true));
        }
        McpApp::Copilot => {
            o = typed(r, "local");
            if is_stdio {
                o.entry("args").or_insert_with(|| json!([]));
            }
            o.insert("tools".into(), json!(["*"]));
        }
    }
    Value::Object(o)
}

/// An app's entry read back as an OwO AI Gateway server.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    pub server: McpServerConfig,
    /// Settings of the entry OwO AI Gateway has no field for (lost if OwO AI Gateway takes the entry over).
    pub dropped: Vec<String>,
}

fn strings(v: &Value) -> Option<Vec<String>> {
    v.as_array()?.iter().map(|s| s.as_str().map(str::to_string)).collect()
}

fn string_map(v: &Value) -> Option<BTreeMap<String, String>> {
    v.as_object()?
        .iter()
        .map(|(k, v)| {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Number(_) | Value::Bool(_) => v.to_string(),
                _ => return None,
            };
            Some((k.clone(), s))
        })
        .collect()
}

pub fn parse(app: McpApp, entry: &Value) -> Result<Parsed, String> {
    let obj = entry.as_object().ok_or("the entry is not an object")?;
    let mut s = McpServerConfig::default();
    let mut used: Vec<&str> = Vec::new();
    let mut take = |key: &'static str| -> Option<&Value> {
        let v = obj.get(key)?;
        used.push(key);
        Some(v)
    };
    let bad = |key: &str| format!("`{key}` has an unexpected shape");

    let kind = take("type").and_then(Value::as_str).map(str::to_ascii_lowercase);
    if let Some(d) = take("description").and_then(Value::as_str) {
        s.description = Some(d.to_string());
    }
    match take("command") {
        Some(Value::String(c)) => s.command = Some(c.clone()),
        // OpenCode's `["npx", "-y", "pkg"]`.
        Some(v @ Value::Array(_)) => {
            let mut parts = strings(v).ok_or_else(|| bad("command"))?.into_iter();
            s.command = parts.next();
            s.args = parts.collect();
        }
        Some(_) => return Err(bad("command")),
        None => {}
    }
    if let Some(v) = take("args") {
        s.args.extend(strings(v).ok_or_else(|| bad("args"))?);
    }
    for key in ["env", "environment"] {
        if let Some(v) = take(key) {
            s.env.extend(string_map(v).ok_or_else(|| bad(key))?);
        }
    }
    if let Some(v) = take("cwd") {
        s.cwd = Some(v.as_str().ok_or_else(|| bad("cwd"))?.to_string());
    }
    if let Some(v) = take("url") {
        s.url = Some(v.as_str().ok_or_else(|| bad("url"))?.to_string());
    }
    for key in ["headers", "http_headers"] {
        if let Some(v) = take(key) {
            s.headers.extend(string_map(v).ok_or_else(|| bad(key))?);
        }
    }
    // Defaults OwO AI Gateway writes anyway.
    if obj.get("enabled") == Some(&json!(true)) {
        take("enabled");
    }
    if app == McpApp::Copilot && obj.get("tools") == Some(&json!(["*"])) {
        take("tools");
    }

    s.transport = match (kind.as_deref(), &s.command, &s.url) {
        (Some("sse"), _, Some(_)) => Some(McpTransport::Sse),
        (_, Some(_), None) => None,
        (_, None, Some(_)) => None,
        (_, None, None) => return Err("the entry has neither `command` nor `url`".into()),
        (_, Some(_), Some(_)) => return Err("the entry has both `command` and `url`".into()),
    };
    if s.command.as_deref().is_some_and(|c| c.trim().is_empty()) {
        return Err("`command` is empty".into());
    }
    let mut dropped: Vec<String> = obj.keys().filter(|k| !used.contains(&k.as_str())).cloned().collect();
    dropped.sort();
    Ok(Parsed { server: s, dropped })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stdio_server() -> Resolved {
        Resolved {
            transport: McpTransport::Stdio,
            command: "npx".into(),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            env: [("GITHUB_TOKEN".to_string(), "ghp_x".to_string())].into(),
            cwd: None,
            url: String::new(),
            headers: BTreeMap::new(),
        }
    }

    fn http_server(transport: McpTransport) -> Resolved {
        Resolved {
            transport,
            command: String::new(),
            args: Vec::new(),
            env: BTreeMap::new(),
            cwd: None,
            url: "https://mcp.example.com/mcp".into(),
            headers: [("Authorization".to_string(), "Bearer t".to_string())].into(),
        }
    }

    #[test]
    fn per_app_shapes() {
        let s = stdio_server();
        assert_eq!(
            render(McpApp::Claude, &s),
            json!({"type": "stdio", "command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"], "env": {"GITHUB_TOKEN": "ghp_x"}})
        );
        assert_eq!(render(McpApp::Cursor, &s), json!({"command": "npx", "args": ["-y", "@modelcontextprotocol/server-github"], "env": {"GITHUB_TOKEN": "ghp_x"}}));
        assert_eq!(
            render(McpApp::Opencode, &s),
            json!({"type": "local", "command": ["npx", "-y", "@modelcontextprotocol/server-github"], "environment": {"GITHUB_TOKEN": "ghp_x"}, "enabled": true})
        );
        assert_eq!(render(McpApp::Copilot, &s)["type"], "local");
        assert_eq!(render(McpApp::Copilot, &s)["tools"], json!(["*"]));

        let h = http_server(McpTransport::Http);
        assert_eq!(render(McpApp::Codex, &h), json!({"url": "https://mcp.example.com/mcp", "http_headers": {"Authorization": "Bearer t"}}));
        assert_eq!(render(McpApp::Grok, &h), json!({"url": "https://mcp.example.com/mcp", "headers": {"Authorization": "Bearer t"}}));
        assert_eq!(render(McpApp::Claude, &http_server(McpTransport::Sse))["type"], "sse");
        assert_eq!(render(McpApp::Opencode, &h), json!({"type": "remote", "url": "https://mcp.example.com/mcp", "headers": {"Authorization": "Bearer t"}, "enabled": true}));
        assert_eq!(render(McpApp::Mcode, &h)["type"], "http");
        assert_eq!(render(McpApp::Zcode, &h)["type"], "http");
    }

    #[test]
    fn support_matrix() {
        let sse = McpServerConfig { transport: Some(McpTransport::Sse), url: Some("https://x".into()), ..Default::default() };
        assert!(unsupported_reason(McpApp::Codex, &sse).is_some());
        assert!(unsupported_reason(McpApp::Cursor, &sse).is_none());
        let http = McpServerConfig { url: Some("https://x".into()), ..Default::default() };
        assert!(unsupported_reason(McpApp::ClaudeDesktop, &http).is_some());
        let cwd = McpServerConfig { command: Some("x".into()), cwd: Some("/tmp".into()), ..Default::default() };
        assert!(unsupported_reason(McpApp::Codex, &cwd).is_none());
        assert!(unsupported_reason(McpApp::Claude, &cwd).is_some());
    }

    #[test]
    fn rendered_entries_parse_back() {
        let expected = McpServerConfig {
            command: Some("npx".into()),
            args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
            env: [("GITHUB_TOKEN".to_string(), "ghp_x".to_string())].into(),
            ..Default::default()
        };
        for app in McpApp::ALL {
            let parsed = parse(app, &render(app, &stdio_server())).unwrap();
            assert_eq!(parsed.server, expected, "{app:?}");
            assert!(parsed.dropped.is_empty(), "{app:?}: {:?}", parsed.dropped);
        }
        for app in McpApp::ALL.into_iter().filter(|a| a.transports().contains(&McpTransport::Sse)) {
            let parsed = parse(app, &render(app, &http_server(McpTransport::Sse))).unwrap();
            let want = if matches!(app, McpApp::Grok | McpApp::Cursor | McpApp::Opencode) { McpTransport::Http } else { McpTransport::Sse };
            assert_eq!(parsed.server.transport(), want, "{app:?}");
            assert_eq!(parsed.server.headers["Authorization"], "Bearer t");
        }
    }

    #[test]
    fn unknown_settings_are_reported() {
        let p = parse(McpApp::Codex, &json!({"command": "x", "startup_timeout_sec": 30, "enabled": false})).unwrap();
        assert_eq!(p.dropped, ["enabled", "startup_timeout_sec"]);
        assert!(parse(McpApp::Cursor, &json!({"args": ["x"]})).is_err());
        assert!(parse(McpApp::Cursor, &json!({"command": 3})).is_err());
    }
}
