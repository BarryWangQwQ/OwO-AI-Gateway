//! A skill source as a flat list of files, from a directory or a zip archive. Discovery,
//! hashing, and installing work the same on both, so a skill hashes the same whether it
//! was read from GitHub's archive or from `~/.agents/skills`.

use std::io::Read;
use std::path::{Component, Path, PathBuf};

use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};

pub const SKILL_FILE: &str = "SKILL.md";

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_files: usize,
    pub max_file_bytes: u64,
    pub max_total_bytes: u64,
}

/// A whole source (a repository archive, a folder of skills).
pub const SOURCE_LIMITS: Limits = Limits { max_files: 50_000, max_file_bytes: 50 << 20, max_total_bytes: 512 << 20 };
/// One installed skill.
pub const SKILL_LIMITS: Limits = Limits { max_files: 5_000, max_file_bytes: 50 << 20, max_total_bytes: 100 << 20 };
/// Deepest directory walked in a folder source.
const MAX_DEPTH: usize = 12;
/// Never part of a skill.
const SKIPPED_DIRS: [&str; 2] = [".git", "node_modules"];

enum Data {
    Disk(PathBuf),
    Memory(Vec<u8>),
}

struct File {
    /// Relative, `/`-separated, validated.
    path: String,
    size: u64,
    data: Data,
    executable: bool,
}

pub struct Tree {
    files: Vec<File>,
    /// Symbolic links left out (never followed, never recreated).
    pub skipped_links: usize,
    /// Archive entries whose names cannot be written on every platform; a skill holding
    /// one is not installed.
    unportable: Vec<String>,
}

impl Tree {
    /// Every regular file below `root`. Links below `root` are skipped, not followed.
    pub fn from_dir(root: &Path, limits: Limits) -> Result<Self> {
        let mut tree = Tree { files: Vec::new(), skipped_links: 0, unportable: Vec::new() };
        let mut total = 0u64;
        walk(root, "", 0, limits, &mut total, &mut tree)?;
        tree.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(tree)
    }

