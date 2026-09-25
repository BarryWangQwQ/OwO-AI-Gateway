//! Skills written in OwO AI Gateway. Every Markdown file of a skill can be written here
//! (`SKILL.md` is the entry point); folders can be made, renamed, and removed. Other files
//! belong to the file manager: they are listed, never written, and only go away with the
//! folder holding them. Every path stays inside the skill folder and no link is followed.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use owo_client_apps::managed::{now_unix, write_atomic};
use serde::{Deserialize, Serialize};

use crate::frontmatter::{self, SkillMeta};
use crate::fsops::{self, TempDir};
use crate::layout::AppId;
use crate::state::{Managed, Source};
use crate::tree::{self, Tree, SKILL_FILE, SKILL_LIMITS, SOURCE_LIMITS};
use crate::{Report, Skills};

/// Largest Markdown file read into or written from the editor.
pub const MAX_MD: usize = 1 << 20;
/// Entries a tree listing shows before it only counts the rest.
pub const TREE_MAX: usize = 500;
/// Deepest folder a tree listing opens.
const TREE_DEPTH: usize = 8;
/// Entries a tree listing counts at most (a huge folder is not walked to the end).
const TREE_COUNT: usize = 10_000;

/// One change to a skill's files; paths are relative to the skill folder, `/`-separated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Change {
    /// Create or replace a Markdown file; missing parent folders are made.
    Write { path: String, content: String },
    Mkdir { path: String },
    /// A Markdown file, or a folder with everything in it (links inside are removed as links).
    Delete { path: String },
    /// A Markdown file or an empty folder.
    Rename { from: String, to: String },
}

/// [`Change`] as the CLI reads it: a written file's content comes from `file`, never argv.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ManifestChange {
    Write { path: String, file: PathBuf },
    Mkdir { path: String },
    Delete { path: String },
    Rename { from: String, to: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub changes: Vec<ManifestChange>,
}

/// The changes of a manifest file, with each written file's content read in.
pub fn read_manifest(path: &Path) -> Result<Vec<Change>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    let manifest: Manifest = serde_json::from_str(&text).with_context(|| format!("{} is not a valid change list", path.display()))?;
    manifest
        .changes
        .into_iter()
        .map(|c| {
            Ok(match c {
                ManifestChange::Write { path, file } => {
                    let size = std::fs::metadata(&file).with_context(|| format!("cannot read {}", file.display()))?.len();
                    if size as usize > MAX_MD {
                        bail!("{path} is larger than {} KB", MAX_MD >> 10);
                    }
                    Change::Write { content: std::fs::read_to_string(&file).with_context(|| format!("{} is not UTF-8 text", file.display()))?, path }
                }
                ManifestChange::Mkdir { path } => Change::Mkdir { path },
                ManifestChange::Delete { path } => Change::Delete { path },
                ManifestChange::Rename { from, to } => Change::Rename { from, to },
            })
        })
        .collect()
}

/// A managed skill's Markdown file, for the editor.
#[derive(Debug, Serialize)]
pub struct SkillFile {
    pub name: String,
    pub path: PathBuf,
    pub content: String,
    /// The source kind, as in the listing (`github`, `authored`, ...).
    pub source: &'static str,
    /// `false` when the skill's folder is a link to somewhere else.
    pub editable: bool,
}

/// One file or folder of a skill, for the tree.
#[derive(Debug, Serialize)]
pub struct TreeEntry {
    /// Relative to the skill folder, `/`-separated.
    pub path: String,
    pub depth: usize,
    pub dir: bool,
    pub size: u64,
    /// A symbolic link or junction: listed, never followed.
    pub link: bool,
}

#[derive(Debug, Serialize)]
pub struct SkillTree {
    pub path: PathBuf,
    /// Folders first, then files, by name; at most `TREE_MAX`.
    pub entries: Vec<TreeEntry>,
    /// Entries not listed.
    pub more: usize,
}

/// A starting point for a new skill.
pub fn template(name: &str) -> String {
    format!(
        "---\nname: {name}\ndescription: What this skill does and when to use it.\n---\n\n# {name}\n\n## When to use\n\n- \n\n## Steps\n\n1. \n"
    )
}

