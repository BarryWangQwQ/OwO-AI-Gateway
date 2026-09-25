//! ZCode. Releases from 3.14 keep custom providers in `~/.zcode/v2/provider_config.json`
//! (`schemaVersion` 1); `~/.zcode/v2/config.json` is only imported once, when that store
//! is missing. OwO AI Gateway writes the store when it exists and the legacy file otherwise, over the
//! OpenAI Responses protocol (`<base>/v1/responses`).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::{json, Map, Value};

use crate::managed::{FileEdit, Format, Fragment, Seg};
use crate::{efforts, AppModel, Target, PROVIDER_ID};

pub const CLIENT_ID: &str = "zcode";
const STORE_SCHEMA_VERSION: u64 = 1;
const ZCODE_EFFORTS: [&str; 7] = ["minimal", "low", "medium", "high", "xhigh", "max", "ultra"];

/// `$ZCODE_HOME/v2`, else `~/.zcode/v2`.
pub fn zcode_dir() -> Option<PathBuf> {
    crate::env_dir("ZCODE_HOME").or_else(|| crate::home().map(|h| h.join(".zcode"))).map(|d| d.join("v2"))
}

pub fn store_path(dir: &Path) -> PathBuf {
    dir.join("provider_config.json")
}

pub fn legacy_path(dir: &Path) -> PathBuf {
    dir.join("config.json")
}

fn store_edit(path: PathBuf, target: &Target, models: &[AppModel]) -> FileEdit {
    let ids: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    let rule = json!({
        "providerId": PROVIDER_ID,
        "enabled": true,
        "providerName": target.name,
        "config": {
            "group": "standard-personal",
            "access": { "apiKey": target.key() },
            "api": { "type": "openai-responses", "baseUrl": target.v1() },
            "personalModelIds": ids,
            "modelOrder": ids,
        },
    });
    let mut fragments = vec![Fragment {
        path: vec![Seg::key("config"), Seg::key("providerConfigRules"), Seg::key("providerRules"), Seg::item(&[("providerId", PROVIDER_ID)])],
        value: rule,
    }];
    for m in models {
        if let Some(ctx) = m.context_window {
            fragments.push(Fragment {
                path: vec![
                    Seg::key("config"),
                    Seg::key("modelConfigRules"),
                    Seg::key("providerModelRules"),
                    Seg::item(&[("providerId", PROVIDER_ID), ("modelId", &m.id)]),
                ],
                value: json!({ "providerId": PROVIDER_ID, "modelId": m.id, "config": { "properties": { "contextWindow": ctx } } }),
            });
        }
    }
    FileEdit { path, format: Format::Json, fragments }
}

fn legacy_edit(path: PathBuf, target: &Target, models: &[AppModel]) -> FileEdit {
    let mut entries = Map::new();
    for m in models {
        let input = if m.vision { json!(["text", "image"]) } else { json!(["text"]) };
        let mut e = json!({ "name": m.display_name, "modalities": { "input": input, "output": ["text"] } });
        if let Some(ctx) = m.context_window {
            e["limit"] = json!({ "context": ctx });
        }
        let ladder = efforts(m, &ZCODE_EFFORTS);
        if !ladder.is_empty() {
            e["reasoning"] = json!({ "enabled": true, "variants": ladder });
            if let Some(d) = m.default_reasoning_effort.as_ref().filter(|d| ladder.contains(d)) {
                e["reasoning"]["defaultVariant"] = json!(d);
            }
        }
        entries.insert(m.id.clone(), e);
    }
    let block = json!({
        "name": target.name,
        "kind": "openai",
        "enabled": true,
        "source": "custom",
        "options": { "apiKey": target.key(), "baseURL": target.v1(), "apiKeyRequired": true },
        "models": entries,
    });
    FileEdit { path, format: Format::Json, fragments: vec![Fragment { path: vec![Seg::key("provider"), Seg::key(PROVIDER_ID)], value: block }] }
}

/// The edit for the file this ZCode install actually reads.
pub fn edit(dir: &Path, target: &Target, models: &[AppModel]) -> Result<FileEdit> {
    let store = store_path(dir);
    match std::fs::read(&store) {
        Ok(bytes) => {
            let doc: Value = serde_json::from_slice(&bytes).with_context(|| format!("{} is not valid JSON", store.display()))?;
            if doc.get("schemaVersion").and_then(Value::as_u64) != Some(STORE_SCHEMA_VERSION) {
                bail!(
                    "{} has a schemaVersion OwO AI Gateway does not know; add the provider in ZCode's settings instead (base URL {}, any non-empty key)",
                    store.display(),
                    target.v1()
                );
            }
            Ok(store_edit(store, target, models))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(legacy_edit(legacy_path(dir), target, models)),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", store.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> Vec<AppModel> {
        vec![AppModel { id: "claude-sonnet-5".into(), display_name: "Sonnet".into(), context_window: Some(200_000), max_output_tokens: None, reasoning_efforts: vec!["high".into()], default_reasoning_effort: Some("high".into()), vision: true }]
    }

    #[test]
    fn picks_the_store_when_present() {
        let dir = std::env::temp_dir().join(format!("owo-zcode-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = Target { name: "OwO".into(), root: "http://127.0.0.1:8787/c/zcode".into(), token: None };
        let e = edit(&dir, &target, &models()).unwrap();
        assert_eq!(e.path, legacy_path(&dir));
        assert_eq!(e.fragments[0].value["models"]["claude-sonnet-5"]["reasoning"]["defaultVariant"], "high");

        std::fs::write(store_path(&dir), r#"{"schemaVersion": 1, "config": {}}"#).unwrap();
        let e = edit(&dir, &target, &models()).unwrap();
        assert_eq!(e.path, store_path(&dir));
        assert_eq!(e.fragments.len(), 2);
        assert_eq!(e.fragments[0].value["config"]["api"]["baseUrl"], "http://127.0.0.1:8787/c/zcode/v1");

        std::fs::write(store_path(&dir), r#"{"schemaVersion": 2}"#).unwrap();
        assert!(edit(&dir, &target, &models()).is_err());
    }
}
