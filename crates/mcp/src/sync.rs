//! Writing the configured servers into apps, and reading back what the apps have.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};
use owo_client_apps::managed::{self, FileEdit, Fragment, Options, Seg, Store};
use owo_config::{is_valid_mcp_name, looks_secret, mcp_value_ref, McpServerConfig, McpTransport};
use owo_credentials::CredentialStore;
use serde::Serialize;
use serde_json::Value;

use crate::apps::{Env, McpApp, UNSUPPORTED};
use crate::render::{parse, render, unsupported_reason, Parsed, Resolved};

/// Everything a sync needs: where the apps live, OwO AI Gateway's state, and the keyring.
pub struct Ctx<'a> {
    pub env: &'a Env,
    pub store: &'a Store,
    pub credentials: &'a CredentialStore,
}

/// The targets a server is enabled for, deduplicated (`codex-desktop` is `codex`).
pub fn server_apps(server: &McpServerConfig) -> Vec<McpApp> {
    let mut apps: Vec<McpApp> = server.apps.iter().filter_map(|a| McpApp::from_name(a)).collect();
    apps.sort();
    apps.dedup();
    apps
}

/// Replaces credential references in `env` and `headers` with their values.
pub fn resolve(credentials: &CredentialStore, name: &str, server: &McpServerConfig) -> Result<Resolved> {
    let values = |field: &str, m: &BTreeMap<String, String>| -> Result<BTreeMap<String, String>> {
        m.iter()
            .map(|(k, v)| {
                let value = match mcp_value_ref(v) {
                    None => v.clone(),
                    Some(r) => {
                        let r = r.map_err(|e| anyhow!("mcp.{name}.{field}.{k}: {e}"))?;
                        let secret = credentials.resolve(&r).map_err(|e| anyhow!("mcp.{name}.{field}.{k}: {e}"))?;
                        secret.map(|s| s.expose().to_string()).unwrap_or_default()
                    }
                };
                Ok((k.clone(), value))
            })
            .collect()
    };
    Ok(Resolved {
        transport: server.transport(),
        command: server.command.clone().unwrap_or_default(),
        args: server.args.clone(),
        env: values("env", &server.env)?,
        cwd: server.cwd.clone(),
        url: server.url.clone().unwrap_or_default(),
        headers: values("headers", &server.headers)?,
    })
}

/// Whether two definitions are the same server once their references are resolved.
pub fn same_server(credentials: &CredentialStore, a: &McpServerConfig, b: &McpServerConfig) -> bool {
    a.same_server(b) || matches!((resolve(credentials, "", a), resolve(credentials, "", b)), (Ok(x), Ok(y)) if x == y)
}

