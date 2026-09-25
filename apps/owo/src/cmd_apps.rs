//! File-based app integrations (grok, opencode, mcode, zcode), the Copilot app, and the `owo launch` launchers.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use owo_client_apps::managed::{self, FileEdit, Store};
use owo_client_apps::{copilot_app, grok_build, minimax, opencode, zcode, AppModel, Target};
use crate::cli::GlobalArgs;
use crate::cmd_client::{client_address, gateway_running};
use crate::context;

/// What to do with a file-based integration.
pub enum Action {
    /// Write OwO AI Gateway's entries (refreshes the model list when run again).
    Enable { force: bool },
    /// Remove them and put back what was there.
    Restore { force: bool },
    Status,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FileApp {
    GrokBuild,
    Opencode,
    MinimaxCode,
    Zcode,
}

impl FileApp {
    fn id(self) -> &'static str {
        match self {
            FileApp::GrokBuild => grok_build::CLIENT_ID,
            FileApp::Opencode => opencode::CLIENT_ID,
            FileApp::MinimaxCode => minimax::CODE_CLIENT_ID,
            FileApp::Zcode => zcode::CLIENT_ID,
        }
    }

    fn command(self) -> &'static str {
        match self {
            FileApp::GrokBuild => "grok",
            FileApp::Opencode => "opencode",
            FileApp::MinimaxCode => "mcode",
            FileApp::Zcode => "zcode",
        }
    }

    fn next_step(self) -> &'static str {
        match self {
            FileApp::GrokBuild => "Grok Build reloads the file by itself: `grok models` lists the owo-* entries.",
            FileApp::Opencode => "Start a new `opencode` session; OwO AI Gateway models are under the `owo` provider.",
            FileApp::MinimaxCode => "Start a new MiniMax Code session and pick `custom_provider:owo/<model>`.",
            FileApp::Zcode => "Restart ZCode; OwO AI Gateway models are under their own provider (named by `name` in config.toml).",
        }
    }
}

struct Ctx {
    store: Store,
    target: Target,
    models: Vec<AppModel>,
    address: String,
}

fn ctx(global: &GlobalArgs, client: &str) -> Result<Ctx> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, warnings) = context::build_router(&config)?;
    context::print_diagnostics(&warnings);
    let models: Vec<AppModel> = router
        .registry()
        .available_models()
        .map(|m| AppModel {
            id: m.exposed_id(Some(client)).to_string(),
            display_name: m.display_name.clone(),
            context_window: m.context_window,
            max_output_tokens: m.max_output_tokens,
            reasoning_efforts: m.reasoning_efforts.clone(),
            default_reasoning_effort: m.default_reasoning_effort.clone(),
            vision: m.capabilities.vision == Some(true),
        })
        .collect();
    let token = crate::cmd_claude::gateway_token_opt(&config)?;
    let address = client_address(&config.server.listen);
    paths.ensure_dirs()?;
    Ok(Ctx {
        store: Store { state_dir: paths.state, backups_dir: paths.backups },
        target: Target { name: config.client_name(client).to_string(), root: format!("http://{address}/c/{client}"), token },
        models,
        address,
    })
}

fn installed_dir(dir: Option<PathBuf>, what: &str, hint: &str) -> Result<PathBuf> {
    let dir = dir.with_context(|| format!("cannot locate {what}'s config directory"))?;
    if !dir.is_dir() {
        bail!("{what} was not found ({} does not exist); {hint}", dir.display());
    }
    Ok(dir)
}

fn edits(app: FileApp, c: &Ctx) -> Result<Vec<FileEdit>> {
    Ok(vec![match app {
        FileApp::GrokBuild => {
            let dir = installed_dir(grok_build::grok_dir(), "Grok Build", "install it and run `grok` once")?;
            grok_build::edit(&dir, &c.target, &c.models)
        }
        FileApp::Opencode => {
            let path = opencode::global_config_path().context("cannot locate OpenCode's config directory")?;
            opencode::edit(path, &c.target, &c.models)
        }
        FileApp::MinimaxCode => {
            let dir = installed_dir(minimax::mcode_dir(), "MiniMax Code", "install it and sign in once")?;
            minimax::mcode_edit(&dir, &c.target, &c.models)
        }
        FileApp::Zcode => {
            let dir = installed_dir(zcode::zcode_dir(), "ZCode", "install it and start it once")?;
            zcode::edit(&dir, &c.target, &c.models)?
        }
    }])
}

fn print(report: &managed::Report) {
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
}

pub fn is_connected(global: &GlobalArgs, app: FileApp) -> bool {
    context::paths(global)
        .ok()
        .map(|p| Store { state_dir: p.state, backups_dir: p.backups })
        .and_then(|s| s.load(app.id()).ok().flatten())
        .is_some()
}

