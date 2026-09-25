use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use owo_client_codex::{self as codex, EnableRequest, Environment, ProviderSettings, Surface};

use crate::cli::GlobalArgs;
use crate::context;

/// Environment variable Codex reads the OwO AI Gateway access token from when the gateway requires one.
const CODEX_TOKEN_ENV: &str = "OWO_API_KEY";

pub struct CodexOptions {
    /// Model Codex starts with (default: `[clients.codex]` / `[clients.codex-desktop]` model,
    /// else the first available model).
    pub model: Option<String>,
    /// codex (CLI profile) or codex-desktop (config.toml for Codex Desktop).
    pub surface: Surface,
    pub native_aliases: bool,
    pub force: bool,
    pub codex_home: Option<PathBuf>,
    pub codex_bin: Option<PathBuf>,
}

fn environment(global: &GlobalArgs, codex_home: Option<PathBuf>, codex_bin: Option<PathBuf>) -> Result<Environment> {
    let paths = context::paths(global)?;
    let codex_home = match codex_home {
        Some(dir) => dir,
        None => codex::detect::codex_home().context("cannot locate the Codex home; pass --dir")?,
    };
    Ok(Environment {
        state_dir: paths.state,
        backups_dir: paths.backups,
        codex_home,
        codex_bin: codex_bin.or_else(codex::detect::find_codex_binary),
    })
}

/// The address clients should dial: a wildcard bind is reached through loopback.
pub(crate) fn client_address(listen: &str) -> String {
    match listen.parse::<SocketAddr>() {
        Ok(addr) if addr.ip().is_unspecified() => format!("127.0.0.1:{}", addr.port()),
        _ => listen.to_string(),
    }
}

pub(crate) fn gateway_running(address: &str) -> bool {
    address
        .parse::<SocketAddr>()
        .is_ok_and(|a| TcpStream::connect_timeout(&a, Duration::from_millis(300)).is_ok())
}

pub fn codex_enable(global: &GlobalArgs, args: CodexOptions) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, warnings) = context::build_router(&config)?;
    context::print_diagnostics(&warnings);
    let client = args.surface.client_id();
    let models = codex::catalog_models(router.registry(), client);

    let configured = config.clients.get(client).and_then(|c| c.model.clone());
    let model = args
        .model
        .or(configured)
        .or_else(|| models.first().map(|m| m.slug.clone()))
        .context("no models are available; add a provider with models to OwO AI Gateway's config.toml")?;

    let address = client_address(&config.server.listen);
    let env = environment(global, args.codex_home, args.codex_bin)?;
    let signed_in = env.codex_home.join("auth.json").is_file();
    let desktop = args.surface == Surface::Desktop;
    let native_aliases = args.native_aliases || (desktop && !signed_in);
    if native_aliases && !args.native_aliases {
        println!("Codex is not signed in to ChatGPT: OwO AI Gateway models will occupy native model slots in the Desktop picker.");
    }
    let request = EnableRequest {
        provider: ProviderSettings {
            name: config.client_name(client).to_string(),
            base_url: format!("http://{address}/c/{client}/v1"),
            env_key: config.server.auth_token.as_ref().map(|_| CODEX_TOKEN_ENV.to_string()),
        },
        model,
        models,
        surface: args.surface,
        force: args.force,
        native_aliases,
    };
    paths.ensure_dirs()?;
    let report = codex::enable(&env, &request)?;

    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    println!("\nCodex now sees {} OwO AI Gateway model(s); it starts with `{}`.", request.models.len(), request.model);
    if request.provider.env_key.is_some() {
        println!("The gateway requires a token: set {CODEX_TOKEN_ENV} to it in the environment Codex runs in.");
    }
    if !gateway_running(&address) {
        println!("OwO AI Gateway is not running yet — start it with:  owo start");
    }
    if desktop {
        println!("Restart Codex Desktop to pick up the change. Undo with:  owo disconnect codex-desktop");
    } else {
        println!("Try it:  codex -p {}        (your normal `codex` sessions are unchanged)", codex::PROVIDER_ID);
    }
    Ok(())
}

pub fn codex_restore(global: &GlobalArgs, surface: Surface, force: bool, codex_home: Option<PathBuf>) -> Result<()> {
    let env = environment(global, codex_home, None)?;
    let report = codex::restore(&env, surface, force)?;
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
    Ok(())
}

