//! Runs the `owo` command line for actions with side effects (starting the gateway,
//! connecting apps), so the desktop app and the CLI share one tested implementation.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};

pub struct Output {
    pub ok: bool,
    /// stdout followed by stderr.
    pub text: String,
}

/// `OWO_BIN`, else `owo` next to this app, on PATH, or in this repository's build output.
pub fn binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OWO_BIN").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    let exe = if cfg!(windows) { "owo.exe" } else { "owo" };
    let beside_app = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.join(exe)));
    let on_path = std::env::var_os("PATH").into_iter().flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).map(|d| d.join(exe));
    if let Some(found) = beside_app.into_iter().chain(on_path).find(|p| p.is_file()) {
        return Ok(found);
    }
    let target = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../target");
    let built = ["release", "debug"].iter().map(|profile| target.join(profile).join(exe)).filter(|p| p.is_file());
    if let Some(newest) = built.max_by_key(|p| p.metadata().and_then(|m| m.modified()).ok()) {
        return Ok(newest);
    }
    bail!("cannot find the `owo` executable; put it next to this app or on PATH (or set OWO_BIN)")
}

pub fn run(args: &[&str]) -> Result<Output> {
    let bin = binary()?;
    let mut cmd = Command::new(&bin);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().with_context(|| format!("cannot run {}", bin.display()))?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !stderr.trim().is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&stderr);
    }
    Ok(Output { ok: out.status.success(), text: text.trim_end().to_string() })
}
