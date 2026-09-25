//! Edits to config.toml that keep its comments and layout, saved only when the result still
//! loads and builds a valid registry (the same rule as `owo config edit`).

use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use owo_config::{Config, OwoPaths, Price};
use owo_registry::{PresetCatalog, Registry};
use toml_edit::{value, Array, ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

/// Adapter kinds the `owo` build implements.
pub const ADAPTERS: [&str; 2] = ["openai-chat", "anthropic"];

pub fn read_text(paths: &OwoPaths) -> Result<Option<String>> {
    match std::fs::read_to_string(&paths.config) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", paths.config.display())),
    }
}

/// Loads and builds the registry, as `owo start` would.
pub fn check(text: &str, origin: &str) -> Result<(Config, Registry)> {
    let (config, _) = Config::from_toml_str(text, origin).map_err(|e| anyhow!("{e}"))?;
    let (registry, _) = Registry::build(&config, PresetCatalog::builtin(), &ADAPTERS).map_err(|errors| {
        anyhow!("{}", errors.iter().map(|d| format!("  - {d}")).collect::<Vec<_>>().join("\n"))
    })?;
    Ok((config, registry))
}

/// Replaces config.toml with `text` if it is valid.
pub fn save_text(paths: &OwoPaths, text: &str) -> Result<()> {
    check(text, &paths.config.display().to_string()).context("the config is not valid, so nothing was written")?;
    write_atomic(paths, text)
}

/// The config a first launch writes: no providers, so every page starts empty.
const EMPTY_CONFIG: &str = r#"# OwO AI Gateway — one local endpoint for every AI coding app
#
# Providers, models and apps added in the app (or with `owo add`) are written here.
# `owo providers presets` lists the built-in presets.
"#;

/// Writes an empty config when there is no config.toml yet, so a first launch opens on empty
/// pages instead of "config not found" errors.
pub fn ensure_exists(paths: &OwoPaths) -> Result<()> {
    if paths.config.exists() {
        return Ok(());
    }
    save_text(paths, EMPTY_CONFIG)?;
    paths.ensure_dirs().with_context(|| format!("cannot create the data directories under {}", paths.root.display()))
}

/// Replaces config.toml with the starter config, after copying the current file to
/// `<backups>/config/<unix-ts>-config.toml`. Returns that backup's path (`None` when there
/// was no config.toml to back up). Keys in the OS keyring are untouched.
pub fn reset(paths: &OwoPaths) -> Result<Option<PathBuf>> {
    let backup = match read_text(paths)? {
        Some(current) => {
            let dir = paths.backups.join("config");
            std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default();
            let backup = dir.join(format!("{stamp}-config.toml"));
            std::fs::write(&backup, current).with_context(|| format!("cannot write the backup {}", backup.display()))?;
            Some(backup)
        }
        None => None,
    };
    save_text(paths, owo_config::SAMPLE_CONFIG).context("the starter config did not validate, so nothing was reset")?;
    Ok(backup)
}

pub fn edit(paths: &OwoPaths, change: impl FnOnce(&mut DocumentMut) -> Result<()>) -> Result<()> {
    let original = read_text(paths)?;
    let mut doc: DocumentMut = original.as_deref().unwrap_or("").parse().context("config.toml is not valid TOML")?;
    change(&mut doc)?;
    let text = match original {
        Some(_) => doc.to_string(),
        None => format!("# OwO AI Gateway configuration. The desktop app and `owo` edit this file; so can you.\n\n{doc}"),
    };
    check(&text, &paths.config.display().to_string()).context("the change would make the config invalid, so nothing was written")?;
    write_atomic(paths, &text)
}

fn write_atomic(paths: &OwoPaths, text: &str) -> Result<()> {
    if let Some(dir) = paths.config.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    let tmp = paths.config.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, text).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, &paths.config).with_context(|| format!("cannot replace {}", paths.config.display()))
}

/// A string key: set when `Some` and non-empty, removed otherwise.
pub fn set_str(table: &mut Table, key: &str, text: Option<&str>) {
    match text.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) => {
            table.insert(key, value(t));
        }
        None => {
            table.remove(key);
        }
    }
}

/// A parent table such as `[providers]` or `[clients]`, kept implicit so only its children print.
pub fn parent_table<'a>(doc: &'a mut DocumentMut, key: &str) -> Result<&'a mut Table> {
    let item = doc.entry(key).or_insert_with(|| {
        let mut t = Table::new();
        t.set_implicit(true);
        Item::Table(t)
    });
    item.as_table_mut().ok_or_else(|| anyhow!("`{key}` in config.toml is not a table"))
}

pub fn child_table<'a>(parent: &'a mut Table, key: &str) -> Result<&'a mut Table> {
    let item = parent.entry(key).or_insert_with(|| Item::Table(Table::new()));
    if let Some(inline) = item.as_inline_table().cloned() {
        *item = Item::Table(inline.into_table());
    }
    item.as_table_mut().ok_or_else(|| anyhow!("`{key}` in config.toml is not a table"))
}

