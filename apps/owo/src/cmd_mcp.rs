//! `owo mcp`: one list of MCP servers in config.toml (`[mcp.<name>]`), written into the apps
//! each server is enabled for (see `owo-mcp`).

use std::collections::BTreeMap;

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use owo_client_apps::managed::Store;
use owo_config::{is_valid_mcp_name, looks_secret, mcp_keyring_name, mcp_value_ref, Config, Diagnostic, McpServerConfig, McpTransport, OwoPaths};
use owo_credentials::{CredentialStore, Secret};
use owo_mcp::sync::{self, EntryState, Relation};
use owo_mcp::{Ctx, Env, McpApp, Outcome, SyncReport};
use toml_edit::{value, Array, InlineTable, Item, Table};

use crate::cli::GlobalArgs;
use crate::{cmd_config, context};

#[derive(Subcommand)]
pub enum McpCommand {
    /// Configured servers and where each one is enabled (the default).
    List {
        /// Print as JSON (apps and servers).
        #[arg(long)]
        json: bool,
    },
    /// Add a server (`--replace` to change one), optionally enabling it for apps.
    Add(Box<AddArgs>),
    /// Take a server out of every app and delete it.
    Remove {
        name: String,
        /// Also remove entries that were edited outside OwO AI Gateway.
        #[arg(long, short)]
        force: bool,
    },
    /// Write a server into apps.
    Enable(ToggleArgs),
    /// Take a server out of apps.
    Disable(ToggleArgs),
    /// Rewrite the apps' MCP entries from config.toml (after editing it by hand or changing a key).
    Sync {
        /// Only these apps, comma-separated.
        #[arg(long, value_delimiter = ',')]
        app: Vec<String>,
        /// Replace or remove entries that were edited outside OwO AI Gateway.
        #[arg(long, short)]
        force: bool,
    },
    /// MCP servers found in the apps' own config files.
    Scan {
        #[arg(long)]
        json: bool,
    },
    /// Take servers found by `scan` into OwO AI Gateway's list; OwO AI Gateway then manages those entries
    /// (disabling a server removes it from the app; each file is backed up first).
    Import(ImportArgs),
    /// Where each app keeps its MCP servers, and whether it is installed.
    Apps {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
pub struct AddArgs {
    /// Server name: letters, digits, `-` and `_`.
    pub name: String,
    /// Command that starts a local (stdio) server.
    #[arg(long, conflicts_with = "url", required_unless_present = "url")]
    pub command: Option<String>,
    /// An argument for --command (repeat in order).
    #[arg(long = "arg", value_name = "ARG", allow_hyphen_values = true, requires = "command")]
    pub args: Vec<String>,
    /// Environment variable for the server, `KEY=VALUE`; VALUE may be `keyring:NAME` or `env:NAME`.
    #[arg(long = "env", value_name = "KEY=VALUE", requires = "command")]
    pub env: Vec<String>,
    /// Working directory for the server (only Codex and Grok Build support it).
    #[arg(long, requires = "command")]
    pub cwd: Option<String>,
    /// URL of a remote server (streamable HTTP).
    #[arg(long)]
    pub url: Option<String>,
    /// The remote server uses the older SSE transport.
    #[arg(long, requires = "url")]
    pub sse: bool,
    /// HTTP header, `KEY=VALUE`; VALUE may be `keyring:NAME` or `env:NAME`.
    #[arg(long = "header", value_name = "KEY=VALUE", requires = "url")]
    pub headers: Vec<String>,
    #[arg(long)]
    pub description: Option<String>,
    /// Apps to enable it for, comma-separated (`owo mcp apps`); with --replace the current apps are kept when omitted.
    #[arg(long, value_delimiter = ',')]
    pub app: Option<Vec<String>>,
    /// Replace the server of this name.
    #[arg(long)]
    pub replace: bool,
    /// With --replace: keep the current value of this env variable.
    #[arg(long = "keep-env", value_name = "KEY", hide = true)]
    pub keep_env: Vec<String>,
    /// With --replace: keep the current value of this header.
    #[arg(long = "keep-header", value_name = "KEY", hide = true)]
    pub keep_headers: Vec<String>,
    /// Replace or remove app entries that were edited outside OwO AI Gateway.
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Args)]
pub struct ToggleArgs {
    pub name: String,
    /// Apps, comma-separated, or `all` (every installed app).
    #[arg(long, value_delimiter = ',', required = true)]
    pub app: Vec<String>,
    /// Replace or remove entries that were edited outside OwO AI Gateway.
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Args)]
pub struct ImportArgs {
    /// The app to import from, or `all`.
    pub app: String,
    /// Servers to import (see `owo mcp scan`).
    #[arg(required_unless_present = "all")]
    pub names: Vec<String>,
    /// Every server the app has that OwO AI Gateway does not manage yet.
    #[arg(long, conflicts_with = "names")]
    pub all: bool,
    /// Move secret-looking literal values (tokens, keys) into the OS keyring; config.toml then refers to them.
    #[arg(long)]
    pub keyring: bool,
    /// Import even when an entry has settings OwO AI Gateway has no field for (they are dropped).
    #[arg(long, short)]
    pub force: bool,
}

