//! `owo connect|disconnect|status claude|claude-desktop`

use std::path::PathBuf;

use anyhow::{Context, Result};
use owo_client_claude_code::desktop::{self, DesktopEnableRequest, DesktopEnvironment, DesktopModel};
use owo_client_claude_code::{self as claude, EnableRequest, Environment};
use owo_credentials::CredentialStore;

use crate::cli::GlobalArgs;
use crate::cmd_client::{client_address, gateway_running};
use crate::context;

/// Sent when the gateway has no token: the Claude clients need some credential to skip sign-in.
const PLACEHOLDER_TOKEN: &str = "owo-local";

fn environment(global: &GlobalArgs, claude_dir: Option<PathBuf>) -> Result<Environment> {
    let paths = context::paths(global)?;
    let claude_dir = match claude_dir {
        Some(dir) => dir,
        None => claude::default_claude_dir().context("cannot locate Claude Code's config directory; pass --dir")?,
    };
    Ok(Environment { state_dir: paths.state, backups_dir: paths.backups, claude_dir })
}

/// The gateway token clients present, when the gateway requires one.
pub(crate) fn gateway_token_opt(config: &owo_config::Config) -> Result<Option<String>> {
    Ok(match &config.server.auth_token {
        Some(reference) => Some(
            CredentialStore::new(config.credentials.backend)
                .resolve(reference)
                .context("cannot resolve server.auth_token")?
                .context("server.auth_token must not be `none`")?
                .expose()
                .to_string(),
        ),
        None => None,
    })
}

/// The gateway token clients present, or a placeholder when the gateway has none.
pub(crate) fn gateway_token(config: &owo_config::Config) -> Result<String> {
    Ok(gateway_token_opt(config)?.unwrap_or_else(|| PLACEHOLDER_TOKEN.to_string()))
}

/// What Claude Code needs to reach OwO AI Gateway (the same for `connect` and `launch`).
fn claude_request(config: &owo_config::Config, router: &owo_routing::Router, model: Option<String>, force: bool) -> Result<(EnableRequest, String)> {
    let models: Vec<String> = router.registry().available_models().map(|m| m.exposed_id(Some(claude::CLIENT_ID)).to_string()).collect();
    let address = client_address(&config.server.listen);
    let request = EnableRequest {
        base_url: format!("http://{address}/c/{}", claude::CLIENT_ID),
        auth_token: gateway_token(config)?,
        model: model.or_else(|| configured_model(config, claude::CLIENT_ID)),
        models,
        force,
    };
    Ok((request, address))
}

/// `owo launch claude`: Claude Code pointed at OwO AI Gateway through its environment; settings.json is not touched.
pub fn launch(global: &GlobalArgs, args: Vec<String>) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, _) = context::build_router(&config)?;
    let (request, address) = claude_request(&config, &router, None, false)?;
    let env = claude::launch_env(&request)?;
    let program = crate::cmd_apps::find_program("claude").context("`claude` is not installed (see https://docs.claude.com/claude-code)")?;
    // settings.json `env` wins over the process environment in Claude Code.
    if let Ok(e) = environment(global, None) {
        let own = std::fs::read_to_string(e.settings_path()).ok().and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok());
        if let Some(url) = own.as_ref().and_then(|s| s["env"][claude::BASE_URL].as_str()).filter(|u| *u != request.base_url) {
            eprintln!("warning: {} sets {} = {url}, which overrides this launch", e.settings_path().display(), claude::BASE_URL);
        }
    }
    if !gateway_running(&address) {
        eprintln!("warning: OwO AI Gateway is not running — start it with `owo start`");
    }
    let status = std::process::Command::new(&program)
        .args(&args)
        .envs(&env)
        .env_remove("ANTHROPIC_API_KEY")
        .status()
        .with_context(|| format!("cannot start {}", program.display()))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn configured_model(config: &owo_config::Config, client: &str) -> Option<String> {
    config.clients.get(client).and_then(|c| c.model.clone())
}

pub fn enabled(global: &GlobalArgs) -> bool {
    environment(global, None).ok().and_then(|e| claude::status(&e).ok().flatten()).is_some()
}

pub fn enable(global: &GlobalArgs, model: Option<String>, force: bool, claude_dir: Option<PathBuf>) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, warnings) = context::build_router(&config)?;
    context::print_diagnostics(&warnings);
    let (request, address) = claude_request(&config, &router, model, force)?;
    let env = environment(global, claude_dir)?;
    paths.ensure_dirs()?;
    let report = claude::enable(&env, &request)?;
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    if std::env::var_os("ANTHROPIC_API_KEY").is_some() {
        eprintln!("warning: ANTHROPIC_API_KEY is set in this shell; unset it so Claude Code uses OwO AI Gateway's token");
    }
    println!("\nClaude Code now sends every request to OwO AI Gateway ({} model(s); `/model` lists them).", request.models.len());
    if !gateway_running(&address) {
        println!("OwO AI Gateway is not running yet — start it with:  owo start");
    }
    println!("Try it:  claude        Undo with:  owo disconnect claude");
    Ok(())
}

