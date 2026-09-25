//! Claude Desktop integration (spec §21), through Desktop's own third-party inference
//! ("gateway") configuration.
//!
//! Desktop keeps named configurations in `<Claude-3p>/configLibrary`: `_meta.json` lists
//! the entries and names the applied one (`appliedId`, a UUID), and each entry is
//! `<id>.json`. OwO AI Gateway adds one entry of its own, applies it, and on restore re-applies
//! whatever was applied before. Desktop then runs in third-party mode against the OwO AI Gateway
//! gateway: no claude.ai sign-in, and nothing about a Claude subscription is claimed.
//! Layout and field names follow Desktop 2.7032 (`inferenceModels`, `maxEffort`, ...).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::Digest as _;

use crate::{now_unix, sha256, write_atomic, Report};

/// Client id used for aliases (`models[].aliases.claude_desktop`) and the `/c/claude_desktop` route.
pub const CLIENT_ID: &str = "claude_desktop";
const TIERS: [&str; 5] = ["opus", "sonnet", "haiku", "fable", "mythos"];
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];
const ONE_MILLION: u32 = 1_000_000;

pub struct DesktopEnvironment {
    pub state_dir: PathBuf,
    pub backups_dir: PathBuf,
    /// Desktop's `configLibrary` directory.
    pub library_dir: PathBuf,
}

impl DesktopEnvironment {
    fn meta_path(&self) -> PathBuf {
        self.library_dir.join("_meta.json")
    }

    fn profile_path(&self, id: &str) -> PathBuf {
        self.library_dir.join(format!("{id}.json"))
    }

    fn state_path(&self) -> PathBuf {
        self.state_dir.join(CLIENT_ID).join("state.json")
    }

    /// Desktop's own settings next to the library (`deploymentMode`, ...).
    fn desktop_config_path(&self) -> Option<PathBuf> {
        self.library_dir.parent().map(|p| p.join("claude_desktop_config.json"))
    }
}

/// Desktop's library location: `$CLAUDE_USER_DATA_DIR/configLibrary`, else
/// `%LOCALAPPDATA%\Claude-3p\configLibrary` on Windows and `<userData>-3p/configLibrary`
/// elsewhere (the same lookup Desktop performs).
pub fn default_library_dir() -> Option<PathBuf> {
    let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(dir) = var("CLAUDE_USER_DATA_DIR") {
        return Some(dir.join("configLibrary"));
    }
    // Windows: %LOCALAPPDATA%; macOS: ~/Library/Application Support; Linux: $XDG_CONFIG_HOME or ~/.config.
    let base = if cfg!(windows) { var("LOCALAPPDATA").or_else(dirs::data_local_dir) } else if cfg!(target_os = "macos") { dirs::data_dir() } else { dirs::config_dir() };
    Some(base?.join("Claude-3p").join("configLibrary"))
}

/// Substrings Desktop treats as "not an Anthropic model" (a superset of its own list,
/// so anything Desktop would drop gets an alias).
const FOREIGN_MARKERS: &[&str] = &[
    "ark-code", "astron", "command-r", "deepseek", "doubao", "gemini", "gemma", "glm", "gpt", "grok", "hermes",
    "hy3", "kimi", "lfm", "ling", "llama", "longcat", "mimo", "minimax", "mistral", "mixtral", "moonshot",
    "nemotron", "openai", "phi", "qianfan", "qwen", "tc-code", "unic", "yi-", "stepfun", "step-3", "seed-",
    "bytedance", "hunyuan", "granite", "nova", "devstral", "ministral", "ernie", "codex", "arcee", "trinity",
    "abab", "k2.", "m2.", "jamba", "arctic", "solar", "mercury", "zamba", "kat-coder", "ds-", "dpsk",
];
const ANTHROPIC_MARKERS: [&str; 7] = ["claude", "sonnet", "opus", "haiku", "fable", "mythos", "anthropic"];
const ALIAS_PREFIX: &str = "claude-owo-";

