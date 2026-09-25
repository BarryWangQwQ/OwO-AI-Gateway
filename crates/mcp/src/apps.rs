//! Where each app keeps its MCP servers, and in which shape.
//!
//! | app            | file                                                        | key              |
//! |----------------|-------------------------------------------------------------|------------------|
//! | codex          | `$CODEX_HOME/config.toml` (Codex CLI and Codex Desktop)     | `[mcp_servers.x]`|
//! | claude         | `~/.claude.json` (`$CLAUDE_CONFIG_DIR/.claude.json`)        | `mcpServers`     |
//! | claude-desktop | `<config dir>/Claude[-3p]/claude_desktop_config.json`       | `mcpServers`     |
//! | cursor         | `~/.cursor/mcp.json`                                        | `mcpServers`     |
//! | opencode       | `~/.config/opencode/opencode.json`                          | `mcp`            |
//! | grok           | `$GROK_HOME/config.toml`                                    | `[mcp_servers.x]`|
//! | mcode          | `$MINIMAX_DATA_DIR/mcp.json`                                | `mcpServers`     |
//! | zcode          | `$ZCODE_HOME/cli/config.json`                               | `mcp.servers`    |
//! | copilot        | `$COPILOT_HOME/mcp-config.json`                             | `mcpServers`     |

use std::path::{Path, PathBuf};

use owo_client_apps::managed::{Format, Seg};
use owo_config::McpTransport;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum McpApp {
    Codex,
    Claude,
    ClaudeDesktop,
    Cursor,
    Opencode,
    Grok,
    Mcode,
    Zcode,
    Copilot,
}

/// Apps `owo connect` knows that have no MCP configuration OwO AI Gateway could write.
pub const UNSUPPORTED: [(&str, &str); 1] = [("mmx", "the MiniMax CLI (`mmx`) runs single text commands and has no MCP support")];

const ALL_TRANSPORTS: &[McpTransport] = &[McpTransport::Stdio, McpTransport::Http, McpTransport::Sse];

impl McpApp {
    pub const ALL: [McpApp; 9] = [
        McpApp::Codex,
        McpApp::Claude,
        McpApp::ClaudeDesktop,
        McpApp::Cursor,
        McpApp::Opencode,
        McpApp::Grok,
        McpApp::Mcode,
        McpApp::Zcode,
        McpApp::Copilot,
    ];

    /// The `owo connect` name.
    pub fn name(self) -> &'static str {
        match self {
            McpApp::Codex => "codex",
            McpApp::Claude => "claude",
            McpApp::ClaudeDesktop => "claude-desktop",
            McpApp::Cursor => "cursor",
            McpApp::Opencode => "opencode",
            McpApp::Grok => "grok",
            McpApp::Mcode => "mcode",
            McpApp::Zcode => "zcode",
            McpApp::Copilot => "copilot",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            McpApp::Codex => "Codex (CLI and Desktop)",
            McpApp::Claude => "Claude Code",
            McpApp::ClaudeDesktop => "Claude Desktop",
            McpApp::Cursor => "Cursor",
            McpApp::Opencode => "OpenCode",
            McpApp::Grok => "Grok Build",
            McpApp::Mcode => "MiniMax Code",
            McpApp::Zcode => "ZCode",
            McpApp::Copilot => "GitHub Copilot (CLI and app)",
        }
    }

    /// `codex-desktop` reads the same `config.toml` as the Codex CLI, so both names are one target.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "codex-desktop" => Some(McpApp::Codex),
            _ => Self::ALL.into_iter().find(|a| a.name() == name),
        }
    }

    pub fn format(self) -> Format {
        match self {
            McpApp::Codex | McpApp::Grok => Format::Toml,
            _ => Format::Json,
        }
    }

    /// The object holding one entry per server.
    pub fn container(self) -> Vec<Seg> {
        match self {
            McpApp::Codex | McpApp::Grok => vec![Seg::key("mcp_servers")],
            McpApp::Opencode => vec![Seg::key("mcp")],
            McpApp::Zcode => vec![Seg::key("mcp"), Seg::key("servers")],
            _ => vec![Seg::key("mcpServers")],
        }
    }

    pub fn entry_path(self, server: &str) -> Vec<Seg> {
        let mut path = self.container();
        path.push(Seg::key(server));
        path
    }

    pub fn transports(self) -> &'static [McpTransport] {
        match self {
            // Codex speaks stdio and streamable HTTP only.
            McpApp::Codex => &[McpTransport::Stdio, McpTransport::Http],
            // Remote servers are added through Claude Desktop's Connectors UI, not its config file.
            McpApp::ClaudeDesktop => &[McpTransport::Stdio],
            _ => ALL_TRANSPORTS,
        }
    }

    /// Whether the app's config has a working-directory field for stdio servers.
    pub fn supports_cwd(self) -> bool {
        matches!(self, McpApp::Codex | McpApp::Grok)
    }

    /// The managed-edit client id of this app's MCP integration (`state/mcp-<app>/`).
    pub fn state_id(self) -> String {
        format!("mcp-{}", self.name())
    }
}

