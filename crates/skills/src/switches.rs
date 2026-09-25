//! The per-skill "off" settings of apps that read `~/.agents/skills` themselves:
//!
//! - Codex: `[[skills.config]] path = ".../SKILL.md"`, `enabled = false` in `config.toml`
//! - OpenCode: `permission.skill.<name> = "deny"` in `opencode.json`
//! - Grok Build: `[skills] disabled = ["<name>"]` in `config.toml`
//! - GitHub Copilot: `disabledSkills: ["<name>"]` in `settings.json`
//!
//! OwO AI Gateway adds or removes exactly one entry and leaves the rest of the file as it
//! is. It does not use the managed text blocks of `owo connect`: these files are also
//! edited by those integrations, and a skill switch must survive their restore (and the
//! other way round). Every write is preceded by a backup of the file.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value as Json};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, Item, Table, Value};

use crate::layout::{AppId, Layout};
use crate::tree::SKILL_FILE;

/// What an app's own settings say about one skill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setting {
    /// Nothing about this skill: on (the default).
    Unset,
    Off,
    /// An explicit per-skill entry that turns it on (Codex `enabled = true`, an OpenCode
    /// `allow`/`ask`); OwO AI Gateway does not override it.
    On,
}

/// Backups kept per app besides the first one (`original-*`).
const KEEP_BACKUPS: usize = 10;

fn read_text(path: &Path) -> Result<Option<String>> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(String::from_utf8(b).with_context(|| format!("{} is not UTF-8", path.display()))?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

// ---------------------------------------------------------------------------
// Codex

pub fn codex_skill_path(layout: &Layout, name: &str) -> String {
    layout.agents.join(name).join(SKILL_FILE).display().to_string()
}

/// Paths compare without `\\?\`, trailing separators, separator style, or (on Windows) case.
fn norm(p: &str) -> String {
    let mut s = p.replace('\\', "/");
    for prefix in ["//?/", "//./"] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
        }
    }
    let s = s.trim_end_matches('/').to_string();
    if cfg!(windows) { s.to_lowercase() } else { s }
}

fn codex_entries(doc: &DocumentMut) -> Vec<(String, Option<bool>)> {
    let Some(config) = doc.get("skills").and_then(|s| s.get("config")) else { return Vec::new() };
    let entry = |path: Option<&str>, enabled: Option<bool>| path.map(|p| (p.to_string(), enabled));
    match config {
        Item::ArrayOfTables(list) => list.iter().filter_map(|t| entry(t.get("path").and_then(Item::as_str), t.get("enabled").and_then(Item::as_bool))).collect(),
        Item::Value(Value::Array(list)) => list
            .iter()
            .filter_map(Value::as_inline_table)
            .filter_map(|t| entry(t.get("path").and_then(Value::as_str), t.get("enabled").and_then(Value::as_bool)))
            .collect(),
        _ => Vec::new(),
    }
}

fn codex_setting(doc: &DocumentMut, path: &str) -> Setting {
    match codex_entries(doc).into_iter().find(|(p, _)| norm(p) == norm(path)) {
        Some((_, Some(false))) => Setting::Off,
        Some(_) => Setting::On,
        None => Setting::Unset,
    }
}

fn parse_toml(text: Option<&str>, file: &str) -> Result<DocumentMut> {
    text.unwrap_or("").parse().with_context(|| format!("{file} is not valid TOML; OwO AI Gateway will not rewrite it"))
}

