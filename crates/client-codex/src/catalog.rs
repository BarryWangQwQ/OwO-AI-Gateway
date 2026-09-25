//! Codex model catalog (`model_catalog_json` / `GET /models?client_version=`).
//!
//! Entries are derived from a template taken from the installed Codex build
//! (`codex debug models --bundled`), so every field Codex's strict parser needs
//! is present for exactly that version. OpenAI-only capabilities are removed
//! from the template and OwO AI Gateway's own model metadata is applied.

use serde_json::{json, Map, Value};
use owo_registry::{Model, Registry};

/// What the catalog needs to know about one OwO AI Gateway model.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogModel {
    /// The id Codex sends back as `model` (the model's alias for this client, or its canonical id).
    pub slug: String,
    pub display_name: String,
    pub description: String,
    pub context_window: Option<u32>,
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
    pub vision: bool,
    pub parallel_tools: bool,
}

impl CatalogModel {
    pub fn from_model(model: &Model, provider_name: &str, client: &str) -> Self {
        Self {
            slug: model.exposed_id(Some(client)).to_string(),
            display_name: model.display_name.clone(),
            description: format!("{provider_name} via OwO AI Gateway"),
            context_window: model.context_window,
            reasoning_efforts: model.reasoning_efforts.clone(),
            default_reasoning_effort: model.default_reasoning_effort.clone(),
            vision: model.capabilities.vision == Some(true),
            parallel_tools: model.capabilities.parallel_tools == Some(true),
        }
    }
}

/// Every model currently routable, as the Codex surface with this client id should see it.
pub fn catalog_models(registry: &Registry, client: &str) -> Vec<CatalogModel> {
    registry
        .available_models()
        .map(|m| {
            let provider = registry.provider(&m.provider).map(|p| p.display_name.as_str()).unwrap_or(&m.provider);
            CatalogModel::from_model(m, provider, client)
        })
        .collect()
}

/// Picks the entry to clone from a bundled catalog: a classic-tools (no `tool_mode`),
/// non-Responses-Lite, listed model, preferring `gpt-5.5`, whose shape fits a routed model best.
pub fn pick_template(bundled: &Value) -> Option<Value> {
    let models = bundled.get("models")?.as_array()?;
    let classic = |m: &&Value| {
        m.get("tool_mode").is_none()
            && m.get("use_responses_lite") != Some(&Value::Bool(true))
            && m.get("visibility").and_then(Value::as_str) == Some("list")
    };
    models
        .iter()
        .find(|m| classic(m) && m.get("slug").and_then(Value::as_str) == Some("gpt-5.5"))
        .or_else(|| models.iter().find(classic))
        .or_else(|| models.first())
        .cloned()
}

/// Minimal template used when no Codex build is available to supply one.
pub fn builtin_template() -> Value {
    json!({
        "slug": "template",
        "display_name": "template",
        "description": "",
        "default_reasoning_level": "medium",
        "supported_reasoning_levels": [],
        "shell_type": "unified_exec",
        "visibility": "list",
        "supported_in_api": true,
        "priority": 0,
        "availability_nux": null,
        "upgrade": null,
        "include_skills_usage_instructions": true,
        "include_plugin_usage_instructions": true,
        "include_apps_usage_instructions": true,
        "default_reasoning_summary": "none",
        "support_verbosity": false,
        "apply_patch_tool_type": "freeform",
        "truncation_policy": { "mode": "tokens", "limit": 10000 },
        "supports_image_detail_original": false,
        "context_window": 128000,
        "max_context_window": 128000,
        "comp_hash": "owo",
        "effective_context_window_percent": 95,
        "experimental_supported_tools": [],
        "input_modalities": ["text"],
        "supports_search_tool": false,
        "supports_experimental_context": false,
        "use_responses_lite": false,
        "node_repl_auto_review_required": false,
        "node_repl_disabled": false,
        "base_instructions": "You are Codex, a coding agent. You and the user share one workspace. \
Work carefully: read the relevant code before changing it, make focused edits with the provided tools, \
run checks when possible, and report clearly what you changed and why."
    })
}

