use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "owo",
    version,
    about = "OwO AI Gateway — one local endpoint for every AI coding app",
    after_help = "Run `owo` on its own to see what is set up and what to do next.\n\n\
        Getting started:\n  owo add anthropic        add a provider (asks for its key)\n  owo start                run the gateway\n  owo connect claude       point an app at OwO AI Gateway (see `owo apps`)"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: GlobalArgs,

    /// More log output (-v debug, -vv trace). `OWO_LOG` overrides.
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Args, Clone, Debug)]
pub struct GlobalArgs {
    /// Portable mode: config.toml next to the executable, state under ./data.
    #[arg(long, global = true)]
    pub portable: bool,

    /// Use this config file instead of the default location.
    #[arg(long, global = true, value_name = "PATH")]
    pub config: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a starter config.toml.
    Init {
        /// Overwrite an existing config.toml.
        #[arg(long, short)]
        force: bool,
    },
    /// Add a provider: store its key and write it into config.toml.
    Add(AddArgs),
    /// Run the gateway in the foreground (`-d`: in the background).
    Start {
        /// Listen address, overriding `server.listen`.
        #[arg(long, value_name = "ADDR")]
        listen: Option<String>,
        /// Run in the background; logs go to the OwO AI Gateway log directory. Stop it with `owo stop`.
        #[arg(long, short)]
        detach: bool,
    },
    /// Stop the running gateway.
    Stop,
    /// What is set up: gateway, models, connected apps (or one app in detail).
    Status {
        /// Show one app's integration in detail.
        app: Option<App>,
        /// Print the running gateway's raw status as JSON.
        #[arg(long, conflicts_with = "app")]
        json: bool,
    },
    /// Check config, keys, and models without starting the gateway.
    Check,
    /// Tokens used, per model, app, provider, or day.
    Usage {
        /// How many days to cover, counting today.
        #[arg(long, default_value_t = 7, value_name = "N")]
        days: u32,
        /// What to total by.
        #[arg(long, value_enum, default_value_t = UsageBy::Model)]
        by: UsageBy,
    },
    /// Recent model calls and whether they succeeded (`owo history <ID>` shows one in full).
    History {
        /// Show this call in full.
        id: Option<i64>,
        /// How many calls to list.
        #[arg(short = 'n', long, default_value_t = 20)]
        limit: u32,
        /// Only failed calls.
        #[arg(long)]
        failed: bool,
        /// Only calls to this model.
        #[arg(long)]
        model: Option<String>,
        /// Only calls from this app.
        #[arg(long)]
        app: Option<App>,
    },