pub fn file_client(global: &GlobalArgs, app: FileApp, command: Action) -> Result<()> {
    let c = ctx(global, app.id())?;
    match command {
        Action::Enable { force } => {
            if c.models.is_empty() {
                bail!("no models are available; add a provider with models to OwO AI Gateway's config.toml first");
            }
            let edits = edits(app, &c)?;
            let report = managed::enable(&c.store, app.id(), &edits, force)?;
            print(&report);
            println!("\n{} OwO AI Gateway model(s) exposed to {} (gateway {}).", c.models.len(), app.command(), c.target.root);
            if app == FileApp::Opencode && c.target.token.is_some() {
                println!("The gateway requires a token: set {} to it in the environment OpenCode runs in.", opencode::TOKEN_ENV);
            }
            if !gateway_running(&c.address) {
                println!("OwO AI Gateway is not running yet — start it with:  owo start");
            }
            println!("{}  Undo with:  owo disconnect {}", app.next_step(), app.command());
        }
        Action::Restore { force } => print(&managed::restore(&c.store, app.id(), force)?),
        Action::Status => match c.store.load(app.id())? {
            None => println!("{}: not connected (`owo connect {}`)", app.command(), app.command()),
            Some(st) => {
                println!("{}: connected (OwO AI Gateway {}, updated {})", app.command(), st.owo_version, st.updated_at_unix);
                for f in &st.files {
                    println!("  file:      {}", f.path.display());
                    for frag in f.fragments.iter().filter(|s| !s.path.is_empty()) {
                        println!("    entry:   {}", managed::display_path(&frag.path));
                    }
                    if let Some(b) = &f.backup {
                        println!("    backup:  {}", b.display());
                    }
                }
                println!("  gateway:   {}", if gateway_running(&c.address) { "reachable" } else { "not running (owo start)" });
            }
        },
    }
    Ok(())
}

/// Finds an executable on PATH, including Windows `.cmd`/`.exe` shims (npm installs).
pub(crate) fn find_program(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into()).split(';').map(|e| e.to_ascii_lowercase()).collect()
    } else {
        vec![String::new()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?).find_map(|dir| {
        exts.iter().map(|e| dir.join(format!("{name}{e}"))).find(|p| p.is_file())
    })
}

fn run_child(program: &Path, args: &[String], configure: impl FnOnce(&mut std::process::Command)) -> Result<()> {
    let mut cmd = std::process::Command::new(program);
    cmd.args(args);
    configure(&mut cmd);
    let status = cmd.status().with_context(|| format!("cannot start {}", program.display()))?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

pub fn opencode_run(global: &GlobalArgs, args: Vec<String>) -> Result<()> {
    let c = ctx(global, opencode::CLIENT_ID)?;
    let program = find_program("opencode").context("`opencode` is not on PATH (npm install -g opencode-ai)")?;
    let inherited = std::env::var(opencode::CONFIG_CONTENT_ENV).ok();
    let content = opencode::runtime_config(inherited.as_deref(), &c.target, &c.models)?;
    if !gateway_running(&c.address) {
        eprintln!("warning: OwO AI Gateway is not running — start it with `owo start`");
    }
    run_child(&program, &args, |cmd| {
        cmd.env(opencode::CONFIG_CONTENT_ENV, content);
        if let Some(t) = &c.target.token {
            cmd.env(opencode::TOKEN_ENV, t);
        }
    })
}

pub fn mmx_run(global: &GlobalArgs, args: Vec<String>) -> Result<()> {
    minimax::check_mmx_command(&args)?;
    let c = ctx(global, minimax::CLI_CLIENT_ID)?;
    let program = find_program("mmx").context("`mmx` is not on PATH (npm install -g mmx-cli)")?;
    let dir = c.store.state_dir.join(minimax::CLI_CLIENT_ID).join(format!("run-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join("config.json"), minimax::mmx_config(&c.target))?;
    if !gateway_running(&c.address) {
        eprintln!("warning: OwO AI Gateway is not running — start it with `owo start`");
    }
    let (remove, set) = minimax::mmx_env(&c.target, &dir, std::env::vars().map(|(k, _)| k));
    let mut cmd = std::process::Command::new(&program);
    cmd.args(&args);
    for k in &remove {
        cmd.env_remove(k);
    }
    cmd.envs(&set);
    let result = cmd.status().with_context(|| format!("cannot start {}", program.display()));
    let _ = std::fs::remove_dir_all(&dir);
    let status = result?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

pub fn copilot_app(global: &GlobalArgs) -> Result<()> {
    let c = ctx(global, copilot_app::CLIENT_ID)?;
    let info = copilot_app::instructions(&c.target, &c.models);
    println!("In the GitHub Copilot app: Settings → Model providers → Add provider");
    println!("  Name:      {}", c.target.name);
    println!("  Base URL:  {}", info.base_url);
    match &info.api_key {
        Some(_) => println!("  API key:   your OwO AI Gateway token (server.auth_token)"),
        None => println!("  API key:   leave empty"),
    }
    println!("Then sync models from the endpoint. It will list:");
    for id in &info.model_ids {
        println!("  - {id}");
    }
    if !gateway_running(&c.address) {
        println!("OwO AI Gateway is not running yet — start it with:  owo start");
    }
    Ok(())
}