fn server_name(path: &[Seg]) -> &str {
    match path.last() {
        Some(Seg::Key(k)) => k,
        _ => "?",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    /// The app's file now holds exactly the enabled servers.
    Synced,
    /// No server is enabled for the app any more; OwO AI Gateway's changes were undone.
    Removed,
    /// Nothing to do.
    Unchanged,
    NotInstalled,
    /// Nothing was written; `message` says why.
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncReport {
    pub app: &'static str,
    pub outcome: Outcome,
    pub message: Option<String>,
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
    /// Enabled servers that were not (re)written: the app cannot run them, or a key they
    /// refer to cannot be read (an entry written before is then left as it was).
    pub skipped: Vec<String>,
}

impl SyncReport {
    fn new(app: McpApp, outcome: Outcome) -> Self {
        Self { app: app.name(), outcome, message: None, lines: Vec::new(), warnings: Vec::new(), skipped: Vec::new() }
    }

    fn failed(app: McpApp, message: String, warnings: Vec<String>) -> Self {
        Self { message: Some(message), warnings, ..Self::new(app, Outcome::Failed) }
    }

    pub fn ok(&self) -> bool {
        self.outcome != Outcome::Failed
    }
}

/// Makes `app`'s MCP config hold exactly the servers enabled for it. Plans first and writes
/// nothing when an entry was changed outside OwO AI Gateway or clashes with one it does not manage
/// (unless `force`).
pub fn sync_app(ctx: &Ctx, servers: &BTreeMap<String, McpServerConfig>, app: McpApp, force: bool) -> SyncReport {
    match try_sync(ctx, servers, app, force) {
        Ok(r) => r,
        Err(e) => SyncReport::failed(app, format!("{e:#}"), Vec::new()),
    }
}

fn try_sync(ctx: &Ctx, servers: &BTreeMap<String, McpServerConfig>, app: McpApp, force: bool) -> Result<SyncReport> {
    let client = app.state_id();
    let prior = ctx.store.load(&client)?;
    let wanted: Vec<(&String, &McpServerConfig)> = servers.iter().filter(|(_, s)| server_apps(s).contains(&app)).collect();
    if !ctx.env.installed(app) {
        let mut r = SyncReport::new(app, Outcome::NotInstalled);
        if !wanted.is_empty() || prior.is_some() {
            r.warnings.push(format!("{} is not installed; its MCP config was left alone", app.label()));
        }
        return Ok(r);
    }
    if let Some(reason) = ctx.env.blocked(app) {
        return Ok(SyncReport::failed(app, reason, Vec::new()));
    }

    let files = ctx.env.files(app);
    let format = app.format();
    let mut warnings = Vec::new();
    let mut skipped = Vec::new();
    let mut fragments = Vec::new();
    for (name, server) in wanted {
        if let Some(reason) = unsupported_reason(app, server) {
            warnings.push(format!("`{name}` not written: {reason}"));
            skipped.push(name.clone());
            continue;
        }
        let path = app.entry_path(name);
        match resolve(ctx.credentials, name, server) {
            Ok(resolved) => fragments.push(Fragment { path, value: render(app, &resolved) }),
            Err(e) => {
                skipped.push(name.clone());
                // Keep what OwO AI Gateway wrote before, so one missing key does not remove the server.
                let kept = files.first().and_then(|file| {
                    let s = prior.as_ref()?.files.iter().find(|f| f.path == *file)?.fragments.iter().find(|s| s.path == path)?;
                    managed::read_value(file, format, &path).ok()?.filter(|v| managed::is_written(s, v))
                });
                match kept {
                    Some(value) => {
                        warnings.push(format!("`{name}` left as it was: {e:#}"));
                        fragments.push(Fragment { path, value });
                    }
                    None => warnings.push(format!("`{name}` not written: {e:#}")),
                }
            }
        }
    }

    let mut conflicts = Vec::new();
    for file in &files {
        let prior_file = prior.as_ref().and_then(|p| p.files.iter().find(|f| f.path == *file));
        let owned = |path: &[Seg]| prior_file.and_then(|f| f.fragments.iter().find(|s| s.path == path));
        for frag in &fragments {
            let name = server_name(&frag.path);
            match (owned(&frag.path), managed::read_value(file, format, &frag.path)?) {
                (Some(s), Some(current)) if !managed::is_written(s, &current) => {
                    conflicts.push(format!("`{name}` in {} was changed outside OwO AI Gateway", file.display()))
                }
                (None, Some(_)) => conflicts.push(format!(
                    "{} already has an MCP server `{name}` that OwO AI Gateway does not manage; import it (`owo mcp import {} {name}`) or rename one of them",
                    file.display(),
                    app.name()
                )),
                _ => {}
            }
        }
        for s in prior_file.map(|f| f.fragments.as_slice()).unwrap_or_default() {
            if fragments.iter().any(|f| f.path == s.path) {
                continue;
            }
            if managed::read_value(file, format, &s.path)?.is_some_and(|current| !managed::is_written(s, &current)) {
                conflicts.push(format!("`{}` in {} was changed outside OwO AI Gateway, so it was not removed", server_name(&s.path), file.display()));
            }
        }
    }
    if !conflicts.is_empty() && !force {
        let message = format!("{}\n(nothing was written to {}; re-run with --force to replace or remove these entries anyway)", conflicts.join("\n"), app.label());
        return Ok(SyncReport { skipped, ..SyncReport::failed(app, message, warnings) });
    }

    let report = if fragments.is_empty() {
        if prior.is_none() {
            return Ok(SyncReport { warnings, skipped, ..SyncReport::new(app, Outcome::Unchanged) });
        }
        (Outcome::Removed, managed::restore(ctx.store, &client, force)?)
    } else {
        let edits: Vec<FileEdit> = files.iter().map(|f| FileEdit { path: f.clone(), format, fragments: fragments.clone() }).collect();
        (Outcome::Synced, managed::enable_with(ctx.store, &client, &edits, Options { force, redact: true })?)
    };
    let (outcome, report) = report;
    warnings.extend(report.warnings);
    Ok(SyncReport { app: app.name(), outcome, message: None, lines: report.lines, warnings, skipped })
}

/// Records an entry the app already has as OwO AI Gateway's own (see [`managed::adopt`]).
pub fn adopt(ctx: &Ctx, app: McpApp, file: &Path, name: &str) -> Result<Vec<String>> {
    Ok(managed::adopt(ctx.store, &app.state_id(), file, app.format(), &app.entry_path(name), true)?.lines)
}

// ---------------------------------------------------------------------------
// Views (what `owo mcp list/scan --json` print). Secrets never appear in them.

/// One `env` or header entry. A literal value under a secret-looking name is hidden.
#[derive(Debug, Clone, Serialize)]
pub struct KeyValue {
    pub key: String,
    /// `None` when hidden.
    pub value: Option<String>,
    /// The value is `keyring:NAME` / `env:NAME`.
    pub reference: bool,
    /// A reference whose value cannot be read right now (missing keyring entry or variable).
    pub missing: Option<String>,
    pub secret: bool,
}

fn key_values(m: &BTreeMap<String, String>, credentials: Option<&CredentialStore>) -> Vec<KeyValue> {
    m.iter()
        .map(|(k, v)| {
            let reference = mcp_value_ref(v);
            let secret = looks_secret(k);
            let missing = match (&reference, credentials) {
                (Some(Ok(r)), Some(c)) => c.resolve(r).err().map(|e| e.to_string()),
                (Some(Err(e)), _) => Some(e.to_string()),
                _ => None,
            };
            let hidden = reference.is_none() && secret && !v.is_empty();
            KeyValue { key: k.clone(), value: (!hidden).then(|| v.clone()), reference: reference.is_some(), missing, secret }
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerView {
    pub transport: McpTransport,
    pub description: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub url: Option<String>,
    pub env: Vec<KeyValue>,
    pub headers: Vec<KeyValue>,
    /// `command args…` or the URL, for one-line display.
    pub summary: String,
}

impl ServerView {
    pub fn new(server: &McpServerConfig, credentials: Option<&CredentialStore>) -> Self {
        let summary = match server.transport() {
            McpTransport::Stdio => std::iter::once(server.command.clone().unwrap_or_default()).chain(server.args.iter().cloned()).collect::<Vec<_>>().join(" "),
            _ => server.url.clone().unwrap_or_default(),
        };
        Self {
            transport: server.transport(),
            description: server.description.clone(),
            command: server.command.clone(),
            args: server.args.clone(),
            cwd: server.cwd.clone(),
            url: server.url.clone(),
            env: key_values(&server.env, credentials),
            headers: key_values(&server.headers, credentials),
            summary,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryState {
    /// Enabled, and the app's file holds what OwO AI Gateway wrote.
    Synced,
    /// Enabled and untouched, but written from an older definition or key (`owo mcp sync`).
    Outdated,
    /// Enabled, but the entry was edited outside OwO AI Gateway.
    Modified,
    /// Enabled, but the entry was deleted outside OwO AI Gateway.
    Missing,
    /// Enabled, not written yet (a failed or pending sync).
    Pending,
    /// Enabled, but the app is not installed.
    NotInstalled,
    /// Enabled, but the app cannot run this server.
    Unsupported,
    /// Disabled, but OwO AI Gateway's entry is still in the file (removal was blocked by an edit).
    Stale,
    Off,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppState {
    pub app: &'static str,
    pub enabled: bool,
    pub state: EntryState,
    /// Why the app cannot run this server, if it cannot.
    pub unsupported: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ListedServer {
    pub name: String,
    #[serde(flatten)]
    pub view: ServerView,
    pub apps: Vec<AppState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub app: &'static str,
    pub label: &'static str,
    pub supported: bool,
    pub installed: bool,
    pub files: Vec<PathBuf>,
    pub transports: Vec<McpTransport>,
    pub cwd: bool,
    /// Why OwO AI Gateway cannot write to the app (supported apps: an unwritable file).
    pub reason: Option<String>,
}

pub fn app_infos(env: &Env) -> Vec<AppInfo> {
    let supported = McpApp::ALL.into_iter().map(|app| AppInfo {
        app: app.name(),
        label: app.label(),
        supported: true,
        installed: env.installed(app),
        files: env.files(app),
        transports: app.transports().to_vec(),
        cwd: app.supports_cwd(),
        reason: env.blocked(app),
    });
    let unsupported = UNSUPPORTED.iter().map(|(app, reason)| AppInfo {
        app,
        label: app,
        supported: false,
        installed: false,
        files: Vec::new(),
        transports: Vec::new(),
        cwd: false,
        reason: Some(reason.to_string()),
    });
    supported.chain(unsupported).collect()
}

/// Where each app stands for one server.
pub fn server_states(ctx: &Ctx, name: &str, server: &McpServerConfig) -> Result<Vec<AppState>> {
    let enabled_for = server_apps(server);
    let mut out = Vec::new();
    for app in McpApp::ALL {
        let enabled = enabled_for.contains(&app);
        let unsupported = unsupported_reason(app, server);
        let path = app.entry_path(name);
        let owned = ctx.store.load(&app.state_id())?.and_then(|st| {
            st.files.into_iter().find_map(|f| {
                let s = f.fragments.iter().find(|s| s.path == path)?.clone();
                Some((f.path, s))
            })
        });
        let state = match (owned, enabled) {
            (Some((file, s)), true) => match managed::read_value(&file, app.format(), &path).ok().flatten() {
                Some(v) if managed::is_written(&s, &v) => {
                    let current = unsupported.is_none().then(|| resolve(ctx.credentials, name, server).ok()).flatten();
                    match current {
                        Some(r) if !managed::is_written(&s, &render(app, &r)) => EntryState::Outdated,
                        _ => EntryState::Synced,
                    }
                }
                Some(_) => EntryState::Modified,
                None => EntryState::Missing,
            },
            (Some(_), false) => EntryState::Stale,
            (None, true) if !ctx.env.installed(app) => EntryState::NotInstalled,
            (None, true) if unsupported.is_some() => EntryState::Unsupported,
            (None, true) => EntryState::Pending,
            (None, false) => EntryState::Off,
        };
        out.push(AppState { app: app.name(), enabled, state, unsupported });
    }
    Ok(out)
}

pub fn list(ctx: &Ctx, servers: &BTreeMap<String, McpServerConfig>) -> Result<Vec<ListedServer>> {
    servers
        .iter()
        .map(|(name, s)| Ok(ListedServer { name: name.clone(), view: ServerView::new(s, Some(ctx.credentials)), apps: server_states(ctx, name, s)? }))
        .collect()
}

/// How an app's entry relates to OwO AI Gateway's server of the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    /// OwO AI Gateway has no server of that name.
    New,
    /// OwO AI Gateway has the same server under that name.
    Same,
    /// OwO AI Gateway has a different server under that name.
    Different,
}

#[derive(Debug, Clone, Serialize)]
pub struct Found {
    pub app: &'static str,
    pub file: PathBuf,
    pub name: String,
    /// OwO AI Gateway wrote (or adopted) this entry.
    pub managed: bool,
    pub relation: Relation,
    /// `None` when the entry cannot be read as a server (`error` says why).
    pub server: Option<ServerView>,
    pub error: Option<String>,
    /// The entry's settings OwO AI Gateway has no field for.
    pub dropped: Vec<String>,
    /// Literal values under secret-looking names (`env.X`, `headers.Y`), offered for the keyring.
    pub secrets: Vec<String>,
}

/// An entry found in an app, with the server it reads as.
pub struct Candidate {
    pub found: Found,
    pub parsed: Option<Parsed>,
}

/// Every MCP server entry in every installed app's config.
pub fn scan(ctx: &Ctx, servers: &BTreeMap<String, McpServerConfig>) -> Result<Vec<Candidate>> {
    let mut out = Vec::new();
    for app in McpApp::ALL {
        let owned = ctx.store.load(&app.state_id())?;
        for file in ctx.env.files(app) {
            let entries = match managed::read_value(&file, app.format(), &app.container()) {
                Ok(Some(Value::Object(entries))) => entries,
                Ok(_) => continue,
                Err(e) => {
                    out.push(Candidate {
                        found: Found {
                            app: app.name(),
                            file: file.clone(),
                            name: String::new(),
                            managed: false,
                            relation: Relation::New,
                            server: None,
                            error: Some(format!("{e:#}")),
                            dropped: Vec::new(),
                            secrets: Vec::new(),
                        },
                        parsed: None,
                    });
                    continue;
                }
            };
            for (name, entry) in entries {
                let path = app.entry_path(&name);
                let managed = owned.as_ref().is_some_and(|st| st.files.iter().any(|f| f.path == file && f.fragments.iter().any(|s| s.path == path)));
                let parsed = parse(app, &entry).and_then(|p| if is_valid_mcp_name(&name) { Ok(p) } else { Err("OwO AI Gateway server names may only contain [A-Za-z0-9-_]".into()) });
                let (server, error, dropped, secrets, relation) = match &parsed {
                    Ok(p) => {
                        let secrets = p
                            .server
                            .env
                            .iter()
                            .map(|(k, v)| (format!("env.{k}"), k, v))
                            .chain(p.server.headers.iter().map(|(k, v)| (format!("headers.{k}"), k, v)))
                            .filter(|(_, k, v)| looks_secret(k) && !v.is_empty() && mcp_value_ref(v).is_none())
                            .map(|(label, _, _)| label)
                            .collect();
                        let relation = match servers.get(&name) {
                            None => Relation::New,
                            Some(s) if same_server(ctx.credentials, s, &p.server) => Relation::Same,
                            Some(_) => Relation::Different,
                        };
                        (Some(ServerView::new(&p.server, None)), None, p.dropped.clone(), secrets, relation)
                    }
                    Err(e) => (None, Some(e.clone()), Vec::new(), Vec::new(), if servers.contains_key(&name) { Relation::Different } else { Relation::New }),
                };
                out.push(Candidate {
                    found: Found { app: app.name(), file: file.clone(), name, managed, relation, server, error, dropped, secrets },
                    parsed: parsed.ok(),
                });
            }
        }
    }
    Ok(out)
}
