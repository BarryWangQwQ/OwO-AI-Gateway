//! Locating Codex: its home directory and an executable to read the bundled catalog from.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde_json::Value;

/// `$CODEX_HOME`, else `~/.codex`.
pub fn codex_home() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    dirs::home_dir().map(|h| h.join(".codex"))
}

/// A Codex executable: `codex` on PATH, else the newest one installed by Codex Desktop.
pub fn find_codex_binary() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "codex.exe" } else { "codex" };
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let candidate = dir.join(exe);
            if candidate.is_file() {
                return Some(candidate);
            }
            if cfg!(windows) {
                // npm installs a `codex.cmd` shim; it runs the same CLI.
                let shim = dir.join("codex.cmd");
                if shim.is_file() {
                    return Some(shim);
                }
            }
        }
    }
    desktop_bundled_binary(exe)
}

fn desktop_bundled_binary(exe: &str) -> Option<PathBuf> {
    let roots: Vec<PathBuf> = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|d| vec![PathBuf::from(d).join("OpenAI").join("Codex").join("bin")]).unwrap_or_default()
    } else if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Applications/Codex.app/Contents/Resources")]
    } else {
        Vec::new()
    };
    let mut best: Option<(std::time::SystemTime, PathBuf)> = None;
    for root in roots {
        let direct = root.join(exe);
        if direct.is_file() {
            return Some(direct);
        }
        let Ok(entries) = std::fs::read_dir(&root) else { continue };
        for entry in entries.flatten() {
            let candidate = entry.path().join(exe);
            if let Ok(meta) = candidate.metadata() {
                let modified = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
                if best.as_ref().is_none_or(|(t, _)| modified > *t) {
                    best = Some((modified, candidate));
                }
            }
        }
    }
    best.map(|(_, p)| p)
}

fn command(bin: &Path) -> Command {
    let is_cmd = bin.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if is_cmd {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(bin);
        c
    } else {
        Command::new(bin)
    }
}

pub fn codex_version(bin: &Path) -> Result<String> {
    let out = command(bin).arg("--version").output().with_context(|| format!("cannot run {}", bin.display()))?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// `codex debug models --bundled`: the catalog compiled into this Codex build.
pub fn bundled_catalog(bin: &Path, codex_home: &Path) -> Result<Value> {
    let out = command(bin)
        .args(["debug", "models", "--bundled"])
        .env("CODEX_HOME", codex_home)
        .output()
        .with_context(|| format!("cannot run {}", bin.display()))?;
    if !out.status.success() {
        bail!("`codex debug models --bundled` failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    serde_json::from_slice(&out.stdout).context("`codex debug models --bundled` did not print JSON")
}

/// Asks Codex to load its config (plus `overrides`, as `-c` layers) exactly as it would at
/// startup, and returns the model slugs it lists.
///
/// `debug models` validates the whole config, including the selected provider, without
/// starting MCP servers or plugins the way `debug prompt-input` does.
pub fn verify_config(bin: &Path, codex_home: &Path, overrides: &[String]) -> Result<Vec<String>> {
    let mut cmd = command(bin);
    for o in overrides {
        cmd.arg("-c").arg(o);
    }
    let out = cmd
        .args(["debug", "models"])
        .env("CODEX_HOME", codex_home)
        .output()
        .with_context(|| format!("cannot run {}", bin.display()))?;
    if !out.status.success() {
        bail!("Codex rejected the settings: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let v: Value = serde_json::from_slice(&out.stdout).context("`codex debug models` did not print JSON")?;
    Ok(v.get("models")
        .and_then(Value::as_array)
        .map(|m| m.iter().filter_map(|e| e.get("slug").and_then(Value::as_str).map(str::to_string)).collect())
        .unwrap_or_default())
}