/// Fields that describe OpenAI-hosted capabilities a routed model does not have.
const OPENAI_ONLY_FIELDS: &[&str] = &[
    "model_messages",
    "tool_mode",
    "multi_agent_version",
    "multi_agent_reasoning_effort",
    "use_responses_lite",
    "supports_websockets",
    "supports_experimental_context",
    "supports_reasoning_summaries",
    "additional_speed_tiers",
    "service_tier",
    "service_tiers",
    "default_service_tier",
    "web_search_tool_type",
    "available_in_plans",
    "available_access_programs",
    "minimal_client_version",
    "model_specialty",
    "auto_compact_token_limit",
];

pub fn build_entry(template: &Value, model: &CatalogModel, priority: usize) -> Value {
    let mut e: Map<String, Value> = template.as_object().cloned().unwrap_or_default();
    for key in OPENAI_ONLY_FIELDS {
        e.remove(*key);
    }

    e.insert("slug".into(), json!(model.slug));
    e.insert("display_name".into(), json!(model.display_name));
    e.insert("description".into(), json!(model.description));
    e.insert("priority".into(), json!(priority));
    e.insert("visibility".into(), json!("list"));
    e.insert("supported_in_api".into(), json!(true));
    e.insert("availability_nux".into(), Value::Null);
    e.insert("upgrade".into(), Value::Null);

    // Deferred tool search and hosted web search are OpenAI-backend features.
    e.insert("supports_search_tool".into(), json!(false));
    e.insert("use_responses_lite".into(), json!(false));
    e.insert("supports_experimental_context".into(), json!(false));
    e.insert("supports_parallel_tool_calls".into(), json!(model.parallel_tools));
    e.insert("supports_image_detail_original".into(), json!(false));
    e.insert("input_modalities".into(), if model.vision { json!(["text", "image"]) } else { json!(["text"]) });

    let context = model.context_window.unwrap_or(128_000);
    e.insert("context_window".into(), json!(context));
    e.insert("max_context_window".into(), json!(context));
    e.insert("effective_context_window_percent".into(), json!(95));

    let (levels, default) = reasoning_levels(model);
    e.insert("supported_reasoning_levels".into(), levels);
    e.insert("default_reasoning_level".into(), json!(default));
    e.insert("default_reasoning_summary".into(), json!("none"));

    if let Some(Value::String(text)) = e.get_mut("base_instructions") {
        *text = retarget_identity(text, &model.display_name);
    }
    Value::Object(e)
}

fn reasoning_levels(model: &CatalogModel) -> (Value, String) {
    if model.reasoning_efforts.is_empty() {
        // The provider decides; Codex still needs one selectable level.
        return (json!([{ "effort": "medium", "description": "Provider default reasoning" }]), "medium".into());
    }
    let levels: Vec<Value> = model
        .reasoning_efforts
        .iter()
        .map(|e| json!({ "effort": e, "description": format!("{e} reasoning") }))
        .collect();
    let default = model
        .default_reasoning_effort
        .clone()
        .filter(|d| model.reasoning_efforts.contains(d))
        .or_else(|| model.reasoning_efforts.iter().find(|e| *e == "medium").cloned())
        .unwrap_or_else(|| model.reasoning_efforts[0].clone());
    (Value::Array(levels), default)
}

/// The bundled prompt introduces itself as "based on GPT-5"; a routed model is not.
fn retarget_identity(text: &str, display_name: &str) -> String {
    let mut out = text.to_string();
    for phrase in ["based on GPT-5", "based on GPT‑5"] {
        out = out.replace(phrase, &format!("running on {display_name}"));
    }
    out
}

pub fn build_catalog(template: &Value, models: &[CatalogModel]) -> Value {
    let entries: Vec<Value> = models.iter().enumerate().map(|(i, m)| build_entry(template, m, i + 1)).collect();
    json!({ "models": entries })
}

