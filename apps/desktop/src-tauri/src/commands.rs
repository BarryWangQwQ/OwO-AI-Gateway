//! The commands the web UI invokes. Reads go straight to config.toml, the registry, the OS
//! keyring, and `usage.db`; actions with side effects run the `owo` CLI.

use std::collections::BTreeMap;
use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use owo_config::{OwoPaths, Price};
use owo_credentials::{CredentialRef, CredentialStore, Secret};
use owo_registry::PresetCatalog;
use owo_usage::{CallFilter, GroupBy, StoredCall, Summary, UsageLog};
use serde::{Deserialize, Serialize};
use toml_edit::{value, Item, Table};

use crate::config_edit::{self as edit, ADAPTERS};
use crate::owo_cli;

type Reply<T> = std::result::Result<T, String>;

fn reply<T>(result: Result<T>) -> Reply<T> {
    result.map_err(|e| format!("{e:#}"))
}

fn paths() -> Result<OwoPaths> {
    OwoPaths::home().context("cannot determine the home directory; set OWO_HOME")
}

fn loaded() -> Result<(owo_config::Config, owo_registry::Registry)> {
    let paths = paths()?;
    let text = edit::read_text(&paths)?.context("there is no config.toml yet; add a provider first")?;
    edit::check(&text, &paths.config.display().to_string())
}

// ---------------------------------------------------------------- status

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    version: &'static str,
    config_path: String,
    config_exists: bool,
    config_error: Option<String>,
    /// Where `reset_config` puts the copy of the old config.toml.
    config_backups_path: String,
    name: String,
    gateway_running: bool,
    gateway_address: String,
    owo_binary: Option<String>,
}

/// The address clients dial: a wildcard bind is reached through loopback.
fn client_address(listen: &str) -> String {
    match listen.parse::<SocketAddr>() {
        Ok(addr) if addr.ip().is_unspecified() => format!("127.0.0.1:{}", addr.port()),
        _ => listen.to_string(),
    }
}

fn reachable(address: &str) -> bool {
    address.parse::<SocketAddr>().is_ok_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
}

#[tauri::command]
pub async fn status() -> Reply<Status> {
    reply(
        tokio::task::spawn_blocking(|| {
            let paths = paths()?;
            let text = edit::read_text(&paths)?;
            let (config, config_error) = match text.as_deref().map(|t| edit::check(t, "config.toml")) {
                Some(Ok((config, _))) => (Some(config), None),
                Some(Err(e)) => (None, Some(format!("{e:#}"))),
                None => (None, None),
            };
            let listen = config.as_ref().map(|c| c.server.listen.clone()).unwrap_or_else(|| "127.0.0.1:8787".into());
            // A gateway started with `--listen` records its address in `state/owo.pid`.
            let recorded = std::fs::read_to_string(paths.state.join("owo.pid"))
                .ok()
                .and_then(|t| t.lines().nth(1).map(str::to_string))
                .filter(|a| reachable(a));
            let address = recorded.unwrap_or_else(|| client_address(&listen));
            Ok(Status {
                version: env!("CARGO_PKG_VERSION"),
                config_path: paths.config.display().to_string(),
                config_exists: text.is_some(),
                config_error,
                config_backups_path: paths.backups.join("config").display().to_string(),
                name: config.as_ref().map(|c| c.name.clone()).unwrap_or_else(|| owo_config::DEFAULT_NAME.into()),
                gateway_running: reachable(&address),
                gateway_address: address,
                owo_binary: owo_cli::binary().ok().map(|p| p.display().to_string()),
            })
        })
        .await
        .map_err(|e| anyhow!("{e}"))
        .and_then(|r| r),
    )
}

// ---------------------------------------------------------------- usage

async fn usage_log() -> Result<Option<UsageLog>> {
    let path = paths()?.state.join(owo_usage::FILE_NAME);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(UsageLog::open(&path).await.with_context(|| format!("cannot open {}", path.display()))?))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageReport {
    since: String,
    rows: Vec<Summary>,
    total: Summary,
}

#[tauri::command]
pub async fn usage(days: u32, by: String) -> Reply<UsageReport> {
    reply(async {
        let group = match by.as_str() {
            "app" => GroupBy::App,
            "provider" => GroupBy::Provider,
            "day" => GroupBy::Day,
            _ => GroupBy::Model,
        };
        let Some(log) = usage_log().await? else {
            return Ok(UsageReport { since: String::new(), rows: Vec::new(), total: Summary::default() });
        };
        let rows = log.summary(days.max(1), group).await?;
        let mut total = Summary { key: "TOTAL".into(), ..Summary::default() };
        rows.iter().for_each(|r| total.add(r));
        Ok(UsageReport { since: log.period_start(days.max(1)).await?, rows, total })
    }
    .await)
}

