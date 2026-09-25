//! The Skills page. Reads (the listing, the last discovery, the repository list) come
//! straight from `owo-skills`; installs, switches, removals, and network fetches run the
//! `owo skills` CLI, so the desktop app and the CLI share one implementation.

use anyhow::{anyhow, bail, Context, Result};
use owo_config::OwoPaths;
use owo_skills::author::{Change, Manifest, ManifestChange};
use owo_skills::{Listing, RepoView, Skills};
use serde::Serialize;
use tauri_plugin_dialog::DialogExt;

use crate::owo_cli;

type Reply<T> = std::result::Result<T, String>;

fn reply<T>(result: Result<T>) -> Reply<T> {
    result.map_err(|e| format!("{e:#}"))
}

fn skills() -> Result<Skills> {
    let paths = OwoPaths::home().context("cannot determine the home directory; set OWO_HOME")?;
    Skills::from_env(&paths.state, &paths.backups)
}

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Result<T> + Send + 'static) -> Reply<T> {
    reply(tokio::task::spawn_blocking(f).await.map_err(|e| anyhow!("{e}")).and_then(|r| r))
}

#[derive(Serialize)]
pub struct SkillsResult {
    ok: bool,
    output: String,
}

async fn owo(args: Vec<String>) -> Reply<SkillsResult> {
    blocking(move || {
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        owo_cli::run(&args).map(|o| SkillsResult { ok: o.ok, output: o.text })
    })
    .await
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

#[tauri::command]
pub async fn skills_list() -> Reply<Listing> {
    blocking(|| skills()?.list()).await
}

/// The configured repositories as last discovered (no network).
#[tauri::command]
pub async fn skills_discovered() -> Reply<Vec<RepoView>> {
    blocking(|| skills()?.discover_cached()).await
}

#[tauri::command]
pub async fn skills_repos() -> Reply<Vec<String>> {
    blocking(|| Ok(skills()?.repos()?.iter().map(ToString::to_string).collect())).await
}

/// `owo skills discover --json` (the same `RepoView` list as `skills_discovered`): asks
/// GitHub for listings older than a few hours, or for all of them with `refresh`.
/// Per-repository errors are part of the result.
#[tauri::command]
pub async fn skills_discover(refresh: bool) -> Reply<serde_json::Value> {
    let mut a = args(&["skills", "discover", "--json"]);
    if refresh {
        a.push("--refresh".into());
    }
    let result = owo(a).await?;
    if !result.ok {
        return Err(result.output);
    }
    serde_json::from_str(&result.output).map_err(|e| format!("unexpected output from `owo skills discover --json`: {e}"))
}

/// `apps`: `None` keeps the default (on everywhere); an empty list turns it off wherever
/// the apps allow.
#[tauri::command]
pub async fn skills_install(source: String, apps: Option<Vec<String>>, skills: Vec<String>, all: bool, force: bool) -> Reply<SkillsResult> {
    let mut a = args(&["skills", "install"]);
    if let Some(apps) = apps {
        a.push("--app".into());
        a.push(if apps.is_empty() { "none".into() } else { apps.join(",") });
    }
    for s in skills {
        a.push("--skill".into());
        a.push(s);
    }
    if all {
        a.push("--all".into());
    }
    if force {
        a.push("--force".into());
    }
    // After `--`, a source that starts with `-` is still a source.
    a.push("--".into());
    a.push(source);
    owo(a).await
}

/// A Markdown file of a managed skill (`SKILL.md` without `path`), for the editor. The path
/// is checked like every write: inside the skill folder, `.md`, no links, at most 1 MB.
#[tauri::command]
pub async fn skills_read(name: String, path: Option<String>) -> Reply<owo_skills::author::SkillFile> {
    blocking(move || skills()?.read_skill(&name, path.as_deref())).await
}

/// A managed skill's files and folders (read-only; links are listed, never followed).
#[tauri::command]
pub async fn skills_tree(name: String) -> Reply<owo_skills::author::SkillTree> {
    blocking(move || skills()?.tree(&name)).await
}

/// `owo skills sync`: records hand-written skills' files after changes in the file manager
/// and refreshes the copies apps got (`names` empty: all of them).
#[tauri::command]
pub async fn skills_sync(names: Vec<String>) -> Reply<SkillsResult> {
    let mut a = args(&["skills", "sync", "--"]);
    a.extend(names);
    owo(a).await
}

/// Writes the change list and every file's content into a private temporary folder, so
/// nothing but its path reaches the command line.
fn manifest(dir: &std::path::Path, changes: Vec<Change>) -> Result<std::path::PathBuf> {
    std::fs::create_dir_all(dir)?;
    let mut out = Manifest::default();
    for (i, c) in changes.into_iter().enumerate() {
        out.changes.push(match c {
            Change::Write { path, content } => {
                let file = dir.join(format!("{i}.md"));
                std::fs::write(&file, content)?;
                ManifestChange::Write { path, file }
            }
            Change::Mkdir { path } => ManifestChange::Mkdir { path },
            Change::Delete { path } => ManifestChange::Delete { path },
            Change::Rename { from, to } => ManifestChange::Rename { from, to },
        });
    }
    let file = dir.join("changes.json");
    std::fs::write(&file, serde_json::to_vec_pretty(&out)?)?;
    Ok(file)
}

/// `owo skills new|edit <name> --apply …`: one save of every changed Markdown file and
/// folder. `apps` applies to a new skill only (`None`: on everywhere).
#[tauri::command]
pub async fn skills_apply(name: String, create: bool, changes: Vec<Change>, apps: Option<Vec<String>>) -> Reply<SkillsResult> {
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let dir = std::env::temp_dir().join(format!("owo-skill-{}-{stamp}", std::process::id()));
    let file = reply(manifest(&dir, changes).with_context(|| format!("cannot write {}", dir.display())));
    let result = match file {
        Ok(file) => {
            let mut a = args(&["skills", if create { "new" } else { "edit" }, "--apply"]);
            a.push(file.display().to_string());
            if let (true, Some(apps)) = (create, apps) {
                a.push("--app".into());
                a.push(if apps.is_empty() { "none".into() } else { apps.join(",") });
            }
            a.push("--".into());
            a.push(name);
            owo(a).await
        }
        Err(e) => Err(e),
    };
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[tauri::command]
pub async fn skills_remove(name: String) -> Reply<SkillsResult> {
    owo(vec!["skills".into(), "remove".into(), name]).await
}

#[tauri::command]
pub async fn skills_toggle(name: String, app: String, enabled: bool) -> Reply<SkillsResult> {
    let verb = if enabled { "enable" } else { "disable" };
    owo(vec!["skills".into(), verb.into(), name, "--app".into(), app]).await
}

#[tauri::command]
pub async fn skills_adopt(name: String) -> Reply<SkillsResult> {
    owo(vec!["skills".into(), "adopt".into(), name]).await
}

/// `names` empty: every managed skill.
#[tauri::command]
pub async fn skills_update(names: Vec<String>, check: bool) -> Reply<SkillsResult> {
    let mut a = args(&["skills", "update"]);
    if names.is_empty() {
        a.push("--all".into());
    } else {
        a.extend(names);
    }
    if check {
        a.push("--check".into());
    }
    owo(a).await
}

#[tauri::command]
pub async fn skills_repo_add(repo: String) -> Reply<SkillsResult> {
    owo(vec!["skills".into(), "repos".into(), "add".into(), "--".into(), repo]).await
}

#[tauri::command]
pub async fn skills_repo_remove(repo: String) -> Reply<SkillsResult> {
    owo(vec!["skills".into(), "repos".into(), "remove".into(), "--".into(), repo]).await
}

#[tauri::command]
pub async fn skills_repo_reset() -> Reply<SkillsResult> {
    owo(args(&["skills", "repos", "reset"])).await
}

/// A native picker: `kind` is `folder` or `zip`. `None` when the user cancels.
#[tauri::command]
pub async fn skills_pick(app: tauri::AppHandle, kind: String) -> Reply<Option<String>> {
    blocking(move || {
        let dialog = app.dialog().file();
        let picked = match kind.as_str() {
            "folder" => dialog.blocking_pick_folder(),
            "zip" => dialog.add_filter("Zip", &["zip"]).blocking_pick_file(),
            other => bail!("unknown picker `{other}`"),
        };
        picked.map(|p| p.into_path().map(|p| p.display().to_string()).map_err(|e| anyhow!("{e}"))).transpose()
    })
    .await
}

/// Shows a skill's folder in the file manager. Only existing directories are opened, so
/// nothing can be launched through here.
#[tauri::command]
pub async fn skills_open(path: String) -> Reply<()> {
    blocking(move || {
        let dir = std::path::PathBuf::from(&path);
        if !dir.is_dir() {
            bail!("{path} is not a folder");
        }
        let program = if cfg!(windows) {
            "explorer"
        } else if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(program).arg(&dir).spawn().with_context(|| format!("cannot open {path}"))?;
        Ok(())
    })
    .await
}
