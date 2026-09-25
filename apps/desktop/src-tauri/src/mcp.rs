//! The MCP page: every read and write goes through `owo mcp`, so the desktop app and the CLI
//! share one implementation of the app formats, backups and conflict checks. Secrets reach
//! the CLI only as `keyring:` references (the UI stores them with `set_key` first).

use anyhow::anyhow;
use serde::{Deserialize, Serialize};

use crate::owo_cli;

type Reply<T> = std::result::Result<T, String>;

#[derive(Serialize)]
pub struct McpResult {
    ok: bool,
    output: String,
}

async fn owo(args: Vec<String>) -> Reply<McpResult> {
    tokio::task::spawn_blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        owo_cli::run(&args).map(|o| McpResult { ok: o.ok, output: o.text })
    })
    .await
    .map_err(|e| anyhow!("{e}"))
    .and_then(|r| r)
    .map_err(|e| format!("{e:#}"))
}

async fn owo_json(args: &[&str]) -> Reply<serde_json::Value> {
    let result = owo(args.iter().map(|a| a.to_string()).collect()).await?;
    if !result.ok {
        return Err(result.output);
    }
    serde_json::from_str(&result.output).map_err(|e| format!("unexpected output from `owo {}`: {e}", args.join(" ")))
}

/// `owo mcp list --json`: `{ apps, servers }`.
#[tauri::command]
pub async fn mcp_list() -> Reply<serde_json::Value> {
    owo_json(&["mcp", "list", "--json"]).await
}

/// `owo mcp scan --json`: every server entry in the installed apps' own configs.
#[tauri::command]
pub async fn mcp_scan() -> Reply<serde_json::Value> {
    owo_json(&["mcp", "scan", "--json"]).await
}

/// One `env` / header entry; `value: None` keeps the current value (hidden from the UI).
#[derive(Deserialize)]
pub struct Pair {
    key: String,
    value: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpEdit {
    name: String,
    /// Change the existing server of this name.
    replace: bool,
    /// `stdio`, `http` or `sse`.
    transport: String,
    description: Option<String>,
    command: Option<String>,
    args: Vec<String>,
    env: Vec<Pair>,
    cwd: Option<String>,
    url: Option<String>,
    headers: Vec<Pair>,
    apps: Vec<String>,
    force: bool,
}

fn pair_args(out: &mut Vec<String>, pairs: &[Pair], flag: &str, keep_flag: &str) {
    for p in pairs.iter().filter(|p| !p.key.trim().is_empty()) {
        match &p.value {
            Some(v) => out.push(format!("--{flag}={}={v}", p.key.trim())),
            None => out.push(format!("--{keep_flag}={}", p.key.trim())),
        }
    }
}

/// `owo mcp add` (with `--replace` when editing).
#[tauri::command]
pub async fn mcp_save(server: McpEdit) -> Reply<McpResult> {
    let some = |v: &Option<String>| v.as_deref().map(str::trim).filter(|v| !v.is_empty()).map(str::to_string);
    let mut args = vec!["mcp".to_string(), "add".to_string(), server.name.trim().to_string()];
    if server.transport == "stdio" {
        args.push(format!("--command={}", some(&server.command).unwrap_or_default()));
        args.extend(server.args.iter().map(|a| format!("--arg={a}")));
        pair_args(&mut args, &server.env, "env", "keep-env");
        if let Some(cwd) = some(&server.cwd) {
            args.push(format!("--cwd={cwd}"));
        }
    } else {
        args.push(format!("--url={}", some(&server.url).unwrap_or_default()));
        if server.transport == "sse" {
            args.push("--sse".into());
        }
        pair_args(&mut args, &server.headers, "header", "keep-header");
    }
    if let Some(d) = some(&server.description) {
        args.push(format!("--description={d}"));
    }
    args.push(format!("--app={}", server.apps.join(",")));
    if server.replace {
        args.push("--replace".into());
    }
    if server.force {
        args.push("--force".into());
    }
    owo(args).await
}

#[tauri::command]
pub async fn mcp_remove(name: String, force: bool) -> Reply<McpResult> {
    let mut args = vec!["mcp".into(), "remove".into(), name];
    if force {
        args.push("--force".into());
    }
    owo(args).await
}

/// `owo mcp enable|disable <name> --app <app>`.
#[tauri::command]
pub async fn mcp_toggle(name: String, app: String, enabled: bool, force: bool) -> Reply<McpResult> {
    let mut args = vec!["mcp".into(), if enabled { "enable" } else { "disable" }.into(), name, format!("--app={app}")];
    if force {
        args.push("--force".into());
    }
    owo(args).await
}

/// `owo mcp import <app|all> <names…>`; `keyring` moves secret-looking values into the keyring.
#[tauri::command]
pub async fn mcp_import(app: String, names: Vec<String>, keyring: bool, force: bool) -> Reply<McpResult> {
    let mut args = vec!["mcp".into(), "import".into(), app];
    args.extend(names);
    if keyring {
        args.push("--keyring".into());
    }
    if force {
        args.push("--force".into());
    }
    owo(args).await
}

#[tauri::command]
pub async fn mcp_sync(force: bool) -> Reply<McpResult> {
    let mut args = vec!["mcp".into(), "sync".into()];
    if force {
        args.push("--force".into());
    }
    owo(args).await
}
