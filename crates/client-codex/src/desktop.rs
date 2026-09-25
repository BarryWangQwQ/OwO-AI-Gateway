//! Reversible edits to `<codex_home>/config.toml` for Codex Desktop, which has no
//! profile switch: three root keys plus `[model_providers.owo]`. Everything else in
//! the file is left byte-for-byte as it was.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{bail, Result};
use toml_edit::{value, DocumentMut, Item, Table};

use crate::profile::{provider_table, table_fingerprint, ProviderSettings};
use crate::state::{DesktopEdit, OwnedValue};

pub struct Applied {
    pub text: String,
    pub root_keys: BTreeMap<String, OwnedValue>,
    pub provider_table: String,
    pub created_providers_table: bool,
}

fn value_source(item: Option<&Item>) -> Option<String> {
    item.and_then(Item::as_value).map(|v| v.to_string().trim().to_string())
}

/// Applies OwO AI Gateway's keys. `prior` is the edit recorded by an earlier enable, whose
/// "previous" values must survive a re-enable.
pub fn apply(
    text: &str,
    model: &str,
    catalog: &Path,
    settings: &ProviderSettings,
    prior: Option<&DesktopEdit>,
    force: bool,
) -> Result<Applied> {
    let mut doc: DocumentMut = text.parse()?;

    let created_providers_table = match doc.get("model_providers") {
        None => {
            let mut t = Table::new();
            t.set_implicit(true);
            doc.insert("model_providers", Item::Table(t));
            true
        }
        Some(Item::Table(_)) => prior.is_some_and(|p| p.created_providers_table),
        Some(_) => bail!("`model_providers` in config.toml is not a standard table; OwO AI Gateway will not rewrite it"),
    };
    let providers = doc["model_providers"].as_table_mut().expect("checked above");
    if let Some(existing) = providers.get(crate::PROVIDER_ID).and_then(Item::as_table) {
        let ours = prior.is_some_and(|p| p.provider_table == table_fingerprint(existing));
        if !ours && !force {
            bail!("config.toml already defines [model_providers.{}] that OwO AI Gateway did not write (use --force to replace it)", crate::PROVIDER_ID);
        }
    }
    let table = provider_table(settings);
    let provider_table = table_fingerprint(&table);
    providers.insert(crate::PROVIDER_ID, Item::Table(table));

    let mut root_keys = BTreeMap::new();
    for (key, new) in [
        ("model_provider", crate::PROVIDER_ID.to_string()),
        ("model", model.to_string()),
        ("model_catalog_json", catalog.display().to_string()),
    ] {
        let current = value_source(doc.get(key));
        let previous = match prior.and_then(|p| p.root_keys.get(key)) {
            Some(owned) => owned.previous.clone(),
            None => current,
        };
        doc[key] = value(new);
        let written = value_source(doc.get(key)).expect("just written");
        root_keys.insert(key.to_string(), OwnedValue { previous, written });
    }

    Ok(Applied { text: doc.to_string(), root_keys, provider_table, created_providers_table })
}

pub struct Reverted {
    pub text: String,
    /// Keys the user changed after OwO AI Gateway wrote them; left as the user set them.
    pub conflicts: Vec<String>,
}

