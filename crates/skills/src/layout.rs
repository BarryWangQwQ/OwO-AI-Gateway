//! Where each app keeps its skills, and how OwO AI Gateway turns a skill on or off for it.
//!
//! The one store is `~/.agents/skills/<name>/` (the cross-tool location). Apps that read it
//! natively get every skill there; OwO AI Gateway turns one off through the app's own setting
//! when it has one. Apps that only read their own directory get a link to the store.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// For tests only: the user home for every directory this crate touches. App-specific
/// variables (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, ...) are ignored while it is set.
pub const TEST_HOME_ENV: &str = "OWO_TEST_USER_HOME";

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).filter(|v| !v.is_empty()).map(PathBuf::from)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub home: PathBuf,
    /// `~/.agents/skills`, the store.
    pub agents: PathBuf,
    /// `$CODEX_HOME`, else `~/.codex`.
    pub codex_home: PathBuf,
    /// `$CLAUDE_CONFIG_DIR`, else `~/.claude`.
    pub claude_home: PathBuf,
    pub cursor_home: PathBuf,
    /// `$XDG_CONFIG_HOME/opencode`, else `~/.config/opencode` (on every platform).
    pub opencode_home: PathBuf,
    /// `$GROK_HOME`, else `~/.grok`.
    pub grok_home: PathBuf,
    /// `$ZCODE_HOME`, else `~/.zcode`.
    pub zcode_home: PathBuf,
    /// `$COPILOT_HOME`, else `~/.copilot`.
    pub copilot_home: PathBuf,
}

impl Layout {
    pub fn for_home(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            agents: home.join(".agents").join("skills"),
            codex_home: home.join(".codex"),
            claude_home: home.join(".claude"),
            cursor_home: home.join(".cursor"),
            opencode_home: home.join(".config").join("opencode"),
            grok_home: home.join(".grok"),
            zcode_home: home.join(".zcode"),
            copilot_home: home.join(".copilot"),
        }
    }

    pub fn from_env() -> Option<Self> {
        if let Some(home) = env_dir(TEST_HOME_ENV) {
            return Some(Self::for_home(&home));
        }
        let mut l = Self::for_home(&dirs::home_dir()?);
        if let Some(d) = env_dir("CODEX_HOME") {
            l.codex_home = d;
        }
        if let Some(d) = env_dir("CLAUDE_CONFIG_DIR") {
            l.claude_home = d;
        }
        if let Some(d) = env_dir("XDG_CONFIG_HOME") {
            l.opencode_home = d.join("opencode");
        }
        if let Some(d) = env_dir("GROK_HOME") {
            l.grok_home = d;
        }
        if let Some(d) = env_dir("ZCODE_HOME") {
            l.zcode_home = d;
        }
        if let Some(d) = env_dir("COPILOT_HOME") {
            l.copilot_home = d;
        }
        Some(l)
    }

    pub fn dir(&self, loc: Location) -> PathBuf {
        match loc {
            Location::Agents => self.agents.clone(),
            Location::Codex => self.codex_home.join("skills"),
            Location::Claude => self.claude_home.join("skills"),
            Location::Cursor => self.cursor_home.join("skills"),
            Location::Opencode => self.opencode_home.join("skills"),
            Location::Grok => self.grok_home.join("skills"),
            Location::Zcode => self.zcode_home.join("skills"),
            Location::Copilot => self.copilot_home.join("skills"),
        }
    }

    /// The file holding the app's per-skill switch, for apps that have one.
    pub fn switch_file(&self, app: AppId) -> Option<PathBuf> {
        match app {
            AppId::Codex => Some(self.codex_home.join("config.toml")),
            // A lone `opencode.jsonc` is used (and must hold plain JSON) rather than adding
            // an `opencode.json` beside it.
            AppId::Opencode => {
                let (json, jsonc) = (self.opencode_home.join("opencode.json"), self.opencode_home.join("opencode.jsonc"));
                Some(if !json.exists() && jsonc.exists() { jsonc } else { json })
            }
            AppId::Grok => Some(self.grok_home.join("config.toml")),
            AppId::Copilot => Some(self.copilot_home.join("settings.json")),
            _ => None,
        }
    }

    /// Whether the app looks installed (its own directory exists).
    pub fn detected(&self, app: AppId) -> bool {
        match app {
            AppId::Codex => self.codex_home.is_dir(),
            AppId::Claude => self.claude_home.is_dir(),
            AppId::Cursor => self.cursor_home.is_dir(),
            AppId::Opencode => self.opencode_home.is_dir(),
            AppId::Grok => self.grok_home.is_dir(),
            AppId::Zcode => self.zcode_home.is_dir(),
            AppId::Copilot => self.copilot_home.is_dir(),
            AppId::Mcode => self.home.join(".minimax").is_dir(),
            AppId::ClaudeDesktop => false,
        }
    }
}

/// A user-level skills directory some app reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Location {
    Agents,
    Codex,
    Claude,
    Cursor,
    Opencode,
    Grok,
    Zcode,
    Copilot,
}

impl Location {
    pub const ALL: [Location; 8] =
        [Location::Agents, Location::Codex, Location::Claude, Location::Cursor, Location::Opencode, Location::Grok, Location::Zcode, Location::Copilot];

