//! Everything that changes files: install, update, remove, adopt, per-app switches, and the
//! discovery repositories. Files OwO AI Gateway replaces or removes go to the backups first.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use owo_client_apps::managed::now_unix;
use serde::Serialize;

use crate::frontmatter;
use crate::fsops::{self, LinkKind, TempDir};
use crate::github::{self, CachedRepo, Found};
use crate::layout::{AppId, Location, Support};
use crate::state::{self, Link, Managed, RepoSpec, Source, State};
use crate::switches::{self, Setting, Switches};
use crate::tree::{Tree, SKILL_FILE, SKILL_LIMITS, SOURCE_LIMITS};
use crate::{Report, Skills};

/// Claude Code keeps the skills it syncs from claude.ai under this name.
const CLAUDE_RESERVED: &str = "synced";
/// How old a resolved commit may be when installing from GitHub.
const INSTALL_FRESH_SECS: u64 = 600;

pub enum InstallSource {
    /// A skill folder, a folder of skills, or a `.zip`.
    Path(PathBuf),
    Github(RepoSpec),
}

impl InstallSource {
    pub fn parse(s: &str) -> Result<Self> {
        let t = s.trim();
        if ["github:", "https://github.com/", "http://github.com/", "github.com/"].iter().any(|p| t.starts_with(p)) {
            return Ok(Self::Github(RepoSpec::parse(t)?));
        }
        let path = PathBuf::from(t);
        if !path.exists() {
            bail!("{t} does not exist (GitHub sources are written github:owner/repo[/subdir][@ref])");
        }
        Ok(Self::Path(std::path::absolute(&path)?))
    }
}

#[derive(Debug, Clone, Default)]
pub struct InstallOptions {
    /// Apps to turn the skills on for, off for the rest; `None` leaves the native apps on
    /// and links Claude Code when it is installed.
    pub apps: Option<Vec<AppId>>,
    /// Skills to take from a source that holds several (by name).
    pub only: Vec<String>,
    pub all: bool,
    /// Replace a skill OwO AI Gateway already manages (its files are backed up first).
    pub force: bool,
}

fn merge(into: &mut Report, from: Report) {
    into.lines.extend(from.lines);
    into.warnings.extend(from.warnings);
}

fn names(found: &[Found]) -> String {
    found.iter().map(|f| f.name.as_str()).collect::<Vec<_>>().join(", ")
}

fn short(commit: &str) -> &str {
    &commit[..commit.len().min(7)]
}

fn skill_file(dir: &str) -> String {
    if dir.is_empty() { SKILL_FILE.to_string() } else { format!("{dir}/{SKILL_FILE}") }
}

