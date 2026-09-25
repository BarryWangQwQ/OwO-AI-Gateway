use anyhow::{bail, Context, Result};
use owo_config::SAMPLE_CONFIG;

use crate::cli::GlobalArgs;
use crate::context;

pub fn init(global: &GlobalArgs, force: bool) -> Result<()> {
    let paths = context::paths(global)?;
    if paths.config.exists() && !force {
        bail!("{} already exists (edit it, or use --force to start over)", paths.config.display());
    }
    if let Some(dir) = paths.config.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    write_atomic(&paths.config, SAMPLE_CONFIG)?;
    paths.ensure_dirs()?;
    println!("wrote {}", paths.config.display());
    println!("\nNext:");
    println!("  1. owo add <provider>      e.g. `owo add anthropic`; asks for the key (`owo providers presets` lists providers)");
    println!("  2. owo start               run the gateway");
    println!("  3. owo connect <app>       point an app at OwO AI Gateway (`owo apps` lists them)");
    Ok(())
}

/// Opens config.toml in `$VISUAL` / `$EDITOR`, else the platform's default text editor.
pub fn edit(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    if !paths.config.exists() {
        bail!("there is no config yet at {} (create it with `owo init` or `owo add <provider>`)", paths.config.display());
    }
    let configured = ["VISUAL", "EDITOR"].iter().find_map(|v| std::env::var(v).ok().filter(|s| !s.trim().is_empty()));
    let (program, mut args): (String, Vec<String>) = match &configured {
        Some(cmd) => {
            let mut parts = cmd.split_whitespace().map(str::to_string);
            (parts.next().unwrap_or_default(), parts.collect())
        }
        None if cfg!(windows) => ("notepad".into(), Vec::new()),
        None if cfg!(target_os = "macos") => ("open".into(), vec!["-t".into()]),
        None => ("xdg-open".into(), Vec::new()),
    };
    args.push(paths.config.display().to_string());
    let status = std::process::Command::new(&program)
        .args(&args)
        .status()
        .with_context(|| format!("cannot start `{program}`; set $EDITOR, or open {} yourself", paths.config.display()))?;
    if !status.success() {
        bail!("`{program}` exited with {status}");
    }
    println!("Check your changes with `owo check`; restart `owo start` if it is running.");
    Ok(())
}

pub fn path(global: &GlobalArgs) -> Result<()> {
    let p = context::paths(global)?;
    println!("mode:    {:?}", p.mode);
    println!("config:  {}", p.config.display());
    println!("state:   {}", p.state.display());
    println!("backups: {}", p.backups.display());
    println!("logs:    {}", p.logs.display());
    Ok(())
}

/// Edits config.toml in place (comments and layout kept) and saves it only if the result
/// still loads and builds a valid registry; otherwise the file is left as it was.
pub fn edit_config(paths: &owo_config::OwoPaths, edit: impl FnOnce(&mut toml_edit::DocumentMut) -> Result<()>) -> Result<()> {
    let original = match std::fs::read_to_string(&paths.config) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e).with_context(|| format!("cannot read {}", paths.config.display())),
    };
    let mut doc: toml_edit::DocumentMut =
        original.as_deref().unwrap_or("").parse().with_context(|| format!("{} is not valid TOML", paths.config.display()))?;
    edit(&mut doc)?;
    let text = match &original {
        Some(_) => doc.to_string(),
        None => format!("# OwO AI Gateway configuration. `owo add <provider>` edits this file; so can you.\n\n{doc}"),
    };
    let (config, _) = owo_config::Config::from_toml_str(&text, &paths.config.display().to_string())
        .map_err(|e| anyhow::anyhow!("the change would make the config invalid, so nothing was written:\n{e}"))?;
    context::build_router(&config).context("the change would make the config invalid, so nothing was written")?;
    if let Some(dir) = paths.config.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    write_atomic(&paths.config, &text)
}

/// Writes via a sibling temp file and rename so a crash never leaves a half-written file.
pub fn write_atomic(path: &std::path::Path, contents: &str) -> Result<()> {
    let tmp = path.with_extension(format!("tmp-{}", std::process::id()));
    std::fs::write(&tmp, contents).with_context(|| format!("cannot write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("cannot replace {}", path.display()))?;
    Ok(())
}