/// Resolved locations of every app's files.
#[derive(Debug, Clone)]
pub struct Env {
    home: PathBuf,
    /// Roaming AppData on Windows, Application Support on macOS, `~/.config` on Linux.
    config_dir: PathBuf,
    /// Local AppData on Windows; `config_dir` elsewhere.
    local_dir: PathBuf,
    /// Honor the apps' own location variables (`CODEX_HOME`, ...).
    app_vars: bool,
}

impl Env {
    /// The real locations. `OWO_TEST_HOME` (for tests only) puts every app under that one
    /// directory and ignores the apps' own location variables.
    pub fn system() -> Option<Env> {
        if let Some(root) = std::env::var_os("OWO_TEST_HOME").filter(|v| !v.is_empty()) {
            return Some(Env::rooted(Path::new(&root)));
        }
        let home = dirs::home_dir()?;
        let config_dir = dirs::config_dir().unwrap_or_else(|| home.join(".config"));
        let local_dir = if cfg!(windows) { dirs::data_local_dir().unwrap_or_else(|| config_dir.clone()) } else { config_dir.clone() };
        Some(Env { home, config_dir, local_dir, app_vars: true })
    }

    /// Every app under `root`, laid out like this platform's home directory.
    pub fn rooted(root: &Path) -> Env {
        let (config_dir, local_dir) = if cfg!(windows) {
            (root.join("AppData").join("Roaming"), root.join("AppData").join("Local"))
        } else if cfg!(target_os = "macos") {
            let d = root.join("Library").join("Application Support");
            (d.clone(), d)
        } else {
            (root.join(".config"), root.join(".config"))
        };
        Env { home: root.to_path_buf(), config_dir, local_dir, app_vars: false }
    }

    fn var(&self, key: &str) -> Option<PathBuf> {
        if !self.app_vars {
            return None;
        }
        std::env::var_os(key).filter(|v| !v.is_empty()).map(PathBuf::from)
    }

    fn opencode_dir(&self) -> PathBuf {
        self.var("XDG_CONFIG_HOME").unwrap_or_else(|| self.home.join(".config")).join("opencode")
    }

    /// Claude Desktop's user-data directories: the regular one, and the one it uses in
    /// third-party inference mode (`owo connect claude-desktop`).
    fn claude_desktop_dirs(&self) -> [PathBuf; 2] {
        let third_party = self.var("CLAUDE_USER_DATA_DIR").unwrap_or_else(|| self.local_dir.join("Claude-3p"));
        [self.config_dir.join("Claude"), third_party]
    }