/// A native GPT slug lent to an OwO AI Gateway model.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeAlias {
    /// The slug Codex shows and sends (`gpt-5.5`, ...).
    pub native: String,
    /// The OwO AI Gateway model's Codex-facing slug it stands for.
    pub model: String,
}

/// Picker-visible native slugs of a bundled catalog, in picker order.
pub fn native_slugs(bundled: &Value) -> Vec<String> {
    let mut rows: Vec<(i64, String)> = bundled
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter(|m| m.get("visibility").and_then(Value::as_str) == Some("list"))
                .filter_map(|m| {
                    let slug = m.get("slug")?.as_str()?.to_string();
                    Some((m.get("priority").and_then(Value::as_i64).unwrap_or(i64::MAX), slug))
                })
                .collect()
        })
        .unwrap_or_default();
    rows.sort();
    rows.into_iter().map(|(_, s)| s).collect()
}

/// Lends native slugs to OwO AI Gateway models in order. Models beyond the available slots keep only
/// their own slug (usable from the CLI, not shown by a signed-out Desktop picker).
pub fn assign_native_aliases(native: &[String], models: &[CatalogModel]) -> Vec<NativeAlias> {
    native
        .iter()
        .zip(models)
        .map(|(n, m)| NativeAlias { native: n.clone(), model: m.slug.clone() })
        .collect()
}