    /// Every regular file in a zip archive; `strip_root` drops the single top folder
    /// GitHub puts around a repository. Unsafe entry names reject the whole archive.
    pub fn from_zip(bytes: &[u8], strip_root: bool, limits: Limits) -> Result<Self> {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).context("not a valid zip archive")?;
        if zip.len() > limits.max_files {
            bail!("the archive has {} entries; at most {} are accepted", zip.len(), limits.max_files);
        }
        let mut tree = Tree { files: Vec::new(), skipped_links: 0, unportable: Vec::new() };
        let mut total = 0u64;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i)?;
            let raw = entry.name().to_string();
            let Some(mut rel) = safe_relative(&raw)? else { continue };
            if strip_root {
                match rel.split_once('/') {
                    Some((_, rest)) => rel = rest.to_string(),
                    None => continue,
                }
            }
            if rel.is_empty() || entry.is_dir() {
                continue;
            }
            if entry.is_symlink() {
                tree.skipped_links += 1;
                continue;
            }
            if rel.split('/').any(|c| SKIPPED_DIRS.contains(&c)) {
                continue;
            }
            if !portable(&rel) {
                tree.unportable.push(rel);
                continue;
            }
            if entry.size() > limits.max_file_bytes {
                bail!("`{rel}` is {} bytes; files over {} bytes are not accepted", entry.size(), limits.max_file_bytes);
            }
            let mut data = Vec::with_capacity(entry.size() as usize);
            // `take` bounds what a lying size header can make us inflate.
            (&mut entry).take(limits.max_file_bytes + 1).read_to_end(&mut data).with_context(|| format!("cannot read `{rel}` from the archive"))?;
            if data.len() as u64 > limits.max_file_bytes {
                bail!("`{rel}` is larger than {} bytes", limits.max_file_bytes);
            }
            total += data.len() as u64;
            if total > limits.max_total_bytes {
                bail!("the archive unpacks to more than {} bytes", limits.max_total_bytes);
            }
            let executable = entry.unix_mode().is_some_and(|m| m & 0o111 != 0);
            tree.files.push(File { path: rel, size: data.len() as u64, data: Data::Memory(data), executable });
        }
        tree.files.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(tree)
    }

    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|f| f.path.as_str())
    }

    fn read_file(f: &File) -> Result<Vec<u8>> {
        match &f.data {
            Data::Memory(b) => Ok(b.clone()),
            Data::Disk(p) => std::fs::read(p).with_context(|| format!("cannot read {}", p.display())),
        }
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>> {
        let f = self.files.iter().find(|f| f.path == rel).with_context(|| format!("`{rel}` is not in the source"))?;
        Self::read_file(f)
    }

    fn under<'a>(&'a self, dir: &'a str) -> impl Iterator<Item = (&'a str, &'a File)> + 'a {
        self.files.iter().filter_map(move |f| {
            if dir.is_empty() {
                Some((f.path.as_str(), f))
            } else {
                f.path.strip_prefix(dir).and_then(|r| r.strip_prefix('/')).map(|r| (r, f))
            }
        })
    }

    /// Directories holding a `SKILL.md` (`""` for the root), outermost only: a `SKILL.md`
    /// inside another skill belongs to that skill.
    pub fn skill_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self
            .files
            .iter()
            .filter_map(|f| match f.path.rsplit_once('/') {
                Some((dir, SKILL_FILE)) => Some(dir.to_string()),
                None if f.path == SKILL_FILE => Some(String::new()),
                _ => None,
            })
            .collect();
        dirs.sort();
        let mut out: Vec<String> = Vec::new();
        for d in dirs {
            let nested = out.iter().any(|o| o.is_empty() || d.starts_with(&format!("{o}/")));
            if !nested {
                out.push(d);
            }
        }
        out
    }

    /// Content hash of the files under `dir`: relative paths and bytes, in path order.
    pub fn hash(&self, dir: &str) -> Result<String> {
        let mut h = Sha256::new();
        for (rel, f) in self.under(dir) {
            let bytes = Self::read_file(f)?;
            h.update((rel.len() as u64).to_le_bytes());
            h.update(rel.as_bytes());
            h.update((bytes.len() as u64).to_le_bytes());
            h.update(&bytes);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }

    /// Checks the files under `dir` against `limits`; returns (files, bytes).
    pub fn check(&self, dir: &str, limits: Limits) -> Result<(usize, u64)> {
        let inside = |p: &String| dir.is_empty() || p.strip_prefix(dir).is_some_and(|r| r.starts_with('/'));
        if let Some(bad) = self.unportable.iter().find(|p| inside(p)) {
            bail!("the skill contains `{bad}`, a name that cannot be written on every platform");
        }
        let (mut n, mut total) = (0usize, 0u64);
        for (_, f) in self.under(dir) {
            n += 1;
            total += f.size;
        }
        if n > limits.max_files {
            bail!("the skill has {n} files; at most {} are accepted", limits.max_files);
        }
        if total > limits.max_total_bytes {
            bail!("the skill is {total} bytes; at most {} are accepted", limits.max_total_bytes);
        }
        Ok((n, total))
    }

    /// Writes the files under `dir` into `dest`, which must not exist yet.
    pub fn write(&self, dir: &str, dest: &Path) -> Result<()> {
        if dest.exists() {
            bail!("{} already exists", dest.display());
        }
        std::fs::create_dir_all(dest).with_context(|| format!("cannot create {}", dest.display()))?;
        for (rel, f) in self.under(dir) {
            let target = dest.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).with_context(|| format!("cannot create {}", parent.display()))?;
            }
            match &f.data {
                Data::Memory(b) => std::fs::write(&target, b),
                Data::Disk(p) => std::fs::copy(p, &target).map(|_| ()),
            }
            .with_context(|| format!("cannot write {}", target.display()))?;
            set_executable(&target, f.executable);
        }
        Ok(())
    }
}

