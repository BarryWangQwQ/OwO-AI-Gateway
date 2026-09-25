//! Grok Build (xAI's `grok` CLI): a marked block in `~/.grok/config.toml` with one
//! `[model_providers.owo]` entry (OpenAI Responses) and one `[model.owo-…]` table per
//! OwO AI Gateway model. Grok reloads the file on change; everything outside the block is untouched.
//! The inherited provider fields need Grok Build 0.2.109 or later.

use std::path::PathBuf;

use serde_json::Value;

use crate::managed::{FileEdit, Format, Fragment};
use crate::{efforts, title_case, AppModel, Target, PROVIDER_ID};

pub const CLIENT_ID: &str = "grok_build";
/// Efforts Grok Build accepts.
const GROK_EFFORTS: [&str; 6] = ["none", "minimal", "low", "medium", "high", "xhigh"];

/// `$GROK_HOME`, else `~/.grok`.
pub fn grok_dir() -> Option<PathBuf> {
    crate::env_dir("GROK_HOME").or_else(|| crate::home().map(|h| h.join(".grok")))
}

pub fn config_path(dir: &std::path::Path) -> PathBuf {
    dir.join("config.toml")
}

/// Model alias inside Grok: `owo-` plus the id with anything but `[A-Za-z0-9_-]` turned
/// into `-` (a dot would split the TOML key path).
pub fn alias(id: &str) -> String {
    let cleaned: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '-' }).collect();
    format!("{PROVIDER_ID}-{cleaned}")
}

fn q(s: &str) -> String {
    // JSON string escapes are valid TOML basic-string escapes.
    Value::String(s.to_string()).to_string()
}

pub fn render(target: &Target, models: &[AppModel]) -> String {
    let mut out = format!(
        "[model_providers.{PROVIDER_ID}]\nbase_url = {}\napi_backend = \"responses\"\napi_key = {}\n",
        q(&target.v1()),
        q(target.key())
    );
    let mut used = Vec::new();
    for m in models {
        let mut name = alias(&m.id);
        let mut n = 2;
        while used.contains(&name) {
            name = format!("{}-{n}", alias(&m.id));
            n += 1;
        }
        used.push(name.clone());
        out.push_str(&format!("\n[model.{name}]\nmodel = {}\nmodel_provider = \"{PROVIDER_ID}\"\nname = {}\n", q(&m.id), q(&m.display_name)));
        if let Some(ctx) = m.context_window {
            out.push_str(&format!("context_window = {ctx}\n"));
        }
        let ladder = efforts(m, &GROK_EFFORTS);
        if !ladder.is_empty() {
            let default = m.default_reasoning_effort.clone().filter(|d| ladder.contains(d)).unwrap_or_else(|| ladder[0].clone());
            out.push_str(&format!("supports_reasoning_effort = true\nreasoning_effort = {}\n", q(&default)));
            for e in &ladder {
                out.push_str(&format!(
                    "\n[[model.{name}.reasoning_efforts]]\nid = {e}\nvalue = {e}\nlabel = {}\n",
                    q(&title_case(e)),
                    e = q(e)
                ));
                if *e == default {
                    out.push_str("default = true\n");
                }
            }
        }
    }
    out
}

pub fn edit(dir: &std::path::Path, target: &Target, models: &[AppModel]) -> FileEdit {
    FileEdit {
        path: config_path(dir),
        format: Format::TomlBlock,
        fragments: vec![Fragment { path: Vec::new(), value: Value::String(render(target, models)) }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_valid_toml() {
        let target = Target { name: "OwO".into(), root: "http://127.0.0.1:8787/c/grok_build".into(), token: None };
        let models = vec![
            AppModel {
                id: "claude-sonnet-5".into(),
                display_name: "Claude \"Sonnet\" 5".into(),
                context_window: Some(200_000),
                max_output_tokens: None,
                reasoning_efforts: vec!["low".into(), "medium".into(), "max".into()],
                default_reasoning_effort: Some("medium".into()),
                vision: true,
            },
            AppModel { id: "gpt-5.5".into(), display_name: "GPT".into(), context_window: None, max_output_tokens: None, reasoning_efforts: vec![], default_reasoning_effort: None, vision: false },
        ];
        let text = render(&target, &models);
        let doc: toml::Table = text.parse().unwrap();
        assert_eq!(doc["model_providers"]["owo"]["base_url"].as_str(), Some("http://127.0.0.1:8787/c/grok_build/v1"));
        assert_eq!(doc["model_providers"]["owo"]["api_key"].as_str(), Some("owo-local"));
        let sonnet = &doc["model"]["owo-claude-sonnet-5"];
        assert_eq!(sonnet["reasoning_effort"].as_str(), Some("medium"));
        let ladder = sonnet["reasoning_efforts"].as_array().unwrap();
        assert_eq!(ladder.len(), 2, "`max` is not a Grok effort");
        assert_eq!(ladder[1]["default"].as_bool(), Some(true));
        assert_eq!(doc["model"]["owo-gpt-5-5"]["model"].as_str(), Some("gpt-5.5"));
    }
}
