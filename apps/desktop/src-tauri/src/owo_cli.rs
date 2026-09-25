//! Runs the `owo` command line for actions with side effects (starting the gateway,
//! connecting apps), so the desktop app and the CLI share one tested implementation.
//! The CLI is built into this executable and reached through [`crate::CLI_FLAG`].

use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result};

pub struct Output {
    pub ok: bool,
    /// stdout followed by stderr.
    pub text: String,
}

/// The executable that runs `owo` commands: `OWO_BIN` when set (a separately built `owo`),
/// otherwise this app itself.
pub fn binary() -> Result<PathBuf> {
    if let Some(path) = std::env::var_os("OWO_BIN").filter(|v| !v.is_empty()) {
        return Ok(PathBuf::from(path));
    }
    std::env::current_exe().context("cannot locate this executable")
}

pub fn run(args: &[&str]) -> Result<Output> {
    let external = std::env::var_os("OWO_BIN").is_some_and(|v| !v.is_empty());
    let bin = binary()?;
    let mut cmd = Command::new(&bin);
    if !external {
        cmd.arg(crate::CLI_FLAG);
    }
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