#[tauri::command]
pub async fn today() -> Reply<Summary> {
    reply(async {
        match usage_log().await? {
            Some(log) => Ok(log.today().await?),
            None => Ok(Summary::default()),
        }
    }
    .await)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CallQuery {
    failed_only: bool,
    model: Option<String>,
    client: Option<String>,
    limit: u32,
}

#[tauri::command]
pub async fn calls(query: CallQuery) -> Reply<Vec<StoredCall>> {
    reply(async {
        let Some(log) = usage_log().await? else { return Ok(Vec::new()) };
        let filter = CallFilter {
            failed_only: query.failed_only,
            model: query.model.filter(|m| !m.is_empty()),
            client: query.client.filter(|c| !c.is_empty()),
            limit: query.limit.clamp(1, 1000),
        };
        Ok(log.calls(&filter).await?)
    }
    .await)
}

#[tauri::command]
pub async fn call(id: i64) -> Reply<Option<StoredCall>> {
    reply(async {
        match usage_log().await? {
            Some(log) => Ok(log.call(id).await?),
            None => Ok(None),
        }
    }
    .await)
}

/// Deletes every recorded call (and with it the cost data); returns how many were deleted.
/// A running gateway keeps its handle on the database and records new calls as before.
#[tauri::command]
pub async fn clear_history() -> Reply<u64> {
    reply(async {
        match usage_log().await? {
            Some(log) => log.clear().await.context("cannot clear the call history"),
            None => Ok(0),
        }
    }
    .await)
}

// ---------------------------------------------------------------- models & providers

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelView {
    id: String,
    display_name: String,
    provider: String,
    upstream_model: String,
    aliases: BTreeMap<String, String>,
    price: Option<Price>,
    context_window: Option<u32>,
    available: bool,
    /// Has its own `[[models]]` entry in config.toml.
    configured: bool,
}

#[tauri::command]
pub fn models() -> Reply<Vec<ModelView>> {
    reply((|| {
        let (config, registry) = loaded()?;
        Ok(registry
            .models()
            .map(|m| ModelView {
                id: m.id.clone(),
                display_name: m.display_name.clone(),
                provider: m.provider.clone(),
                upstream_model: m.upstream_model.clone(),
                aliases: m.aliases.clone(),
                price: m.price,
                context_window: m.context_window,
                available: m.enabled && registry.provider(&m.provider).is_some_and(|p| p.enabled),
                configured: config.models.iter().any(|c| c.id == m.id),
            })
            .collect())
    })())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderView {
    id: String,
    display_name: String,
    adapter: String,
    base_url: String,
    preset: Option<String>,
    enabled: bool,
    /// The key reference as written (`keyring:NAME`, `env:NAME`, `inline`, `none`).
    api_key: String,
    /// `ok`, `missing`, or `none` (no key needed).
    key_status: &'static str,
    key_message: Option<String>,
    models: Vec<String>,
    /// What config.toml itself sets for this provider (`None` for a preset nobody configured).
    raw: Option<ProviderRaw>,
}

/// A provider's own config.toml fields. A key written into the file is never sent to the
/// UI: `api_key` is `None` and `inline_key` is set instead.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderRaw {
    preset: Option<String>,
    adapter: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
    inline_key: bool,
    auth: Option<String>,
    models: Option<Vec<String>>,
    enabled: bool,
}

impl ProviderRaw {
    fn from_config(c: &owo_config::ProviderConfig) -> Self {
        let inline_key = matches!(c.api_key, Some(CredentialRef::Inline(_)));
        Self {
            preset: c.preset.clone(),
            adapter: c.adapter.clone(),
            base_url: c.base_url.clone(),
            api_key: c.api_key.as_ref().filter(|_| !inline_key).map(ToString::to_string),
            inline_key,
            auth: c.auth.and_then(|a| serde_json::to_value(a).ok()).and_then(|v| v.as_str().map(str::to_string)),
            models: c.models.clone(),
            enabled: c.enabled.unwrap_or(true),
        }
    }
}

#[tauri::command]
pub fn providers() -> Reply<Vec<ProviderView>> {
    reply((|| {
        let (config, registry) = loaded()?;
        let store = CredentialStore::new(config.credentials.backend);
        let presets = PresetCatalog::builtin();
        Ok(registry
            .providers()
            .map(|p| {
                let cfg = config.providers.get(&p.id);
                let (key_status, key_message) = match store.resolve(&p.api_key) {
                    Ok(Some(_)) => ("ok", None),
                    Ok(None) => ("none", None),
                    Err(e) => ("missing", Some(e.to_string())),
                };
                ProviderView {
                    id: p.id.clone(),
                    display_name: p.display_name.clone(),
                    adapter: p.adapter.clone(),
                    base_url: p.base_url.to_string(),
                    preset: cfg.and_then(|c| c.preset.clone()).or_else(|| presets.get(&p.id).map(|_| p.id.clone())),
                    enabled: p.enabled,
                    api_key: p.api_key.to_string(),
                    key_status,
                    key_message,
                    models: p.models.clone(),
                    raw: cfg.map(ProviderRaw::from_config),
                }
            })
            .collect())
    })())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetView {
    id: String,
    display_name: String,
    adapter: String,
    base_url: String,
    api_key: Option<String>,
}

#[tauri::command]
pub fn presets() -> Vec<PresetView> {
    PresetCatalog::builtin()
        .iter()
        .filter(|p| ADAPTERS.contains(&p.adapter.as_str()))
        .map(|p| PresetView {
            id: p.id.clone(),
            display_name: p.display_name.clone(),
            adapter: p.adapter.clone(),
            base_url: p.base_url.clone(),
            api_key: p.api_key.as_ref().map(ToString::to_string),
        })
        .collect()
}

// ---------------------------------------------------------------- apps & gateway (via the CLI)

#[derive(Serialize)]
pub struct ActionResult {
    ok: bool,
    output: String,
}

async fn owo(args: Vec<String>) -> Reply<ActionResult> {
    reply(
        tokio::task::spawn_blocking(move || {
            let args: Vec<&str> = args.iter().map(String::as_str).collect();
            owo_cli::run(&args).map(|o| ActionResult { ok: o.ok, output: o.text })
        })
        .await
        .map_err(|e| anyhow!("{e}"))
        .and_then(|r| r),
    )
}

#[tauri::command]
pub async fn apps() -> Reply<serde_json::Value> {
    let result = owo(vec!["apps".into(), "--json".into()]).await?;
    if !result.ok {
        return Err(result.output);
    }
    serde_json::from_str(&result.output).map_err(|e| format!("unexpected output from `owo apps --json`: {e}"))
}

#[tauri::command]
pub async fn gateway_start() -> Reply<ActionResult> {
    owo(vec!["start".into(), "-d".into()]).await
}

#[tauri::command]
pub async fn gateway_stop() -> Reply<ActionResult> {
    owo(vec!["stop".into()]).await
}

#[tauri::command]
pub async fn gateway_restart() -> Reply<ActionResult> {
    let stopped = owo(vec!["stop".into()]).await?;
    if !stopped.ok {
        return Ok(stopped);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;
    owo(vec!["start".into(), "-d".into()]).await
}

#[tauri::command]
pub async fn app_connect(app: String, model: Option<String>, force: bool) -> Reply<ActionResult> {
    let mut args = vec!["connect".to_string(), app];
    if let Some(model) = model.filter(|m| !m.is_empty()) {
        args.extend(["-m".to_string(), model]);
    }
    if force {
        args.push("-f".into());
    }
    owo(args).await
}

#[tauri::command]
pub async fn app_disconnect(app: String, force: bool) -> Reply<ActionResult> {
    let mut args = vec!["disconnect".to_string(), app];
    if force {
        args.push("-f".into());
    }
    owo(args).await
}

#[derive(Serialize)]
pub struct AppOutcome {
    app: String,
    ok: bool,
    output: String,
}

/// `owo disconnect <app>` for every app `owo apps --json` reports as connected, one outcome
/// per app. Each app's own settings come back from the backup `owo connect` made.
#[tauri::command]
pub async fn disconnect_all() -> Reply<Vec<AppOutcome>> {
    let listed = apps().await?;
    let connected: Vec<String> = listed
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a.get("connected").and_then(serde_json::Value::as_bool) == Some(true))
        .filter_map(|a| a.get("app").and_then(serde_json::Value::as_str).map(str::to_string))
        .collect();
    let mut outcomes = Vec::with_capacity(connected.len());
    for app in connected {
        let result = owo(vec!["disconnect".into(), app.clone()]).await?;
        outcomes.push(AppOutcome { app, ok: result.ok, output: result.output });
    }
    Ok(outcomes)
}

// ---------------------------------------------------------------- config editing

#[derive(Serialize)]
pub struct ConfigText {
    path: String,
    text: String,
}

#[tauri::command]
pub fn config_text() -> Reply<ConfigText> {
    reply((|| {
        let paths = paths()?;
        Ok(ConfigText { path: paths.config.display().to_string(), text: edit::read_text(&paths)?.unwrap_or_default() })
    })())
}

/// Checks `text` without saving it; `None` when valid.
#[tauri::command]
pub fn check_config_text(text: String) -> Option<String> {
    edit::check(&text, "config.toml").err().map(|e| format!("{e:#}"))
}

#[tauri::command]
pub fn save_config_text(text: String) -> Reply<()> {
    reply(paths().and_then(|p| edit::save_text(&p, &text)))
}

/// Replaces config.toml with the starter config; returns the path of the backup made first
/// (`None` when there was no config.toml). Keys in the OS keyring are left in place.
#[tauri::command]
pub fn reset_config() -> Reply<Option<String>> {
    reply(paths().and_then(|p| edit::reset(&p)).map(|backup| backup.map(|b| b.display().to_string())))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct General {
    name: String,
    listen: String,
    clients: BTreeMap<String, owo_config::ClientConfig>,
}

#[tauri::command]
pub fn general() -> Reply<General> {
    reply((|| {
        let (config, _) = loaded()?;
        Ok(General { name: config.name, listen: config.server.listen, clients: config.clients })
    })())
}

#[tauri::command]
pub fn save_general(name: String, listen: String) -> Reply<()> {
    reply(paths().and_then(|paths| {
        edit::edit(&paths, |doc| {
            let name = name.trim();
            if name.is_empty() || name == owo_config::DEFAULT_NAME {
                doc.remove("name");
            } else {
                doc.insert("name", value(name));
            }
            let server = edit::parent_table(doc, "server")?;
            server.set_implicit(false);
            edit::set_str(server, "listen", Some(&listen));
            Ok(())
        })
    }))
}

/// `[clients.<app>]`: the app's default model and display name; empty values are removed.
#[tauri::command]
pub fn save_client(app: String, model: Option<String>, name: Option<String>) -> Reply<()> {
    reply(paths().and_then(|paths| {
        edit::edit(&paths, |doc| {
            let clients = edit::parent_table(doc, "clients")?;
            let table = edit::child_table(clients, &app)?;
            edit::set_str(table, "model", model.as_deref());
            edit::set_str(table, "name", name.as_deref());
            if table.is_empty() {
                clients.remove(&app);
            }
            Ok(())
        })
    }))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEdit {
    id: String,
    /// The id the model has now, when editing; differs from `id` for a rename.
    original_id: Option<String>,
    provider: String,
    display_name: Option<String>,
    upstream_model: Option<String>,
    price: Option<Price>,
}

/// The provider whose `models` list carries `id`, with that list as the registry sees it.
fn listing_provider(registry: &owo_registry::Registry, id: &str) -> Option<(String, Vec<String>)> {
    let model = registry.models().find(|m| m.id == id)?;
    let provider = registry.provider(&model.provider)?;
    provider.models.iter().any(|m| m == id).then(|| (provider.id.clone(), provider.models.clone()))
}

/// Creates or updates the model's `[[models]]` entry; renames it when `original_id` differs.
#[tauri::command]
pub fn save_model(model: ModelEdit) -> Reply<()> {
    reply((|| {
        let paths = paths()?;
        let id = model.id.trim();
        if id.is_empty() {
            bail!("the model needs an id");
        }
        let provider = model.provider.trim();
        let original = model.original_id.as_deref().map(str::trim).filter(|o| !o.is_empty() && *o != id);
        // A rename must take the old id out of its provider's list, or it lingers as a second model.
        let listed = match original {
            Some(old) => listing_provider(&loaded()?.1, old),
            None => None,
        };
        edit::edit(&paths, |doc| {
            let index = match original {
                Some(old) => edit::rename_model(doc, old, id, provider, listed.as_ref().map(|(p, list)| (p.as_str(), list.as_slice())))?,
                None => {
                    let list = edit::models_array(doc)?;
                    let found = list.iter().position(|t| edit::has_id(t, id));
                    match found {
                        Some(i) => i,
                        None => {
                            let mut t = Table::new();
                            t.insert("id", value(id));
                            t.insert("provider", value(provider));
                            list.push(t);
                            list.len() - 1
                        }
                    }
                }
            };
            let table = edit::models_array(doc)?.get_mut(index).expect("index is in range");
            table.insert("provider", value(provider));
            edit::set_str(table, "display_name", model.display_name.as_deref().filter(|n| *n != id));
            // After a rename an empty upstream keeps the one the entry has, so routing is unchanged.
            let current_upstream = table.get("upstream_model").and_then(Item::as_str).map(str::to_string);
            let upstream = model.upstream_model.as_deref().or(if original.is_some() { current_upstream.as_deref() } else { None });
            edit::set_str(table, "upstream_model", upstream.filter(|u| *u != id));
            match &model.price {
                Some(price) => {
                    table.insert("price", Item::Value(edit::price_value(price)));
                }
                None => {
                    table.remove("price");
                }
            }
            Ok(())
        })
    })())
}

/// Removes the model: its `[[models]]` entry and its place in the provider's model list.
#[tauri::command]
pub fn delete_model(id: String) -> Reply<()> {
    reply((|| {
        let paths = paths()?;
        let listed = listing_provider(&loaded()?.1, &id);
        edit::edit(&paths, |doc| {
            if let Some(Item::ArrayOfTables(list)) = doc.get_mut("models") {
                list.retain(|t| !edit::has_id(t, &id));
                if list.is_empty() {
                    doc.remove("models");
                }
            }
            if let Some((provider, effective)) = &listed {
                edit::drop_from_provider_list(doc, provider, effective, &id)?;
            }
            Ok(())
        })
    })())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEdit {
    id: String,
    /// The id the provider has now, when editing; differs from `id` for a rename.
    original_id: Option<String>,
    preset: Option<String>,
    adapter: Option<String>,
    base_url: Option<String>,
    /// A key reference (`keyring:NAME`, `env:NAME`, `none`) or the key itself.
    api_key: Option<String>,
    /// Leave `api_key` in config.toml as it is (the UI never sees an inline key).
    keep_api_key: bool,
    auth: Option<String>,
    models: Vec<String>,
    enabled: bool,
}

/// Creates or updates `[providers.<id>]`; renames it (models included) when `original_id` differs.
#[tauri::command]
pub fn save_provider(provider: ProviderEdit) -> Reply<()> {
    reply(paths().and_then(|paths| {
        edit::edit(&paths, |doc| {
            let id = provider.id.trim();
            if id.is_empty() {
                bail!("the provider needs an id");
            }
            if let Some(old) = provider.original_id.as_deref().map(str::trim).filter(|o| !o.is_empty() && *o != id) {
                edit::rename_provider(doc, old, id)?;
            }
            let providers = edit::parent_table(doc, "providers")?;
            let table = edit::child_table(providers, id)?;
            edit::set_str(table, "preset", provider.preset.as_deref().filter(|p| *p != id));
            edit::set_str(table, "adapter", provider.adapter.as_deref());
            edit::set_str(table, "base_url", provider.base_url.as_deref());
            if !provider.keep_api_key {
                edit::set_str(table, "api_key", provider.api_key.as_deref());
            }
            edit::set_str(table, "auth", provider.auth.as_deref());
            if provider.models.iter().any(|m| !m.trim().is_empty()) {
                table.insert("models", Item::Value(edit::string_array(&provider.models)));
            } else {
                table.remove("models");
            }
            if provider.enabled {
                table.remove("enabled");
            } else {
                table.insert("enabled", value(false));
            }
            Ok(())
        })
    }))
}

/// Removes the provider and the `[[models]]` entries that belong to it.
#[tauri::command]
pub fn delete_provider(id: String) -> Reply<()> {
    reply(paths().and_then(|paths| {
        edit::edit(&paths, |doc| {
            if let Some(providers) = doc.get_mut("providers").and_then(Item::as_table_mut) {
                providers.remove(&id);
            }
            if let Some(Item::ArrayOfTables(list)) = doc.get_mut("models") {
                list.retain(|t| t.get("provider").and_then(Item::as_str) != Some(id.as_str()));
                if list.is_empty() {
                    doc.remove("models");
                }
            }
            Ok(())
        })
    }))
}

/// Stores a key in the OS keyring; returns the reference to put in `api_key`.
#[tauri::command]
pub fn set_key(name: String, secret: String) -> Reply<String> {
    reply((|| {
        let secret = secret.trim();
        if secret.is_empty() {
            bail!("the key is empty");
        }
        let backend = loaded().map(|(c, _)| c.credentials.backend).unwrap_or_default();
        CredentialStore::new(backend).set_keyring(name.trim(), &Secret::new(secret))?;
        Ok(CredentialRef::Keyring(name.trim().to_string()).to_string())
    })())
}