/// Removes only what OwO AI Gateway owns. Values the user changed since are reported, not overwritten,
/// unless `force`.
pub fn revert(text: &str, edit: &DesktopEdit, force: bool) -> Result<Reverted> {
    let mut doc: DocumentMut = text.parse()?;
    let mut conflicts = Vec::new();

    for (key, owned) in &edit.root_keys {
        let current = value_source(doc.get(key));
        if current.as_deref() == Some(owned.written.as_str()) || force {
            match &owned.previous {
                Some(src) => doc[key.as_str()] = Item::Value(src.parse()?),
                None => {
                    doc.remove(key);
                }
            }
        } else if current != owned.previous {
            conflicts.push(format!(
                "`{key}` is now {} (OwO AI Gateway wrote {})",
                current.as_deref().unwrap_or("unset"),
                owned.written
            ));
        }
    }

    if let Some(providers) = doc.get_mut("model_providers").and_then(Item::as_table_mut) {
        let fingerprint = providers.get(crate::PROVIDER_ID).and_then(Item::as_table).map(table_fingerprint);
        match fingerprint {
            Some(fp) if fp == edit.provider_table || force => {
                providers.remove(crate::PROVIDER_ID);
            }
            Some(_) => conflicts.push(format!("[model_providers.{}] was edited after OwO AI Gateway wrote it", crate::PROVIDER_ID)),
            None => {}
        }
        if edit.created_providers_table && providers.is_empty() {
            doc.remove("model_providers");
        }
    }

    Ok(Reverted { text: doc.to_string(), conflicts })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    const USER_CONFIG: &str = r#"model = "gpt-5.6-sol"
model_reasoning_effort = "high" # keep me

[desktop]
sansFontSize = 14

[mcp_servers.node_repl]
command = 'C:\tools\node_repl.exe'
"#;

    fn settings() -> ProviderSettings {
        ProviderSettings { name: "OwO".into(), base_url: "http://127.0.0.1:8787/c/codex/v1".into(), env_key: None }
    }

    fn edit(applied: &Applied) -> DesktopEdit {
        DesktopEdit {
            base_url: String::new(),
            model: String::new(),
            catalog_path: PathBuf::new(),
            native_aliases: Vec::new(),
            config_path: PathBuf::from("config.toml"),
            backup_path: None,
            original_sha256: None,
            written_sha256: String::new(),
            root_keys: applied.root_keys.clone(),
            provider_table: applied.provider_table.clone(),
            created_providers_table: applied.created_providers_table,
        }
    }

    #[test]
    fn apply_then_revert_is_lossless() {
        let a = apply(USER_CONFIG, "deepseek-chat", Path::new("C:/owo/models.json"), &settings(), None, false).unwrap();
        let doc: DocumentMut = a.text.parse().unwrap();
        assert_eq!(doc["model_provider"].as_str(), Some("owo"));
        assert_eq!(doc["model"].as_str(), Some("deepseek-chat"));
        assert_eq!(doc["model_providers"]["owo"]["wire_api"].as_str(), Some("responses"));
        // Unrelated settings are untouched, comments included.
        assert!(a.text.contains("model_reasoning_effort = \"high\" # keep me"));
        assert!(a.text.contains("[mcp_servers.node_repl]\ncommand = 'C:\\tools\\node_repl.exe'"));
        assert_eq!(a.root_keys["model"].previous.as_deref(), Some("\"gpt-5.6-sol\""));
        assert_eq!(a.root_keys["model_provider"].previous, None);

        let r = revert(&a.text, &edit(&a), false).unwrap();
        assert!(r.conflicts.is_empty());
        let before: DocumentMut = USER_CONFIG.parse().unwrap();
        let after: DocumentMut = r.text.parse().unwrap();
        assert_eq!(after.to_string().replace(' ', ""), before.to_string().replace(' ', ""));
        assert!(after.get("model_providers").is_none());
        assert!(after.get("model_catalog_json").is_none());
    }

    #[test]
    fn user_changes_after_enable_are_kept_and_reported() {
        let a = apply(USER_CONFIG, "deepseek-chat", Path::new("m.json"), &settings(), None, false).unwrap();
        let changed = a.text.replace("model = \"deepseek-chat\"", "model = \"my-choice\"");
        let r = revert(&changed, &edit(&a), false).unwrap();
        assert_eq!(r.conflicts.len(), 1, "{:?}", r.conflicts);
        let after: DocumentMut = r.text.parse().unwrap();
        assert_eq!(after["model"].as_str(), Some("my-choice"));
        assert!(after.get("model_provider").is_none(), "unchanged owned keys are still reverted");

        let forced = revert(&changed, &edit(&a), true).unwrap();
        let after: DocumentMut = forced.text.parse().unwrap();
        assert_eq!(after["model"].as_str(), Some("gpt-5.6-sol"));
    }

    #[test]
    fn foreign_provider_table_is_protected() {
        let text = "[model_providers.owo]\nname = \"someone else\"\n";
        assert!(apply(text, "m", Path::new("m.json"), &settings(), None, false).is_err());
        assert!(apply(text, "m", Path::new("m.json"), &settings(), None, true).is_ok());
    }

    #[test]
    fn re_enable_keeps_original_previous_values() {
        let a = apply(USER_CONFIG, "m1", Path::new("m.json"), &settings(), None, false).unwrap();
        let prior = edit(&a);
        let b = apply(&a.text, "m2", Path::new("m.json"), &settings(), Some(&prior), false).unwrap();
        assert_eq!(b.root_keys["model"].previous.as_deref(), Some("\"gpt-5.6-sol\""));
        let r = revert(&b.text, &edit(&b), false).unwrap();
        let after: DocumentMut = r.text.parse().unwrap();
        assert_eq!(after["model"].as_str(), Some("gpt-5.6-sol"));
    }
}