fn codex_off(text: Option<&str>, path: &str) -> Result<Option<String>> {
    let mut doc = parse_toml(text, "config.toml")?;
    match codex_setting(&doc, path) {
        Setting::Off => return Ok(None),
        Setting::On => bail!("config.toml has its own [[skills.config]] entry for {path}; change it there"),
        Setting::Unset => {}
    }
    let root = doc.as_table_mut();
    if !root.contains_key("skills") {
        let mut t = Table::new();
        t.set_implicit(true);
        root.insert("skills", Item::Table(t));
    }
    let Some(Item::Table(skills)) = root.get_mut("skills") else { bail!("`skills` in config.toml is not a table OwO AI Gateway can edit") };
    match skills.get_mut("config") {
        None => {
            let mut list = ArrayOfTables::new();
            list.push(codex_entry(path));
            skills.insert("config", Item::ArrayOfTables(list));
        }
        Some(Item::ArrayOfTables(list)) => list.push(codex_entry(path)),
        Some(Item::Value(Value::Array(list))) => {
            let mut t = toml_edit::InlineTable::new();
            t.insert("path", path.into());
            t.insert("enabled", false.into());
            list.push(t);
        }
        Some(_) => bail!("`skills.config` in config.toml is not a list"),
    }
    Ok(Some(doc.to_string()))
}

fn codex_entry(path: &str) -> Table {
    let mut t = Table::new();
    t.insert("path", value(path));
    t.insert("enabled", value(false));
    t
}

fn codex_clear(text: Option<&str>, path: &str) -> Result<Option<String>> {
    let mut doc = parse_toml(text, "config.toml")?;
    if codex_setting(&doc, path) != Setting::Off {
        return Ok(None);
    }
    let ours = |p: Option<&str>, enabled: Option<bool>| p.is_some_and(|p| norm(p) == norm(path)) && enabled == Some(false);
    let Some(Item::Table(skills)) = doc.get_mut("skills") else { return Ok(None) };
    let empty = match skills.get_mut("config") {
        Some(Item::ArrayOfTables(list)) => {
            list.retain(|t| !ours(t.get("path").and_then(Item::as_str), t.get("enabled").and_then(Item::as_bool)));
            list.is_empty()
        }
        Some(Item::Value(Value::Array(list))) => {
            list.retain(|v| !v.as_inline_table().is_some_and(|t| ours(t.get("path").and_then(Value::as_str), t.get("enabled").and_then(Value::as_bool))));
            list.is_empty()
        }
        _ => false,
    };
    if empty {
        skills.remove("config");
    }
    if skills.is_empty() && skills.is_implicit() {
        doc.remove("skills");
    }
    Ok(Some(doc.to_string()))
}

// ---------------------------------------------------------------------------
// Grok Build

fn grok_setting(doc: &DocumentMut, name: &str) -> Setting {
    let listed = doc.get("skills").and_then(|s| s.get("disabled")).and_then(Item::as_array).is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)));
    if listed { Setting::Off } else { Setting::Unset }
}

fn grok_off(text: Option<&str>, name: &str) -> Result<Option<String>> {
    let mut doc = parse_toml(text, "~/.grok/config.toml")?;
    if grok_setting(&doc, name) == Setting::Off {
        return Ok(None);
    }
    match doc.get_mut("skills") {
        Some(Item::Table(t)) => {
            match t.get_mut("disabled") {
                None => {
                    t.insert("disabled", value(Array::from_iter([name])));
                }
                Some(item) => item.as_array_mut().context("`skills.disabled` in ~/.grok/config.toml is not a list")?.push(name),
            }
            Ok(Some(doc.to_string()))
        }
        Some(_) => bail!("`skills` in ~/.grok/config.toml is not a table OwO AI Gateway can edit"),
        None => {
            // A new table goes before the block `owo connect grok` keeps at the end of the
            // file, so disconnecting Grok never takes the skill switch with it.
            let list = Value::from(Array::from_iter([name])).to_string();
            let snippet = format!("[skills]\ndisabled = {}\n", list.trim());
            let text = text.unwrap_or("");
            let out = match text.find(owo_client_apps::managed::BLOCK_BEGIN) {
                Some(i) => format!("{}{snippet}\n{}", &text[..i], &text[i..]),
                None => {
                    let mut s = text.to_string();
                    if !s.is_empty() {
                        if !s.ends_with('\n') {
                            s.push('\n');
                        }
                        s.push('\n');
                    }
                    s.push_str(&snippet);
                    s
                }
            };
            out.parse::<DocumentMut>().context("adding [skills] would break ~/.grok/config.toml; edit it by hand")?;
            Ok(Some(out))
        }
    }
}