    /// The directory whose presence means the app is installed.
    fn app_dir(&self, app: McpApp) -> PathBuf {
        match app {
            McpApp::Codex => self.var("CODEX_HOME").unwrap_or_else(|| self.home.join(".codex")),
            McpApp::Claude => self.var("CLAUDE_CONFIG_DIR").unwrap_or_else(|| self.home.join(".claude")),
            McpApp::ClaudeDesktop => self.claude_desktop_dirs()[0].clone(),
            McpApp::Cursor => self.home.join(".cursor"),
            McpApp::Opencode => self.opencode_dir(),
            McpApp::Grok => self.var("GROK_HOME").unwrap_or_else(|| self.home.join(".grok")),
            McpApp::Mcode => self.var("MINIMAX_DATA_DIR").or_else(|| self.var("MAVIS_DATA_DIR")).unwrap_or_else(|| self.home.join(".minimax")),
            McpApp::Zcode => self.var("ZCODE_HOME").unwrap_or_else(|| self.home.join(".zcode")),
            McpApp::Copilot => self.var("COPILOT_HOME").unwrap_or_else(|| self.home.join(".copilot")),
        }
    }

    /// The files OwO AI Gateway writes the app's servers into (empty when the app is not installed).
    pub fn files(&self, app: McpApp) -> Vec<PathBuf> {
        let dir = self.app_dir(app);
        match app {
            McpApp::Claude => {
                let file = match self.var("CLAUDE_CONFIG_DIR") {
                    Some(d) => d.join(".claude.json"),
                    None => self.home.join(".claude.json"),
                };
                if file.is_file() || dir.is_dir() { vec![file] } else { Vec::new() }
            }
            McpApp::ClaudeDesktop => {
                self.claude_desktop_dirs().into_iter().filter(|d| d.is_dir()).map(|d| d.join("claude_desktop_config.json")).collect()
            }
            _ if !dir.is_dir() => Vec::new(),
            McpApp::Codex | McpApp::Grok => vec![dir.join("config.toml")],
            McpApp::Cursor => vec![dir.join("mcp.json")],
            McpApp::Opencode => vec![dir.join("opencode.json")],
            McpApp::Mcode => vec![dir.join("mcp.json")],
            McpApp::Zcode => vec![dir.join("cli").join("config.json")],
            McpApp::Copilot => vec![dir.join("mcp-config.json")],
        }
    }

    pub fn installed(&self, app: McpApp) -> bool {
        !self.files(app).is_empty()
    }

    /// Why OwO AI Gateway cannot write this installed app's file, if it cannot.
    pub fn blocked(&self, app: McpApp) -> Option<String> {
        if app == McpApp::Opencode {
            let dir = self.opencode_dir();
            if !dir.join("opencode.json").exists() && dir.join("opencode.jsonc").exists() {
                return Some(format!(
                    "OpenCode's config is {} (JSON with comments), which OwO AI Gateway does not rewrite",
                    dir.join("opencode.jsonc").display()
                ));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_codex_desktop_is_codex() {
        for app in McpApp::ALL {
            assert_eq!(McpApp::from_name(app.name()), Some(app));
        }
        assert_eq!(McpApp::from_name("codex-desktop"), Some(McpApp::Codex));
        assert_eq!(McpApp::from_name("mmx"), None);
        // Every app `owo connect` knows is either a target or listed as unsupported.
        for (name, _) in owo_config::APP_NAMES {
            assert!(McpApp::from_name(name).is_some() || UNSUPPORTED.iter().any(|(n, _)| *n == name), "{name}");
        }
    }

    #[test]
    fn rooted_env_detects_installs_by_directory() {
        let root = std::env::temp_dir().join(format!("owo-mcp-env-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".cursor")).unwrap();
        std::fs::create_dir_all(root.join(".config").join("opencode")).unwrap();
        std::fs::write(root.join(".config").join("opencode").join("opencode.jsonc"), "{}").unwrap();
        let env = Env::rooted(&root);
        assert_eq!(env.files(McpApp::Cursor), [root.join(".cursor").join("mcp.json")]);
        assert!(!env.installed(McpApp::Codex));
        assert!(env.blocked(McpApp::Opencode).is_some());
        std::fs::write(root.join(".claude.json"), "{}").unwrap();
        assert!(env.installed(McpApp::Claude), "a ~/.claude.json alone means Claude Code is installed");
    }
}