/// Checks a `SKILL.md` written for `name`: the frontmatter must carry that name and a description.
pub fn check(name: &str, content: &str) -> Result<SkillMeta> {
    if content.len() > MAX_MD {
        bail!("SKILL.md is larger than {} KB", MAX_MD >> 10);
    }
    if !frontmatter::valid_name(name) {
        bail!("`{name}` is not a valid skill name: use 1–{} lowercase letters, digits, and single hyphens", frontmatter::MAX_NAME);
    }
    match frontmatter::parse(content)?.name.as_deref() {
        Some(n) if n == name => {}
        Some(n) => bail!("the frontmatter says `name: {n}`; it must be `name: {name}`"),
        None => bail!("the frontmatter has no `name`; add `name: {name}`"),
    }
    frontmatter::inspect(name, content)
}

/// A relative path inside a skill, normalised to `/`: no absolute paths, drive letters,
/// `..`, or names some platform cannot hold.
pub fn rel_path(raw: &str) -> Result<String> {
    let outside = || anyhow::anyhow!("`{raw}` is not a path inside the skill (no absolute paths, drive letters, or `..`)");
    let rel = tree::safe_relative(raw.trim()).map_err(|_| outside())?.ok_or_else(outside)?;
    if !tree::portable(&rel) {
        bail!("`{rel}` uses a name that cannot be written on every platform");
    }
    Ok(rel)
}

fn is_md(rel: &str) -> bool {
    rel.to_ascii_lowercase().ends_with(".md")
}

/// The entry point (case-insensitively, as on Windows).
fn is_entry(rel: &str) -> bool {
    rel.eq_ignore_ascii_case(SKILL_FILE)
}

fn md_path(raw: &str) -> Result<String> {
    let rel = rel_path(raw)?;
    if !is_md(&rel) {
        bail!("`{rel}` is not a Markdown file; other files are edited in the file manager");
    }
    Ok(rel)
}

fn full(root: &Path, rel: &str) -> PathBuf {
    root.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
}

/// Refuses a path that passes through a link, so nothing is read or written outside `root`.
/// The last component may be a link only with `allow_last` (a link is deleted as itself).
fn no_links(root: &Path, rel: &str, allow_last: bool) -> Result<()> {
    let parts: Vec<&str> = rel.split('/').collect();
    let mut p = root.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        p.push(part);
        if fsops::is_link(&p) && !(allow_last && i + 1 == parts.len()) {
            bail!("`{rel}` goes through a link; OwO AI Gateway does not follow links out of the skill folder");
        }
    }
    Ok(())
}

/// Removes a folder and what is in it; links are removed as links, never followed.
fn remove_tree(dir: &Path) -> Result<()> {
    for e in std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))? {
        let e = e?;
        let path = e.path();
        let ty = e.file_type()?;
        if ty.is_symlink() {
            fsops::unlink_dir(&path).or_else(|_| std::fs::remove_file(&path).map_err(anyhow::Error::from)).with_context(|| format!("cannot remove the link {}", path.display()))?;
        } else if ty.is_dir() {
            remove_tree(&path)?;
        } else {
            std::fs::remove_file(&path).with_context(|| format!("cannot remove {}", path.display()))?;
        }
    }
    std::fs::remove_dir(dir).with_context(|| format!("cannot remove {}", dir.display()))
}

fn walk_tree(dir: &Path, prefix: &str, depth: usize, entries: &mut Vec<TreeEntry>, more: &mut usize) {
    let Ok(read) = std::fs::read_dir(dir) else { return };
    // A link counts as a folder when its target is one; it is never descended into.
    let is_dir = |e: &std::fs::DirEntry| match e.file_type() {
        Ok(t) if t.is_symlink() => std::fs::metadata(e.path()).is_ok_and(|m| m.is_dir()),
        Ok(t) => t.is_dir(),
        Err(_) => false,
    };
    let mut items: Vec<_> = read.filter_map(|e| e.ok()).map(|e| (is_dir(&e), e)).collect();
    items.sort_by_key(|(d, e)| (!*d, e.file_name()));
    for (is_dir, e) in items {
        if entries.len() + *more >= TREE_COUNT {
            return;
        }
        let Ok(ty) = e.file_type() else { continue };
        let name = e.file_name().to_string_lossy().to_string();
        let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let link = ty.is_symlink();
        if entries.len() < TREE_MAX {
            let size = if is_dir || link { 0 } else { e.metadata().map(|m| m.len()).unwrap_or(0) };
            entries.push(TreeEntry { path: path.clone(), depth, dir: is_dir, size, link });
        } else {
            *more += 1;
        }
        if is_dir && !link && depth + 1 < TREE_DEPTH {
            walk_tree(&e.path(), &path, depth + 1, entries, more);
        }
    }
}

