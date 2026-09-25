//! `owo` — one binary for the gateway and every management command.

mod apps;
mod banner;
mod cli;
mod cmd_add;
mod cmd_apps;
mod cmd_claude;
mod cmd_client;
mod cmd_config;
mod cmd_credential;
mod cmd_cursor;
mod cmd_mcp;
mod cmd_provider;
mod cmd_run;
mod cmd_skills;
mod cmd_status;
mod cmd_usage;
mod context;
mod elevate;
mod ui;

use clap::Parser;

use cli::{Cli, Command, ConfigCommand, KeyCommand, LaunchApp, ProvidersCommand};

fn main() {
    let cli = Cli::parse();
    // The gateway reports what it does; every other command speaks for itself and only
    // surfaces warnings from the libraries underneath.
    let gateway = matches!(cli.command, Some(Command::Start { .. }));
    init_logging(cli.verbose, gateway);
    if let Err(err) = dispatch(cli) {
        eprintln!("error: {err:#}");
        std::process::exit(1);
    }
}

fn dispatch(cli: Cli) -> anyhow::Result<()> {
    let global = cli.global;
    let Some(command) = cli.command else {
        return cmd_status::overview(&global);
    };
    match command {
        Command::Init { force } => cmd_config::init(&global, force),
        Command::Add(args) => cmd_add::add(&global, args),
        Command::Start { listen, detach: true } => cmd_run::start_detached(&global, listen),
        Command::Start { listen, detach: false } => cmd_run::run(&global, listen),
        Command::Stop => cmd_run::stop(&global),
        Command::Status { app: Some(app), .. } => apps::status(&global, app),
        Command::Status { json: true, .. } => cmd_status::status_json(&global),
        Command::Status { .. } => cmd_status::overview(&global),
        Command::Check => cmd_status::check(&global),
        Command::Usage { days, by } => cmd_usage::usage(&global, days, by),
        Command::History { id: Some(id), .. } => cmd_usage::show(&global, id),
        Command::History { id: None, limit, failed, model, app } => cmd_usage::history(&global, limit, failed, model, app),
        Command::Connect(args) => apps::connect(&global, args),
        Command::Disconnect(args) => apps::disconnect(&global, args),
        Command::Launch { app: LaunchApp::Claude, args } => cmd_claude::launch(&global, args),
        Command::Launch { app: LaunchApp::Codex, args } => cmd_client::codex_launch(&global, args),
        Command::Launch { app: LaunchApp::Opencode, args } => cmd_apps::opencode_run(&global, args),
        Command::Launch { app: LaunchApp::Mmx, args } => cmd_apps::mmx_run(&global, args),
        Command::Apps { json: false } => apps::list(&global),
        Command::Apps { json: true } => apps::list_json(&global),
        Command::Mcp { command } => cmd_mcp::run(&global, command),
        Command::Skills { command } => cmd_skills::run(&global, command),
        Command::Models { app } => cmd_provider::models(&global, app.map(|a| a.client_id())),
        Command::Providers { command: None } => cmd_provider::list(&global),
        Command::Providers { command: Some(ProvidersCommand::Presets) } => cmd_provider::presets(),
        Command::Providers { command: Some(ProvidersCommand::Discover { id, add, all }) } => {
            cmd_provider::discover(&global, &id, &add, all)
        }
        Command::Key { command: None } => cmd_credential::list(&global),
        Command::Key { command: Some(KeyCommand::Set { name }) } => cmd_credential::set(&global, &name),
        Command::Key { command: Some(KeyCommand::Rm { name }) } => cmd_credential::delete(&global, &name),
        Command::Config { command: None | Some(ConfigCommand::Path) } => cmd_config::path(&global),
        Command::Config { command: Some(ConfigCommand::Edit) } => cmd_config::edit(&global),
    }
}

fn init_logging(verbose: u8, gateway: bool) {
    let default = match (verbose, gateway) {
        (0, true) => "info",
        (0, false) => "warn",
        (1, _) => "debug",
        _ => "trace",
    };
    let filter = tracing_subscriber::EnvFilter::try_from_env("OWO_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default));
    // No colour codes when the output goes to a file (the background gateway's log).
    let ansi = std::io::IsTerminal::is_terminal(&std::io::stderr());
    tracing_subscriber::fmt().with_env_filter(filter).with_writer(std::io::stderr).with_ansi(ansi).with_target(false).init();
}
