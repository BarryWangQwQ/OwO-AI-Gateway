use std::time::Duration;

use anyhow::{Context, Result};
use owo_config::Severity;
use owo_credentials::CredentialStore;

use crate::cli::GlobalArgs;
use crate::{context, ui};

/// `owo` / `owo status`: what is set up and the next thing to do.
pub fn overview(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    crate::banner::print();
    if !paths.config.exists() {
        println!("  {} not created yet ({})", ui::label("Config"), ui::tilde(&paths.config));
        print_next(&[
            ("owo add anthropic".into(), "add a provider; `owo providers presets` lists them"),
            ("owo start".into(), "run the gateway"),
            ("owo connect <app>".into(), "point an app at OwO AI Gateway; `owo apps` lists them"),
        ]);
        return Ok(());
    }
    println!("  {} {}", ui::label("Config"), ui::tilde(&paths.config));
    let loaded = context::load_config(&paths).and_then(|(c, _)| context::build_router(&c).map(|(r, _)| (c, r)));
    let (config, router) = match loaded {
        Ok(v) => v,
        Err(e) => {
            println!("\n  The config has problems:\n{e:#}\n");
            println!("  Fix them, then check again with `owo check`.");
            return Ok(());
        }
    };
    let registry = router.registry();

    let address = crate::cmd_run::gateway_address(&paths, &config);
    let running = crate::cmd_client::gateway_running(&address);
    let gateway = if running { format!("● running  http://{address}") } else { format!("○ stopped  {address}") };
    println!("  {} {gateway}", ui::label("Gateway"));

    let models: Vec<_> = registry.available_models().collect();
    if models.is_empty() {
        println!("  {} none available", ui::label("Models"));
    }
    let id_width = models.iter().take(MAX_MODEL_ROWS).map(|m| m.id.chars().count()).max().unwrap_or(0);
    for (i, m) in models.iter().take(MAX_MODEL_ROWS).enumerate() {
        println!("  {} ● {:<id_width$}  {}", ui::label(if i == 0 { "Models" } else { "" }), m.id, m.provider);
    }
    if models.len() > MAX_MODEL_ROWS {
        println!("  {}   +{} more  (owo models lists them all)", ui::label(""), models.len() - MAX_MODEL_ROWS);
    }

    let store = CredentialStore::new(config.credentials.backend);
    let mut missing_keys = Vec::new();
    let mut key_rows = Vec::new();
    for p in registry.providers().filter(|p| p.enabled) {
        if let Err(e) = store.resolve(&p.api_key) {
            key_rows.push(format!("✗ {}: {e}", p.id));
            if let owo_credentials::CredentialRef::Keyring(name) = &p.api_key {
                missing_keys.push(name.clone());
            }
        } else if matches!(p.api_key, owo_credentials::CredentialRef::Inline(_)) {
            key_rows.push(format!("! {0}: stored in plain text in config.toml  (move it with: owo key set {0})", p.id));
        }
    }
    for (i, row) in key_rows.iter().enumerate() {
        println!("  {} {row}", ui::label(if i == 0 { "Keys" } else { "" }));
    }

    let connected = crate::apps::connected(global);
    if connected.is_empty() {
        println!("  {} none connected", ui::label("Apps"));
    }
    for (i, app) in connected.iter().enumerate() {
        let entry = match crate::apps::usage_hint(*app) {
            Some(hint) => format!("{:<16}{hint}", app.name()),
            None => app.name().to_string(),
        };
        println!("  {} ● {entry}", ui::label(if i == 0 { "Apps" } else { "" }));
    }
    if let Some(today) = crate::cmd_usage::today_line(global) {
        println!("  {} {today}", ui::label("Today"));
    }

    let mut next: Vec<(String, &str)> = Vec::new();
    if models.is_empty() {
        next.push(("owo add <provider>".into(), "`owo providers presets` lists them"));
    }
    for k in &missing_keys {
        next.push((format!("owo key set {k}"), "store the missing key"));
    }
    if !running {
        next.push(("owo start".into(), "run the gateway"));
    }
    if connected.is_empty() {
        next.push(("owo connect <app>".into(), "`owo apps` lists them"));
    }
    print_next(&next);
    Ok(())
}

/// Models listed one per row in the overview; the rest are counted.
const MAX_MODEL_ROWS: usize = 8;

fn print_next(steps: &[(String, &str)]) {
    if steps.is_empty() {
        return;
    }
    println!("\n  Next");
    let width = steps.iter().map(|(cmd, _)| cmd.len()).max().unwrap_or(0);
    for (cmd, note) in steps {
        println!("    › {cmd:<width$}  {note}");
    }
}

/// Queries the running gateway's control API and prints its raw status.
pub fn status_json(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let url = format!("http://{}/control/v1/status", crate::cmd_client::client_address(&config.server.listen));
    let token = match &config.server.auth_token {
        Some(r) => CredentialStore::new(config.credentials.backend).resolve(r).context("cannot resolve server.auth_token")?,
        None => None,
    };
    let body: serde_json::Value = context::runtime()?.block_on(async {
        let client = reqwest::Client::builder().timeout(Duration::from_secs(5)).build()?;
        let mut req = client.get(&url);
        if let Some(t) = &token {
            req = req.bearer_auth(t.expose());
        }
        let resp = req.send().await?.error_for_status()?;
        anyhow::Ok(resp.json().await?)
    })
    .with_context(|| format!("OwO AI Gateway is not reachable at {url} (start it with `owo start`)"))?;
    println!("{}", serde_json::to_string_pretty(&body)?);
    Ok(())
}

/// Offline health check: config, registry, adapters, credentials. Never prints secret values.
pub fn check(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    println!("config: {}", paths.config.display());
    let (config, mut diags) = context::load_config(&paths)?;
    let (router, registry_diags) = context::build_router(&config)?;
    diags.extend(registry_diags);
    println!("adapters in this build: {}", router.adapter_kinds().join(", "));

    let mut problems = diags.iter().filter(|d| d.severity == Severity::Error).count();
    for d in &diags {
        println!("  {d}");
    }

    let store = CredentialStore::new(config.credentials.backend);
    for p in router.registry().providers() {
        let cred = match store.resolve(&p.api_key) {
            Ok(Some(_)) => "api key ok".to_string(),
            Ok(None) => "no api key needed".to_string(),
            Err(e) => {
                problems += 1;
                format!("api key problem: {e}")
            }
        };
        let state = if p.enabled { "enabled" } else { "disabled" };
        println!("provider {:<16} {state:<9} {} — {cred}", p.id, p.adapter);
    }
    let available = router.registry().available_models().count();
    let warnings = diags.iter().filter(|d| d.severity == Severity::Warning).count();
    println!(
        "config valid: {} provider(s), {} model(s), {warnings} warning(s)",
        router.registry().providers().count(),
        router.registry().models().count()
    );
    println!("models available: {available}");
    if available == 0 {
        problems += 1;
    }
    if problems > 0 {
        anyhow::bail!("{problems} problem(s) found");
    }
    println!("no problems found");
    Ok(())
}