    /// Apps that load skills from this directory.
    pub fn readers(self) -> &'static [AppId] {
        use AppId::*;
        match self {
            Location::Agents => &[Codex, Cursor, Opencode, Grok, Zcode, Copilot],
            // Cursor, OpenCode, and Grok read Claude Code's directory for compatibility.
            Location::Claude => &[Claude, Cursor, Opencode, Grok],
            // Codex's older user location; Cursor reads it for compatibility.
            Location::Codex => &[Codex, Cursor],
            Location::Cursor => &[Cursor, Grok],
            Location::Opencode => &[Opencode],
            Location::Grok => &[Grok],
            Location::Zcode => &[Zcode],
            Location::Copilot => &[Copilot],
        }
    }
}

/// How OwO AI Gateway handles one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Support {
    /// Reads the store; OwO AI Gateway turns a skill off through the app's own setting.
    Switch,
    /// Reads the store and has no per-skill setting OwO AI Gateway may edit: always on.
    AlwaysOn,
    /// Reads only its own directory: OwO AI Gateway links the skill there.
    Link,
    /// Has no user-level skills directory OwO AI Gateway can use.
    Unsupported,
}

/// Apps as the skills commands name them (`codex` covers the CLI and Codex Desktop, which
/// share `CODEX_HOME`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppId {
    Codex,
    Claude,
    Cursor,
    Opencode,
    Grok,
    Zcode,
    Copilot,
    Mcode,
    ClaudeDesktop,
}

impl AppId {
    pub const ALL: [AppId; 9] =
        [AppId::Codex, AppId::Claude, AppId::Cursor, AppId::Opencode, AppId::Grok, AppId::Zcode, AppId::Copilot, AppId::Mcode, AppId::ClaudeDesktop];

    pub fn name(self) -> &'static str {
        match self {
            AppId::Codex => "codex",
            AppId::Claude => "claude",
            AppId::Cursor => "cursor",
            AppId::Opencode => "opencode",
            AppId::Grok => "grok",
            AppId::Zcode => "zcode",
            AppId::Copilot => "copilot",
            AppId::Mcode => "mcode",
            AppId::ClaudeDesktop => "claude-desktop",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            AppId::Codex => "Codex",
            AppId::Claude => "Claude Code",
            AppId::Cursor => "Cursor",
            AppId::Opencode => "OpenCode",
            AppId::Grok => "Grok Build",
            AppId::Zcode => "ZCode",
            AppId::Copilot => "GitHub Copilot",
            AppId::Mcode => "MiniMax Code",
            AppId::ClaudeDesktop => "Claude Desktop",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().to_ascii_lowercase();
        let s = match s.as_str() {
            "codex-desktop" => "codex",
            "claude-code" => "claude",
            other => other,
        };
        AppId::ALL.into_iter().find(|a| a.name() == s)
    }

    pub fn support(self) -> Support {
        match self {
            AppId::Codex | AppId::Opencode | AppId::Grok | AppId::Copilot => Support::Switch,
            AppId::Cursor | AppId::Zcode => Support::AlwaysOn,
            AppId::Claude => Support::Link,
            AppId::Mcode | AppId::ClaudeDesktop => Support::Unsupported,
        }
    }

    /// The app's own directory, when it has one besides the store.
    pub fn own_location(self) -> Option<Location> {
        match self {
            AppId::Codex => Some(Location::Codex),
            AppId::Claude => Some(Location::Claude),
            AppId::Cursor => Some(Location::Cursor),
            AppId::Opencode => Some(Location::Opencode),
            AppId::Grok => Some(Location::Grok),
            AppId::Zcode => Some(Location::Zcode),
            AppId::Copilot => Some(Location::Copilot),
            AppId::Mcode | AppId::ClaudeDesktop => None,
        }
    }

    /// One line on how the app loads skills, for `owo skills apps` and the desktop tooltips.
    pub fn note(self) -> &'static str {
        match self {
            AppId::Codex => "reads ~/.agents/skills; off = `[[skills.config]] enabled = false` in config.toml",
            AppId::Claude => "reads only ~/.claude/skills; on = a link there to the skill in ~/.agents/skills",
            AppId::Cursor => "reads ~/.agents/skills (and ~/.claude/skills, ~/.codex/skills); no per-skill setting OwO AI Gateway may edit",
            AppId::Opencode => "reads ~/.agents/skills; off = `permission.skill.<name> = \"deny\"` in opencode.json",
            AppId::Grok => "reads ~/.agents/skills; off = `[skills] disabled` in ~/.grok/config.toml",
            AppId::Zcode => "reads ~/.agents/skills; turn skills off in ZCode's Settings → Skills",
            AppId::Copilot => "reads ~/.agents/skills; off = `disabledSkills` in ~/.copilot/settings.json",
            AppId::Mcode => "loads skills only from MiniMax Code plugins",
            AppId::ClaudeDesktop => "skills are uploaded to your claude.ai account, not read from disk",
        }
    }
}

impl std::fmt::Display for AppId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_store_reader_is_native() {
        for app in Location::Agents.readers() {
            assert!(matches!(app.support(), Support::Switch | Support::AlwaysOn), "{app}");
        }
        assert!(!Location::Agents.readers().contains(&AppId::Claude));
    }

    #[test]
    fn app_names_round_trip() {
        for app in AppId::ALL {
            assert_eq!(AppId::parse(app.name()), Some(app));
        }
        assert_eq!(AppId::parse("codex-desktop"), Some(AppId::Codex));
        assert_eq!(AppId::parse("nope"), None);
    }
}