/// Catalog for a signed-out Codex Desktop, whose picker only lists slugs on a
/// server-delivered native allowlist: aliased entries carry the OwO AI Gateway model's metadata
/// under the native slug, and each aliased model keeps a hidden entry under its own slug.
pub fn build_aliased_catalog(template: &Value, models: &[CatalogModel], aliases: &[NativeAlias]) -> Value {
    let mut entries = Vec::new();
    let mut priority = 1;
    for alias in aliases {
        if let Some(m) = models.iter().find(|m| m.slug == alias.model) {
            let mut e = build_entry(template, m, priority);
            e["slug"] = json!(alias.native);
            entries.push(e);
            priority += 1;
        }
    }
    for m in models {
        let mut e = build_entry(template, m, priority);
        if aliases.iter().any(|a| a.model == m.slug) {
            e["visibility"] = json!("hide");
        }
        entries.push(e);
        priority += 1;
    }
    json!({ "models": entries })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled() -> Value {
        json!({ "models": [
            { "slug": "gpt-6", "visibility": "list", "tool_mode": "code_mode_only", "use_responses_lite": true },
            {
                "slug": "gpt-5.5", "display_name": "GPT-5.5", "visibility": "list", "use_responses_lite": false,
                "model_messages": { "instructions_template": "x" },
                "service_tiers": [{ "id": "priority" }], "additional_speed_tiers": ["fast"],
                "web_search_tool_type": "text_and_image", "supports_search_tool": true,
                "input_modalities": ["text", "image"], "context_window": 272000, "max_context_window": 272000,
                "shell_type": "unified_exec", "apply_patch_tool_type": "freeform", "comp_hash": "2911",
                "base_instructions": "You are Codex, a coding agent based on GPT-5. Be helpful."
            }
        ]})
    }

    fn model() -> CatalogModel {
        CatalogModel {
            slug: "deepseek-chat".into(),
            display_name: "DeepSeek Chat".into(),
            description: "DeepSeek via OwO AI Gateway".into(),
            context_window: Some(64_000),
            reasoning_efforts: vec![],
            default_reasoning_effort: None,
            vision: false,
            parallel_tools: false,
        }
    }

    #[test]
    fn picks_classic_template() {
        assert_eq!(pick_template(&bundled()).unwrap()["slug"], "gpt-5.5");
    }

    #[test]
    fn entry_strips_openai_capabilities_and_applies_metadata() {
        let t = pick_template(&bundled()).unwrap();
        let e = build_entry(&t, &model(), 1);
        for key in ["model_messages", "service_tiers", "additional_speed_tiers", "web_search_tool_type", "tool_mode"] {
            assert!(e.get(key).is_none(), "{key} should be removed");
        }
        assert_eq!(e["slug"], "deepseek-chat");
        assert_eq!(e["supports_search_tool"], false);
        assert_eq!(e["input_modalities"], json!(["text"]));
        assert_eq!(e["context_window"], 64000);
        assert_eq!(e["max_context_window"], 64000);
        assert_eq!(e["default_reasoning_level"], "medium");
        assert_eq!(e["supported_reasoning_levels"].as_array().unwrap().len(), 1);
        // Template fields Codex requires survive.
        assert_eq!(e["shell_type"], "unified_exec");
        assert_eq!(e["comp_hash"], "2911");
        assert_eq!(e["base_instructions"], "You are Codex, a coding agent running on DeepSeek Chat. Be helpful.");
    }

    #[test]
    fn declared_reasoning_ladder() {
        let mut m = model();
        m.reasoning_efforts = vec!["low".into(), "high".into()];
        m.default_reasoning_effort = Some("high".into());
        m.vision = true;
        let e = build_entry(&builtin_template(), &m, 3);
        assert_eq!(e["supported_reasoning_levels"][1]["effort"], "high");
        assert_eq!(e["default_reasoning_level"], "high");
        assert_eq!(e["input_modalities"], json!(["text", "image"]));
        assert_eq!(e["priority"], 3);
    }

    #[test]
    fn native_aliases_fill_visible_native_slots() {
        let bundled = json!({ "models": [
            { "slug": "gpt-5.5", "visibility": "list", "priority": 12 },
            { "slug": "gpt-6-astra", "visibility": "list", "priority": 1 },
            { "slug": "codex-auto-review", "visibility": "hide", "priority": 43 }
        ]});
        let natives = native_slugs(&bundled);
        assert_eq!(natives, vec!["gpt-6-astra", "gpt-5.5"]);

        let mut sonnet = model();
        sonnet.slug = "claude-sonnet-5".into();
        sonnet.display_name = "Claude Sonnet 5".into();
        let mut opus = model();
        opus.slug = "claude-opus-5-5".into();
        let mut haiku = model();
        haiku.slug = "claude-haiku-4-5".into();
        let models = vec![sonnet, opus, haiku];

        let aliases = assign_native_aliases(&natives, &models);
        assert_eq!(aliases, vec![
            NativeAlias { native: "gpt-6-astra".into(), model: "claude-sonnet-5".into() },
            NativeAlias { native: "gpt-5.5".into(), model: "claude-opus-5-5".into() },
        ]);

        let catalog = build_aliased_catalog(&builtin_template(), &models, &aliases);
        let entries = catalog["models"].as_array().unwrap();
        assert_eq!(entries[0]["slug"], "gpt-6-astra");
        assert_eq!(entries[0]["display_name"], "Claude Sonnet 5");
        let by_slug = |s: &str| entries.iter().find(|e| e["slug"] == s).unwrap().clone();
        assert_eq!(by_slug("claude-sonnet-5")["visibility"], "hide");
        assert_eq!(by_slug("claude-haiku-4-5")["visibility"], "list", "no slot left: keeps its own slug");
        assert_eq!(entries.len(), 5);
    }

    #[test]
    fn catalog_from_registry_uses_codex_aliases() {
        let (config, _) = owo_config::Config::from_toml_str(
            "[providers.deepseek]\nmodels = [\"deepseek-chat\"]\n[[models]]\nid = \"deepseek-chat\"\nprovider = \"deepseek\"\naliases = { codex = \"ds\", codex-desktop = \"ds-app\" }\n",
            "t",
        )
        .unwrap();
        let (registry, _) =
            Registry::build(&config, owo_registry::PresetCatalog::builtin(), &["openai-chat"]).unwrap();
        assert_eq!(catalog_models(&registry, crate::DESKTOP_CLIENT_ID)[0].slug, "ds-app");
        let models = catalog_models(&registry, crate::CLIENT_ID);
        assert_eq!(models[0].slug, "ds");
        assert_eq!(models[0].description, "DeepSeek via OwO AI Gateway");
        let catalog = build_catalog(&builtin_template(), &models);
        assert_eq!(catalog["models"][0]["slug"], "ds");
    }
}