#[cfg(unix)]
fn set_executable(path: &Path, executable: bool) {
    use std::os::unix::fs::PermissionsExt;
    if executable {
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755));
    }
}

#[cfg(not(unix))]
fn set_executable(_: &Path, _: bool) {}

fn walk(dir: &Path, prefix: &str, depth: usize, limits: Limits, total: &mut u64, tree: &mut Tree) -> Result<()> {
    if depth > MAX_DEPTH {
        bail!("{} is nested more than {MAX_DEPTH} directories deep", dir.display());
    }
    let mut entries: Vec<_> = std::fs::read_dir(dir).with_context(|| format!("cannot read {}", dir.display()))?.collect::<std::io::Result<_>>()?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().to_string();
        let rel = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
        let ty = entry.file_type()?;
        if ty.is_symlink() {
            tree.skipped_links += 1;
        } else if ty.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) {
                walk(&entry.path(), &rel, depth + 1, limits, total, tree)?;
            }
        } else if ty.is_file() {
            let size = entry.metadata()?.len();
            if size > limits.max_file_bytes {
                bail!("{} is {size} bytes; files over {} bytes are not accepted", entry.path().display(), limits.max_file_bytes);
            }
            *total += size;
            if tree.files.len() >= limits.max_files || *total > limits.max_total_bytes {
                bail!("{} holds more than {} files or {} bytes; pick the skill's own folder", dir.display(), limits.max_files, limits.max_total_bytes);
            }
            tree.files.push(File { path: rel, size, data: Data::Disk(entry.path()), executable: is_executable(&entry) });
        }
    }
    Ok(())
}

#[cfg(unix)]
fn is_executable(entry: &std::fs::DirEntry) -> bool {
    use std::os::unix::fs::PermissionsExt;
    entry.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::DirEntry) -> bool {
    false
}

/// Windows device names, which no path component may use.
const RESERVED: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5",
    "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Whether every component of a relative path can be created on Windows too (no device
/// names, `:` streams, trailing dots, or reserved characters).
pub(crate) fn portable(rel: &str) -> bool {
    rel.split('/').all(|p| {
        let stem = p.split('.').next().unwrap_or(p).to_ascii_lowercase();
        !(p.contains(':') || p.ends_with(['.', ' ']) || RESERVED.contains(&stem.as_str()) || p.chars().any(|c| c.is_control() || "<>\"|?*".contains(c)))
    })
}

