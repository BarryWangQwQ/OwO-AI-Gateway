//! OpenCode: two provider blocks for the same OwO AI Gateway provider, `provider.owo` (opencode V1,
//! AI SDK `openai-compatible`) and `providers.owo` (opencode V2, the only form whose
//! per-model reasoning `variants` apply). They go either into the global config
//! (`owo connect opencode`) or, for one launch, into OpenCode's inline runtime layer
//! `OPENCODE_CONFIG_CONTENT` (`owo launch opencode`), which leaves every file alone.

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};

use crate::managed::{FileEdit, Format, Fragment, Seg};
use crate::{efforts, AppModel, Target, PROVIDER_ID};

pub const CLIENT_ID: &str = "opencode";
pub const CONFIG_CONTENT_ENV: &str = "OPENCODE_CONFIG_CONTENT";
/// Holds the gateway token for OpenCode, which reads it through `{env:…}` instead of from disk.
pub const TOKEN_ENV: &str = "OWO_API_KEY";
const V1_PACKAGE: &str = "@ai-sdk/openai-compatible";
const V2_PACKAGE: &str = "@opencode-ai/ai/providers/openai-compatible";
/// OpenCode rejects `limit.context` without `limit.output`; no catalog value means this budget.
const OUTPUT_FALLBACK: u32 = 32_000;
const OPENCODE_EFFORTS: [&str; 6] = ["minimal", "low", "medium", "high", "xhigh", "max"];

/// `$XDG_CONFIG_HOME/opencode/opencode.json`, else `~/.config/opencode/opencode.json`
/// (OpenCode uses the XDG layout on every platform).
pub fn global_config_path() -> Option<PathBuf> {
    let base = crate::env_dir("XDG_CONFIG_HOME").or_else(|| crate::home().map(|h| h.join(".config")))?;
    Some(base.join("opencode").join("opencode.json"))
}

fn model_entry(m: &AppModel) -> Value {
    let mut e = json!({ "name": m.display_name });
    if let Some(ctx) = m.context_window {
        let output = m.max_output_tokens.unwrap_or(OUTPUT_FALLBACK).min(ctx);
        e["limit"] = json!({ "context": ctx, "output": output });
    }
    if m.vision {
        e["attachment"] = json!(true);
        e["modalities"] = json!({ "input": ["text", "image"], "output": ["text"] });
    }
    e
}

/// (V1 block, V2 block).
pub fn provider_blocks(target: &Target, models: &[AppModel]) -> (Value, Value) {
    let mut connection = json!({ "baseURL": target.v1() });
    connection["apiKey"] = match &target.token {
        Some(_) => json!(format!("{{env:{TOKEN_ENV}}}")),
        None => json!(crate::PLACEHOLDER_KEY),
    };
    let mut v1_models = Map::new();
    let mut v2_models = Map::new();
    for m in models {
        v1_models.insert(m.id.clone(), model_entry(m));
        let mut v2 = model_entry(m);
        let ladder = efforts(m, &OPENCODE_EFFORTS);
        if !ladder.is_empty() {
            v2["variants"] = Value::Array(ladder.iter().map(|e| json!({ "id": e, "settings": { "reasoningEffort": e } })).collect());
        }
        v2_models.insert(m.id.clone(), v2);
    }
    let v1 = json!({ "npm": V1_PACKAGE, "name": target.name, "options": connection, "models": v1_models });
    let v2 = json!({ "package": V2_PACKAGE, "name": target.name, "settings": connection, "models": v2_models });
    (v1, v2)
}

pub fn edit(path: PathBuf, target: &Target, models: &[AppModel]) -> FileEdit {
    let (v1, v2) = provider_blocks(target, models);
    FileEdit {
        path,
        format: Format::Json,
        fragments: vec![
            Fragment { path: vec![Seg::key("provider"), Seg::key(PROVIDER_ID)], value: v1 },
            Fragment { path: vec![Seg::key("providers"), Seg::key(PROVIDER_ID)], value: v2 },
        ],
    }
}

/// Inline runtime config for one launch: the inherited `OPENCODE_CONFIG_CONTENT` with only
/// OwO AI Gateway's two blocks set.
pub fn runtime_config(inherited: Option<&str>, target: &Target, models: &[AppModel]) -> Result<String> {
    let mut doc = match inherited.map(str::trim).filter(|s| !s.is_empty()) {
        Some(text) => serde_json::from_str::<Value>(text).with_context(|| format!("{CONFIG_CONTENT_ENV} is not valid JSON"))?,
        None => json!({}),
    };
    let Some(obj) = doc.as_object_mut() else { bail!("{CONFIG_CONTENT_ENV} must be a JSON object") };
    let (v1, v2) = provider_blocks(target, models);
    for (key, block) in [("provider", v1), ("providers", v2)] {
        let slot = obj.entry(key).or_insert_with(|| json!({}));
        let Some(map) = slot.as_object_mut() else { bail!("`{key}` in {CONFIG_CONTENT_ENV} must be an object") };
        map.insert(PROVIDER_ID.to_string(), block);
    }
    Ok(doc.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> Vec<AppModel> {
        vec![AppModel {
            id: "claude-sonnet-5".into(),
            display_name: "Claude Sonnet 5".into(),
            context_window: Some(20_000),
            max_output_tokens: None,
            reasoning_efforts: vec!["low".into(), "high".into()],
            default_reasoning_effort: None,
            vision: true,
        }]
    }

    #[test]
    fn blocks_and_runtime_layer() {
        let target = Target { name: "OwO".into(), root: "http://127.0.0.1:8787/c/opencode".into(), token: Some("secret".into()) };
        let (v1, v2) = provider_blocks(&target, &models());
        assert_eq!(v1["options"]["apiKey"], "{env:OWO_API_KEY}", "the token itself is not written");
        assert_eq!(v1["models"]["claude-sonnet-5"]["limit"], json!({"context": 20000, "output": 20000}));
        assert!(v1["models"]["claude-sonnet-5"].get("variants").is_none());
        assert_eq!(v2["models"]["claude-sonnet-5"]["variants"][1]["settings"]["reasoningEffort"], "high");
        assert_eq!(v2["settings"]["baseURL"], "http://127.0.0.1:8787/c/opencode/v1");

        let merged = runtime_config(Some(r#"{"theme":"x","provider":{"mine":{}}}"#), &target, &models()).unwrap();
        let merged: Value = serde_json::from_str(&merged).unwrap();
        assert_eq!(merged["theme"], "x");
        assert!(merged["provider"]["mine"].is_object() && merged["provider"]["owo"].is_object());
        assert!(runtime_config(Some("[1]"), &target, &models()).is_err());
    }
}