pub fn run(global: &GlobalArgs, command: Option<McpCommand>) -> Result<()> {
    match command.unwrap_or(McpCommand::List { json: false }) {
        McpCommand::List { json } => list(global, json),
        McpCommand::Add(args) => add(global, *args),
        McpCommand::Remove { name, force } => remove(global, &name, force),
        McpCommand::Enable(args) => toggle(global, args, true),
        McpCommand::Disable(args) => toggle(global, args, false),
        McpCommand::Sync { app, force } => sync_command(global, &app, force),
        McpCommand::Scan { json } => scan(global, json),
        McpCommand::Import(args) => import(global, args),
        McpCommand::Apps { json } => apps(json),
    }
}

// ---------------------------------------------------------------------------
// Loading and saving

struct Loaded {
    paths: OwoPaths,
    config: Config,
    diagnostics: Vec<Diagnostic>,
    env: Env,
    store: Store,
    credentials: CredentialStore,
}

impl Loaded {
    fn ctx(&self) -> Ctx<'_> {
        Ctx { env: &self.env, store: &self.store, credentials: &self.credentials }
    }

    /// Config warnings about MCP servers (plain-text secrets, ...), on stderr.
    fn warn(&self) {
        context::print_diagnostics(&self.diagnostics.iter().filter(|d| d.path.starts_with("mcp.")).cloned().collect::<Vec<_>>());
    }
}

fn load(global: &GlobalArgs) -> Result<Loaded> {
    let paths = context::paths(global)?;
    let (config, diagnostics) = if paths.config.exists() { context::load_config(&paths)? } else { Config::from_toml_str("", "config.toml")? };
    let env = Env::system().context("cannot determine the home directory")?;
    let store = Store { state_dir: paths.state.clone(), backups_dir: paths.backups.clone() };
    let credentials = CredentialStore::new(config.credentials.backend);
    Ok(Loaded { paths, config, diagnostics, env, store, credentials })
}

fn app_by_name(name: &str) -> Result<McpApp> {
    McpApp::from_name(name).with_context(|| match owo_mcp::UNSUPPORTED.iter().find(|(n, _)| *n == name) {
        Some((_, reason)) => format!("`{name}`: {reason}"),
        None => format!("unknown app `{name}` (apps: {})", McpApp::ALL.map(McpApp::name).join(", ")),
    })
}

