//! The app registry behind `owo connect | disconnect | status <app> | apps`: one table of
//! apps, each mapped onto its integration.

use anyhow::Result;
use clap::ValueEnum;

use crate::cli::{App, ConnectArgs, DisconnectArgs, GlobalArgs};
use crate::cmd_apps::{self, Action, FileApp};
use crate::cmd_client::{self, CodexOptions};
use crate::{cmd_claude, cmd_cursor, context};

impl App {
    /// The name used on the command line.
    pub fn name(self) -> &'static str {
        match self {
            App::Codex => "codex",
            App::CodexDesktop => "codex-desktop",
            App::ClaudeCode => "claude",
            App::ClaudeDesktop => "claude-desktop",
            App::Cursor => "cursor",
            App::Grok => "grok",
            App::Opencode => "opencode",
            App::Mcode => "mcode",
            App::Zcode => "zcode",
            App::Copilot => "copilot",
        }
    }

    pub fn about(self) -> String {
        self.to_possible_value().and_then(|v| v.get_help().map(|h| h.to_string())).unwrap_or_default()
    }

    /// The client id used for model aliases (`models[].aliases.<id>`) and gateway routes.
    pub fn client_id(self) -> &'static str {
        match self {
            App::Codex => owo_client_codex::CLIENT_ID,
            App::CodexDesktop => owo_client_codex::DESKTOP_CLIENT_ID,
            App::ClaudeCode => owo_client_claude_code::CLIENT_ID,
            App::ClaudeDesktop => owo_client_claude_code::desktop::CLIENT_ID,
            App::Cursor => cmd_cursor::CLIENT_ID,
            App::Grok => owo_client_apps::grok_build::CLIENT_ID,
            App::Opencode => owo_client_apps::opencode::CLIENT_ID,
            App::Mcode => owo_client_apps::minimax::CODE_CLIENT_ID,
            App::Zcode => owo_client_apps::zcode::CLIENT_ID,
            App::Copilot => owo_client_apps::copilot_app::CLIENT_ID,
        }
    }

    fn codex_surface(self) -> Option<owo_client_codex::Surface> {
        match self {
            App::Codex => Some(owo_client_codex::Surface::Cli),
            App::CodexDesktop => Some(owo_client_codex::Surface::Desktop),
            _ => None,
        }
    }

    fn file_app(self) -> Option<FileApp> {
        match self {
            App::Grok => Some(FileApp::GrokBuild),
            App::Opencode => Some(FileApp::Opencode),
            App::Mcode => Some(FileApp::MinimaxCode),
            App::Zcode => Some(FileApp::Zcode),
            _ => None,
        }
    }

    fn takes_model(self) -> bool {
        matches!(self, App::Codex | App::CodexDesktop | App::ClaudeCode | App::ClaudeDesktop)
    }

    fn takes_dir(self) -> bool {
        matches!(self, App::Codex | App::CodexDesktop | App::ClaudeCode | App::ClaudeDesktop)
    }
}

fn unsupported(app: App, flag: &str) -> anyhow::Error {
    anyhow::anyhow!("`{flag}` does not apply to {}", app.name())
}

pub fn connect(global: &GlobalArgs, args: ConnectArgs) -> Result<()> {
    let Some(app) = args.app else {
        list(global)?;
        println!("\nConnect one with:  owo connect <app>");
        return Ok(());
    };
    if args.model.is_some() && !app.takes_model() {
        return Err(unsupported(app, "--model"));
    }
    if args.dir.is_some() && !app.takes_dir() {
        return Err(unsupported(app, "--dir"));
    }
    if (args.native_aliases || args.codex_bin.is_some()) && !matches!(app, App::Codex | App::CodexDesktop) {
        return Err(unsupported(app, "--native-aliases/--codex-bin"));
    }
    match app {
        App::Codex | App::CodexDesktop => cmd_client::codex_enable(
            global,
            CodexOptions {
                model: args.model,
                surface: app.codex_surface().expect("codex app"),
                native_aliases: args.native_aliases,
                force: args.force,
                codex_home: args.dir,
                codex_bin: args.codex_bin,
            },
        ),
        App::ClaudeCode => cmd_claude::enable(global, args.model, args.force, args.dir),
        App::ClaudeDesktop => cmd_claude::desktop_enable(global, args.model, args.force, args.dir),
        App::Cursor => {
            if args.force {
                return Err(unsupported(app, "--force"));
            }
            cmd_cursor::enable(global)
        }
        App::Copilot => cmd_apps::copilot_app(global),
        _ => cmd_apps::file_client(global, app.file_app().expect("file app"), Action::Enable { force: args.force }),
    }
}