fn grok_clear(text: Option<&str>, name: &str) -> Result<Option<String>> {
    let mut doc = parse_toml(text, "~/.grok/config.toml")?;
    if grok_setting(&doc, name) != Setting::Off {
        return Ok(None);
    }
    let Some(Item::Table(skills)) = doc.get_mut("skills") else { return Ok(None) };
    let empty = match skills.get_mut("disabled").and_then(Item::as_array_mut) {
        Some(list) => {
            list.retain(|v| v.as_str() != Some(name));
            list.is_empty()
        }
        None => false,
    };
    if empty {
        skills.remove("disabled");
    }
    if skills.is_empty() {
        doc.remove("skills");
    }
    Ok(Some(doc.to_string()))
}

// ---------------------------------------------------------------------------
// JSON files (OpenCode, Copilot)

fn parse_json(text: Option<&str>, file: &str) -> Result<Json> {
    let Some(text) = text.filter(|t| !t.trim().is_empty()) else { return Ok(Json::Object(Map::new())) };
    let v: Json = serde_json::from_str(text).with_context(|| format!("{file} is not plain JSON (comments are not supported); edit it by hand"))?;
    if !v.is_object() {
        bail!("{file} does not contain an object at the top level");
    }
    Ok(v)
}

fn render_json(v: &Json) -> Result<String> {
    let mut t = serde_json::to_string_pretty(v)?;
    t.push('\n');
    Ok(t)
}

fn opencode_setting(doc: &Json, name: &str) -> Setting {
    match doc.get("permission").and_then(|p| p.get("skill")).and_then(|s| s.get(name)) {
        Some(Json::String(s)) if s == "deny" => Setting::Off,
        Some(_) => Setting::On,
        None => Setting::Unset,
    }
}

fn object_at<'a>(parent: &'a mut Map<String, Json>, key: &str, file: &str) -> Result<&'a mut Map<String, Json>> {
    parent.entry(key.to_string()).or_insert_with(|| Json::Object(Map::new())).as_object_mut().with_context(|| format!("`{key}` in {file} is not an object OwO AI Gateway can add to"))
}

fn opencode_off(text: Option<&str>, name: &str) -> Result<Option<String>> {
    let mut doc = parse_json(text, "opencode.json")?;
    match opencode_setting(&doc, name) {
        Setting::Off => return Ok(None),
        Setting::On => bail!("opencode.json has its own `permission.skill.{name}` setting; change it there"),
        Setting::Unset => {}
    }
    let root = doc.as_object_mut().expect("checked in parse_json");
    let permission = object_at(root, "permission", "opencode.json")?;
    let skill = object_at(permission, "skill", "opencode.json")?;
    skill.insert(name.to_string(), Json::String("deny".into()));
    Ok(Some(render_json(&doc)?))
}

fn opencode_clear(text: Option<&str>, name: &str) -> Result<Option<String>> {
    let mut doc = parse_json(text, "opencode.json")?;
    if opencode_setting(&doc, name) != Setting::Off {
        return Ok(None);
    }
    let root = doc.as_object_mut().expect("checked in parse_json");
    if let Some(permission) = root.get_mut("permission").and_then(Json::as_object_mut) {
        if let Some(skill) = permission.get_mut("skill").and_then(Json::as_object_mut) {
            skill.shift_remove(name);
            if skill.is_empty() {
                permission.shift_remove("skill");
            }
        }
        if permission.is_empty() {
            root.shift_remove("permission");
        }
    }
    Ok(Some(render_json(&doc)?))
}

const COPILOT_KEY: &str = "disabledSkills";

fn copilot_setting(doc: &Json, name: &str) -> Setting {
    let listed = doc.get(COPILOT_KEY).and_then(Json::as_array).is_some_and(|a| a.iter().any(|v| v.as_str() == Some(name)));
    if listed { Setting::Off } else { Setting::Unset }
}

