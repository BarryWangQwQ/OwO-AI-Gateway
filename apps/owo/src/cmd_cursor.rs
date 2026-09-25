//! Cursor integration: local CA, the takeover switch in OwO AI Gateway's config, and the
//! embedded Cursor backend that runs inside `owo start`.

use std::path::Path;
use std::sync::Arc;

use anyhow::{bail, Result};
use cursor_server::local_app::{CaState, CursorHarness, TrustCommand, CA_COMMON_NAME};
use tokio_util::sync::CancellationToken;
use owo_config::OwoPaths;

use crate::cli::GlobalArgs;
use crate::context;

pub const CLIENT_ID: &str = "cursor";
/// Call records are only read back for a live conversation's context accounting.
const CALL_HISTORY_RETENTION: std::time::Duration = std::time::Duration::from_secs(30 * 24 * 3600);

fn data_dir(paths: &OwoPaths) -> std::path::PathBuf {
    paths.state.join("cursor")
}

async fn harness(paths: &OwoPaths) -> Result<CursorHarness> {
    cursor_server::config::set_managed_data_dir(data_dir(paths));
    let config = cursor_server::Config::for_owo("127.0.0.1:0".parse().expect("static address"))?;
    let store = cursor_server::store::Store::connect(&config.database_url).await?;
    Ok(CursorHarness::new(store)?)
}

/// The switch `owo start` reads: present while Cursor is connected. It lives in OwO AI Gateway's
/// state, not in the user's config.toml.
fn switch_path(paths: &OwoPaths) -> std::path::PathBuf {
    data_dir(paths).join("connected")
}

/// Whether `owo start` should run the Cursor backend.
pub fn connected(paths: &OwoPaths) -> bool {
    switch_path(paths).exists()
}

fn set_connected(paths: &OwoPaths, on: bool) -> Result<()> {
    let path = switch_path(paths);
    if on {
        std::fs::create_dir_all(data_dir(paths))?;
        std::fs::write(&path, b"")?;
    } else if path.exists() {
        std::fs::remove_file(&path)?;
    }
    Ok(())
}

/// Runs the OS trust-store commands with administrator rights.
fn run_trust_commands(commands: &[TrustCommand], log: &Path) -> Result<()> {
    for c in commands {
        crate::elevate::run(&c.program, &c.args, log)?;
    }
    Ok(())
}

pub fn enable(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, _) = context::build_router(&config)?;
    let models = router.registry().available_models().count();
    if models == 0 {
        bail!("no models are available; add a provider with models to OwO AI Gateway's config.toml first");
    }
    paths.ensure_dirs()?;
    context::runtime()?.block_on(async {
        let harness = harness(&paths).await?;
        let mut status = harness.initialize_ca().await?;
        let cert = harness.ca_certificate_path();
        println!("CA:       {} ({CA_COMMON_NAME}; may only issue for *.cursor.sh)", cert.display());
        if matches!(status.ca, CaState::Untrusted) {
            println!("Adding the CA to the system trust store (administrator approval needed)...");
            if let Err(e) = run_trust_commands(&harness.ca_trust_commands(), &data_dir(&paths).join("elevated.log")) {
                let manual = status.ca_install_command.as_deref().unwrap_or("(add the certificate to the system trust store)");
                bail!("{e:#}\nTrust the CA by hand, then re-run this command:\n  {manual}");
            }
            status = harness.status().await?;
        }
        match status.ca {
            CaState::Ready => println!("CA:       trusted"),
            other => bail!("the CA is not trusted ({other:?}); trust {} manually and re-run", cert.display()),
        }
        anyhow::Ok(())
    })?;
    set_connected(&paths, true)?;
    println!("\nCursor will see {models} OwO AI Gateway model(s) next to its own models.");
    println!("Next: (re)start `owo start`, then quit Cursor completely and start it again.");
    println!("Cursor goes back to direct connections whenever `owo start` stops.");
    Ok(())
}

pub fn restore(global: &GlobalArgs, remove_ca: bool) -> Result<()> {
    let paths = context::paths(global)?;
    let harness = context::runtime()?.block_on(async {
        let harness = harness(&paths).await?;
        harness.set_enabled(false).await?;
        anyhow::Ok(harness)
    })?;
    println!("settings: restored OwO AI Gateway-managed keys in {}", cursor_server::local_app::cursor_settings_path()?.display());
    set_connected(&paths, false)?;
    if remove_ca {
        let trusted = || matches!(harness.ca_state(), Ok(CaState::Ready));
        if trusted() {
            println!("Removing the CA from the system trust store (administrator approval needed)...");
            run_trust_commands(&harness.ca_untrust_commands(), &data_dir(&paths).join("elevated.log"))?;
            if trusted() {
                bail!("the CA is still trusted; remove the \"{CA_COMMON_NAME}\" certificate from the system trust store by hand");
            }
        }
        println!("CA:       not in the system trust store");
    }
    println!("Restart Cursor (and `owo start`, if it is running) to finish.");
    Ok(())
}

pub fn status(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let enabled = connected(&paths);
    let status = context::runtime()?.block_on(async { anyhow::Ok(harness(&paths).await?.status().await?) })?;
    println!("cursor: {}", if enabled { "connected (active while `owo start` runs)" } else { "not connected (`owo connect cursor`)" });
    println!("  CA:          {:?}", status.ca);
    println!("  models:      {} synced from OwO AI Gateway", status.configured_models);
    println!("  settings:    {}", cursor_server::local_app::cursor_settings_path()?.display());
    println!("  data:        {}", data_dir(&paths).display());
    Ok(())
}

/// Starts the embedded Cursor backend for `owo start`; returns its task.
pub async fn start(
    paths: &OwoPaths,
    router: Arc<owo_routing::Router>,
    group_name: &str,
    shutdown: CancellationToken,
) -> Result<tokio::task::JoinHandle<()>> {
    cursor_server::config::set_managed_data_dir(data_dir(paths));
    let config = cursor_server::Config::for_owo("127.0.0.1:0".parse().expect("static address"))?;
    let app = cursor_server::App::new(config, router.clone()).await?;
    let report = cursor_server::owo_sync::sync_models(&app.store(), router.registry(), group_name).await?;
    tracing::info!(models = report.total, added = report.added, removed = report.removed, "Cursor models synced from OwO AI Gateway");
    match app.store().prune_llm_calls(CALL_HISTORY_RETENTION).await {
        Ok(0) => {}
        Ok(n) => tracing::info!(deleted = n, "pruned Cursor call records older than 30 days"),
        Err(error) => tracing::warn!(%error, "could not prune old Cursor call records"),
    }

    let listener = app.bind().await?;
    let harness = app.harness();
    harness.set_backend_addr(listener.local_addr()?);
    harness.cleanup_stale_settings().await?;
    let task = tokio::spawn(async move {
        if let Err(error) = app.serve_on(listener, shutdown).await {
            tracing::error!(%error, "Cursor backend stopped");
        }
    });
    match harness.set_enabled(true).await {
        Ok(status) => tracing::info!(
            proxy = status.proxy_url.as_deref().unwrap_or("-"),
            "Cursor takeover active; restart Cursor if it was already open"
        ),
        Err(error) => tracing::warn!(%error, "Cursor takeover is not active; run `owo connect cursor`"),
    }
    Ok(task)
}
