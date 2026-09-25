//! Links, moves, and backups. Nothing here deletes data: a directory OwO AI Gateway takes away is
//! moved into the backups directory, and removing a link never touches what it points at.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::tree::{Tree, SKILL_LIMITS};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// NTFS directory junction (Windows; no administrator rights or developer mode needed).
    Junction,
    Symlink,
    /// A plain copy, when the filesystem allows no link.
    Copy,
}

/// A symbolic link or junction (not followed).
pub fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_symlink())
}

pub fn exists(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// Whether the link at `link` resolves to `target`.
pub fn points_to(link: &Path, target: &Path) -> bool {
    match (std::fs::canonicalize(link), std::fs::canonicalize(target)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Makes `link` show the directory `target`: a junction on Windows, a symlink elsewhere,
/// a copy when neither can be made.
pub fn link_dir(target: &Path, link: &Path) -> Result<LinkKind> {
    if exists(link) {
        bail!("{} already exists", link.display());
    }
    if let Some(parent) = link.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    }
    #[cfg(windows)]
    {
        if junction::create(target, link).is_ok() {
            return Ok(LinkKind::Junction);
        }
        if std::os::windows::fs::symlink_dir(target, link).is_ok() {
            return Ok(LinkKind::Symlink);
        }
    }
    #[cfg(unix)]
    {
        if std::os::unix::fs::symlink(target, link).is_ok() {
            return Ok(LinkKind::Symlink);
        }
    }
    Tree::from_dir(target, SKILL_LIMITS)?.write("", link)?;
    Ok(LinkKind::Copy)
}

/// Removes a link (only the link). Refuses anything that is not a link.
pub fn unlink_dir(link: &Path) -> Result<()> {
    if !is_link(link) {
        bail!("{} is not a link", link.display());
    }
    // A directory junction or directory symlink is removed like an empty directory on
    // Windows; on Unix a symlink is a file.
    #[cfg(windows)]
    let r = std::fs::remove_dir(link).or_else(|_| std::fs::remove_file(link));
    #[cfg(not(windows))]
    let r = std::fs::remove_file(link);
    r.with_context(|| format!("cannot remove the link {}", link.display()))
}

/// A path under `dir` named `<label>-<unix time>` that does not exist yet.
pub fn fresh_path(dir: &Path, label: &str) -> PathBuf {
    let base = format!("{label}-{}", owo_client_apps::managed::now_unix());
    let mut candidate = dir.join(&base);
    let mut n = 1;
    while exists(&candidate) {
        n += 1;
        candidate = dir.join(format!("{base}-{n}"));
    }
    candidate
}

/// Moves `src` (a directory or a link) to `dest`, copying when a rename is not possible
/// (another volume). A link moves as a link: its target is left alone.
pub fn move_path(src: &Path, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
    }
    if std::fs::rename(src, dest).is_ok() {
        return Ok(());
    }
    if is_link(src) {
        // Record where it pointed instead of copying what it points at.
        let target = std::fs::read_link(src).map(|t| t.display().to_string()).unwrap_or_default();
        std::fs::create_dir_all(dest)?;
        std::fs::write(dest.join("LINK_TARGET.txt"), format!("{target}\n"))?;
        return unlink_dir(src);
    }
    Tree::from_dir(src, crate::tree::SOURCE_LIMITS)?.write("", dest).with_context(|| format!("cannot copy {} to {}", src.display(), dest.display()))?;
    std::fs::remove_dir_all(src).with_context(|| format!("copied {} to {}, but cannot remove the original", src.display(), dest.display()))
}

/// Moves `path` into `backups` as `<label>-<time>`; returns where it went.
pub fn backup_move(path: &Path, backups: &Path, label: &str) -> Result<PathBuf> {
    let dest = fresh_path(backups, label);
    move_path(path, &dest)?;
    Ok(dest)
}

/// Removes a directory this process created (staging); errors are ignored.
pub struct TempDir(pub PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_and_unlink_leave_the_target_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("store/pdf");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "---\nname: pdf\n---\n").unwrap();
        let link = tmp.path().join("claude/skills/pdf");
        let kind = link_dir(&target, &link).unwrap();
        assert_ne!(kind, LinkKind::Copy, "junctions and symlinks need no special rights here");
        assert!(is_link(&link));
        assert!(points_to(&link, &target));
        assert_eq!(std::fs::read_to_string(link.join("SKILL.md")).unwrap(), "---\nname: pdf\n---\n");
        assert!(link_dir(&target, &link).is_err(), "never replaces what is there");
        unlink_dir(&link).unwrap();
        assert!(!exists(&link));
        assert!(target.join("SKILL.md").is_file(), "the store is untouched");
        assert!(unlink_dir(&target).is_err(), "a real directory is not a link");
    }

    #[test]
    fn moving_a_link_moves_only_the_link() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("elsewhere");
        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("SKILL.md"), "x").unwrap();
        let link = tmp.path().join("store/x");
        link_dir(&target, &link).unwrap();
        let dest = backup_move(&link, &tmp.path().join("backups"), "x").unwrap();
        assert!(!exists(&link));
        assert!(target.join("SKILL.md").is_file());
        assert!(exists(&dest));
    }
}
