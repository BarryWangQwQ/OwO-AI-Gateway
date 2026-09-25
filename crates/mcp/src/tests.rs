use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use owo_client_apps::managed::Store;
use owo_config::{McpServerConfig, McpTransport};
use owo_credentials::{CredentialBackend, CredentialStore};
use serde_json::{json, Value};

use crate::sync::{self, EntryState, Relation};
use crate::{sync_app, Ctx, Env, McpApp, Outcome};

struct Sandbox {
    root: PathBuf,
    env: Env,
    store: Store,
    credentials: CredentialStore,
}

impl Sandbox {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("owo-mcp-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = Store { state_dir: root.join("owo").join("state"), backups_dir: root.join("owo").join("backups") };
        Self { env: Env::rooted(&root.join("home")), root, store, credentials: CredentialStore::new(CredentialBackend::Env) }
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx { env: &self.env, store: &self.store, credentials: &self.credentials }
    }

    fn home(&self, rel: &str) -> PathBuf {
        self.root.join("home").join(rel)
    }

    fn write(&self, rel: &str, text: &str) -> PathBuf {
        let p = self.home(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
        p
    }

    fn sync(&self, servers: &BTreeMap<String, McpServerConfig>, app: McpApp, force: bool) -> sync::SyncReport {
        sync_app(&self.ctx(), servers, app, force)
    }
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap()
}

fn json_of(p: &Path) -> Value {
    serde_json::from_str(&read(p)).unwrap()
}

fn github(apps: &[&str]) -> McpServerConfig {
    McpServerConfig {
        command: Some("npx".into()),
        args: vec!["-y".into(), "@modelcontextprotocol/server-github".into()],
        env: [("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), "ghp_live_secret".to_string())].into(),
        apps: apps.iter().map(|a| a.to_string()).collect(),
        ..Default::default()
    }
}

fn servers(list: &[(&str, McpServerConfig)]) -> BTreeMap<String, McpServerConfig> {
    list.iter().map(|(n, s)| (n.to_string(), s.clone())).collect()
}

const CURSOR: &str = "{\n  \"mcpServers\": {\n    \"mine\": {\n      \"command\": \"mine.exe\"\n    }\n  }\n}\n";
const CODEX: &str = "model = \"gpt-5.6-sol\" # keep\n\n[mcp_servers.node_repl]\ncommand = 'C:\\tools\\node_repl.exe'\n";
const CLAUDE: &str = "{\n  \"numStartups\": 12,\n  \"projects\": {}\n}\n";

#[test]
fn enable_then_disable_restores_every_file_byte_for_byte() {
    let s = Sandbox::new("roundtrip");
    let cursor = s.write(".cursor/mcp.json", CURSOR);
    let codex = s.write(".codex/config.toml", CODEX);
    let claude = s.write(".claude.json", CLAUDE);
    let on = servers(&[("github", github(&["cursor", "codex-desktop", "claude"]))]);

    for app in [McpApp::Cursor, McpApp::Codex, McpApp::Claude] {
        let r = s.sync(&on, app, false);
        assert_eq!(r.outcome, Outcome::Synced, "{app:?}: {:?}", r.message);
    }
    let c = json_of(&cursor);
    assert_eq!(c["mcpServers"]["github"]["env"]["GITHUB_PERSONAL_ACCESS_TOKEN"], "ghp_live_secret");
    assert_eq!(c["mcpServers"]["mine"]["command"], "mine.exe", "the user's own server is untouched");
    let toml_text = read(&codex);
    let t: toml::Table = toml_text.parse().unwrap();
    assert_eq!(t["mcp_servers"]["github"]["command"].as_str(), Some("npx"));
    assert!(toml_text.contains("# keep") && toml_text.contains("[mcp_servers.node_repl]"), "{toml_text}");
    assert_eq!(json_of(&claude)["mcpServers"]["github"]["type"], "stdio");
    assert_eq!(json_of(&claude)["numStartups"], 12);

    let listed = sync::list(&s.ctx(), &on).unwrap();
    let states: BTreeMap<_, _> = listed[0].apps.iter().map(|a| (a.app, a.state)).collect();
    assert_eq!(states["cursor"], EntryState::Synced);
    assert_eq!(states["codex"], EntryState::Synced);
    assert_eq!(states["zcode"], EntryState::Off);
    let secret_value = listed[0].view.env[0].value.clone();
    assert_eq!(secret_value, None, "a literal secret is hidden in listings");

    let state_files: String = ["mcp-cursor", "mcp-codex", "mcp-claude"].iter().map(|c| read(&s.store.state_dir.join(c).join("state.json"))).collect();
    assert!(!state_files.contains("ghp_live_secret"), "resolved secrets never reach OwO AI Gateway's state");

    let off = servers(&[("github", github(&[]))]);
    for app in [McpApp::Cursor, McpApp::Codex, McpApp::Claude] {
        assert_eq!(s.sync(&off, app, false).outcome, Outcome::Removed, "{app:?}");
    }
    assert_eq!(read(&cursor), CURSOR);
    assert_eq!(read(&codex), CODEX);
    assert_eq!(read(&claude), CLAUDE);
    assert!(!s.store.state_dir.join("mcp-cursor").join("state.json").exists());
}

#[test]
fn toggling_one_server_leaves_the_others_and_outside_edits() {
    let s = Sandbox::new("toggle");
    let claude = s.write(".claude.json", CLAUDE);
    let fetch = McpServerConfig { command: Some("uvx".into()), args: vec!["mcp-server-fetch".into()], apps: vec!["claude".into()], ..Default::default() };
    let both = servers(&[("github", github(&["claude"])), ("fetch", fetch.clone())]);
    assert!(s.sync(&both, McpApp::Claude, false).ok());
    // Claude Code rewrites its own file between OwO AI Gateway's writes.
    let mut doc = json_of(&claude);
    doc["numStartups"] = json!(13);
    std::fs::write(&claude, serde_json::to_string_pretty(&doc).unwrap()).unwrap();

    let only_fetch = servers(&[("github", github(&[])), ("fetch", fetch)]);
    assert!(s.sync(&only_fetch, McpApp::Claude, false).ok());
    let doc = json_of(&claude);
    assert!(doc["mcpServers"].get("github").is_none());
    assert_eq!(doc["mcpServers"]["fetch"]["command"], "uvx");
    assert_eq!(doc["numStartups"], 13);

    assert_eq!(s.sync(&BTreeMap::new(), McpApp::Claude, false).outcome, Outcome::Removed);
    let doc = json_of(&claude);
    assert_eq!(doc, json!({"numStartups": 13, "projects": {}}), "Claude's own change survives; OwO AI Gateway's entries are gone");
}

#[test]
fn outside_edits_and_foreign_entries_are_conflicts() {
    let s = Sandbox::new("conflict");
    let cursor = s.write(".cursor/mcp.json", CURSOR);
    let on = servers(&[("github", github(&["cursor"]))]);
    assert!(s.sync(&on, McpApp::Cursor, false).ok());

    let edited = read(&cursor).replace("server-github", "server-github@1.2.3");
    std::fs::write(&cursor, &edited).unwrap();
    let states = sync::server_states(&s.ctx(), "github", &on["github"]).unwrap();
    assert_eq!(states.iter().find(|a| a.app == "cursor").unwrap().state, EntryState::Modified);
    let r = s.sync(&servers(&[("github", github(&[]))]), McpApp::Cursor, false);
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.message.as_deref().unwrap().contains("changed outside OwO AI Gateway"), "{r:?}");
    assert_eq!(read(&cursor), edited, "nothing is written on a conflict");
    assert!(s.sync(&servers(&[("github", github(&[]))]), McpApp::Cursor, true).ok());
    assert_eq!(json_of(&cursor), json!({"mcpServers": {"mine": {"command": "mine.exe"}}}));

    // A server of the same name OwO AI Gateway did not write.
    let mine = servers(&[("mine", McpServerConfig { command: Some("other".into()), apps: vec!["cursor".into()], ..Default::default() })]);
    let r = s.sync(&mine, McpApp::Cursor, false);
    assert_eq!(r.outcome, Outcome::Failed);
    assert!(r.message.as_deref().unwrap().contains("owo mcp import cursor mine"), "{r:?}");
    assert_eq!(json_of(&cursor)["mcpServers"]["mine"]["command"], "mine.exe");
}

#[test]
fn scanning_and_adopting_existing_entries() {
    let s = Sandbox::new("adopt");
    let cursor = s.write(
        ".cursor/mcp.json",
        r#"{"mcpServers": {"gh": {"command": "npx", "args": ["gh"], "env": {"GH_TOKEN": "t0ken"}}, "bad name": {"command": "x"}, "odd": {"url": 5}}}"#,
    );
    let found = sync::scan(&s.ctx(), &BTreeMap::new()).unwrap();
    let gh = found.iter().find(|c| c.found.name == "gh").unwrap();
    assert!(!gh.found.managed);
    assert_eq!(gh.found.relation, Relation::New);
    assert_eq!(gh.found.secrets, ["env.GH_TOKEN"]);
    assert_eq!(gh.found.server.as_ref().unwrap().env[0].value, None, "scans hide secrets too");
    assert!(found.iter().find(|c| c.found.name == "bad name").unwrap().found.error.is_some());
    assert!(found.iter().find(|c| c.found.name == "odd").unwrap().found.error.is_some());

    // Import: the server joins OwO AI Gateway's list and OwO AI Gateway takes the entry over.
    let mut server = gh.parsed.clone().unwrap().server;
    server.apps = vec!["cursor".into()];
    let list = servers(&[("gh", server.clone())]);
    sync::adopt(&s.ctx(), McpApp::Cursor, &cursor, "gh").unwrap();
    let r = s.sync(&list, McpApp::Cursor, false);
    assert!(r.ok(), "{r:?}");
    let found = sync::scan(&s.ctx(), &list).unwrap();
    let gh = found.iter().find(|c| c.found.name == "gh").unwrap();
    assert!(gh.found.managed);
    assert_eq!(gh.found.relation, Relation::Same);

    // Disabling removes the adopted entry and nothing else.
    server.apps.clear();
    assert!(s.sync(&servers(&[("gh", server)]), McpApp::Cursor, false).ok());
    let doc = json_of(&cursor);
    assert!(doc["mcpServers"].get("gh").is_none());
    assert!(doc["mcpServers"].get("bad name").is_some() && doc["mcpServers"].get("odd").is_some());
}

#[test]
fn missing_and_unsupported_apps_are_skipped() {
    let s = Sandbox::new("skip");
    let sse = McpServerConfig { transport: Some(McpTransport::Sse), url: Some("http://127.0.0.1:9/sse".into()), apps: vec!["codex".into(), "zcode".into()], ..Default::default() };
    let list = servers(&[("old", sse)]);
    let r = s.sync(&list, McpApp::Zcode, false);
    assert_eq!(r.outcome, Outcome::NotInstalled);
    assert!(!s.home(".zcode").exists(), "nothing is created for an app that is not installed");

    let codex = s.write(".codex/config.toml", CODEX);
    let r = s.sync(&list, McpApp::Codex, false);
    assert_eq!(r.outcome, Outcome::Unchanged);
    assert!(r.warnings[0].contains("does not support sse"), "{r:?}");
    assert_eq!(read(&codex), CODEX);
    let states = sync::server_states(&s.ctx(), "old", &list["old"]).unwrap();
    let state = |app: &str| states.iter().find(|a| a.app == app).unwrap().state;
    assert_eq!((state("codex"), state("zcode")), (EntryState::Unsupported, EntryState::NotInstalled));
}

#[test]
fn app_specific_files() {
    let s = Sandbox::new("files");
    let desktop_dirs = if cfg!(windows) {
        ["AppData/Roaming/Claude", "AppData/Local/Claude-3p"]
    } else if cfg!(target_os = "macos") {
        ["Library/Application Support/Claude", "Library/Application Support/Claude-3p"]
    } else {
        [".config/Claude", ".config/Claude-3p"]
    };
    for d in desktop_dirs {
        std::fs::create_dir_all(s.home(d)).unwrap();
    }
    s.write(".config/opencode/opencode.json", "{\"theme\": \"x\"}");
    s.write(".zcode/cli/config.json", "{}");
    s.write(".copilot/config.json", "{}");
    let list = servers(&[("github", github(&["claude-desktop", "opencode", "zcode", "copilot"]))]);
    for app in [McpApp::ClaudeDesktop, McpApp::Opencode, McpApp::Zcode, McpApp::Copilot] {
        let r = s.sync(&list, app, false);
        assert_eq!(r.outcome, Outcome::Synced, "{app:?}: {r:?}");
    }
    for d in desktop_dirs {
        let doc = json_of(&s.home(d).join("claude_desktop_config.json"));
        assert_eq!(doc["mcpServers"]["github"]["command"], "npx", "{d}");
    }
    let oc = json_of(&s.home(".config/opencode/opencode.json"));
    assert_eq!(oc["theme"], "x");
    assert_eq!(oc["mcp"]["github"]["command"][0], "npx");
    assert_eq!(oc["mcp"]["github"]["environment"]["GITHUB_PERSONAL_ACCESS_TOKEN"], "ghp_live_secret");
    assert_eq!(json_of(&s.home(".zcode/cli/config.json"))["mcp"]["servers"]["github"]["type"], "stdio");
    assert_eq!(json_of(&s.home(".copilot/mcp-config.json"))["mcpServers"]["github"]["tools"], json!(["*"]));

    // References are resolved when written. A missing one leaves that server as it was
    // (or unwritten) without holding up the others.
    let mut broken = github(&["opencode"]);
    broken.env.insert("OTHER_TOKEN".into(), "env:OWO_TEST_SURELY_UNSET_VAR".into());
    let fetch = McpServerConfig { command: Some("uvx".into()), apps: vec!["opencode".into()], ..Default::default() };
    let other = McpServerConfig { url: Some("https://x.example/mcp".into()), headers: [("Authorization".to_string(), "env:OWO_TEST_SURELY_UNSET_VAR".to_string())].into(), apps: vec!["opencode".into()], ..Default::default() };
    let list = servers(&[("github", broken), ("fetch", fetch), ("other", other)]);
    let r = s.sync(&list, McpApp::Opencode, false);
    assert_eq!(r.outcome, Outcome::Synced, "{r:?}");
    assert_eq!(r.skipped, ["github", "other"]);
    assert!(r.warnings.iter().any(|w| w.contains("left as it was") && w.contains("mcp.github.env.OTHER_TOKEN")), "{r:?}");
    let oc = json_of(&s.home(".config/opencode/opencode.json"));
    assert_eq!(oc["mcp"]["github"]["environment"]["GITHUB_PERSONAL_ACCESS_TOKEN"], "ghp_live_secret", "kept as written before");
    assert!(oc["mcp"].get("fetch").is_some() && oc["mcp"].get("other").is_none());
}

#[test]
fn changed_definitions_show_as_outdated() {
    let s = Sandbox::new("outdated");
    s.write(".cursor/mcp.json", "{}");
    let list = servers(&[("github", github(&["cursor"]))]);
    assert!(s.sync(&list, McpApp::Cursor, false).ok());
    let mut changed = github(&["cursor"]);
    changed.args.push("--read-only".into());
    let states = sync::server_states(&s.ctx(), "github", &changed).unwrap();
    assert_eq!(states.iter().find(|a| a.app == "cursor").unwrap().state, EntryState::Outdated);
    assert!(s.sync(&servers(&[("github", changed.clone())]), McpApp::Cursor, false).ok());
    let states = sync::server_states(&s.ctx(), "github", &changed).unwrap();
    assert_eq!(states.iter().find(|a| a.app == "cursor").unwrap().state, EntryState::Synced);
}