/// Checks `changes` for skill `name` before anything is written: paths, file kinds, the
/// protected entry point, and the entry point's frontmatter.
fn validate(name: &str, changes: &[Change]) -> Result<Vec<Change>> {
    changes
        .iter()
        .map(|c| {
            Ok(match c {
                Change::Write { path, content } => {
                    let rel = md_path(path)?;
                    if is_entry(&rel) {
                        check(name, content)?;
                    } else if content.len() > MAX_MD {
                        bail!("{rel} is larger than {} KB", MAX_MD >> 10);
                    }
                    Change::Write { path: rel, content: content.clone() }
                }
                Change::Mkdir { path } => Change::Mkdir { path: rel_path(path)? },
                Change::Delete { path } => {
                    let rel = rel_path(path)?;
                    if is_entry(&rel) {
                        bail!("SKILL.md is the skill's entry point; it cannot be deleted");
                    }
                    Change::Delete { path: rel }
                }
                Change::Rename { from, to } => {
                    let (from, to) = (rel_path(from)?, rel_path(to)?);
                    if is_entry(&from) || is_entry(&to) {
                        bail!("SKILL.md is the skill's entry point; it cannot be renamed or replaced");
                    }
                    Change::Rename { from, to }
                }
            })
        })
        .collect()
}

/// Applies validated `changes` under `root`; returns what changed. A write whose content is
/// already on disk changes nothing.
fn apply_in(root: &Path, changes: &[Change], report: &mut Report) -> Result<usize> {
    let mut done = 0;
    for c in changes {
        match c {
            Change::Write { path, content } => {
                no_links(root, path, false)?;
                let target = full(root, path);
                if target.is_dir() {
                    bail!("`{path}` is a folder");
                }
                if std::fs::read_to_string(&target).is_ok_and(|t| &t == content) {
                    continue;
                }
                write_atomic(&target, content.as_bytes())?;
                report.lines.push(format!("wrote:    {path}"));
            }
            Change::Mkdir { path } => {
                no_links(root, path, false)?;
                let target = full(root, path);
                if target.is_dir() {
                    continue;
                }
                if fsops::exists(&target) {
                    bail!("`{path}` already exists and is not a folder");
                }
                std::fs::create_dir_all(&target).with_context(|| format!("cannot create {}", target.display()))?;
                report.lines.push(format!("created:  {path}/"));
            }
            Change::Delete { path } => {
                no_links(root, path, true)?;
                let target = full(root, path);
                if fsops::is_link(&target) {
                    fsops::unlink_dir(&target).or_else(|_| std::fs::remove_file(&target).map_err(anyhow::Error::from))?;
                } else if target.is_dir() {
                    remove_tree(&target)?;
                } else if target.is_file() {
                    if !is_md(path) {
                        bail!("`{path}` is not a Markdown file; remove it in the file manager (or delete its folder)");
                    }
                    std::fs::remove_file(&target).with_context(|| format!("cannot remove {}", target.display()))?;
                } else {
                    bail!("`{path}` does not exist");
                }
                report.lines.push(format!("deleted:  {path}"));
            }
            Change::Rename { from, to } => {
                no_links(root, from, false)?;
                no_links(root, to, false)?;
                let (src, dest) = (full(root, from), full(root, to));
                if fsops::exists(&dest) {
                    bail!("`{to}` already exists");
                }
                if src.is_dir() {
                    if std::fs::read_dir(&src)?.next().is_some() {
                        bail!("`{from}` is not empty; rename it in the file manager");
                    }
                } else if src.is_file() {
                    if !is_md(from) || !is_md(to) {
                        bail!("only Markdown files are renamed here");
                    }
                } else {
                    bail!("`{from}` does not exist");
                }
                if let Some(parent) = dest.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::rename(&src, &dest).with_context(|| format!("cannot rename `{from}` to `{to}`"))?;
                report.lines.push(format!("renamed:  {from} -> {to}"));
            }
        }
        done += 1;
    }
    Ok(done)
}

