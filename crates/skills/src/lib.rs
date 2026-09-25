//! Agent Skills for every app from one place.
//!
//! A skill is a folder with a `SKILL.md` (YAML frontmatter `name`, `description`). OwO AI
//! Gateway keeps each one it manages in `~/.agents/skills/<name>/`, which Codex, Cursor,
//! OpenCode, Grok Build, ZCode, and GitHub Copilot read natively, and links it into the
//! folders of apps that read only their own (Claude Code). What it installed, from where,
//! and what it created or switched off for which app is recorded in `<state>/skills.json`,
//! so everything it did can be undone. Skills OwO AI Gateway did not install are listed but
//! never changed until the user adopts them.

pub mod author;
pub mod frontmatter;
pub mod fsops;
pub mod github;
pub mod layout;
mod ops;
pub mod state;
pub mod switches;
pub mod tree;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

pub use layout::{AppId, Layout, Location, Support};
pub use ops::{InstallOptions, InstallSource, RepoView, DiscoveredView};
pub use owo_client_apps::managed::Report;
pub use state::{RepoSpec, Source};

use crate::fsops::LinkKind;
use crate::state::{Managed, State};
use crate::switches::{Setting, Switches};
use crate::tree::{Tree, SKILL_FILE, SOURCE_LIMITS};

/// The skills store plus OwO AI Gateway's own state and backups.
pub struct Skills {
    pub layout: Layout,
    pub state_dir: PathBuf,
    /// `<backups>/skills`.
    pub backups: PathBuf,
}

impl Skills {
    pub fn new(layout: Layout, state_dir: &Path, backups_root: &Path) -> Self {
        Self { layout, state_dir: state_dir.to_path_buf(), backups: backups_root.join("skills") }
    }

    /// The real user directories (or `OWO_TEST_USER_HOME`).
    pub fn from_env(state_dir: &Path, backups_root: &Path) -> Result<Self> {
        let layout = Layout::from_env().context("cannot determine the home directory")?;
        Ok(Self::new(layout, state_dir, backups_root))
    }

    fn load(&self) -> Result<State> {
        state::load(&self.state_dir)
    }

    fn save(&self, s: &State) -> Result<()> {
        state::save(&self.state_dir, s)
    }

    pub fn store_path(&self, name: &str) -> PathBuf {
        self.layout.agents.join(name)
    }
}

// ---------------------------------------------------------------------------
// Listing

#[derive(Debug, Serialize)]
pub struct Listing {
    /// `~/.agents/skills`.
    pub store: PathBuf,
    pub apps: Vec<AppView>,
    /// Skills OwO AI Gateway manages.
    pub skills: Vec<SkillView>,
    /// Everything else found in the store and in the apps' own folders; never changed.
    pub unmanaged: Vec<SkillView>,
}

#[derive(Debug, Serialize)]
pub struct AppView {
    pub id: AppId,
    pub name: &'static str,
    pub support: Support,
    pub detected: bool,
    /// The app's own skills folder, when it has one.
    pub dir: Option<PathBuf>,
    pub switch_file: Option<PathBuf>,
    pub note: &'static str,
}

/// One skill's state in one app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppStateKind {
    /// Loaded from the store; nothing turns it off.
    On,
    /// Off through the app's setting, written by OwO AI Gateway.
    OffByOwo,
    /// Off through the app's setting, written by someone else.
    OffByUser,
    /// The app's settings turn it on explicitly (not OwO AI Gateway's entry).
    OnByUser,
    /// OwO AI Gateway linked (or copied) it into the app's folder.
    Linked,
    /// Not in the app's folder.
    NotLinked,
    /// The app's folder has a skill of this name OwO AI Gateway did not put there.
    Foreign,
    /// Loaded from the store; the app offers OwO AI Gateway no per-skill switch.
    AlwaysOn,
    /// The skill sits in a folder this app reads (unmanaged skills outside the store).
    InFolder,
    Unsupported,
    /// The app's settings file could not be read (`detail` says why).
    Error,
}

