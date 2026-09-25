//! Mirrors OwO AI Gateway's model registry into the Cursor backend's model table, which is what
//! Cursor's model picker and run routing read. The table is owned by OwO AI Gateway: rows are
//! derived, never edited, and removed when the model leaves OwO AI Gateway's registry.
use std::collections::HashSet;

use owo_registry::Registry;

use crate::{
    model::{model_hash, ModelConfigInput, ModelType, OPENAI_CHAT_ENDPOINT},
    provider::owo::CLIENT_ID,
    store::Store,
    Result,
};

/// Placeholder endpoint: requests for these rows never leave the process through it.
const PLACEHOLDER_URL: &str = "http://127.0.0.1/owo";
const PLACEHOLDER_KEY: &str = "owo";
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncReport {
    pub total: usize,
    pub added: usize,
    pub removed: usize,
}

fn inputs(registry: &Registry, group: &str) -> Vec<ModelConfigInput> {
    registry
        .available_models()
        .enumerate()
        .map(|(i, m)| {
            let provider = registry.provider(&m.provider).map(|p| p.display_name.as_str()).unwrap_or(&m.provider);
            ModelConfigInput {
                sort_order: i as i64 + 1,
                display_name: m.display_name.clone(),
                group_name: Some(group.into()),
                model_type: ModelType::OpenAi,
                base_url: PLACEHOLDER_URL.into(),
                use_full_url: true,
                api_key: PLACEHOLDER_KEY.into(),
                tooltip_data: format!("{} · {provider} via {group}", m.display_name),
                model_id: m.exposed_id(Some(CLIENT_ID)).to_string(),
                reasoning_effort: m.default_reasoning_effort.clone().filter(|e| EFFORTS.contains(&e.as_str())),
                openai_endpoint: OPENAI_CHAT_ENDPOINT.into(),
                openai_extra_params_enabled: false,
                openai_extra_params: serde_json::json!({}),
                custom_headers_enabled: false,
                custom_headers: serde_json::json!({}),
                anthropic_extra_params_enabled: false,
                anthropic_extra_params: serde_json::json!({}),
                context_window_tokens: m.context_window.map(u64::from),
                max_completion_tokens: m.max_output_tokens.map(u64::from),
                anthropic_max_tokens: None,
                anthropic_thinking_effort: None,
                thinking_budget_tokens: None,
            }
        })
        .collect()
}

/// `group` is the label Cursor shows for these models.
pub async fn sync_models(store: &Store, registry: &Registry, group: &str) -> Result<SyncReport> {
    let desired = inputs(registry, group);
    let mut desired_hashes = HashSet::new();
    for input in &desired {
        desired_hashes.insert(model_hash(input)?);
    }
    let existing = store.models().await?;
    let existing_hashes: HashSet<String> = existing.iter().map(|m| m.model_hash.clone()).collect();

    let mut removed = 0;
    for model in &existing {
        if !desired_hashes.contains(&model.model_hash) {
            store.delete_model(&model.model_hash).await?;
            removed += 1;
        }
    }
    let mut to_add = Vec::new();
    for input in desired.iter() {
        if !existing_hashes.contains(&model_hash(input)?) {
            to_add.push(input.clone());
        }
    }
    let added = to_add.len();
    if !to_add.is_empty() {
        store.create_models(&to_add).await?;
    }
    Ok(SyncReport { total: desired.len(), added, removed })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry(text: &str) -> Registry {
        let (config, _) = owo_config::Config::from_toml_str(text, "t").unwrap();
        Registry::build(&config, owo_registry::PresetCatalog::builtin(), &["anthropic", "openai-chat"]).unwrap().0
    }

    #[tokio::test]
    async fn mirrors_the_registry_and_keeps_unchanged_rows() {
        let dir = std::env::temp_dir().join(format!("owo-cursor-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::connect(&format!("sqlite://{}", dir.join("t.db").display())).await.unwrap();

        let two = registry("[providers.anthropic]\nmodels = [\"claude-sonnet-5\", \"claude-opus-5-5\"]\n");
        let r = sync_models(&store, &two, "OwO").await.unwrap();
        assert_eq!(r, SyncReport { total: 2, added: 2, removed: 0 });
        let models = store.models().await.unwrap();
        assert_eq!(models[0].model_id, "claude-sonnet-5");
        assert_eq!(models[0].group_name.as_deref(), Some("OwO"));
        assert_eq!(models[0].context_window_tokens, Some(200_000));
        assert_eq!(models[0].reasoning_effort.as_deref(), Some("medium"));
        let first_hash = models[0].model_hash.clone();

        assert_eq!(sync_models(&store, &two, "OwO").await.unwrap(), SyncReport { total: 2, added: 0, removed: 0 });

        let one = registry("[providers.anthropic]\nmodels = [\"claude-sonnet-5\"]\n");
        assert_eq!(sync_models(&store, &one, "OwO").await.unwrap(), SyncReport { total: 1, added: 0, removed: 1 });
        assert_eq!(store.models().await.unwrap()[0].model_hash, first_hash);
    }
}
