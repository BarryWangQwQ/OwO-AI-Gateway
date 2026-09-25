//! Claude Code integration (spec §20).
//!
//! Claude Code reads `env` from `<claude_dir>/settings.json` for every session (CLI and
//! IDE extensions alike). OwO AI Gateway owns a fixed set of keys there: it points
//! `ANTHROPIC_BASE_URL` at the gateway, supplies the gateway token as
//! `ANTHROPIC_AUTH_TOKEN` (so no Claude account login is needed), and maps Claude
//! Code's model tiers onto OwO AI Gateway models. The user's previous values of those keys are
//! recorded and put back by `restore`; every other setting is left alone.

pub mod desktop;
pub mod ids;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

/// Client id used for aliases (`models[].aliases.claude_code`) and the `/c/claude_code` route.
pub const CLIENT_ID: &str = "claude_code";

pub const BASE_URL: &str = "ANTHROPIC_BASE_URL";
pub const AUTH_TOKEN: &str = "ANTHROPIC_AUTH_TOKEN";
pub const MODEL: &str = "ANTHROPIC_MODEL";
pub const DISCOVERY: &str = "CLAUDE_CODE_ENABLE_GATEWAY_MODEL_DISCOVERY";
const TIER_KEYS: [(&str, &str); 4] = [
    ("opus", "ANTHROPIC_DEFAULT_OPUS_MODEL"),
    ("sonnet", "ANTHROPIC_DEFAULT_SONNET_MODEL"),
    ("haiku", "ANTHROPIC_DEFAULT_HAIKU_MODEL"),
    ("fable", "ANTHROPIC_DEFAULT_FABLE_MODEL"),
];

pub struct Environment {
    pub state_dir: PathBuf,
    pub backups_dir: PathBuf,
    /// `CLAUDE_CONFIG_DIR`, or `~/.claude`.
    pub claude_dir: PathBuf,
}

impl Environment {
    pub fn settings_path(&self) -> PathBuf {
        self.claude_dir.join("settings.json")
    }

    fn state_path(&self) -> PathBuf {
        self.state_dir.join(CLIENT_ID).join("state.json")
    }

    fn backups(&self) -> PathBuf {
        self.backups_dir.join(CLIENT_ID)
    }
}

/// Claude Code's config directory: `CLAUDE_CONFIG_DIR`, else `~/.claude`.
pub fn default_claude_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CLAUDE_CONFIG_DIR").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    dirs::home_dir().map(|h| h.join(".claude"))
}

pub struct EnableRequest {
    /// Gateway root for Claude Code, e.g. `http://127.0.0.1:8787/c/claude_code`.
    pub base_url: String,
    /// Gateway access token, or a placeholder when the gateway has none.
    pub auth_token: String,
    /// Model to start with (`ANTHROPIC_MODEL`); `None` lets Claude Code pick its tier default.
    pub model: Option<String>,
    /// OwO AI Gateway model ids available to Claude Code, in config order.
    pub models: Vec<String>,
    pub force: bool,
}

#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeCodeState {
    pub settings_path: PathBuf,
    pub owo_version: String,
    pub updated_at_unix: u64,
    /// Values OwO AI Gateway wrote, by key.
    pub written: BTreeMap<String, String>,
    /// The user's values before OwO AI Gateway took the key over (`None` = absent).
    pub originals: BTreeMap<String, Option<Value>>,
    /// `settings.json` did not exist before OwO AI Gateway created it.
    pub created_file: bool,
    /// The file had no `env` object before OwO AI Gateway added one.
    pub created_env: bool,
    pub written_sha256: String,
    pub backup_path: Option<PathBuf>,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

pub(crate) fn now_unix() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    let tmp = path.with_file_name(format!(".{name}.owo-{}.tmp", std::process::id()));
    std::fs::write(&tmp, contents).with_context(|| format!("cannot write {}", tmp.display()))?;
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("cannot replace {}", path.display()));
    }
    Ok(())
}

fn backup(path: &Path, dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let dest = dir.join(format!("{}-settings.json", now_unix()));
    std::fs::copy(path, &dest).with_context(|| format!("cannot back up {}", path.display()))?;
    Ok(dest)
}