/// App names as given, checked; `all` means every installed app.
fn app_names(names: &[String], env: &Env) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for n in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        if n == "all" {
            out.extend(McpApp::ALL.into_iter().filter(|a| env.installed(*a)).map(|a| a.name().to_string()));
        } else {
            out.push(app_by_name(n)?.name().to_string());
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn string_array(items: &[String]) -> Array {
    items.iter().map(String::as_str).collect()
}

fn string_table(m: &BTreeMap<String, String>) -> InlineTable {
    m.iter().map(|(k, v)| (k.as_str(), toml_edit::Value::from(v.as_str()))).collect()
}

/// Sets every field of `[mcp.<name>]` in place (comments on the table are kept).
fn write_server(t: &mut Table, s: &McpServerConfig) {
    fn set(t: &mut Table, key: &str, item: Option<Item>) {
        match item {
            Some(i) => {
                t.insert(key, i);
            }
            None => {
                t.remove(key);
            }
        }
    }
    let str_item = |v: &Option<String>| v.as_deref().map(value);
    set(t, "description", str_item(&s.description));
    set(t, "transport", (s.transport() == McpTransport::Sse).then(|| value("sse")));
    set(t, "command", str_item(&s.command));
    set(t, "args", (!s.args.is_empty()).then(|| value(string_array(&s.args))));
    set(t, "env", (!s.env.is_empty()).then(|| value(string_table(&s.env))));
    set(t, "cwd", str_item(&s.cwd));
    set(t, "url", str_item(&s.url));
    set(t, "headers", (!s.headers.is_empty()).then(|| value(string_table(&s.headers))));
    set(t, "apps", (!s.apps.is_empty()).then(|| value(string_array(&s.apps))));
}

/// Writes the servers that differ between `before` and `after` into config.toml.
fn save(paths: &OwoPaths, before: &BTreeMap<String, McpServerConfig>, after: &BTreeMap<String, McpServerConfig>) -> Result<()> {
    if before == after {
        return Ok(());
    }
    cmd_config::edit_config(paths, |doc| {
        if doc.get("mcp").is_none() {
            let mut t = Table::new();
            t.set_implicit(true);
            doc.insert("mcp", Item::Table(t));
        }
        let mcp = doc["mcp"].as_table_mut().context("`mcp` in config.toml is not a table")?;
        for name in before.keys().filter(|n| !after.contains_key(*n)) {
            mcp.remove(name);
        }
        for (name, s) in after.iter().filter(|(n, s)| before.get(*n) != Some(s)) {
            if !mcp.contains_key(name) {
                mcp.insert(name, Item::Table(Table::new()));
            }
            let t = mcp[name.as_str()].as_table_mut().with_context(|| format!("`mcp.{name}` in config.toml is not a table"))?;
            write_server(t, s);
        }
        if mcp.is_empty() {
            doc.remove("mcp");
        }
        Ok(())
    })
}

fn print_report(r: &SyncReport) {
    let head = match r.outcome {
        Outcome::Synced => "updated".to_string(),
        Outcome::Removed => "OwO AI Gateway's entries removed".to_string(),
        Outcome::Unchanged => "nothing to write".to_string(),
        Outcome::NotInstalled => "not installed, skipped".to_string(),
        Outcome::Failed => format!("FAILED — {}", r.message.as_deref().unwrap_or("")),
    };
    println!("{:<15} {head}", format!("{}:", r.app));
    for l in &r.lines {
        println!("{:<15} {l}", "");
    }
    for w in &r.warnings {
        println!("{:<15} warning: {w}", "");
    }
}

/// The servers with `app`'s membership taken from `from` (for apps whose sync failed).
fn with_membership_of(servers: &BTreeMap<String, McpServerConfig>, from: &BTreeMap<String, McpServerConfig>, app: McpApp) -> BTreeMap<String, McpServerConfig> {
    let is_app = |n: &String| McpApp::from_name(n) == Some(app);
    let mut out = servers.clone();
    for (name, old) in from {
        if !out.contains_key(name) && old.apps.iter().any(is_app) {
            out.insert(name.clone(), McpServerConfig { apps: old.apps.iter().filter(|n| is_app(n)).cloned().collect(), ..old.clone() });
        }
    }
    for (name, s) in out.iter_mut() {
        s.apps.retain(|n| !is_app(n));
        if let Some(old) = from.get(name) {
            s.apps.extend(old.apps.iter().filter(|n| is_app(n)).cloned());
        }
        s.apps.sort();
    }
    out
}

/// Saves `after`, syncs `apps`, and for every app whose sync failed (or that could not take
/// the `focus` server) puts its membership back as it was, so config.toml keeps describing
/// what the app's files hold.
fn apply(l: &Loaded, after: BTreeMap<String, McpServerConfig>, apps: &[McpApp], force: bool, focus: Option<&str>) -> Result<()> {
    let before = &l.config.mcp;
    save(&l.paths, before, &after)?;
    l.paths.ensure_dirs()?;
    let mut failed = Vec::new();
    for app in apps {
        let r = owo_mcp::sync_app(&l.ctx(), &after, *app, force);
        print_report(&r);
        if !r.ok() || focus.is_some_and(|n| r.skipped.iter().any(|s| s == n)) {
            failed.push(*app);
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    let mut kept = after.clone();
    for app in &failed {
        kept = with_membership_of(&kept, before, *app);
    }
    save(&l.paths, &after, &kept)?;
    bail!("{} not changed (see above); config.toml still lists what they hold", failed.iter().map(|a| a.name()).collect::<Vec<_>>().join(", "))
}

fn affected(servers: &[&McpServerConfig]) -> Vec<McpApp> {
    let mut apps: Vec<McpApp> = servers.iter().flat_map(|s| sync::server_apps(s)).collect();
    apps.sort();
    apps.dedup();
    apps
}

// ---------------------------------------------------------------------------
// Commands

fn parse_pairs(items: &[String], what: &str) -> Result<BTreeMap<String, String>> {
    items
        .iter()
        .map(|kv| {
            let (k, v) = kv.split_once('=').with_context(|| format!("{what} `{}` must be KEY=VALUE", kv.split('=').next().unwrap_or("")))?;
            Ok((k.trim().to_string(), v.to_string()))
        })
        .collect()
}

fn add(global: &GlobalArgs, args: AddArgs) -> Result<()> {
    let l = load(global)?;
    let name = args.name.trim().to_string();
    if !is_valid_mcp_name(&name) {
        bail!("`{name}` is not a valid server name (letters, digits, `-` and `_`, at most 64)");
    }
    let old = l.config.mcp.get(&name);
    if old.is_some() && !args.replace {
        bail!("there is already an MCP server `{name}` (use --replace to change it)");
    }
    let mut env = parse_pairs(&args.env, "--env")?;
    let mut headers = parse_pairs(&args.headers, "--header")?;
    for (keep, map, old_map) in [(&args.keep_env, &mut env, old.map(|o| &o.env)), (&args.keep_headers, &mut headers, old.map(|o| &o.headers))] {
        for k in keep {
            let v = old_map.and_then(|m| m.get(k)).with_context(|| format!("`{name}` has no `{k}` to keep"))?;
            map.insert(k.clone(), v.clone());
        }
    }
    let apps = match &args.app {
        Some(list) => app_names(list, &l.env)?,
        None => old.map(|o| o.apps.clone()).unwrap_or_default(),
    };
    let server = McpServerConfig {
        description: args.description.filter(|d| !d.trim().is_empty()),
        transport: args.sse.then_some(McpTransport::Sse),
        command: args.command,
        args: args.args,
        env,
        cwd: args.cwd.filter(|c| !c.trim().is_empty()),
        url: args.url,
        headers,
        apps,
    };
    let mut after = l.config.mcp.clone();
    after.insert(name.clone(), server.clone());
    let apps = affected(&[&server].into_iter().chain(old).collect::<Vec<_>>());
    println!("config:         [mcp.{name}] {} in {}", if old.is_some() { "updated" } else { "added" }, l.paths.config.display());
    let result = apply(&l, after, &apps, args.force, Some(&name));
    if let Ok(reloaded) = load(global) {
        reloaded.warn();
    }
    result
}

fn remove(global: &GlobalArgs, name: &str, force: bool) -> Result<()> {
    let l = load(global)?;
    let old = l.config.mcp.get(name).with_context(|| format!("there is no MCP server `{name}`"))?;
    let mut after = l.config.mcp.clone();
    after.remove(name);
    let apps = affected(&[old]);
    apply(&l, after, &apps, force, None)?;
    println!("config:         [mcp.{name}] removed from {}", l.paths.config.display());
    Ok(())
}

fn toggle(global: &GlobalArgs, args: ToggleArgs, on: bool) -> Result<()> {
    let l = load(global)?;
    let server = l.config.mcp.get(&args.name).with_context(|| format!("there is no MCP server `{}` (add it with `owo mcp add`)", args.name))?;
    let names = app_names(&args.app, &l.env)?;
    let mut changed = server.clone();
    let targets: Vec<McpApp> = names.iter().filter_map(|n| McpApp::from_name(n)).collect();
    changed.apps.retain(|a| !McpApp::from_name(a).is_some_and(|x| targets.contains(&x)));
    if on {
        changed.apps.extend(names);
        changed.apps.sort();
    }
    let mut after = l.config.mcp.clone();
    after.insert(args.name.clone(), changed);
    apply(&l, after, &targets, args.force, Some(&args.name))
}

fn sync_command(global: &GlobalArgs, apps: &[String], force: bool) -> Result<()> {
    let l = load(global)?;
    l.warn();
    let targets: Vec<McpApp> = if apps.is_empty() { McpApp::ALL.to_vec() } else { apps.iter().map(|a| app_by_name(a)).collect::<Result<_>>()? };
    apply(&l, l.config.mcp.clone(), &targets, force, None)
}

fn state_mark(s: EntryState) -> &'static str {
    match s {
        EntryState::Synced => "✓",
        EntryState::Outdated => " (outdated: owo mcp sync)",
        EntryState::Modified => " (edited outside OwO AI Gateway)",
        EntryState::Missing => " (deleted outside OwO AI Gateway)",
        EntryState::Pending => " (not written yet: owo mcp sync)",
        EntryState::NotInstalled => " (not installed)",
        EntryState::Unsupported => " (unsupported)",
        EntryState::Stale => " (disabled, entry still there)",
        EntryState::Off => "",
    }
}

fn list(global: &GlobalArgs, json: bool) -> Result<()> {
    let l = load(global)?;
    let servers = sync::list(&l.ctx(), &l.config.mcp)?;
    if json {
        let out = serde_json::json!({ "apps": sync::app_infos(&l.env), "servers": servers });
        println!("{}", serde_json::to_string_pretty(&out)?);
        return Ok(());
    }
    l.warn();
    if servers.is_empty() {
        println!("No MCP servers yet. Add one with `owo mcp add <name> --command … | --url …`, or bring in the apps' own with `owo mcp scan` / `owo mcp import`.");
        return Ok(());
    }
    println!("{:<20} {:<9} {:<44} APPS", "NAME", "TRANSPORT", "COMMAND / URL");
    for s in &servers {
        let apps: Vec<String> = s.apps.iter().filter(|a| a.state != EntryState::Off).map(|a| format!("{}{}", a.app, state_mark(a.state))).collect();
        let mut summary = s.view.summary.clone();
        if summary.chars().count() > 44 {
            summary = format!("{}…", summary.chars().take(43).collect::<String>());
        }
        println!("{:<20} {:<9} {:<44} {}", s.name, s.view.transport.as_str(), summary, if apps.is_empty() { "-".into() } else { apps.join(", ") });
        for kv in s.view.env.iter().chain(&s.view.headers) {
            let shown = kv.value.clone().unwrap_or_else(|| "(hidden)".into());
            let missing = kv.missing.as_ref().map(|m| format!("  — {m}")).unwrap_or_default();
            println!("{:<20}   {}={shown}{missing}", "", kv.key);
        }
    }
    Ok(())
}

fn scan(global: &GlobalArgs, json: bool) -> Result<()> {
    let l = load(global)?;
    let found: Vec<sync::Found> = sync::scan(&l.ctx(), &l.config.mcp)?.into_iter().map(|c| c.found).collect();
    if json {
        println!("{}", serde_json::to_string_pretty(&found)?);
        return Ok(());
    }
    if found.is_empty() {
        println!("No installed app has MCP servers configured.");
        return Ok(());
    }
    let mut file = None;
    for f in &found {
        if file != Some(&f.file) {
            println!("\n{} — {}", f.app, f.file.display());
            file = Some(&f.file);
        }
        if f.name.is_empty() {
            println!("  cannot read: {}", f.error.as_deref().unwrap_or(""));
            continue;
        }
        let note = match (f.managed, f.relation, &f.error) {
            (true, _, _) => "managed by OwO AI Gateway".to_string(),
            (false, _, Some(e)) => format!("cannot import: {e}"),
            (false, Relation::Different, _) => "OwO AI Gateway has a different server with this name".to_string(),
            (false, Relation::Same, _) => format!("same as OwO AI Gateway's `{}` (import to manage it)", f.name),
            (false, Relation::New, _) => format!("owo mcp import {} {}", f.app, f.name),
        };
        let summary = f.server.as_ref().map(|s| s.summary.as_str()).unwrap_or("");
        println!("  {:<20} {:<44} {note}", f.name, summary);
        if !f.dropped.is_empty() {
            println!("  {:<20} settings OwO AI Gateway would drop: {}", "", f.dropped.join(", "));
        }
    }
    Ok(())
}

fn import(global: &GlobalArgs, args: ImportArgs) -> Result<()> {
    let l = load(global)?;
    let from = if args.app == "all" { None } else { Some(app_by_name(&args.app)?) };
    let candidates: Vec<sync::Candidate> = sync::scan(&l.ctx(), &l.config.mcp)?
        .into_iter()
        .filter(|c| !c.found.managed && !c.found.name.is_empty())
        .filter(|c| from.is_none_or(|a| a.name() == c.found.app))
        .filter(|c| args.all || args.names.contains(&c.found.name))
        .collect();
    for n in &args.names {
        if !candidates.iter().any(|c| &c.found.name == n) {
            bail!("no unmanaged MCP server `{n}` in {} (see `owo mcp scan`)", from.map_or("any app", McpApp::label));
        }
    }
    if candidates.is_empty() {
        println!("Nothing to import: every MCP server the apps have is already managed by OwO AI Gateway.");
        return Ok(());
    }

    let mut after = l.config.mcp.clone();
    // (app, file, name) entries OwO AI Gateway takes over.
    let mut takeovers = Vec::new();
    let mut problems = Vec::new();
    for c in &candidates {
        let f = &c.found;
        let label = format!("{} `{}`", f.app, f.name);
        let Some(parsed) = &c.parsed else {
            problems.push(format!("{label}: {}", f.error.as_deref().unwrap_or("cannot be read")));
            continue;
        };
        if !parsed.dropped.is_empty() && !args.force {
            problems.push(format!("{label}: has settings OwO AI Gateway has no field for ({}); re-run with --force to import without them", parsed.dropped.join(", ")));
            continue;
        }
        match after.get_mut(&f.name) {
            Some(existing) if sync::same_server(&l.credentials, existing, &parsed.server) => {
                existing.apps.push(f.app.to_string());
                existing.apps.sort();
                existing.apps.dedup();
            }
            Some(_) => {
                problems.push(format!("{label}: OwO AI Gateway already has a different server named `{}`; rename one of them", f.name));
                continue;
            }
            None => {
                let mut server = parsed.server.clone();
                if args.keyring {
                    for (field, map) in [("env", &mut server.env), ("headers", &mut server.headers)] {
                        for (k, v) in map.iter_mut().filter(|(k, v)| looks_secret(k) && !v.is_empty() && mcp_value_ref(v).is_none()) {
                            let entry = mcp_keyring_name(&f.name, k);
                            l.credentials.set_keyring(&entry, &Secret::new(v.as_str())).with_context(|| format!("cannot store {label} {field}.{k} in the keyring"))?;
                            println!("keyring:        {field}.{k} of `{}` stored as keyring:{entry}", f.name);
                            *v = format!("keyring:{entry}");
                        }
                    }
                }
                server.apps = vec![f.app.to_string()];
                after.insert(f.name.clone(), server);
            }
        }
        takeovers.push((app_by_name(f.app)?, f.file.clone(), f.name.clone()));
    }
    for p in &problems {
        eprintln!("skipped: {p}");
    }
    if takeovers.is_empty() {
        bail!("nothing was imported");
    }
    save(&l.paths, &l.config.mcp, &after)?;
    l.paths.ensure_dirs()?;
    for (app, file, name) in &takeovers {
        for line in sync::adopt(&l.ctx(), *app, file, name)? {
            println!("{:<15} {line}", format!("{}:", app.name()));
        }
        println!("config:         `{name}` enabled for {}", app.name());
    }
    let mut apps: Vec<McpApp> = takeovers.iter().map(|(a, _, _)| *a).collect();
    apps.sort();
    apps.dedup();
    let mut failed = false;
    for app in apps {
        let r = owo_mcp::sync_app(&l.ctx(), &after, app, false);
        print_report(&r);
        failed |= !r.ok();
    }
    if failed || !problems.is_empty() {
        bail!("some servers were not imported or written (see above)");
    }
    Ok(())
}

fn apps(json: bool) -> Result<()> {
    let env = Env::system().context("cannot determine the home directory")?;
    let infos = sync::app_infos(&env);
    if json {
        println!("{}", serde_json::to_string_pretty(&infos)?);
        return Ok(());
    }
    println!("{:<16} {:<10} {:<16} FILE", "APP", "INSTALLED", "TRANSPORTS");
    for a in &infos {
        if !a.supported {
            println!("{:<16} {:<10} {:<16} no MCP support: {}", a.app, "-", "-", a.reason.as_deref().unwrap_or(""));
            continue;
        }
        let transports = a.transports.iter().map(|t| t.as_str()).collect::<Vec<_>>().join(",");
        let files = a.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>().join(", ");
        let file = if files.is_empty() { "-".to_string() } else { files };
        println!("{:<16} {:<10} {:<16} {file}", a.app, if a.installed { "yes" } else { "no" }, transports);
        if let Some(r) = &a.reason {
            println!("{:<16} {r}", "");
        }
    }
    println!("\n`codex` covers Codex CLI and Codex Desktop (one config.toml; `codex-desktop` is accepted too).");
    Ok(())
}