pub fn disconnect(global: &GlobalArgs, args: DisconnectArgs) -> Result<()> {
    let app = args.app;
    if args.dir.is_some() && !app.takes_dir() {
        return Err(unsupported(app, "--dir"));
    }
    if args.remove_ca && app != App::Cursor {
        return Err(unsupported(app, "--remove-ca"));
    }
    match app {
        App::Codex | App::CodexDesktop => cmd_client::codex_restore(global, app.codex_surface().expect("codex app"), args.force, args.dir),
        App::ClaudeCode => cmd_claude::restore(global, args.force, args.dir),
        App::ClaudeDesktop => cmd_claude::desktop_restore(global, args.force, args.dir),
        App::Cursor => cmd_cursor::restore(global, args.remove_ca),
        App::Copilot => {
            println!("OwO AI Gateway changed nothing for the Copilot app: remove the OwO AI Gateway provider under Settings → Model providers.");
            Ok(())
        }
        _ => cmd_apps::file_client(global, app.file_app().expect("file app"), Action::Restore { force: args.force }),
    }
}

pub fn status(global: &GlobalArgs, app: App) -> Result<()> {
    match app {
        App::Codex | App::CodexDesktop => cmd_client::codex_status(global, app.codex_surface().expect("codex app"), None),
        App::ClaudeCode => cmd_claude::status(global, None),
        App::ClaudeDesktop => cmd_claude::desktop_status(global, None),
        App::Cursor => cmd_cursor::status(global),
        App::Copilot => cmd_apps::copilot_app(global),
        _ => cmd_apps::file_client(global, app.file_app().expect("file app"), Action::Status),
    }
}

pub fn is_connected(global: &GlobalArgs, app: App) -> bool {
    match app {
        App::Codex => cmd_client::codex_state(global).0,
        App::CodexDesktop => cmd_client::codex_state(global).1,
        App::ClaudeCode => cmd_claude::enabled(global),
        App::ClaudeDesktop => cmd_claude::desktop_enabled(global),
        App::Cursor => context::paths(global).is_ok_and(|p| cmd_cursor::connected(&p)),
        App::Copilot => false,
        _ => app.file_app().is_some_and(|f| cmd_apps::is_connected(global, f)),
    }
}

/// What a connected app still needs from the user, when connecting alone is not enough.
pub fn usage_hint(app: App) -> Option<&'static str> {
    match app {
        App::Codex => Some("start with: codex -p owo"),
        App::Cursor => Some("active while the gateway runs"),
        _ => None,
    }
}

pub fn list(global: &GlobalArgs) -> Result<()> {
    println!("{:<16} {:<44} STATE", "APP", "");
    for app in App::value_variants() {
        let state = if is_connected(global, *app) {
            usage_hint(*app).map_or_else(|| "connected".to_string(), |h| format!("connected · {h}"))
        } else if *app == App::Copilot {
            "set up in the app (`owo connect copilot`)".into()
        } else {
            "-".into()
        };
        println!("{:<16} {:<44} {state}", app.name(), app.about());
    }
    println!("{:<16} {:<44} launch only (`owo launch mmx text chat …`)", "mmx", "MiniMax CLI text commands");
    Ok(())
}

/// `owo apps --json`: the same table for other programs (the desktop app).
pub fn list_json(global: &GlobalArgs) -> Result<()> {
    let apps: Vec<serde_json::Value> = App::value_variants()
        .iter()
        .map(|app| {
            serde_json::json!({
                "app": app.name(),
                "about": app.about(),
                "client_id": app.client_id(),
                "connected": is_connected(global, *app),
                "hint": usage_hint(*app),
                "takes_model": app.takes_model(),
            })
        })
        .collect();
    println!("{}", serde_json::to_string_pretty(&apps)?);
    Ok(())
}

/// Connected apps, for the overview.
pub fn connected(global: &GlobalArgs) -> Vec<App> {
    App::value_variants().iter().copied().filter(|a| is_connected(global, *a)).collect()
}