fn load_state(env: &Environment) -> Result<Option<ClaudeCodeState>> {
    match std::fs::read_to_string(env.state_path()) {
        Ok(t) => Ok(Some(serde_json::from_str(&t).context("Claude Code integration state is corrupt")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// The file's bytes and its parsed object.
type Settings = (Vec<u8>, Map<String, Value>);

fn read_settings(path: &Path) -> Result<Option<Settings>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", path.display())),
    };
    let map = if bytes.iter().all(u8::is_ascii_whitespace) {
        Map::new()
    } else {
        match serde_json::from_slice::<Value>(&bytes)
            .with_context(|| format!("{} is not valid JSON; fix it (or move it aside) and re-run", path.display()))?
        {
            Value::Object(m) => m,
            _ => bail!("{} is not a JSON object", path.display()),
        }
    };
    Ok(Some((bytes, map)))
}

fn render(settings: &Map<String, Value>) -> Result<Vec<u8>> {
    let mut text = serde_json::to_string_pretty(settings)?;
    text.push('\n');
    Ok(text.into_bytes())
}

/// The tier mapping: each Claude tier goes to the first OwO AI Gateway model of that family,
/// else to the start model.
pub fn tier_models(models: &[String], fallback: &str) -> Vec<(&'static str, String)> {
    TIER_KEYS
        .iter()
        .map(|(tier, key)| {
            let hit = models.iter().find(|m| m.to_ascii_lowercase().contains(tier));
            (*key, ids::picker_id(hit.map_or(fallback, String::as_str)))
        })
        .collect()
}

fn desired(req: &EnableRequest) -> Result<BTreeMap<String, String>> {
    let Some(first) = req.models.first() else {
        bail!("no models are available to expose to Claude Code; add models to OwO AI Gateway's config.toml first");
    };
    if let Some(m) = &req.model {
        if !req.models.contains(m) {
            bail!("model `{m}` is not available (available: {})", req.models.join(", "));
        }
    }
    let start = req.model.as_deref().unwrap_or(first);
    let mut out = BTreeMap::new();
    out.insert(BASE_URL.to_string(), req.base_url.clone());
    out.insert(AUTH_TOKEN.to_string(), req.auth_token.clone());
    out.insert(DISCOVERY.to_string(), "1".to_string());
    if let Some(m) = &req.model {
        out.insert(MODEL.to_string(), ids::picker_id(m));
    }
    for (key, model) in tier_models(&req.models, start) {
        out.insert(key.to_string(), model);
    }
    Ok(out)
}

/// The environment `enable` would write into settings.json, for a one-off launch instead.
pub fn launch_env(req: &EnableRequest) -> Result<BTreeMap<String, String>> {
    desired(req)
}

pub fn enable(env: &Environment, req: &EnableRequest) -> Result<Report> {
    let mut report = Report::default();
    let want = desired(req)?;
    let path = env.settings_path();
    let prior = load_state(env)?;
    if let Some(p) = &prior {
        if p.settings_path != path {
            bail!("the Claude Code integration is already enabled for {}; run `owo disconnect claude` first", p.settings_path.display());
        }
    }

    let current = read_settings(&path)?;
    let (bytes, mut settings) = match current {
        Some((b, m)) => (Some(b), m),
        None => (None, Map::new()),
    };
    if let Some(p) = &prior {
        if bytes.as_deref().map(sha256).as_deref() != Some(p.written_sha256.as_str()) {
            let changed: Vec<&String> = p
                .written
                .iter()
                .filter(|(k, v)| settings.get("env").and_then(|e| e.get(k.as_str())).and_then(Value::as_str) != Some(v.as_str()))
                .map(|(k, _)| k)
                .collect();
            if !changed.is_empty() && !req.force {
                bail!("these settings changed since OwO AI Gateway wrote them: {} (re-run with --force to take them over again)",
                    changed.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
            }
        }
    }

    let created_env = match &prior {
        Some(p) => p.created_env,
        None => !settings.contains_key("env"),
    };
    let env_obj = settings.entry("env").or_insert_with(|| Value::Object(Map::new()));
    let Value::Object(env_map) = env_obj else { bail!("`env` in {} is not an object", path.display()) };

    let mut originals = prior.as_ref().map(|p| p.originals.clone()).unwrap_or_default();
    // Keys OwO AI Gateway wrote before but no longer wants go back to the user's value.
    if let Some(p) = &prior {
        for key in p.written.keys().filter(|k| !want.contains_key(*k)) {
            match originals.remove(key).flatten() {
                Some(v) => env_map.insert(key.clone(), v),
                None => env_map.remove(key),
            };
        }
    }
    for (key, value) in &want {
        if !originals.contains_key(key) {
            let old = env_map.get(key).cloned();
            if let Some(Value::String(s)) = &old {
                if s != value && matches!(key.as_str(), BASE_URL | AUTH_TOKEN) {
                    report.warnings.push(format!("replaced your own {key} (restored by `owo disconnect claude`)"));
                }
            }
            originals.insert(key.clone(), old);
        }
        env_map.insert(key.clone(), Value::String(value.clone()));
    }
    if env_map.contains_key("ANTHROPIC_API_KEY") {
        report.warnings.push("settings.json also sets ANTHROPIC_API_KEY; Claude Code may prefer it over OwO AI Gateway's token".into());
    }

    let backup_path = match (&prior, &bytes) {
        (Some(p), _) => p.backup_path.clone(),
        (None, Some(_)) => {
            let b = backup(&path, &env.backups())?;
            report.lines.push(format!("backup:   {}", b.display()));
            Some(b)
        }
        (None, None) => None,
    };
    let text = render(&settings)?;
    write_atomic(&path, &text)?;
    report.lines.push(format!("settings: {} (env: {})", path.display(), want.keys().cloned().collect::<Vec<_>>().join(", ")));
    report.lines.push(format!("gateway:  {}", req.base_url));
    for (_, key) in TIER_KEYS {
        if let Some(m) = want.get(key) {
            report.lines.push(format!("tier:     {key} = {m}"));
        }
    }

    let state = ClaudeCodeState {
        settings_path: path,
        owo_version: env!("CARGO_PKG_VERSION").to_string(),
        updated_at_unix: now_unix(),
        written: want,
        originals,
        created_file: prior.as_ref().map_or(bytes.is_none(), |p| p.created_file),
        created_env,
        written_sha256: sha256(&text),
        backup_path,
    };
    write_atomic(&env.state_path(), serde_json::to_string_pretty(&state)?.as_bytes())?;
    Ok(report)
}

/// Undoes `enable`. Writes nothing when an OwO AI Gateway key was changed by someone else, unless `force`.
pub fn restore(env: &Environment, force: bool) -> Result<Report> {
    let mut report = Report::default();
    let Some(st) = load_state(env)? else { bail!("the Claude Code integration is not enabled") };
    let current = read_settings(&st.settings_path)?;
    let untouched = current.as_ref().is_some_and(|(b, _)| sha256(b) == st.written_sha256);

    if untouched {
        match (&st.backup_path, st.created_file) {
            (Some(b), _) => {
                let bytes = std::fs::read(b).with_context(|| format!("backup {} is missing", b.display()))?;
                write_atomic(&st.settings_path, &bytes)?;
                report.lines.push(format!("settings: {} restored byte-for-byte from backup", st.settings_path.display()));
            }
            (None, true) => {
                std::fs::remove_file(&st.settings_path).with_context(|| format!("cannot remove {}", st.settings_path.display()))?;
                report.lines.push(format!("settings: {} removed (OwO AI Gateway created it)", st.settings_path.display()));
            }
            (None, false) => bail!("state has no backup for {}", st.settings_path.display()),
        }
    } else if let Some((_, mut settings)) = current {
        let mut conflicts = Vec::new();
        if let Some(Value::Object(env_map)) = settings.get_mut("env") {
            for (key, written) in &st.written {
                let now = env_map.get(key).and_then(Value::as_str);
                if now != Some(written.as_str()) {
                    conflicts.push(key.clone());
                    if !force {
                        continue;
                    }
                }
                match st.originals.get(key).cloned().flatten() {
                    Some(v) => env_map.insert(key.clone(), v),
                    None => env_map.remove(key),
                };
            }
        }
        if !conflicts.is_empty() && !force {
            bail!(
                "these settings changed after OwO AI Gateway wrote them; nothing was restored: {}\nResolve them by hand or re-run with --force.",
                conflicts.join(", ")
            );
        }
        if st.created_env && settings.get("env").is_some_and(|e| e.as_object().is_some_and(Map::is_empty)) {
            settings.remove("env");
        }
        if st.created_file && settings.is_empty() {
            std::fs::remove_file(&st.settings_path)?;
            report.lines.push(format!("settings: {} removed (OwO AI Gateway created it)", st.settings_path.display()));
        } else {
            write_atomic(&st.settings_path, &render(&settings)?)?;
            report.lines.push(format!("settings: {} — put back only OwO AI Gateway's keys (other edits kept)", st.settings_path.display()));
        }
    } else {
        report.warnings.push(format!("{} no longer exists; nothing to restore", st.settings_path.display()));
    }
    std::fs::remove_file(env.state_path())?;
    report.lines.push("Claude Code integration removed.".into());
    Ok(report)
}

pub fn status(env: &Environment) -> Result<Option<ClaudeCodeState>> {
    load_state(env)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_env(name: &str) -> Environment {
        let root = std::env::temp_dir().join(format!("owo-claude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("claude")).unwrap();
        Environment { state_dir: root.join("state"), backups_dir: root.join("backups"), claude_dir: root.join("claude") }
    }

    fn request() -> EnableRequest {
        EnableRequest {
            base_url: "http://127.0.0.1:8787/c/claude_code".into(),
            auth_token: "owo-local".into(),
            model: None,
            models: vec!["claude-sonnet-5".into(), "claude-opus-5-5".into(), "deepseek-chat".into()],
            force: false,
        }
    }

    fn read(env: &Environment) -> Value {
        serde_json::from_str(&std::fs::read_to_string(env.settings_path()).unwrap()).unwrap()
    }

    const USER: &str = "{\n  \"permissions\": {\"allow\": [\"Bash(ls)\"]},\n  \"env\": {\"ANTHROPIC_BASE_URL\": \"https://relay.example\", \"FOO\": \"1\"},\n  \"theme\": \"dark\"\n}\n";

    #[test]
    fn enable_and_byte_exact_restore() {
        let env = temp_env("exact");
        std::fs::write(env.settings_path(), USER).unwrap();
        let report = enable(&env, &request()).unwrap();
        assert!(report.warnings.iter().any(|w| w.contains("ANTHROPIC_BASE_URL")));
        let s = read(&env);
        assert_eq!(s["env"]["ANTHROPIC_BASE_URL"], "http://127.0.0.1:8787/c/claude_code");
        assert_eq!(s["env"]["ANTHROPIC_AUTH_TOKEN"], "owo-local");
        assert_eq!(s["env"]["ANTHROPIC_DEFAULT_OPUS_MODEL"], "claude-opus-5-5");
        assert_eq!(s["env"]["ANTHROPIC_DEFAULT_HAIKU_MODEL"], "claude-sonnet-5", "no haiku model: start model");
        assert!(s["env"].get("ANTHROPIC_MODEL").is_none());
        assert_eq!(s["env"]["FOO"], "1");
        assert_eq!(s["theme"], "dark");

        // Re-enabling keeps the user's originals.
        enable(&env, &request()).unwrap();
        restore(&env, false).unwrap();
        assert_eq!(std::fs::read_to_string(env.settings_path()).unwrap(), USER);
        assert!(status(&env).unwrap().is_none());
    }

    #[test]
    fn restore_keeps_later_user_edits() {
        let env = temp_env("edits");
        std::fs::write(env.settings_path(), USER).unwrap();
        let mut req = request();
        req.model = Some("deepseek-chat".into());
        enable(&env, &req).unwrap();
        assert_eq!(read(&env)["env"]["ANTHROPIC_MODEL"], "claude-owo--deepseek-chat");

        let mut s = read(&env);
        s["model"] = Value::String("opus".into());
        s["env"]["BAR"] = Value::String("2".into());
        std::fs::write(env.settings_path(), serde_json::to_string_pretty(&s).unwrap()).unwrap();

        restore(&env, false).unwrap();
        let after = read(&env);
        assert_eq!(after["env"]["ANTHROPIC_BASE_URL"], "https://relay.example");
        assert_eq!(after["env"]["BAR"], "2");
        assert_eq!(after["model"], "opus");
        for key in [AUTH_TOKEN, MODEL, DISCOVERY, "ANTHROPIC_DEFAULT_OPUS_MODEL"] {
            assert!(after["env"].get(key).is_none(), "{key}");
        }
    }

    #[test]
    fn conflicting_edit_blocks_restore_without_force() {
        let env = temp_env("conflict");
        enable(&env, &request()).unwrap();
        let mut s = read(&env);
        s["env"]["ANTHROPIC_BASE_URL"] = Value::String("http://elsewhere".into());
        std::fs::write(env.settings_path(), serde_json::to_string(&s).unwrap()).unwrap();
        assert!(restore(&env, false).unwrap_err().to_string().contains("ANTHROPIC_BASE_URL"));
        assert!(status(&env).unwrap().is_some(), "nothing restored");
        restore(&env, true).unwrap();
        assert!(!env.settings_path().exists(), "OwO AI Gateway created the file and nothing else is left");
    }

    #[test]
    fn created_file_is_removed_on_restore() {
        let env = temp_env("created");
        enable(&env, &request()).unwrap();
        assert!(env.settings_path().is_file());
        restore(&env, false).unwrap();
        assert!(!env.settings_path().exists());
    }

    #[test]
    fn rejects_invalid_settings_and_unknown_model() {
        let env = temp_env("invalid");
        std::fs::write(env.settings_path(), "{ // comment\n}").unwrap();
        assert!(enable(&env, &request()).unwrap_err().to_string().contains("not valid JSON"));
        std::fs::remove_file(env.settings_path()).unwrap();
        let mut req = request();
        req.model = Some("nope".into());
        assert!(enable(&env, &req).unwrap_err().to_string().contains("not available"));
    }
}