pub fn restore(global: &GlobalArgs, force: bool, claude_dir: Option<PathBuf>) -> Result<()> {
    let env = environment(global, claude_dir)?;
    let report = claude::restore(&env, force)?;
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

pub fn status(global: &GlobalArgs, claude_dir: Option<PathBuf>) -> Result<()> {
    let env = environment(global, claude_dir)?;
    match claude::status(&env)? {
        None => println!("claude: not connected (`owo connect claude`)"),
        Some(st) => {
            println!("claude: connected");
            println!("  settings:    {}", st.settings_path.display());
            for (key, value) in &st.written {
                let shown = if key == claude::AUTH_TOKEN { "<hidden>" } else { value.as_str() };
                println!("  {key:<42} {shown}");
            }
            if let Some(base) = st.written.get(claude::BASE_URL) {
                let address = base.trim_start_matches("http://").split('/').next().unwrap_or_default();
                println!("  gateway:     {}", if gateway_running(address) { "reachable" } else { "not running (owo start)" });
            }
        }
    }
    Ok(())
}

fn desktop_environment(global: &GlobalArgs, library_dir: Option<PathBuf>) -> Result<DesktopEnvironment> {
    let paths = context::paths(global)?;
    let library_dir = match library_dir {
        Some(dir) => dir,
        None => desktop::default_library_dir().context("cannot locate Claude Desktop's configuration library; pass --dir")?,
    };
    Ok(DesktopEnvironment { state_dir: paths.state, backups_dir: paths.backups, library_dir })
}

pub fn desktop_enabled(global: &GlobalArgs) -> bool {
    desktop_environment(global, None).ok().and_then(|e| desktop::status(&e).ok().flatten()).is_some()
}

pub fn desktop_enable(global: &GlobalArgs, model: Option<String>, force: bool, library_dir: Option<PathBuf>) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, warnings) = context::build_router(&config)?;
    context::print_diagnostics(&warnings);
    let models: Vec<DesktopModel> = router
        .registry()
        .available_models()
        .map(|m| DesktopModel {
            id: m.exposed_id(Some(desktop::CLIENT_ID)).to_string(),
            display_name: m.display_name.clone(),
            context_window: m.context_window,
            reasoning_efforts: m.reasoning_efforts.clone(),
        })
        .collect();
    let address = client_address(&config.server.listen);
    let request = DesktopEnableRequest {
        name: config.client_name(desktop::CLIENT_ID).to_string(),
        base_url: format!("http://{address}/c/{}", desktop::CLIENT_ID),
        api_key: gateway_token(&config)?,
        default_model: model.or_else(|| configured_model(&config, desktop::CLIENT_ID)),
        models,
        force,
    };
    let env = desktop_environment(global, library_dir)?;
    paths.ensure_dirs()?;
    let report = desktop::enable(&env, &request)?;
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    println!("\nClaude Desktop will use OwO AI Gateway ({} model(s)) in third-party mode — no claude.ai sign-in.", request.models.len());
    if !gateway_running(&address) {
        println!("OwO AI Gateway is not running yet — start it with:  owo start");
    }
    println!("Quit Claude Desktop completely (tray icon too) and reopen it. Undo with:  owo disconnect claude-desktop");
    Ok(())
}

pub fn desktop_restore(global: &GlobalArgs, force: bool, library_dir: Option<PathBuf>) -> Result<()> {
    let env = desktop_environment(global, library_dir)?;
    let report = desktop::restore(&env, force)?;
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

pub fn desktop_status(global: &GlobalArgs, library_dir: Option<PathBuf>) -> Result<()> {
    let env = desktop_environment(global, library_dir)?;
    match desktop::status(&env)? {
        None => println!("claude-desktop: not connected (`owo connect claude-desktop`)"),
        Some(st) => {
            println!("claude-desktop: connected");
            println!("  library:     {}", st.library_dir.display());
            let name = desktop::entry_name(&env, &st).unwrap_or_else(|| "missing from Desktop's library".into());
            println!("  entry:       {name} ({})", st.profile_id);
            println!("  applied:     {}", if desktop::applied(&env, &st) { "yes" } else { "no — another entry is selected in Desktop" });
            match &st.previous_applied_id {
                Some(p) => println!("  restore to:  entry {p}"),
                None => println!("  restore to:  no entry (claude.ai sign-in)"),
            }
        }
    }
    Ok(())
}