fn copilot_off(text: Option<&str>, name: &str) -> Result<Option<String>> {
    let mut doc = parse_json(text, "settings.json")?;
    if copilot_setting(&doc, name) == Setting::Off {
        return Ok(None);
    }
    let root = doc.as_object_mut().expect("checked in parse_json");
    root.entry(COPILOT_KEY).or_insert_with(|| Json::Array(Vec::new())).as_array_mut().context("`disabledSkills` in settings.json is not a list")?.push(Json::String(name.into()));
    Ok(Some(render_json(&doc)?))
}

fn copilot_clear(text: Option<&str>, name: &str) -> Result<Option<String>> {
    let mut doc = parse_json(text, "settings.json")?;
    if copilot_setting(&doc, name) != Setting::Off {
        return Ok(None);
    }
    let root = doc.as_object_mut().expect("checked in parse_json");
    let empty = match root.get_mut(COPILOT_KEY).and_then(Json::as_array_mut) {
        Some(list) => {
            list.retain(|v| v.as_str() != Some(name));
            list.is_empty()
        }
        None => false,
    };
    if empty {
        root.shift_remove(COPILOT_KEY);
    }
    Ok(Some(render_json(&doc)?))
}

// ---------------------------------------------------------------------------
// Reading and writing

/// Every switch file, read once (a listing asks about many skills).
pub struct Switches {
    codex: std::result::Result<DocumentMut, String>,
    opencode: std::result::Result<Json, String>,
    grok: std::result::Result<DocumentMut, String>,
    copilot: std::result::Result<Json, String>,
}

impl Switches {
    pub fn load(layout: &Layout) -> Self {
        let text = |app| layout.switch_file(app).map_or(Ok(None), |p| read_text(&p)).map_err(|e| format!("{e:#}"));
        let toml = |app, file| text(app).and_then(|t| parse_toml(t.as_deref(), file).map_err(|e| format!("{e:#}")));
        let json = |app, file| text(app).and_then(|t| parse_json(t.as_deref(), file).map_err(|e| format!("{e:#}")));
        Self {
            codex: toml(AppId::Codex, "config.toml"),
            opencode: json(AppId::Opencode, "opencode.json"),
            grok: toml(AppId::Grok, "~/.grok/config.toml"),
            copilot: json(AppId::Copilot, "settings.json"),
        }
    }

    /// `Err` holds why the app's file could not be read.
    pub fn get(&self, layout: &Layout, app: AppId, name: &str) -> std::result::Result<Setting, String> {
        match app {
            AppId::Codex => self.codex.as_ref().map(|d| codex_setting(d, &codex_skill_path(layout, name))).map_err(Clone::clone),
            AppId::Opencode => self.opencode.as_ref().map(|d| opencode_setting(d, name)).map_err(Clone::clone),
            AppId::Grok => self.grok.as_ref().map(|d| grok_setting(d, name)).map_err(Clone::clone),
            AppId::Copilot => self.copilot.as_ref().map(|d| copilot_setting(d, name)).map_err(Clone::clone),
            _ => Ok(Setting::Unset),
        }
    }
}

/// Turns `name` off for `app`; returns the file written (`None` when it was off already).
pub fn turn_off(layout: &Layout, backups: &Path, app: AppId, name: &str) -> Result<Option<PathBuf>> {
    let path = layout.switch_file(app).with_context(|| format!("{} has no per-skill setting", app.title()))?;
    let skill = codex_skill_path(layout, name);
    edit(&path, backups, app, |text| match app {
        AppId::Codex => codex_off(text, &skill),
        AppId::Opencode => opencode_off(text, name),
        AppId::Grok => grok_off(text, name),
        AppId::Copilot => copilot_off(text, name),
        _ => Ok(None),
    })
}

/// Removes the "off" entry for `name` (the caller knows OwO AI Gateway wrote it).
pub fn clear_off(layout: &Layout, backups: &Path, app: AppId, name: &str) -> Result<Option<PathBuf>> {
    let Some(path) = layout.switch_file(app) else { return Ok(None) };
    let skill = codex_skill_path(layout, name);
    edit(&path, backups, app, |text| match app {
        AppId::Codex => codex_clear(text, &skill),
        AppId::Opencode => opencode_clear(text, name),
        AppId::Grok => grok_clear(text, name),
        AppId::Copilot => copilot_clear(text, name),
        _ => Ok(None),
    })
}

