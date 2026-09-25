//! Runs a command with administrator rights: a UAC prompt on Windows, `sudo` (prompting
//! in the terminal) on macOS and Linux, or directly when already running as root.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// Runs `program args…` elevated and waits. `log` receives the command's output where
/// the elevated process cannot write to this terminal (Windows).
pub fn run(program: &str, args: &[String], log: &Path) -> Result<()> {
    imp::run(program, args, log)
}

/// The command as a user would type it, for messages.
pub fn display(program: &str, args: &[String]) -> String {
    std::iter::once(program.to_string())
        .chain(args.iter().map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a.clone() }))
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
mod imp {
    use super::*;

    fn ps_quote(value: &str) -> String {
        format!("'{}'", value.replace('\'', "''"))
    }

    fn encoded(script: &str) -> String {
        use base64::Engine;
        let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
        base64::engine::general_purpose::STANDARD.encode(utf16)
    }

    /// Windows PowerShell 5 redirection writes UTF-16LE with a BOM.
    fn decode_log(bytes: &[u8]) -> String {
        match bytes {
            [0xFF, 0xFE, rest @ ..] => {
                let units: Vec<u16> = rest.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units)
            }
            _ => String::from_utf8_lossy(bytes).into_owned(),
        }
    }

    pub fn run(program: &str, args: &[String], log: &Path) -> Result<()> {
        let _ = std::fs::remove_file(log);
        // The elevated command runs through the call operator with every argument quoted,
        // and reaches the elevated PowerShell base64-encoded: nothing is re-split on spaces.
        let call: Vec<String> = std::iter::once(program).chain(args.iter().map(String::as_str)).map(ps_quote).collect();
        let inner = format!(
            "$ProgressPreference = 'SilentlyContinue'; & {} *> {}; exit $LASTEXITCODE",
            call.join(" "),
            ps_quote(&log.display().to_string())
        );
        let outer = format!(
            "$p = Start-Process -FilePath powershell -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{}' -Verb RunAs -Wait -PassThru -WindowStyle Hidden; exit $p.ExitCode",
            encoded(&inner)
        );
        let status = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &outer])
            .status()
            .context("cannot start PowerShell to request administrator rights")?;
        if status.success() {
            return Ok(());
        }
        let detail = std::fs::read(log).map(|b| decode_log(&b)).unwrap_or_default();
        match detail.trim() {
            "" => bail!("`{}` did not succeed (was the administrator prompt declined?)", display(program, args)),
            d => bail!("`{}` did not succeed:\n{d}", display(program, args)),
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::*;

    fn is_root() -> bool {
        std::process::Command::new("id").arg("-u").output().is_ok_and(|o| String::from_utf8_lossy(&o.stdout).trim() == "0")
    }

    pub fn run(program: &str, args: &[String], _log: &Path) -> Result<()> {
        let mut cmd = if is_root() {
            std::process::Command::new(program)
        } else {
            let mut c = std::process::Command::new("sudo");
            c.arg(program);
            c
        };
        cmd.args(args);
        let status = match cmd.status() {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                bail!("`sudo` is not available; run this as root, then re-run:\n  {}", display(program, args))
            }
            Err(e) => return Err(e).with_context(|| format!("cannot run {program}")),
        };
        if !status.success() {
            bail!("`{}` did not succeed (exit status {status})", display(program, args));
        }
        Ok(())
    }
}