fn kind(s: &Source) -> &'static str {
    match s {
        Source::Local { .. } => "local",
        Source::Zip { .. } => "zip",
        Source::Github { .. } => "github",
        Source::Adopted => "adopted",
        Source::Authored { .. } => "authored",
    }
}

impl Skills {
    fn managed_or_bail(&self, name: &str) -> Result<Managed> {
        match self.load()?.skills.get(name) {
            Some(m) => Ok(m.clone()),
            None if fsops::exists(&self.store_path(name)) => bail!("OwO AI Gateway does not manage `{name}`; `owo skills adopt {name}` takes it over first"),
            None => bail!("no skill `{name}` is installed"),
        }
    }

    /// A Markdown file of a managed skill (`SKILL.md` when `rel` is `None`). Reads only.
    pub fn read_skill(&self, name: &str, rel: Option<&str>) -> Result<SkillFile> {
        let m = self.managed_or_bail(name)?;
        let root = self.store_path(name);
        let rel = md_path(rel.unwrap_or(SKILL_FILE))?;
        no_links(&root, &rel, false)?;
        let file = full(&root, &rel);
        let size = std::fs::metadata(&file).with_context(|| format!("cannot read {}", file.display()))?.len();
        if size as usize > MAX_MD {
            bail!("{rel} is larger than {} KB; edit it in the file manager", MAX_MD >> 10);
        }
        let content = std::fs::read_to_string(&file).with_context(|| format!("{} is not UTF-8 text", file.display()))?;
        Ok(SkillFile { name: name.to_string(), editable: !fsops::is_link(&root), path: root, content, source: kind(&m.source) })
    }

    /// The files and folders of a managed skill. Reads only; links are listed, not followed.
    pub fn tree(&self, name: &str) -> Result<SkillTree> {
        self.managed_or_bail(name)?;
        let path = self.store_path(name);
        if !path.is_dir() {
            bail!("{} is missing", path.display());
        }
        let (mut entries, mut more) = (Vec::new(), 0);
        walk_tree(&path, "", 0, &mut entries, &mut more);
        Ok(SkillTree { path, entries, more })
    }

    /// Records the current files of hand-written skills (`names` empty: all of them), which
    /// the user may change in the file manager, and refreshes the copies apps got.
    pub fn sync(&self, names: &[String]) -> Result<Report> {
        let mut report = Report::default();
        let mut st = self.load()?;
        if let Some(unknown) = names.iter().find(|n| !st.skills.contains_key(*n)) {
            bail!("OwO AI Gateway does not manage a skill `{unknown}`");
        }
        let mut changed = false;
        for (name, m) in st.skills.iter_mut() {
            if !names.is_empty() && !names.contains(name) {
                continue;
            }
            if !matches!(m.source, Source::Authored { .. }) {
                if !names.is_empty() {
                    report.lines.push(format!("{name}: not hand-written; nothing to sync"));
                }
                continue;
            }
            let Ok(hash) = Tree::from_dir(&self.store_path(name), SOURCE_LIMITS).and_then(|t| t.hash("")) else {
                report.warnings.push(format!("{name}: cannot read its files"));
                continue;
            };
            if hash == m.hash {
                continue;
            }
            m.hash = hash;
            m.updated_at = now_unix();
            self.refresh_copies(name, m, &mut report)?;
            report.lines.push(format!("{name}: synced"));
            changed = true;
        }
        if changed {
            self.save(&st)?;
        }
        Ok(report)
    }