fn edit(path: &Path, backups: &Path, app: AppId, change: impl FnOnce(Option<&str>) -> Result<Option<String>>) -> Result<Option<PathBuf>> {
    let before = read_text(path)?;
    let Some(after) = change(before.as_deref())? else { return Ok(None) };
    if before.as_deref() == Some(after.as_str()) {
        return Ok(None);
    }
    if let Some(before) = &before {
        backup(backups, app, path, before)?;
    }
    owo_client_apps::managed::write_atomic(path, after.as_bytes())?;
    Ok(Some(path.to_path_buf()))
}

/// `<backups>/config/<app>/`: the first copy ever as `original-<file>`, then one per write
/// (the newest few are kept).
fn backup(backups: &Path, app: AppId, path: &Path, text: &str) -> Result<()> {
    let dir = backups.join("config").join(app.name());
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "file".into());
    let original = dir.join(format!("original-{file}"));
    if !original.exists() {
        std::fs::write(&original, text).with_context(|| format!("cannot back up {}", path.display()))?;
    }
    let dest = crate::fsops::fresh_path(&dir, &file);
    std::fs::write(&dest, text).with_context(|| format!("cannot back up {}", path.display()))?;
    let mut copies: Vec<PathBuf> = std::fs::read_dir(&dir)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with(&format!("{file}-"))))
        .collect();
    copies.sort_by_key(|p| p.metadata().and_then(|m| m.modified()).ok());
    let excess = copies.len().saturating_sub(KEEP_BACKUPS);
    for old in copies.into_iter().take(excess) {
        let _ = std::fs::remove_file(old);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CODEX: &str = "model = \"gpt-5.6-sol\" # mine\n\n[[skills.config]]\npath = \"/u/.agents/skills/theirs/SKILL.md\"\nenabled = false\n\n[mcp_servers.x]\ncommand = \"x\"\n";

    #[test]
    fn codex_round_trip_keeps_the_rest() {
        let path = "/u/.agents/skills/pdf/SKILL.md";
        let off = codex_off(Some(CODEX), path).unwrap().unwrap();
        let doc: DocumentMut = off.parse().unwrap();
        assert_eq!(codex_setting(&doc, path), Setting::Off);
        assert_eq!(codex_setting(&doc, "/u/.agents/skills/theirs/SKILL.md"), Setting::Off);
        assert!(off.contains("# mine"));
        assert!(codex_off(Some(&off), path).unwrap().is_none(), "already off");
        let back = codex_clear(Some(&off), path).unwrap().unwrap();
        assert_eq!(back, CODEX, "the user's entry and formatting survive");
    }

    #[test]
    fn codex_new_table_is_removed_again() {
        let user = "model = \"m\"\n";
        let off = codex_off(Some(user), "C:\\Users\\u\\.agents\\skills\\pdf\\SKILL.md").unwrap().unwrap();
        assert!(off.contains("[[skills.config]]"), "{off}");
        let doc: DocumentMut = off.parse().unwrap();
        assert_eq!(codex_setting(&doc, "C:/Users/u/.agents/skills/pdf/SKILL.md"), Setting::Off, "separators do not matter");
        let back = codex_clear(Some(&off), "C:\\Users\\u\\.agents\\skills\\pdf\\SKILL.md").unwrap().unwrap();
        assert_eq!(back.trim_end(), user.trim_end());
        assert!(codex_off(None, "/x/SKILL.md").unwrap().unwrap().contains("enabled = false"));
    }

    #[test]
    fn codex_user_entries_win() {
        let user = "[[skills.config]]\npath = \"/s/pdf/SKILL.md\"\nenabled = true\n";
        assert!(codex_off(Some(user), "/s/pdf/SKILL.md").is_err());
        let off = "[[skills.config]]\npath = \"/s/pdf/SKILL.md\"\nenabled = false\n";
        assert!(codex_off(Some(off), "/s/pdf/SKILL.md").unwrap().is_none());
        assert!(codex_off(Some("not = [valid"), "/s/pdf/SKILL.md").is_err());
    }

    #[test]
    fn opencode_round_trip() {
        let user = "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"permission\": {\n    \"bash\": \"ask\"\n  }\n}\n";
        let off = opencode_off(Some(user), "pdf").unwrap().unwrap();
        let doc: Json = serde_json::from_str(&off).unwrap();
        assert_eq!(doc["permission"]["skill"]["pdf"], "deny");
        assert_eq!(doc["permission"]["bash"], "ask");
        assert_eq!(opencode_clear(Some(&off), "pdf").unwrap().unwrap(), user);
        let allowed = "{\"permission\": {\"skill\": {\"pdf\": \"allow\"}}}";
        assert!(opencode_off(Some(allowed), "pdf").is_err(), "the user's own setting is not overridden");
        assert!(opencode_off(Some("{ // comment\n}"), "pdf").is_err());
        assert_eq!(opencode_clear(Some(&opencode_off(None, "a").unwrap().unwrap()), "a").unwrap().unwrap(), "{}\n");
    }

    #[test]
    fn grok_goes_before_the_connect_block() {
        let block = format!("{}\n[model_providers.owo]\nbase_url = \"x\"\n{}\n", owo_client_apps::managed::BLOCK_BEGIN, owo_client_apps::managed::BLOCK_END);
        let user = format!("[ui]\ntheme = \"dark\"\n\n{block}");
        let off = grok_off(Some(&user), "pdf").unwrap().unwrap();
        let skills_at = off.find("[skills]").unwrap();
        assert!(skills_at < off.find(owo_client_apps::managed::BLOCK_BEGIN).unwrap(), "{off}");
        assert!(off.ends_with(&block));
        let doc: DocumentMut = off.parse().unwrap();
        assert_eq!(grok_setting(&doc, "pdf"), Setting::Off);
        let two = grok_off(Some(&off), "docx").unwrap().unwrap();
        let back = grok_clear(Some(&grok_clear(Some(&two), "pdf").unwrap().unwrap()), "docx").unwrap().unwrap();
        let (a, b): (DocumentMut, DocumentMut) = (back.parse().unwrap(), user.parse().unwrap());
        assert_eq!(a.to_string().replace("\n\n", "\n"), b.to_string().replace("\n\n", "\n"));
        assert!(back.ends_with(&block), "the connect block is untouched");
    }

    #[test]
    fn copilot_round_trip() {
        let user = "{\n  \"theme\": \"dark\",\n  \"disabledSkills\": [\n    \"mine\"\n  ]\n}\n";
        let off = copilot_off(Some(user), "pdf").unwrap().unwrap();
        let doc: Json = serde_json::from_str(&off).unwrap();
        assert_eq!(doc["disabledSkills"], serde_json::json!(["mine", "pdf"]));
        assert_eq!(copilot_clear(Some(&off), "pdf").unwrap().unwrap(), user);
    }

    #[test]
    fn writes_back_up_first() {
        let tmp = tempfile::tempdir().unwrap();
        let layout = Layout::for_home(tmp.path());
        std::fs::create_dir_all(&layout.codex_home).unwrap();
        let config = layout.codex_home.join("config.toml");
        std::fs::write(&config, CODEX).unwrap();
        let backups = tmp.path().join("backups");
        assert!(turn_off(&layout, &backups, AppId::Codex, "pdf").unwrap().is_some());
        assert_eq!(std::fs::read_to_string(backups.join("config/codex/original-config.toml")).unwrap(), CODEX);
        assert!(turn_off(&layout, &backups, AppId::Codex, "pdf").unwrap().is_none());
        let switches = Switches::load(&layout);
        assert_eq!(switches.get(&layout, AppId::Codex, "pdf"), Ok(Setting::Off));
        assert!(clear_off(&layout, &backups, AppId::Codex, "pdf").unwrap().is_some());
        assert_eq!(std::fs::read_to_string(&config).unwrap(), CODEX);
    }
}