/// Desktop removes gateway models whose name does not look like an Anthropic model.
fn looks_anthropic(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    ANTHROPIC_MARKERS.iter().any(|m| lower.contains(m)) && !FOREIGN_MARKERS.iter().any(|m| lower.contains(m))
}

/// The name Desktop gets for an OwO AI Gateway model: the id itself when Desktop accepts it, else a
/// stable digits-only alias (`claude-owo-0123456789`), which cannot contain a marker.
pub fn model_name(id: &str) -> String {
    if looks_anthropic(id) {
        return id.to_string();
    }
    let digest = sha2::Sha256::digest(id.as_bytes());
    let n = u64::from_be_bytes(digest[..8].try_into().expect("8 bytes")) % 10_000_000_000;
    format!("{ALIAS_PREFIX}{n:010}")
}

#[derive(Debug, Clone)]
pub struct DesktopModel {
    /// Id Desktop sends back as `model`.
    pub id: String,
    pub display_name: String,
    pub context_window: Option<u32>,
    pub reasoning_efforts: Vec<String>,
}

pub struct DesktopEnableRequest {
    /// Name of OwO AI Gateway's entry in Desktop's configuration library.
    pub name: String,
    /// Gateway root for Desktop, e.g. `http://127.0.0.1:8787/c/claude_desktop`.
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<DesktopModel>,
    /// Model Desktop selects by default (the first `inferenceModels` entry).
    pub default_model: Option<String>,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DesktopState {
    pub library_dir: PathBuf,
    pub profile_id: String,
    /// The entry that was applied before OwO AI Gateway's, re-applied on restore.
    pub previous_applied_id: Option<String>,
    pub meta_existed: bool,
    pub meta_backup: Option<PathBuf>,
    pub profile_sha256: String,
    pub owo_version: String,
    pub updated_at_unix: u64,
}

fn load_state(env: &DesktopEnvironment) -> Result<Option<DesktopState>> {
    match std::fs::read_to_string(env.state_path()) {
        Ok(t) => Ok(Some(serde_json::from_str(&t).context("Claude Desktop integration state is corrupt")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn read_meta(path: &Path) -> Result<Option<Map<String, Value>>> {
    let text = match std::fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let meta: Value = serde_json::from_str(&text).with_context(|| format!("{} is not valid JSON", path.display()))?;
    match meta {
        Value::Object(m) if m.get("entries").is_some_and(Value::is_array) => Ok(Some(m)),
        _ => bail!("{} is not a configuration library index (no `entries` array)", path.display()),
    }
}

fn entries(meta: &mut Map<String, Value>) -> &mut Vec<Value> {
    meta.entry("entries").or_insert_with(|| Value::Array(Vec::new()));
    meta.get_mut("entries").and_then(Value::as_array_mut).expect("entries is an array")
}

fn entry_id(entry: &Value) -> Option<&str> {
    entry.get("id").and_then(Value::as_str)
}

fn is_uuid(id: &str) -> bool {
    id.len() == 36 && id.chars().all(|c| c.is_ascii_digit() || matches!(c, 'a'..='f' | '-'))
}

/// `inferenceModels`: the default model first; each Claude tier pinned to the first model
/// of that family; `maxEffort` from the model's declared efforts; 1M rows only for models
/// with an authoritative context window of at least 1M.
pub fn inference_models(models: &[DesktopModel], default_model: Option<&str>) -> Vec<Value> {
    let mut ordered: Vec<&DesktopModel> = models.iter().collect();
    if let Some(pos) = default_model.and_then(|d| ordered.iter().position(|m| m.id == d)) {
        let first = ordered.remove(pos);
        ordered.insert(0, first);
    }
    let mut pinned = Vec::new();
    ordered
        .into_iter()
        .map(|m| {
            let mut entry = json!({ "name": model_name(&m.id), "labelOverride": m.display_name });
            let lower = m.id.to_ascii_lowercase();
            if let Some(tier) = TIERS.iter().find(|t| lower.contains(**t)) {
                entry["anthropicFamilyTier"] = json!(tier);
                if !pinned.contains(tier) {
                    pinned.push(*tier);
                    entry["isFamilyDefault"] = json!(true);
                }
            }
            if let Some(max) = EFFORTS.iter().rev().find(|e| m.reasoning_efforts.iter().any(|x| x == *e)) {
                entry["maxEffort"] = json!(max);
            }
            if m.context_window.is_some_and(|c| c >= ONE_MILLION) {
                entry["supports1m"] = json!(true);
            }
            entry
        })
        .collect()
}

fn render_profile(req: &DesktopEnableRequest) -> Result<Vec<u8>> {
    let profile = json!({
        "inferenceProvider": "gateway",
        "inferenceCredentialKind": "static",
        "inferenceGatewayBaseUrl": req.base_url,
        "inferenceGatewayApiKey": req.api_key,
        "modelDiscoveryEnabled": false,
        "inferenceModels": inference_models(&req.models, req.default_model.as_deref()),
    });
    let mut text = serde_json::to_string_pretty(&profile)?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn render_meta(meta: &Map<String, Value>) -> Result<Vec<u8>> {
    let mut text = serde_json::to_string_pretty(meta)?;
    text.push('\n');
    Ok(text.into_bytes())
}

pub fn enable(env: &DesktopEnvironment, req: &DesktopEnableRequest) -> Result<Report> {
    let mut report = Report::default();
    if req.models.is_empty() {
        bail!("no models are available to expose to Claude Desktop; add models to OwO AI Gateway's config.toml first");
    }
    if let Some(d) = &req.default_model {
        if !req.models.iter().any(|m| &m.id == d) {
            let known: Vec<_> = req.models.iter().map(|m| m.id.as_str()).collect();
            bail!("model `{d}` is not available (available: {})", known.join(", "));
        }
    }
    let prior = load_state(env)?;
    if let Some(p) = &prior {
        if p.library_dir != env.library_dir {
            bail!("the Claude Desktop integration is already enabled for {}; run `owo disconnect claude-desktop` first", p.library_dir.display());
        }
    }

    let meta_path = env.meta_path();
    let existing = read_meta(&meta_path)?;
    let meta_existed = prior.as_ref().map_or(existing.is_some(), |p| p.meta_existed);
    let mut meta = existing.unwrap_or_default();

    let reuse = prior.as_ref().map(|p| p.profile_id.clone()).or_else(|| {
        meta.get("entries")?.as_array()?.iter().find(|e| e.get("name").and_then(Value::as_str) == Some(req.name.as_str())).and_then(entry_id).map(str::to_string)
    });
    let id = reuse.filter(|id| is_uuid(id)).unwrap_or_else(|| uuid::Uuid::new_v4().hyphenated().to_string());
    let profile_path = env.profile_path(&id);

    if let (Some(p), Ok(current)) = (&prior, std::fs::read(&profile_path)) {
        if sha256(&current) != p.profile_sha256 && !req.force {
            bail!("{} was edited after OwO AI Gateway wrote it (for example in Desktop's settings); re-run with --force to overwrite it", profile_path.display());
        }
    }

    let previous_applied_id = match &prior {
        Some(p) => p.previous_applied_id.clone(),
        None => meta.get("appliedId").and_then(Value::as_str).filter(|a| *a != id).map(str::to_string),
    };
    let meta_backup = match &prior {
        Some(p) => p.meta_backup.clone(),
        None if meta_existed => {
            let dir = env.backups_dir.join(CLIENT_ID);
            std::fs::create_dir_all(&dir)?;
            let dest = dir.join(format!("{}-_meta.json", now_unix()));
            std::fs::copy(&meta_path, &dest).with_context(|| format!("cannot back up {}", meta_path.display()))?;
            report.lines.push(format!("backup:   {}", dest.display()));
            Some(dest)
        }
        None => None,
    };

    let profile = render_profile(req)?;
    write_atomic(&profile_path, &profile)?;
    let list = entries(&mut meta);
    match list.iter_mut().find(|e| entry_id(e) == Some(id.as_str())) {
        Some(e) => e["name"] = json!(req.name),
        None => list.push(json!({ "id": id, "name": req.name })),
    }
    meta.insert("appliedId".into(), json!(id));
    write_atomic(&meta_path, &render_meta(&meta)?)?;

    report.lines.push(format!("profile:  {} (entry `{}`, applied)", profile_path.display(), req.name));
    report.lines.push(format!("gateway:  {}", req.base_url));
    for m in inference_models(&req.models, req.default_model.as_deref()) {
        let tier = m.get("anthropicFamilyTier").and_then(Value::as_str).map(|t| format!(" [{t}]")).unwrap_or_default();
        let name = m["name"].as_str().unwrap_or_default();
        let label = m["labelOverride"].as_str().unwrap_or_default();
        report.lines.push(format!("model:    {name}{tier}  ({label})"));
    }
    if let Some(prev) = &previous_applied_id {
        report.lines.push(format!("previous: entry {prev} is re-applied by restore"));
    }
    if let Some(path) = env.desktop_config_path() {
        let mode = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()).and_then(|v| v.get("deploymentMode").and_then(Value::as_str).map(str::to_string));
        if mode.as_deref() == Some("1p") {
            report.warnings.push("Desktop was switched to claude.ai sign-in (deploymentMode = 1p); choose the third-party / gateway option in Desktop to use OwO AI Gateway".into());
        }
    }

    let state = DesktopState {
        library_dir: env.library_dir.clone(),
        profile_id: id,
        previous_applied_id,
        meta_existed,
        meta_backup,
        profile_sha256: sha256(&profile),
        owo_version: env!("CARGO_PKG_VERSION").to_string(),
        updated_at_unix: now_unix(),
    };
    write_atomic(&env.state_path(), serde_json::to_string_pretty(&state)?.as_bytes())?;
    Ok(report)
}

pub fn restore(env: &DesktopEnvironment, force: bool) -> Result<Report> {
    let mut report = Report::default();
    let Some(st) = load_state(env)? else { bail!("the Claude Desktop integration is not enabled") };
    let profile_path = env.profile_path(&st.profile_id);
    if let Ok(current) = std::fs::read(&profile_path) {
        if sha256(&current) != st.profile_sha256 && !force {
            bail!("{} was edited after OwO AI Gateway wrote it; nothing was restored (re-run with --force)", profile_path.display());
        }
    }

    let meta_path = env.meta_path();
    if let Some(mut meta) = read_meta(&meta_path)? {
        entries(&mut meta).retain(|e| entry_id(e) != Some(st.profile_id.as_str()));
        if meta.get("appliedId").and_then(Value::as_str) == Some(st.profile_id.as_str()) {
            let previous = st.previous_applied_id.as_deref().filter(|p| {
                meta.get("entries").and_then(Value::as_array).is_some_and(|l| l.iter().any(|e| entry_id(e) == Some(*p)))
            });
            match previous {
                Some(p) => {
                    meta.insert("appliedId".into(), json!(p));
                    report.lines.push(format!("applied:  entry {p} (applied before OwO AI Gateway)"));
                }
                None => {
                    meta.remove("appliedId");
                }
            }
        }
        let only_ours = !st.meta_existed
            && meta.get("entries").and_then(Value::as_array).is_some_and(Vec::is_empty)
            && meta.keys().all(|k| k == "entries");
        if only_ours {
            std::fs::remove_file(&meta_path)?;
        } else {
            write_atomic(&meta_path, &render_meta(&meta)?)?;
        }
        report.lines.push(format!("library:  {} — removed entry {}", meta_path.display(), st.profile_id));
    }
    match std::fs::remove_file(&profile_path) {
        Ok(()) => report.lines.push(format!("profile:  {} removed", profile_path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e).with_context(|| format!("cannot remove {}", profile_path.display())),
    }
    std::fs::remove_file(env.state_path())?;
    report.lines.push("Claude Desktop integration removed. Quit and reopen Claude Desktop.".into());
    Ok(report)
}

pub fn status(env: &DesktopEnvironment) -> Result<Option<DesktopState>> {
    load_state(env)
}

/// The name Desktop shows for OwO AI Gateway's entry.
pub fn entry_name(env: &DesktopEnvironment, st: &DesktopState) -> Option<String> {
    let meta = read_meta(&env.meta_path()).ok().flatten()?;
    let entry = meta.get("entries")?.as_array()?.iter().find(|e| entry_id(e) == Some(st.profile_id.as_str()))?;
    entry.get("name").and_then(Value::as_str).map(str::to_string)
}

/// Whether Desktop currently applies OwO AI Gateway's entry.
pub fn applied(env: &DesktopEnvironment, st: &DesktopState) -> bool {
    read_meta(&env.meta_path()).ok().flatten().and_then(|m| m.get("appliedId").and_then(Value::as_str).map(str::to_string)).as_deref()
        == Some(st.profile_id.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_env(name: &str) -> DesktopEnvironment {
        let root = std::env::temp_dir().join(format!("owo-desktop-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Claude-3p/configLibrary")).unwrap();
        DesktopEnvironment { state_dir: root.join("state"), backups_dir: root.join("backups"), library_dir: root.join("Claude-3p/configLibrary") }
    }

    fn model(id: &str, efforts: &[&str], ctx: Option<u32>) -> DesktopModel {
        DesktopModel { id: id.into(), display_name: id.to_uppercase(), context_window: ctx, reasoning_efforts: efforts.iter().map(|s| s.to_string()).collect() }
    }

    fn request() -> DesktopEnableRequest {
        DesktopEnableRequest {
            name: "OwO".into(),
            base_url: "http://127.0.0.1:8787/c/claude_desktop".into(),
            api_key: "owo-local".into(),
            models: vec![
                model("claude-opus-5-5", &["low", "medium", "high", "xhigh", "max"], Some(1_000_000)),
                model("claude-sonnet-5", &["low", "high"], Some(200_000)),
                model("deepseek-chat", &[], None),
            ],
            default_model: Some("claude-sonnet-5".into()),
            force: false,
        }
    }

    fn read_json(path: &Path) -> Value {
        serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    #[test]
    fn model_entries() {
        let r = request();
        let m = inference_models(&r.models, r.default_model.as_deref());
        assert_eq!(m[0]["name"], "claude-sonnet-5", "default first");
        assert_eq!(m[0]["anthropicFamilyTier"], "sonnet");
        assert_eq!(m[0]["maxEffort"], "high");
        assert!(m[0].get("supports1m").is_none());
        assert_eq!(m[1]["name"], "claude-opus-5-5");
        assert_eq!(m[1]["maxEffort"], "max");
        assert_eq!(m[1]["supports1m"], true);
        assert_eq!(m[1]["isFamilyDefault"], true);
        let alias = m[2]["name"].as_str().unwrap();
        assert!(alias.starts_with("claude-owo-") && alias.len() == 21, "{alias}");
        assert_eq!(m[2]["labelOverride"], "DEEPSEEK-CHAT");
    }

    #[test]
    fn names_desktop_accepts() {
        assert_eq!(model_name("claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(model_name("anthropic/claude-opus-5-5"), "anthropic/claude-opus-5-5");
        for foreign in ["deepseek-chat", "gpt-5.6-sol", "claude-owo--qwen3", "kimi-k3"] {
            let name = model_name(foreign);
            assert_ne!(name, foreign);
            assert!(looks_anthropic(&name), "{name}");
            assert_eq!(name, model_name(foreign), "stable");
        }
        assert_ne!(model_name("deepseek-chat"), model_name("deepseek-reasoner"));
    }

    const USER_META: &str = "{\n  \"appliedId\": \"11111111-2222-3333-4444-555555555555\",\n  \"entries\": [\n    {\n      \"id\": \"11111111-2222-3333-4444-555555555555\",\n      \"name\": \"Work gateway\"\n    }\n  ],\n  \"isManaged\": false\n}\n";

    #[test]
    fn enable_and_restore_reapply_the_previous_entry() {
        let env = temp_env("previous");
        std::fs::write(env.meta_path(), USER_META).unwrap();
        let report = enable(&env, &request()).unwrap();
        assert!(report.lines.iter().any(|l| l.starts_with("previous:")));
        let meta = read_json(&env.meta_path());
        let id = meta["appliedId"].as_str().unwrap().to_string();
        assert!(is_uuid(&id));
        assert_eq!(meta["entries"].as_array().unwrap().len(), 2);
        assert_eq!(meta["isManaged"], false, "other keys kept");
        let profile = read_json(&env.profile_path(&id));
        assert_eq!(profile["inferenceProvider"], "gateway");
        assert_eq!(profile["inferenceCredentialKind"], "static");
        assert_eq!(profile["inferenceGatewayBaseUrl"], "http://127.0.0.1:8787/c/claude_desktop");
        assert_eq!(profile["modelDiscoveryEnabled"], false);

        let st = status(&env).unwrap().unwrap();
        assert_eq!(entry_name(&env, &st).as_deref(), Some("OwO"));

        // Re-enable reuses the entry (renaming it) and keeps the original previous entry.
        enable(&env, &DesktopEnableRequest { name: "Team AI".into(), ..request() }).unwrap();
        assert_eq!(read_json(&env.meta_path())["appliedId"], id.as_str());
        assert_eq!(entry_name(&env, &st).as_deref(), Some("Team AI"));

        restore(&env, false).unwrap();
        assert_eq!(std::fs::read_to_string(env.meta_path()).unwrap(), USER_META);
        assert!(!env.profile_path(&id).exists());
        assert!(status(&env).unwrap().is_none());
    }

    #[test]
    fn fresh_library_is_left_empty() {
        let env = temp_env("fresh");
        enable(&env, &request()).unwrap();
        let st = status(&env).unwrap().unwrap();
        assert!(applied(&env, &st));
        restore(&env, false).unwrap();
        assert!(!env.meta_path().exists());
        assert_eq!(std::fs::read_dir(&env.library_dir).unwrap().count(), 0);
    }

    #[test]
    fn user_switching_entries_is_respected() {
        let env = temp_env("switched");
        std::fs::write(env.meta_path(), USER_META).unwrap();
        enable(&env, &request()).unwrap();
        let mut meta = read_json(&env.meta_path());
        meta["appliedId"] = json!("11111111-2222-3333-4444-555555555555");
        std::fs::write(env.meta_path(), serde_json::to_string(&meta).unwrap()).unwrap();
        let st = status(&env).unwrap().unwrap();
        assert!(!applied(&env, &st));
        restore(&env, false).unwrap();
        let after = read_json(&env.meta_path());
        assert_eq!(after["appliedId"], "11111111-2222-3333-4444-555555555555");
        assert_eq!(after["entries"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn edited_profile_needs_force() {
        let env = temp_env("edited");
        enable(&env, &request()).unwrap();
        let st = status(&env).unwrap().unwrap();
        std::fs::write(env.profile_path(&st.profile_id), "{\"inferenceProvider\":\"gateway\"}").unwrap();
        assert!(restore(&env, false).is_err());
        assert!(enable(&env, &request()).is_err());
        restore(&env, true).unwrap();
        assert!(!env.meta_path().exists());
    }

    #[test]
    fn rejects_a_foreign_index_and_unknown_default() {
        let env = temp_env("foreign");
        std::fs::write(env.meta_path(), "{\"appliedId\": 3}").unwrap();
        assert!(enable(&env, &request()).unwrap_err().to_string().contains("entries"));
        std::fs::remove_file(env.meta_path()).unwrap();
        let mut req = request();
        req.default_model = Some("nope".into());
        assert!(enable(&env, &req).unwrap_err().to_string().contains("not available"));
    }
}