#[derive(Debug, Serialize)]
pub struct RepoView {
    /// `owner/repo[/subdir][@ref]`.
    pub repo: String,
    pub builtin: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fetched_at: Option<u64>,
    pub skills: Vec<DiscoveredView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DiscoveredView {
    pub name: String,
    pub description: String,
    /// Folder inside the repository.
    pub dir: String,
    /// The source to pass to `owo skills install`.
    pub install: String,
    /// Installed from this very folder.
    pub installed: bool,
    pub update_available: bool,
    /// A skill of this name is already in the store from somewhere else.
    pub conflict: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

impl Skills {
    // -----------------------------------------------------------------------
    // Install

    pub async fn install(&self, source: &InstallSource, opts: &InstallOptions) -> Result<Report> {
        match source {
            InstallSource::Path(p) => self.install_path(p, opts),
            InstallSource::Github(spec) => self.install_github(spec, opts).await,
        }
    }

    pub fn install_path(&self, path: &Path, opts: &InstallOptions) -> Result<Report> {
        if path.starts_with(&self.layout.agents) {
            bail!("{} is already in the skills folder; `owo skills adopt <name>` takes a skill there over", path.display());
        }
        let root = path.to_path_buf();
        if path.is_file() {
            let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
            let tree = Tree::from_zip(&bytes, false, SOURCE_LIMITS).with_context(|| format!("{} cannot be used", path.display()))?;
            self.install_tree(&tree, None, &|dir| Source::Zip { path: root.clone(), subdir: dir.to_string() }, opts)
        } else {
            let tree = Tree::from_dir(path, SOURCE_LIMITS)?;
            self.install_tree(&tree, None, &|dir| Source::Local { path: if dir.is_empty() { root.clone() } else { root.join(dir) } }, opts)
        }
    }

    pub async fn install_github(&self, spec: &RepoSpec, opts: &InstallOptions) -> Result<Report> {
        let cached = github::refresh(&self.state_dir, spec, INSTALL_FRESH_SECS).await?;
        let tree = github::open(&self.state_dir, &cached)?;
        let make = |dir: &str| Source::Github {
            owner: spec.owner.clone(),
            repo: spec.repo.clone(),
            reference: spec.reference.clone(),
            subdir: dir.to_string(),
            commit: cached.commit.clone(),
        };
        self.install_tree(&tree, spec.subdir.as_deref(), &make, opts)
    }

    fn install_tree(&self, tree: &Tree, subdir: Option<&str>, source: &dyn Fn(&str) -> Source, opts: &InstallOptions) -> Result<Report> {
        let mut report = Report::default();
        if tree.skipped_links > 0 {
            report.warnings.push(format!("{} symbolic link(s) in the source were left out", tree.skipped_links));
        }
        let found: Vec<Found> = github::scan(tree).into_iter().filter(|f| github::within(&f.dir, subdir)).collect();
        if found.is_empty() {
            bail!("no SKILL.md found{}", subdir.map(|s| format!(" under `{s}`")).unwrap_or_default());
        }
        let picked: Vec<&Found> = if !opts.only.is_empty() {
            let mut v = Vec::new();
            for want in &opts.only {
                match found.iter().find(|f| &f.name == want) {
                    Some(f) => v.push(f),
                    None => bail!("the source has no skill `{want}` (it has: {})", names(&found)),
                }
            }
            v
        } else if found.len() == 1 || opts.all {
            found.iter().collect()
        } else {
            bail!("the source holds {} skills: {}; pick with --skill NAME (repeatable) or take all with --all", found.len(), names(&found));
        };
        let many = picked.len() > 1;
        for f in picked {
            if let Some(p) = &f.problem {
                if many {
                    report.warnings.push(format!("skipped {}: {p}", f.dir));
                    continue;
                }
                bail!("cannot install `{}`: {p}", f.name);
            }
            match self.place(tree, &f.dir, &f.name, source(&f.dir), opts) {
                Ok(r) => merge(&mut report, r),
                Err(e) if many => report.warnings.push(format!("{}: {e:#}", f.name)),
                Err(e) => return Err(e),
            }
        }
        Ok(report)
    }

    fn place(&self, tree: &Tree, dir: &str, name: &str, source: Source, opts: &InstallOptions) -> Result<Report> {
        let mut report = Report::default();
        let leaf = dir.rsplit('/').next().unwrap_or("");
        let meta = frontmatter::inspect(leaf, &String::from_utf8_lossy(&tree.read(&skill_file(dir))?))?;
        report.warnings.extend(meta.warnings.iter().map(|w| format!("{name}: {w}")));
        tree.check(dir, SKILL_LIMITS).with_context(|| format!("cannot install `{name}`"))?;
        let hash = tree.hash(dir)?;
        let dest = self.store_path(name);
        let mut st = self.load()?;
        let prior = st.skills.get(name).cloned();
        if fsops::exists(&dest) {
            match &prior {
                None => bail!("{} already exists and OwO AI Gateway does not manage it; `owo skills adopt {name}` takes it over", dest.display()),
                Some(p) if p.hash == hash && p.source == source => {
                    report.lines.push(format!("{name}: already installed"));
                    return Ok(report);
                }
                Some(p) if !opts.force => {
                    bail!("`{name}` is already installed from {}; `owo skills update {name}` refreshes it, --force replaces it (the current files are backed up first)", p.source.label())
                }
                Some(_) => {}
            }
        }
        let fresh = prior.is_none();
        self.replace(&mut st, name, tree, dir, source, hash, prior, &mut report)?;
        self.save(&st)?;
        merge(&mut report, self.apply_apps(name, opts.apps.as_deref(), fresh));
        Ok(report)
    }

    /// Puts the files under `dir` into the store as `name`; what was there goes to the backups.
    #[allow(clippy::too_many_arguments)]
    fn replace(&self, st: &mut State, name: &str, tree: &Tree, dir: &str, source: Source, hash: String, prior: Option<Managed>, report: &mut Report) -> Result<()> {
        let dest = self.store_path(name);
        let staging = TempDir(fsops::fresh_path(&self.state_dir.join("skills-tmp"), name));
        tree.write(dir, &staging.0)?;
        let backup = if fsops::exists(&dest) {
            let b = fsops::backup_move(&dest, &self.backups, name)?;
            report.lines.push(format!("backup:   {}", b.display()));
            Some(b)
        } else {
            None
        };
        if let Err(e) = fsops::move_path(&staging.0, &dest) {
            if let Some(b) = &backup {
                let _ = fsops::move_path(b, &dest);
            }
            return Err(e.context(format!("cannot put `{name}` into {}", self.layout.agents.display())));
        }
        report.lines.push(format!("{}: {}", if prior.is_some() { "updated" } else { "installed" }, dest.display()));
        let now = now_unix();
        let mut m = prior.unwrap_or_else(|| Managed { source: source.clone(), installed_at: now, updated_at: now, hash: hash.clone(), links: Vec::new(), disabled: Default::default() });
        m.source = source;
        m.hash = hash;
        m.updated_at = now;
        self.refresh_copies(name, &mut m, report)?;
        st.skills.insert(name.to_string(), m);
        Ok(())
    }

    /// A copy in an app's folder does not follow the store: it is backed up and made again.
    pub(crate) fn refresh_copies(&self, name: &str, m: &mut Managed, report: &mut Report) -> Result<()> {
        let dest = self.store_path(name);
        for l in m.links.iter_mut().filter(|l| l.kind == LinkKind::Copy) {
            if fsops::exists(&l.path) {
                let b = fsops::backup_move(&l.path, &self.backups, &format!("{name}-{}", l.app))?;
                report.lines.push(format!("backup:   {}", b.display()));
            }
            l.kind = fsops::link_dir(&dest, &l.path)?;
        }
        Ok(())
    }

    pub(crate) fn apply_apps(&self, name: &str, apps: Option<&[AppId]>, fresh: bool) -> Report {
        let mut report = Report::default();
        let plan: Vec<(AppId, bool)> = match apps {
            None if fresh => AppId::ALL.into_iter().filter(|a| a.support() == Support::Link && self.layout.detected(*a)).map(|a| (a, true)).collect(),
            None => Vec::new(),
            Some(list) => {
                for a in AppId::ALL {
                    if !list.contains(&a) && a.support() == Support::AlwaysOn && self.layout.detected(a) {
                        report.warnings.push(format!("{} always loads ~/.agents/skills, so it sees `{name}` too", a.title()));
                    }
                    if list.contains(&a) && a.support() == Support::Unsupported {
                        report.warnings.push(format!("{}: {}", a.title(), a.note()));
                    }
                }
                AppId::ALL.into_iter().filter(|a| matches!(a.support(), Support::Switch | Support::Link)).map(|a| (a, list.contains(&a))).collect()
            }
        };
        for (app, on) in plan {
            if !self.layout.detected(app) {
                if on && apps.is_some() {
                    report.warnings.push(format!("{} is not installed here; skipped", app.title()));
                }
                continue;
            }
            match self.set_enabled(name, app, on) {
                Ok(r) => merge(&mut report, r),
                Err(e) => report.warnings.push(format!("{}: {e:#}", app.title())),
            }
        }
        report
    }

    // -----------------------------------------------------------------------
    // Per-app switches

    /// Turns a managed skill on or off for one app, through the app's own mechanism.
    pub fn set_enabled(&self, name: &str, app: AppId, on: bool) -> Result<Report> {
        let mut report = Report::default();
        let mut st = self.load()?;
        let Some(m) = st.skills.get_mut(name) else {
            if fsops::exists(&self.store_path(name)) {
                bail!("OwO AI Gateway does not manage `{name}`; `owo skills adopt {name}` takes it over first");
            }
            bail!("no skill `{name}` is installed");
        };
        match app.support() {
            Support::Unsupported => bail!("{} is not supported: it {}", app.title(), app.note()),
            Support::AlwaysOn if on => return Ok(report),
            Support::AlwaysOn => bail!("{} {}, so it cannot turn `{name}` off", app.title(), app.note()),
            Support::Switch => {
                let file = self.layout.switch_file(app).expect("switch apps have a settings file");
                let setting = Switches::load(&self.layout).get(&self.layout, app, name).map_err(anyhow::Error::msg)?;
                let ours = m.disabled.contains(&app);
                match (on, setting) {
                    (true, Setting::Off) if ours => {
                        switches::clear_off(&self.layout, &self.backups, app, name)?;
                        m.disabled.remove(&app);
                        report.lines.push(format!("{}: on (OwO AI Gateway's entry removed from {})", app.title(), file.display()));
                    }
                    (true, Setting::Off) => bail!("{} turns `{name}` off in {} with a setting OwO AI Gateway did not write; change it there", app.title(), file.display()),
                    (true, _) => {
                        m.disabled.remove(&app);
                    }
                    (false, Setting::Off) => {}
                    (false, Setting::On) => bail!("{} turns `{name}` on explicitly in {}; change it there", app.title(), file.display()),
                    (false, Setting::Unset) => {
                        if !self.layout.detected(app) {
                            bail!("{} is not installed here, so there is nothing to turn off", app.title());
                        }
                        switches::turn_off(&self.layout, &self.backups, app, name)?;
                        m.disabled.insert(app);
                        report.lines.push(format!("{}: off ({})", app.title(), file.display()));
                    }
                }
            }
            Support::Link => {
                let link = self.layout.dir(app.own_location().expect("link apps have a folder")).join(name);
                let recorded = m.link(app).cloned();
                if on {
                    if recorded.as_ref().is_some_and(|l| fsops::exists(&l.path)) {
                        return Ok(report);
                    }
                    m.links.retain(|l| l.app != app);
                    if fsops::exists(&link) {
                        bail!("{} already exists and OwO AI Gateway did not put it there; rename or remove it first", link.display());
                    }
                    if name == CLAUDE_RESERVED {
                        bail!("Claude Code reserves the name `{CLAUDE_RESERVED}` for skills synced from claude.ai");
                    }
                    if !self.layout.claude_home.is_dir() {
                        bail!("{} does not exist; is Claude Code installed?", self.layout.claude_home.display());
                    }
                    let kind = fsops::link_dir(&self.store_path(name), &link)?;
                    m.links.push(Link { app, path: link.clone(), kind });
                    let how = match kind {
                        LinkKind::Junction => "junction",
                        LinkKind::Symlink => "symlink",
                        LinkKind::Copy => "copy",
                    };
                    report.lines.push(format!("{}: on ({how} {})", app.title(), link.display()));
                    if kind == LinkKind::Copy {
                        report.warnings.push("no link could be made here, so this is a copy; `owo skills update` refreshes it".into());
                    }
                    let also: Vec<&str> = Location::Claude.readers().iter().filter(|a| **a != app).map(|a| a.title()).collect();
                    report.warnings.push(format!("{} also read {}, so they list `{name}` twice (the same files)", also.join(", "), link.parent().unwrap_or(&link).display()));
                } else {
                    match recorded {
                        Some(l) => {
                            self.remove_link(name, &l, &mut report)?;
                            m.links.retain(|x| x.app != app);
                            report.lines.push(format!("{}: off ({} removed)", app.title(), l.path.display()));
                        }
                        None if fsops::exists(&link) => bail!("{} was not put there by OwO AI Gateway; remove it yourself if you want it gone", link.display()),
                        None => {}
                    }
                }
            }
        }
        self.save(&st)?;
        Ok(report)
    }

    fn remove_link(&self, name: &str, l: &Link, report: &mut Report) -> Result<()> {
        if !fsops::exists(&l.path) {
            return Ok(());
        }
        match l.kind {
            LinkKind::Copy => {
                let b = fsops::backup_move(&l.path, &self.backups, &format!("{name}-{}", l.app))?;
                report.lines.push(format!("backup:   {}", b.display()));
                Ok(())
            }
            _ if !fsops::is_link(&l.path) => bail!("{} is no longer OwO AI Gateway's link (something replaced it); left alone", l.path.display()),
            _ => fsops::unlink_dir(&l.path),
        }
    }

    // -----------------------------------------------------------------------
    // Remove, adopt

    /// Takes a managed skill away: its links, OwO AI Gateway's "off" entries, and its folder,
    /// which is moved to the backups.
    pub fn remove(&self, name: &str) -> Result<Report> {
        let mut report = Report::default();
        let mut st = self.load()?;
        let Some(m) = st.skills.get(name).cloned() else {
            if fsops::exists(&self.store_path(name)) {
                bail!("OwO AI Gateway does not manage `{name}` and leaves it alone (`owo skills adopt {name}` first to manage it here)");
            }
            bail!("no skill `{name}` is installed");
        };
        for l in &m.links {
            let existed = fsops::exists(&l.path);
            match self.remove_link(name, l, &mut report) {
                Ok(()) if existed => report.lines.push(format!("removed:  {}", l.path.display())),
                Ok(()) => {}
                Err(e) => report.warnings.push(format!("{e:#}")),
            }
        }
        for app in &m.disabled {
            match switches::clear_off(&self.layout, &self.backups, *app, name) {
                Ok(Some(f)) => report.lines.push(format!("cleaned:  {}", f.display())),
                Ok(None) => {}
                Err(e) => report.warnings.push(format!("{}: {e:#}", app.title())),
            }
        }
        let dest = self.store_path(name);
        if fsops::exists(&dest) {
            let b = fsops::backup_move(&dest, &self.backups, name)?;
            report.lines.push(format!("backup:   {} (move it back to {} to restore)", b.display(), dest.display()));
        }
        st.skills.remove(name);
        self.save(&st)?;
        report.lines.push(format!("{name} removed."));
        Ok(report)
    }

    /// Takes over a skill already in the store. Nothing on disk changes.
    pub fn adopt(&self, name: &str) -> Result<Report> {
        let mut report = Report::default();
        let mut st = self.load()?;
        if st.skills.contains_key(name) {
            bail!("OwO AI Gateway already manages `{name}`");
        }
        let dir = self.store_path(name);
        let text = std::fs::read_to_string(dir.join(SKILL_FILE)).with_context(|| format!("{} has no readable SKILL.md", dir.display()))?;
        if !frontmatter::valid_name(name) {
            bail!("`{name}` is not a valid skill name");
        }
        let meta = frontmatter::inspect(name, &text)?;
        report.warnings.extend(meta.warnings);
        let hash = Tree::from_dir(&dir, SOURCE_LIMITS)?.hash("")?;
        let now = now_unix();
        let mut m = Managed { source: Source::Adopted, installed_at: now, updated_at: now, hash, links: Vec::new(), disabled: Default::default() };
        // A link someone made from Claude Code's folder to this very skill counts as OwO AI Gateway's.
        let claude = self.layout.dir(Location::Claude).join(name);
        if fsops::is_link(&claude) && fsops::points_to(&claude, &dir) {
            m.links.push(Link { app: AppId::Claude, path: claude, kind: LinkKind::Symlink });
        }
        st.skills.insert(name.to_string(), m);
        self.save(&st)?;
        report.lines.push(format!("{name}: now managed by OwO AI Gateway (nothing on disk changed)"));
        Ok(report)
    }

    // -----------------------------------------------------------------------
    // Update

    /// Refreshes managed skills from their sources (`names` empty: all of them); with
    /// `check_only`, reports what would change.
    pub async fn update(&self, names: &[String], check_only: bool) -> Result<Report> {
        let mut report = Report::default();
        let st = self.load()?;
        let targets: Vec<(String, Managed)> = if names.is_empty() {
            st.skills.iter().map(|(n, m)| (n.clone(), m.clone())).collect()
        } else {
            names.iter().map(|n| st.skills.get(n).map(|m| (n.clone(), m.clone())).with_context(|| format!("OwO AI Gateway does not manage a skill `{n}`"))).collect::<Result<_>>()?
        };
        let mut repos: BTreeMap<String, std::result::Result<CachedRepo, String>> = BTreeMap::new();
        for (name, m) in targets {
            let modified = Tree::from_dir(&self.store_path(&name), SOURCE_LIMITS).and_then(|t| t.hash("")).is_ok_and(|h| h != m.hash);
            let (tree, dir, source) = match &m.source {
                Source::Adopted | Source::Authored { .. } => {
                    if !names.is_empty() {
                        report.lines.push(format!("{name}: {}, so there is no source to update from", m.source.label()));
                    }
                    continue;
                }
                Source::Github { owner, repo, reference, subdir, commit } => {
                    let spec = RepoSpec { owner: owner.clone(), repo: repo.clone(), subdir: None, reference: reference.clone() };
                    let key = spec.repo_key();
                    if !repos.contains_key(&key) {
                        let fetched = github::refresh(&self.state_dir, &spec, 0).await.map_err(|e| format!("{e:#}"));
                        repos.insert(key.clone(), fetched);
                    }
                    let cached = match &repos[&key] {
                        Ok(c) => c,
                        Err(e) => {
                            report.warnings.push(format!("{name}: {e}"));
                            continue;
                        }
                    };
                    let Some(found) = cached.skills.iter().find(|f| &f.dir == subdir) else {
                        report.warnings.push(format!("{name}: `{subdir}` is no longer in {owner}/{repo}"));
                        continue;
                    };
                    let source = Source::Github { owner: owner.clone(), repo: repo.clone(), reference: reference.clone(), subdir: subdir.clone(), commit: cached.commit.clone() };
                    if found.hash == m.hash {
                        if &cached.commit != commit {
                            self.record_source(&name, source)?;
                        }
                        report.lines.push(format!("{name}: up to date"));
                        continue;
                    }
                    if check_only {
                        report.lines.push(format!("{name}: update available ({} → {})", short(commit), short(&cached.commit)));
                        continue;
                    }
                    if let Some(p) = &found.problem {
                        report.warnings.push(format!("{name}: the new version cannot be installed: {p}"));
                        continue;
                    }
                    (github::open(&self.state_dir, cached)?, found.dir.clone(), source)
                }
                Source::Local { path } => {
                    if !path.join(SKILL_FILE).is_file() {
                        report.warnings.push(format!("{name}: {} is gone", path.display()));
                        continue;
                    }
                    (Tree::from_dir(path, SKILL_LIMITS)?, String::new(), m.source.clone())
                }
                Source::Zip { path, subdir } => {
                    let Ok(bytes) = std::fs::read(path) else {
                        report.warnings.push(format!("{name}: {} is gone", path.display()));
                        continue;
                    };
                    (Tree::from_zip(&bytes, false, SOURCE_LIMITS)?, subdir.clone(), m.source.clone())
                }
            };
            let hash = tree.hash(&dir)?;
            if hash == m.hash {
                report.lines.push(format!("{name}: up to date"));
                continue;
            }
            if check_only {
                report.lines.push(format!("{name}: update available"));
                continue;
            }
            let meta = frontmatter::inspect(dir.rsplit('/').next().unwrap_or(""), &String::from_utf8_lossy(&tree.read(&skill_file(&dir))?))?;
            if meta.name != name {
                report.warnings.push(format!("{name}: the source now calls the skill `{}`; not updated", meta.name));
                continue;
            }
            tree.check(&dir, SKILL_LIMITS)?;
            let mut st = self.load()?;
            let prior = st.skills.get(&name).cloned();
            self.replace(&mut st, &name, &tree, &dir, source, hash, prior, &mut report)?;
            self.save(&st)?;
            if modified {
                report.warnings.push(format!("{name}: it had local changes; they are in the backup above"));
            }
        }
        if report.lines.is_empty() && report.warnings.is_empty() {
            report.lines.push("nothing to update".into());
        }
        Ok(report)
    }

    fn record_source(&self, name: &str, source: Source) -> Result<()> {
        let mut st = self.load()?;
        if let Some(m) = st.skills.get_mut(name) {
            m.source = source;
            self.save(&st)?;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Discovery

    pub fn repos(&self) -> Result<Vec<RepoSpec>> {
        Ok(self.load()?.repos.unwrap_or_else(state::default_repos))
    }

    pub fn add_repo(&self, spec: RepoSpec) -> Result<Report> {
        let mut st = self.load()?;
        let mut repos = st.repos.take().unwrap_or_else(state::default_repos);
        if repos.iter().any(|r| r.to_string().eq_ignore_ascii_case(&spec.to_string())) {
            bail!("{spec} is already in the list");
        }
        let line = format!("added {spec}");
        repos.push(spec);
        st.repos = Some(repos);
        self.save(&st)?;
        Ok(Report { lines: vec![line], warnings: Vec::new() })
    }

    pub fn remove_repo(&self, spec: &RepoSpec) -> Result<Report> {
        let mut st = self.load()?;
        let mut repos = st.repos.take().unwrap_or_else(state::default_repos);
        let before = repos.len();
        repos.retain(|r| !r.to_string().eq_ignore_ascii_case(&spec.to_string()));
        if repos.len() == before {
            bail!("{spec} is not in the list");
        }
        st.repos = Some(repos);
        self.save(&st)?;
        Ok(Report { lines: vec![format!("removed {spec}")], warnings: Vec::new() })
    }

    pub fn reset_repos(&self) -> Result<Report> {
        let mut st = self.load()?;
        st.repos = None;
        self.save(&st)?;
        Ok(Report { lines: vec!["the repository list is back to the built-in one".into()], warnings: Vec::new() })
    }

    /// Lists the skills in `specs` (the configured repositories when `None`), asking GitHub
    /// again for listings older than `max_age` seconds.
    pub async fn discover(&self, specs: Option<Vec<RepoSpec>>, max_age: u64) -> Result<Vec<RepoView>> {
        let specs = match specs {
            Some(s) => s,
            None => self.repos()?,
        };
        let st = self.load()?;
        let mut out = Vec::new();
        for spec in specs {
            let fetched = github::refresh(&self.state_dir, &spec, max_age).await.map_err(|e| format!("{e:#}"));
            out.push(self.repo_view(&spec, fetched.as_ref().map(Some).map_err(Clone::clone), &st));
        }
        Ok(out)
    }

    /// The configured repositories as last discovered, without the network.
    pub fn discover_cached(&self) -> Result<Vec<RepoView>> {
        let st = self.load()?;
        let cache = github::load_cache(&self.state_dir);
        Ok(self.repos()?.iter().map(|spec| self.repo_view(spec, Ok(cache.repos.get(&spec.repo_key())), &st)).collect())
    }

    fn repo_view(&self, spec: &RepoSpec, cached: std::result::Result<Option<&CachedRepo>, String>, st: &State) -> RepoView {
        let builtin = state::default_repos().contains(spec);
        let (cached, error) = match cached {
            Ok(c) => (c, None),
            Err(e) => (None, Some(e)),
        };
        let skills = cached
            .map(|c| {
                c.skills
                    .iter()
                    .filter(|f| github::within(&f.dir, spec.subdir.as_deref()))
                    .map(|f| {
                        let managed = st.skills.get(&f.name);
                        let same = managed.and_then(|m| match &m.source {
                            Source::Github { owner, repo, subdir, commit, .. } if owner.eq_ignore_ascii_case(&spec.owner) && repo.eq_ignore_ascii_case(&spec.repo) && subdir == &f.dir => {
                                Some((m, commit))
                            }
                            _ => None,
                        });
                        let sub = if f.dir.is_empty() { String::new() } else { format!("/{}", f.dir) };
                        let at = spec.reference.as_ref().map(|r| format!("@{r}")).unwrap_or_default();
                        DiscoveredView {
                            name: f.name.clone(),
                            description: f.description.clone(),
                            dir: f.dir.clone(),
                            install: format!("github:{}/{}{sub}{at}", spec.owner, spec.repo),
                            installed: same.is_some(),
                            update_available: same.is_some_and(|(m, commit)| commit != &c.commit && m.hash != f.hash),
                            conflict: same.is_none() && (managed.is_some() || fsops::exists(&self.store_path(&f.name))),
                            problem: f.problem.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        RepoView { repo: spec.to_string(), builtin, commit: cached.map(|c| c.commit.clone()), fetched_at: cached.map(|c| c.fetched_at), skills, error }
    }
}
