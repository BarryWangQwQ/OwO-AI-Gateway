//! MiniMax's two command-line products.
//!
//! - MiniMax Code (`mcode`): `custom_provider.owo` in `~/.minimax/config.yaml`, an
//!   Anthropic Messages provider. `defaultModel` and the MiniMax login are left alone;
//!   OwO AI Gateway models appear as `custom_provider:owo/<model>`.
//! - MiniMax CLI (`mmx`): only its `text chat` / `text repl` commands speak a protocol OwO AI Gateway
//!   serves (Anthropic Messages under `<base>/anthropic/v1/messages`). `owo launch
//!   mmx` starts `mmx` with a throwaway config directory holding only a
//!   placeholder key, so the user's `~/.mmx` credentials are never read.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{bail, Result};
use serde_json::{json, Map, Value};

use crate::managed::{FileEdit, Format, Fragment, Seg};
use crate::{efforts, AppModel, Target, PROVIDER_ID};

pub const CODE_CLIENT_ID: &str = "minimax_code";
pub const CLI_CLIENT_ID: &str = "minimax_cli";
const MCODE_EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

/// `$MINIMAX_DATA_DIR`, else the legacy `$MAVIS_DATA_DIR`, else `~/.minimax`.
pub fn mcode_dir() -> Option<PathBuf> {
    crate::env_dir("MINIMAX_DATA_DIR").or_else(|| crate::env_dir("MAVIS_DATA_DIR")).or_else(|| crate::home().map(|h| h.join(".minimax")))
}

pub fn mcode_block(target: &Target, models: &[AppModel]) -> Value {
    let mut entries = Map::new();
    for m in models {
        let mut e = json!({});
        if let Some(ctx) = m.context_window {
            e["limit"] = json!({ "context": ctx });
        }
        let ladder = efforts(m, &MCODE_EFFORTS);
        if !ladder.is_empty() {
            e["thinking"] = json!({ "effortOptions": ladder });
        }
        entries.insert(m.id.clone(), e);
    }
    json!({
        "name": target.name,
        "kind": "custom",
        "enabled": true,
        "api": "anthropic-messages",
        "options": { "apiKey": target.key(), "baseURL": target.root, "authMode": "api-key" },
        "models": entries,
    })
}

pub fn mcode_edit(dir: &std::path::Path, target: &Target, models: &[AppModel]) -> FileEdit {
    FileEdit {
        path: dir.join("config.yaml"),
        format: Format::Yaml,
        fragments: vec![Fragment { path: vec![Seg::key("custom_provider"), Seg::key(PROVIDER_ID)], value: mcode_block(target, models) }],
    }
}

/// Flags that would point `mmx` somewhere else or at other credentials.
pub fn refused_mmx_flag(args: &[String]) -> Option<&'static str> {
    for a in args {
        for flag in ["--api-key", "--base-url", "--region"] {
            if a == flag || a.starts_with(&format!("{flag}=")) {
                return Some(flag);
            }
        }
    }
    None
}

/// Only `text chat` and `text repl` go through OwO AI Gateway.
pub fn check_mmx_command(args: &[String]) -> Result<()> {
    if let Some(flag) = refused_mmx_flag(args) {
        bail!("`{flag}` cannot be used through OwO AI Gateway (it sets the destination and credentials)");
    }
    let sub = args.iter().position(|a| a == "text").and_then(|i| args[i + 1..].iter().find(|a| !a.starts_with('-')));
    match sub.map(String::as_str) {
        Some("chat" | "repl") => Ok(()),
        _ => bail!("only `mmx text chat` and `mmx text repl` are routed through OwO AI Gateway; run plain `mmx` for other commands"),
    }
}

/// Proxy variables `mmx` would honor for every destination (it ignores `NO_PROXY`).
const PROXY_VARS: [&str; 6] = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY", "MINIMAX_API_KEY", "MINIMAX_BASE_URL"];

/// The environment changes for an `mmx` child: variables to remove and to set.
pub fn mmx_env(target: &Target, config_dir: &std::path::Path, inherited: impl Iterator<Item = String>) -> (Vec<String>, BTreeMap<String, String>) {
    let remove = inherited.filter(|k| PROXY_VARS.contains(&k.to_ascii_uppercase().as_str())).collect();
    let mut set = BTreeMap::new();
    set.insert("MMX_CONFIG_DIR".to_string(), config_dir.display().to_string());
    set.insert("MINIMAX_BASE_URL".to_string(), target.root.clone());
    set.insert("MINIMAX_REGION".to_string(), "global".to_string());
    (remove, set)
}

/// Contents of the throwaway `config.json` in `MMX_CONFIG_DIR`.
pub fn mmx_config(target: &Target) -> String {
    let mut text = serde_json::to_string_pretty(&json!({ "api_key": target.key(), "region": "global" })).expect("static JSON");
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcode_block_shape() {
        let target = Target { name: "OwO".into(), root: "http://127.0.0.1:8787/c/minimax_code".into(), token: None };
        let m = AppModel { id: "claude-opus-5-5".into(), display_name: "Opus".into(), context_window: Some(200_000), max_output_tokens: None, reasoning_efforts: vec!["low".into(), "max".into()], default_reasoning_effort: None, vision: false };
        let b = mcode_block(&target, &[m]);
        assert_eq!(b["api"], "anthropic-messages");
        assert_eq!(b["options"]["baseURL"], "http://127.0.0.1:8787/c/minimax_code");
        assert_eq!(b["models"]["claude-opus-5-5"]["thinking"]["effortOptions"], json!(["low", "max"]));
    }

    #[test]
    fn mmx_guard_rails() {
        let a = |s: &str| s.split_whitespace().map(str::to_string).collect::<Vec<_>>();
        assert!(check_mmx_command(&a("text chat --model claude-sonnet-5 --message hi")).is_ok());
        assert!(check_mmx_command(&a("--output json text repl")).is_ok());
        assert!(check_mmx_command(&a("image generate")).is_err());
        assert!(check_mmx_command(&a("text chat --api-key=sk")).is_err());
        let target = Target { name: "OwO".into(), root: "http://127.0.0.1:8787/c/minimax_cli".into(), token: None };
        let (remove, set) = mmx_env(&target, std::path::Path::new("/tmp/x"), ["https_proxy".to_string(), "PATH".to_string()].into_iter());
        assert_eq!(remove, ["https_proxy"]);
        assert_eq!(set["MINIMAX_BASE_URL"], "http://127.0.0.1:8787/c/minimax_cli");
    }
}