pub fn price_value(price: &Price) -> Value {
    let mut t = InlineTable::new();
    t.insert("input", price.input.into());
    t.insert("output", price.output.into());
    if let Some(v) = price.cache_read {
        t.insert("cache_read", v.into());
    }
    if let Some(v) = price.cache_write {
        t.insert("cache_write", v.into());
    }
    Value::InlineTable(t)
}

pub fn string_array(items: &[String]) -> Value {
    let mut a = Array::new();
    for item in items.iter().map(|s| s.trim()).filter(|s| !s.is_empty()) {
        a.push(item);
    }
    Value::Array(a)
}

/// The `[[models]]` array, created when absent.
pub fn models_array(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables> {
    let item = doc.entry("models").or_insert_with(|| Item::ArrayOfTables(ArrayOfTables::new()));
    item.as_array_of_tables_mut().ok_or_else(|| anyhow!("`models` in config.toml is not a [[models]] list"))
}

pub fn has_id(table: &Table, id: &str) -> bool {
    table.get("id").and_then(Item::as_str) == Some(id)
}

/// Writes the provider's `models` list without `id`. A provider without its own list uses the
/// preset's, so `effective` (the list as the registry sees it) is written out in full.
pub fn drop_from_provider_list(doc: &mut DocumentMut, provider: &str, effective: &[String], id: &str) -> Result<()> {
    let providers = parent_table(doc, "providers")?;
    let table = child_table(providers, provider)?;
    let remaining: Vec<String> = effective.iter().filter(|m| *m != id).cloned().collect();
    table.insert("models", Item::Value(string_array(&remaining)));
    Ok(())
}

/// Points every `[clients.<app>] model = "<old>"` at `new`.
pub fn retarget_clients(doc: &mut DocumentMut, old: &str, new: &str) {
    let Some(clients) = doc.get_mut("clients").and_then(Item::as_table_like_mut) else { return };
    for (_, app) in clients.iter_mut() {
        let Some(app) = app.as_table_like_mut() else { continue };
        if app.get("model").and_then(Item::as_str) == Some(old) {
            app.insert("model", value(new));
        }
    }
}

/// Renames a model from `old` to `new` and returns the index of its `[[models]]` entry.
///
/// An existing entry keeps everything but its id; it gets an explicit `upstream_model` (the
/// old id when it had none) so requests keep reaching the same upstream model. A model that
/// only came from its provider's `models` list gets a new entry routing to the old id, and
/// `listed` (that provider with its effective list) says which list to drop the old id from
/// so it does not linger as a second model. Apps whose `[clients.*] model` named the old id
/// switch to the new one.
pub fn rename_model(doc: &mut DocumentMut, old: &str, new: &str, provider: &str, listed: Option<(&str, &[String])>) -> Result<usize> {
    let list = models_array(doc)?;
    if list.iter().any(|t| has_id(t, new)) {
        bail!("a model with the id `{new}` already exists");
    }
    let found = list.iter().position(|t| has_id(t, old));
    let index = match found {
        Some(i) => {
            let table = list.get_mut(i).expect("index is in range");
            if table.get("upstream_model").and_then(Item::as_str).is_none() {
                table.insert("upstream_model", value(old));
            }
            table.insert("id", value(new));
            i
        }
        None => {
            let mut t = Table::new();
            t.insert("id", value(new));
            t.insert("provider", value(provider));
            t.insert("upstream_model", value(old));
            list.push(t);
            list.len() - 1
        }
    };
    if let Some((provider, effective)) = listed {
        drop_from_provider_list(doc, provider, effective, old)?;
    }
    retarget_clients(doc, old, new);
    Ok(index)
}

/// Moves `[providers.<old>]` to `[providers.<new>]` with all its contents (a `keyring:<old>`
/// key reference included: keyring entries are named independently of the provider id) and
/// points every `[[models]] provider = "<old>"` at the new id.
pub fn rename_provider(doc: &mut DocumentMut, old: &str, new: &str) -> Result<()> {
    let providers = parent_table(doc, "providers")?;
    if providers.contains_key(new) {
        bail!("a provider with the id `{new}` already exists");
    }
    if let Some(item) = providers.remove(old) {
        providers.insert(new, item);
    }
    if let Some(Item::ArrayOfTables(list)) = doc.get_mut("models") {
        for table in list.iter_mut() {
            if table.get("provider").and_then(Item::as_str) == Some(old) {
                table.insert("provider", value(new));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = r#"# test config

# my relay
[providers.relay]
adapter = "openai-chat"
base_url = "https://relay.example.com/v1"
api_key = "keyring:relay"
models = ["gpt-x", "gpt-y"]

[[models]]
id = "fast"
provider = "relay"

[[models]]
id = "smart"
provider = "relay"
upstream_model = "gpt-y"
price = { input = 1, output = 2 }

[clients.codex]
model = "fast"
"#;

    fn setup() -> (tempfile::TempDir, OwoPaths) {
        let dir = tempfile::tempdir().unwrap();
        let paths = OwoPaths::from_home_root(dir.path().to_path_buf());
        std::fs::write(&paths.config, CONFIG).unwrap();
        (dir, paths)
    }

    fn load(paths: &OwoPaths) -> (String, Config, Registry) {
        let text = std::fs::read_to_string(&paths.config).unwrap();
        let (config, registry) = check(&text, "test").unwrap();
        (text, config, registry)
    }

    #[test]
    fn a_missing_config_is_created_and_an_existing_one_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let paths = OwoPaths::from_home_root(dir.path().join("fresh"));
        ensure_exists(&paths).unwrap();
        let (text, config, _) = load(&paths);
        assert_eq!(text, EMPTY_CONFIG);
        assert!(config.providers.is_empty());
        assert!(paths.state.is_dir());

        let (_dir, paths) = setup();
        ensure_exists(&paths).unwrap();
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), CONFIG);
    }

    #[test]
    fn renaming_a_model_follows_client_references() {
        let (_dir, paths) = setup();
        edit(&paths, |doc| rename_model(doc, "fast", "quick", "relay", None).map(|_| ())).unwrap();
        let (text, config, registry) = load(&paths);
        assert!(text.starts_with("# test config"), "{text}");
        assert_eq!(config.clients["codex"].model.as_deref(), Some("quick"));
        assert!(config.models.iter().all(|m| m.id != "fast"));
        // The entry had no upstream_model, so the old id becomes the explicit upstream.
        let quick = registry.models().find(|m| m.id == "quick").unwrap();
        assert_eq!((quick.provider.as_str(), quick.upstream_model.as_str()), ("relay", "fast"));
        assert!(registry.resolve("quick", Some("codex")).is_ok());
    }

    #[test]
    fn renaming_a_listed_model_gives_it_an_entry_and_leaves_the_list() {
        let (_dir, paths) = setup();
        let effective = vec!["gpt-x".to_string(), "gpt-y".to_string()];
        edit(&paths, |doc| rename_model(doc, "gpt-x", "x1", "relay", Some(("relay", &effective))).map(|_| ())).unwrap();
        let (_, config, registry) = load(&paths);
        assert_eq!(config.providers["relay"].models.as_deref(), Some(&["gpt-y".to_string()][..]));
        let x1 = registry.models().find(|m| m.id == "x1").unwrap();
        assert_eq!(x1.upstream_model, "gpt-x");
        assert!(registry.models().all(|m| m.id != "gpt-x"));
    }

    #[test]
    fn renaming_a_model_onto_an_existing_id_is_refused() {
        let (_dir, paths) = setup();
        let err = edit(&paths, |doc| rename_model(doc, "fast", "smart", "relay", None).map(|_| ())).unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), CONFIG);
    }

    #[test]
    fn renaming_a_provider_moves_the_table_and_its_models() {
        let (_dir, paths) = setup();
        edit(&paths, |doc| rename_provider(doc, "relay", "hub")).unwrap();
        let (text, config, registry) = load(&paths);
        assert!(text.contains("# my relay\n[providers.hub]"), "{text}");
        assert!(!text.contains("[providers.relay]"), "{text}");
        assert!(!config.providers.contains_key("relay"));
        let hub = &config.providers["hub"];
        assert_eq!(hub.api_key.as_ref().map(ToString::to_string).as_deref(), Some("keyring:relay"));
        assert_eq!(hub.models.as_deref(), Some(&["gpt-x".to_string(), "gpt-y".to_string()][..]));
        assert_eq!(config.models.len(), 2);
        assert!(config.models.iter().all(|m| m.provider == "hub"));
        assert_eq!(registry.resolve("smart", None).unwrap().provider.id, "hub");
        assert!(registry.resolve("fast", Some("codex")).is_ok());
    }

    #[test]
    fn resetting_backs_up_the_old_config_and_writes_the_starter() {
        let (_dir, paths) = setup();
        let backup = reset(&paths).unwrap().expect("an existing config is backed up");
        assert_eq!(backup.parent(), Some(paths.backups.join("config").as_path()));
        assert!(backup.file_name().unwrap().to_string_lossy().ends_with("-config.toml"));
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), CONFIG);
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), owo_config::SAMPLE_CONFIG);
        let (_, config, registry) = load(&paths);
        assert!(config.models.is_empty());
        assert!(registry.provider("relay").is_none());
    }

    #[test]
    fn resetting_without_a_config_makes_no_backup() {
        let dir = tempfile::tempdir().unwrap();
        let paths = OwoPaths::from_home_root(dir.path().join("owo"));
        assert_eq!(reset(&paths).unwrap(), None);
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), owo_config::SAMPLE_CONFIG);
        assert!(!paths.backups.exists());
    }

    #[test]
    fn renaming_a_provider_onto_an_existing_id_is_refused() {
        let (_dir, paths) = setup();
        let err = edit(&paths, |doc| {
            let providers = parent_table(doc, "providers")?;
            providers.insert("hub", providers.get("relay").cloned().unwrap());
            rename_provider(doc, "relay", "hub")
        })
        .unwrap_err();
        assert!(err.to_string().contains("already exists"), "{err}");
        assert_eq!(std::fs::read_to_string(&paths.config).unwrap(), CONFIG);
    }
}