pub fn codex_status(global: &GlobalArgs, surface: Surface, codex_home: Option<PathBuf>) -> Result<()> {
    let env = environment(global, codex_home, None)?;
    let app = surface.app();
    let st = codex::status(&env)?;
    let gateway = |base_url: &str| {
        let address = base_url.trim_start_matches("http://").split('/').next().unwrap_or_default().to_string();
        if gateway_running(&address) { "reachable" } else { "not running (owo start)" }
    };
    match (surface, st) {
        (Surface::Cli, Some(codex::CodexState { cli: Some(c), codex_home, template_source, .. })) => {
            println!("{app}: connected");
            println!("  codex home:  {}", codex_home.display());
            println!("  base url:    {}", c.base_url);
            println!("  start model: {}", c.model);
            println!("  profile:     {}  (codex -p {})", c.profile.path.display(), codex::PROVIDER_ID);
            println!("  catalog:     {}  (template: {template_source})", c.catalog_path.display());
            println!("  gateway:     {}", gateway(&c.base_url));
        }
        (Surface::Desktop, Some(codex::CodexState { desktop: Some(d), codex_home, template_source, .. })) => {
            println!("{app}: connected");
            println!("  codex home:  {}", codex_home.display());
            println!("  base url:    {}", d.base_url);
            println!("  start model: {}", d.model);
            println!("  config:      {} ({} keys set)", d.config_path.display(), d.root_keys.len());
            println!("  backup:      {}", d.backup_path.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "none (file was new)".into()));
            println!("  catalog:     {}  (template: {template_source})", d.catalog_path.display());
            for a in &d.native_aliases {
                println!("  alias:       {} → {}", a.native, a.model);
            }
            println!("  gateway:     {}", gateway(&d.base_url));
        }
        _ => println!("{app}: not connected (`owo connect {app}`)"),
    }
    Ok(())
}
/// `owo launch codex`: Codex with OwO AI Gateway's provider as `-c` overrides; no Codex file is written.
pub fn codex_launch(global: &GlobalArgs, args: Vec<String>) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, _) = context::build_router(&config)?;
    let models = codex::catalog_models(router.registry(), codex::CLIENT_ID);
    let configured = config.clients.get(codex::CLIENT_ID).and_then(|c| c.model.clone());
    let model = configured.or_else(|| models.first().map(|m| m.slug.clone())).context("no models are available; add one with `owo add`")?;

    let template = codex::stored_template(&paths.state).unwrap_or_else(codex::catalog::builtin_template);
    let catalog = codex::catalog::build_catalog(&template, &models);
    let catalog_path = paths.state.join("codex").join("launch-models.json");
    std::fs::create_dir_all(catalog_path.parent().expect("has a parent"))?;
    std::fs::write(&catalog_path, serde_json::to_vec_pretty(&catalog)?)?;

    let address = client_address(&config.server.listen);
    let token = crate::cmd_claude::gateway_token_opt(&config)?;
    let settings = ProviderSettings {
        name: config.client_name(codex::CLIENT_ID).to_string(),
        base_url: format!("http://{address}/c/{}/v1", codex::CLIENT_ID),
        env_key: token.as_ref().map(|_| CODEX_TOKEN_ENV.to_string()),
    };
    let bin = codex::detect::find_codex_binary().context("`codex` is not installed (npm install -g @openai/codex)")?;
    if !gateway_running(&address) {
        eprintln!("warning: OwO AI Gateway is not running — start it with `owo start`");
    }
    let mut cmd = std::process::Command::new(&bin);
    for o in codex::profile::overrides(&model, &catalog_path, &settings) {
        cmd.arg("-c").arg(o);
    }
    cmd.args(&args);
    if let Some(t) = &token {
        cmd.env(CODEX_TOKEN_ENV, t);
    }
    let status = cmd.status().with_context(|| format!("cannot start {}", bin.display()))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

/// Whether the CLI profile and the Desktop edit are connected.
pub fn codex_state(global: &GlobalArgs) -> (bool, bool) {
    let Some(st) = environment(global, None, None).ok().and_then(|e| codex::status(&e).ok().flatten()) else { return (false, false) };
    (st.cli.is_some(), st.desktop.is_some())
}