    /// Creates the skill `name` from `content` (its `SKILL.md`) plus `changes` (more Markdown
    /// files and folders), and turns it on like an install.
    pub fn create(&self, name: &str, content: &str, apps: Option<&[AppId]>, changes: &[Change]) -> Result<Report> {
        let meta = check(name, content)?;
        let changes = validate(name, changes)?;
        let mut report = Report { lines: Vec::new(), warnings: meta.warnings };
        let dest = self.store_path(name);
        let mut st = self.load()?;
        if st.skills.contains_key(name) || fsops::exists(&dest) {
            bail!("a skill named `{name}` already exists");
        }
        let staging = TempDir(fsops::fresh_path(&self.state_dir.join("skills-tmp"), name));
        std::fs::create_dir_all(&staging.0).with_context(|| format!("cannot create {}", staging.0.display()))?;
        std::fs::write(staging.0.join(SKILL_FILE), content)?;
        let mut staged = Report::default();
        apply_in(&staging.0, &changes, &mut staged)?;
        // A move across volumes copies files only; empty folders are made again in place.
        let dirs: Vec<String> = changes.iter().filter_map(|c| if let Change::Mkdir { path } = c { Some(path.clone()) } else { None }).collect();
        fsops::move_path(&staging.0, &dest).with_context(|| format!("cannot put `{name}` into {}", self.layout.agents.display()))?;
        for d in &dirs {
            std::fs::create_dir_all(full(&dest, d)).with_context(|| format!("cannot create {d}"))?;
        }
        let hash = Tree::from_dir(&dest, SKILL_LIMITS)?.hash("")?;
        let now = now_unix();
        let m = Managed { source: Source::Authored { from: None }, installed_at: now, updated_at: now, hash, links: Vec::new(), disabled: Default::default() };
        st.skills.insert(name.to_string(), m);
        self.save(&st)?;
        report.lines.push(format!("created:  {}", dest.display()));
        report.lines.extend(staged.lines);
        let apps = self.apply_apps(name, apps, true);
        report.lines.extend(apps.lines);
        report.warnings.extend(apps.warnings);
        Ok(report)
    }

    /// Replaces the `SKILL.md` of a managed skill (see [`Skills::apply`]).
    pub fn edit(&self, name: &str, content: &str) -> Result<Report> {
        self.apply(name, &[Change::Write { path: SKILL_FILE.into(), content: content.into() }])
    }

    /// Applies `changes` to a managed skill. The folder is backed up first; a skill from
    /// GitHub, a folder, or a zip becomes hand-written, so no update overwrites the edit.
    pub fn apply(&self, name: &str, changes: &[Change]) -> Result<Report> {
        let changes = validate(name, changes)?;
        let mut report = Report::default();
        self.managed_or_bail(name)?;
        let dir = self.store_path(name);
        if fsops::is_link(&dir) {
            bail!("{} is a link to another folder; edit the skill there", dir.display());
        }
        for c in &changes {
            if let Change::Write { path, content } = c {
                if is_entry(path) {
                    report.warnings.extend(check(name, content)?.warnings);
                }
            }
        }
        // Only what would change counts: an unchanged save makes no backup.
        let effective: Vec<Change> = changes
            .into_iter()
            .filter(|c| match c {
                Change::Write { path, content } => std::fs::read_to_string(full(&dir, path)).map_or(true, |t| &t != content),
                Change::Mkdir { path } => !full(&dir, path).is_dir(),
                _ => true,
            })
            .collect();
        if effective.is_empty() {
            report.lines.push(format!("{name}: unchanged"));
            return Ok(report);
        }
        let backup = fsops::fresh_path(&self.backups, name);
        Tree::from_dir(&dir, SOURCE_LIMITS)?.write("", &backup).with_context(|| format!("cannot back up {}", dir.display()))?;
        report.lines.push(format!("backup:   {}", backup.display()));
        let applied = apply_in(&dir, &effective, &mut report);
        let mut st = self.load()?;
        let m = st.skills.get_mut(name).context("the skill disappeared from OwO AI Gateway's state")?;
        m.hash = Tree::from_dir(&dir, SOURCE_LIMITS)?.hash("")?;
        m.updated_at = now_unix();
        if matches!(m.source, Source::Github { .. } | Source::Local { .. } | Source::Zip { .. }) {
            let from = m.source.label();
            report.warnings.push(format!("{name} is hand-written now; updates from {from} stop (reinstall it to follow the source again)"));
            m.source = Source::Authored { from: Some(from) };
        }
        self.refresh_copies(name, m, &mut report)?;
        self.save(&st)?;
        if let Err(e) = applied {
            return Err(e.context(format!("some changes to `{name}` were not made; the files before the save are in {}", backup.display())));
        }
        Ok(report)
    }
}