    /// Point an app at OwO AI Gateway (reversible; `owo disconnect` undoes it).
    Connect(ConnectArgs),
    /// Undo `owo connect` and put the app's own settings back.
    Disconnect(DisconnectArgs),
    /// Start an app with OwO AI Gateway for this run only, changing none of its files.
    Launch {
        app: LaunchApp,
        /// Arguments for the app, passed through unchanged.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
    /// Supported apps and whether they are connected.
    Apps {
        /// Print as JSON.
        #[arg(long)]
        json: bool,
    },
    /// MCP servers: one list, written into each app you enable them for.
    Mcp {
        #[command(subcommand)]
        command: Option<crate::cmd_mcp::McpCommand>,
    },
    /// Agent Skills: kept once in ~/.agents/skills, turned on or off per app.
    Skills {
        #[command(subcommand)]
        command: Option<crate::cmd_skills::SkillsCommand>,
    },

    /// Models OwO AI Gateway serves.
    Models {
        /// Show the ids this app sees (after its aliases).
        #[arg(long)]
        app: Option<App>,
    },
    /// Configured providers; also built-in presets and live model discovery.
    Providers {
        #[command(subcommand)]
        command: Option<ProvidersCommand>,
    },
    /// Provider keys: list what config.toml needs, store or delete keys in the OS keyring.
    Key {
        #[command(subcommand)]
        command: Option<KeyCommand>,
    },
    /// Where config.toml lives; open it in an editor.
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
}

/// Apps OwO AI Gateway can connect.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum App {
    /// Codex CLI (profile `codex -p owo`)
    Codex,
    /// Codex Desktop (config.toml)
    CodexDesktop,
    /// Claude Code (CLI and IDE extensions)
    #[value(name = "claude")]
    ClaudeCode,
    /// Claude Desktop (third-party inference mode)
    ClaudeDesktop,
    /// Cursor (OwO AI Gateway models next to Cursor's own)
    Cursor,
    /// Grok Build (`grok`)
    Grok,
    /// OpenCode
    Opencode,
    /// MiniMax Code
    Mcode,
    /// ZCode
    Zcode,
    /// GitHub Copilot app
    Copilot,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum UsageBy {
    Model,
    App,
    Provider,
    Day,
}

/// Apps started through `owo launch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
pub enum LaunchApp {
    /// Claude Code, pointed at OwO AI Gateway through its environment
    #[value(name = "claude")]
    Claude,
    /// Codex CLI, with OwO AI Gateway's provider passed as `-c` overrides
    Codex,
    /// OpenCode, with OwO AI Gateway's provider injected for this run
    Opencode,
    /// MiniMax CLI `mmx text chat|repl`
    Mmx,
}

#[derive(Args)]
pub struct AddArgs {
    /// Provider name: a preset id (`owo providers presets`), or your own name together with --url.
    pub name: String,
    /// Base it on this preset under a different name (e.g. a second Anthropic account or relay).
    #[arg(long)]
    pub preset: Option<String>,
    /// Endpoint: a relay for the preset, or any compatible API.
    #[arg(long, value_name = "URL")]
    pub url: Option<String>,
    /// Protocol of a custom endpoint (default: openai-chat).
    #[arg(long, value_parser = ["openai-chat", "anthropic"])]
    pub adapter: Option<String>,
    /// Models to expose, comma-separated (default: the preset's).
    #[arg(long, short, value_delimiter = ',', num_args = 1..)]
    pub models: Option<Vec<String>>,
    /// Read the key from this environment variable instead of the OS keyring.
    #[arg(long, value_name = "VAR", conflicts_with = "no_key")]
    pub env: Option<String>,
    /// The provider needs no key (a local server such as Ollama).
    #[arg(long)]
    pub no_key: bool,
    /// Replace a provider of the same name.
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Args)]
pub struct ConnectArgs {
    /// The app to connect; omit to list them.
    pub app: Option<App>,
    /// Model the app starts with (codex, claude, claude-desktop).
    #[arg(long, short)]
    pub model: Option<String>,
    /// Replace settings that were changed after OwO AI Gateway wrote them, or not written by OwO AI Gateway (backed up first).
    #[arg(long, short)]
    pub force: bool,
    /// The app's config directory, when it is not in the default place (codex, claude, claude-desktop).
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// Codex: publish OwO AI Gateway models under native GPT slots (automatic for a signed-out Desktop).
    #[arg(long, hide = true)]
    pub native_aliases: bool,
    /// Codex: the executable used to read its bundled model catalog.
    #[arg(long, value_name = "PATH", hide = true)]
    pub codex_bin: Option<PathBuf>,
}

#[derive(Args)]
pub struct DisconnectArgs {
    pub app: App,
    /// Also revert values you changed after connecting.
    #[arg(long, short)]
    pub force: bool,
    /// The app's config directory, when it is not in the default place.
    #[arg(long, value_name = "DIR")]
    pub dir: Option<PathBuf>,
    /// Cursor: also remove OwO AI Gateway's local certificate from the system trust store.
    #[arg(long)]
    pub remove_ca: bool,
}

#[derive(Subcommand)]
pub enum ProvidersCommand {
    /// Built-in provider presets usable as `[providers.<id>]`.
    Presets,
    /// List the models a provider serves right now; `--add` puts some into config.toml.
    Discover {
        id: String,
        /// Add these models to the provider (comma-separated or repeated).
        #[arg(long, value_delimiter = ',', num_args = 1.., value_name = "MODEL")]
        add: Vec<String>,
        /// Add every model the provider lists.
        #[arg(long, conflicts_with = "add")]
        all: bool,
    },
}

#[derive(Subcommand)]
pub enum KeyCommand {
    /// Store a key (typed without echo, or piped on stdin).
    Set { name: String },
    /// Delete a stored key.
    Rm { name: String },
}

#[derive(Subcommand)]
pub enum ConfigCommand {
    /// Print where config, state, and backups live (the default).
    Path,
    /// Open config.toml in your editor ($VISUAL / $EDITOR, else the system default).
    Edit,
}