#[derive(Debug, Clone, Serialize)]
pub struct AppState {
    pub app: AppId,
    pub enabled: bool,
    /// Whether `owo skills enable|disable` can change it now.
    pub can_toggle: bool,
    pub state: AppStateKind,
    /// The app also loads this skill through Claude Code's folder (same files).
    pub duplicate: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SourceView {
    /// `local`, `zip`, `github`, `adopted`, or `authored`.
    pub kind: &'static str,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct UpdateView {
    pub latest_commit: String,
    pub available: bool,
    /// The skill's folder is gone from the latest commit.
    pub missing: bool,
    pub checked_at: u64,
}

#[derive(Debug, Serialize)]
pub struct SkillView {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub location: Location,
    pub managed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<SourceView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub installed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    /// The files differ from what OwO AI Gateway installed.
    pub modified: bool,
    /// From the last discovery of its repository (`owo skills update --check`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub update: Option<UpdateView>,
    /// The folder is itself a link to somewhere else.
    pub is_link: bool,
    /// `owo skills adopt` can take it over (a valid skill directly in the store).
    pub can_adopt: bool,
    pub apps: Vec<AppState>,
    pub warnings: Vec<String>,
}

fn read_meta(dir: &Path, leaf: &str) -> (String, String, Vec<String>) {
    match std::fs::read_to_string(dir.join(SKILL_FILE)).map_err(anyhow::Error::from).and_then(|t| frontmatter::inspect(leaf, &t)) {
        Ok(m) => (m.name, m.description, m.warnings),
        Err(e) => (leaf.to_string(), String::new(), vec![format!("{e:#}")]),
    }
}

impl Skills {
    pub fn apps(&self) -> Vec<AppView> {
        AppId::ALL
            .into_iter()
            .map(|id| AppView {
                id,
                name: id.title(),
                support: id.support(),
                detected: self.layout.detected(id),
                dir: id.own_location().map(|l| self.layout.dir(l)),
                switch_file: self.layout.switch_file(id),
                note: id.note(),
            })
            .collect()
    }

    /// Every skill OwO AI Gateway manages and every other one it can find. Reads only.
    pub fn list(&self) -> Result<Listing> {
        let st = self.load()?;
        let switches = Switches::load(&self.layout);
        let cache = github::load_cache(&self.state_dir);
        let mut skills = Vec::new();
        for (name, m) in &st.skills {
            let path = self.store_path(name);
            let (_, description, mut warnings) = read_meta(&path, name);
            let present = fsops::exists(&path);
            if !present {
                warnings.push(format!("{} is missing; reinstall or remove it", path.display()));
            }
            // A hand-written skill's other files are the user's to change (`owo skills sync`
            // records them); only installed copies can drift from their source.
            let authored = matches!(m.source, Source::Authored { .. });
            let modified = !authored && present && Tree::from_dir(&path, SOURCE_LIMITS).and_then(|t| t.hash("")).is_ok_and(|h| h != m.hash);
            let apps = self.app_states(name, Some(m), &switches);
            skills.push(SkillView {
                name: name.clone(),
                description,
                path: path.clone(),
                location: Location::Agents,
                managed: true,
                source: Some(source_view(&m.source)),
                installed_at: Some(m.installed_at),
                updated_at: Some(m.updated_at),
                modified,
                update: update_view(m, &cache),
                is_link: fsops::is_link(&path),
                can_adopt: false,
                apps,
                warnings,
            });
        }
        let ours: Vec<&Path> = st.skills.values().flat_map(|m| m.links.iter().map(|l| l.path.as_path())).collect();
        let mut unmanaged = Vec::new();
        for loc in Location::ALL {
            let root = self.layout.dir(loc);
            let mut found = Vec::new();
            find_skills(&root, 0, &mut found);
            for (dir, depth) in found {
                if ours.contains(&dir.as_path()) {
                    continue;
                }
                let leaf = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                if loc == Location::Agents && depth == 0 && st.skills.contains_key(&leaf) {
                    continue;
                }
                let (name, description, warnings) = read_meta(&dir, &leaf);
                let direct = loc == Location::Agents && depth == 0;
                let apps = if direct {
                    self.app_states(&leaf, None, &switches)
                } else {
                    loc.readers()
                        .iter()
                        .map(|&app| AppState { app, enabled: true, can_toggle: false, state: AppStateKind::InFolder, duplicate: false, detail: None })
                        .collect()
                };
                let can_adopt = direct && frontmatter::valid_name(&leaf) && !description.is_empty();
                unmanaged.push(SkillView {
                    name,
                    description,
                    is_link: fsops::is_link(&dir),
                    path: dir,
                    location: loc,
                    managed: false,
                    source: None,
                    installed_at: None,
                    updated_at: None,
                    modified: false,
                    update: None,
                    can_adopt,
                    apps,
                    warnings,
                });
            }
        }
        Ok(Listing { store: self.layout.agents.clone(), apps: self.apps(), skills, unmanaged })
    }