/// The `/`-joined relative path of an archive entry; `None` for the root itself. Absolute
/// paths, drive letters, and `..` reject the archive.
pub fn safe_relative(raw: &str) -> Result<Option<String>> {
    let unified = raw.replace('\\', "/");
    let drive = unified.len() >= 2 && unified.as_bytes()[1] == b':' && unified.as_bytes()[0].is_ascii_alphabetic();
    if unified.starts_with('/') || drive || unified.contains('\0') {
        bail!("the archive contains an absolute or invalid path `{raw}`");
    }
    let mut parts = Vec::new();
    for part in unified.split('/') {
        match part {
            "" | "." => continue,
            ".." => bail!("the archive contains a path that leaves its folder: `{raw}`"),
            p => parts.push(p),
        }
    }
    // Belt and braces: the joined path must stay relative.
    let joined = parts.join("/");
    if Path::new(&joined).components().any(|c| !matches!(c, Component::Normal(_))) {
        bail!("the archive contains an unsafe path `{raw}`");
    }
    Ok(if joined.is_empty() { None } else { Some(joined) })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    pub(crate) fn zip_of(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, body) in entries {
            if let Some(target) = body.strip_prefix("->") {
                w.add_symlink(*name, target, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                w.write_all(body.as_bytes()).unwrap();
            }
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn unsafe_entry_names_are_rejected() {
        for bad in ["../evil.txt", "a/../../evil", "/etc/passwd", "C:/Windows/x", "C:\\x", "a\\..\\..\\x"] {
            let bytes = zip_of(&[("repo/SKILL.md", "---\n---\n"), (bad, "x")]);
            assert!(Tree::from_zip(&bytes, false, SOURCE_LIMITS).is_err(), "{bad} must be rejected");
        }
        assert_eq!(safe_relative("a/./b//c").unwrap().as_deref(), Some("a/b/c"));
        assert_eq!(safe_relative("./").unwrap(), None);
    }

    #[test]
    fn unportable_names_block_only_their_skill() {
        let bytes = zip_of(&[("s/a/SKILL.md", "x"), ("s/a/con.txt", "x"), ("s/b/SKILL.md", "x"), ("s/b/c/file:stream", "x")]);
        let tree = Tree::from_zip(&bytes, false, SOURCE_LIMITS).unwrap();
        assert!(tree.check("s/a", SKILL_LIMITS).is_err());
        assert!(tree.check("s/b", SKILL_LIMITS).is_err());
        let ok = Tree::from_zip(&zip_of(&[("s/a/SKILL.md", "x"), ("s/aux-notes/SKILL.md", "x"), ("s/b/aux.py", "x")]), false, SOURCE_LIMITS).unwrap();
        assert!(ok.check("s/a", SKILL_LIMITS).is_ok(), "a sibling's bad name does not matter");
        assert!(ok.check("s/b", SKILL_LIMITS).is_err());
    }

    #[test]
    fn symlinks_are_skipped_and_root_is_stripped() {
        let bytes = zip_of(&[("repo-main/skills/a/SKILL.md", "x"), ("repo-main/skills/a/link", "->/etc/passwd"), ("repo-main/README.md", "r")]);
        let tree = Tree::from_zip(&bytes, true, SOURCE_LIMITS).unwrap();
        assert_eq!(tree.paths().collect::<Vec<_>>(), ["README.md", "skills/a/SKILL.md"]);
        assert_eq!(tree.skipped_links, 1);
    }

    #[test]
    fn size_limits() {
        let big = "x".repeat(2048);
        let bytes = zip_of(&[("a/SKILL.md", &big)]);
        let tight = Limits { max_files: 10, max_file_bytes: 1024, max_total_bytes: 4096 };
        assert!(Tree::from_zip(&bytes, false, tight).is_err());
        let files: Vec<(String, String)> = (0..5).map(|i| (format!("a/{i}.txt"), "y".repeat(1000))).collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        assert!(Tree::from_zip(&zip_of(&refs), false, tight).is_err(), "total size limit");
    }

    #[test]
    fn skill_dirs_are_outermost() {
        let bytes = zip_of(&[("skills/a/SKILL.md", "x"), ("skills/a/examples/b/SKILL.md", "x"), ("skills/c/SKILL.md", "x"), ("other.md", "x")]);
        let tree = Tree::from_zip(&bytes, false, SOURCE_LIMITS).unwrap();
        assert_eq!(tree.skill_dirs(), ["skills/a", "skills/c"]);
    }

    #[test]
    fn zip_and_disk_hash_alike() {
        let bytes = zip_of(&[("r/skills/a/SKILL.md", "---\nname: a\n---\n"), ("r/skills/a/scripts/run.py", "print(1)\n"), ("r/skills/b/SKILL.md", "b")]);
        let tree = Tree::from_zip(&bytes, true, SOURCE_LIMITS).unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let dest = tmp.path().join("a");
        tree.write("skills/a", &dest).unwrap();
        let disk = Tree::from_dir(&dest, SKILL_LIMITS).unwrap();
        assert_eq!(disk.paths().collect::<Vec<_>>(), ["SKILL.md", "scripts/run.py"]);
        assert_eq!(disk.hash("").unwrap(), tree.hash("skills/a").unwrap());
        assert_ne!(tree.hash("skills/b").unwrap(), tree.hash("skills/a").unwrap());
    }
}