    /// Per-app state of the skill `name` in the store (`m` when OwO AI Gateway manages it).
    fn app_states(&self, name: &str, m: Option<&Managed>, switches: &Switches) -> Vec<AppState> {
        let claude_path = self.layout.dir(Location::Claude).join(name);
        let in_claude = fsops::exists(&claude_path);
        AppId::ALL
            .into_iter()
            .map(|app| {
                let detected = self.layout.detected(app);
                let managed = m.is_some();
                let duplicate = in_claude && app != AppId::Claude && Location::Claude.readers().contains(&app);
                let mk = |enabled, can_toggle, state, detail: Option<String>| AppState { app, enabled, can_toggle: managed && can_toggle, state, duplicate, detail };
                match app.support() {
                    Support::Unsupported => mk(false, false, AppStateKind::Unsupported, None),
                    Support::AlwaysOn => mk(true, false, AppStateKind::AlwaysOn, None),
                    Support::Switch => {
                        let ours = m.is_some_and(|m| m.disabled.contains(&app));
                        match switches.get(&self.layout, app, name) {
                            Err(e) => mk(true, false, AppStateKind::Error, Some(e)),
                            Ok(Setting::Off) if ours => mk(false, true, AppStateKind::OffByOwo, None),
                            Ok(Setting::Off) => mk(false, false, AppStateKind::OffByUser, None),
                            Ok(Setting::On) => mk(true, false, AppStateKind::OnByUser, None),
                            Ok(Setting::Unset) => mk(true, detected, AppStateKind::On, None),
                        }
                    }
                    Support::Link => match m.and_then(|m| m.link(app)) {
                        Some(l) if fsops::exists(&l.path) => {
                            let detail = (l.kind == LinkKind::Copy).then(|| "copy".to_string());
                            mk(true, true, AppStateKind::Linked, detail)
                        }
                        _ if in_claude => mk(true, false, AppStateKind::Foreign, None),
                        _ => mk(false, detected, AppStateKind::NotLinked, None),
                    },
                }
            })
            .collect()
    }
}

fn source_view(s: &Source) -> SourceView {
    let (kind, repo, commit) = match s {
        Source::Local { .. } => ("local", None, None),
        Source::Zip { .. } => ("zip", None, None),
        Source::Github { owner, repo, commit, .. } => ("github", Some(format!("{owner}/{repo}")), Some(commit.clone())),
        Source::Adopted => ("adopted", None, None),
        Source::Authored { .. } => ("authored", None, None),
    };
    SourceView { kind, label: s.label(), repo, commit }
}

fn update_view(m: &Managed, cache: &github::Cache) -> Option<UpdateView> {
    let Source::Github { owner, repo, reference, subdir, commit } = &m.source else { return None };
    let key = RepoSpec { owner: owner.clone(), repo: repo.clone(), subdir: None, reference: reference.clone() }.repo_key();
    let c = cache.repos.get(&key)?;
    let found = c.skills.iter().find(|f| &f.dir == subdir);
    Some(UpdateView {
        latest_commit: c.commit.clone(),
        available: &c.commit != commit && found.is_some_and(|f| f.hash != m.hash && f.problem.is_none()),
        missing: found.is_none(),
        checked_at: c.fetched_at,
    })
}

/// Folders holding a `SKILL.md` below `dir`, with their depth (0 = directly in `dir`).
/// Hidden folders (`.system`, `.trash`, ...) are skipped, as the apps skip them.
fn find_skills(dir: &Path, depth: usize, out: &mut Vec<(PathBuf, usize)>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let name = e.file_name().to_string_lossy().to_string();
        let path = e.path();
        if name.starts_with('.') || name == "node_modules" || !path.is_dir() {
            continue;
        }
        if path.join(SKILL_FILE).is_file() {
            out.push((path, depth));
        } else if depth < 2 {
            find_skills(&path, depth + 1, out);
        }
    }
}

#[cfg(test)]
mod tests;
