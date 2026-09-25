//! `<state>/skills.json`: which skills OwO AI Gateway manages, where each came from, what it
//! created for which app, and the GitHub repositories used for discovery.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::fsops::LinkKind;
use crate::layout::AppId;

pub const STATE_FILE: &str = "skills.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct State {
    /// Discovery repositories; `None` means the built-in list.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repos: Option<Vec<RepoSpec>>,
    #[serde(default)]
    pub skills: BTreeMap<String, Managed>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Managed {
    pub source: Source,
    pub installed_at: u64,
    pub updated_at: u64,
    /// Content hash of the files as installed (see `Tree::hash`).
    pub hash: String,
    /// Directories OwO AI Gateway created in apps' own skills folders.
    #[serde(default)]
    pub links: Vec<Link>,
    /// Apps whose own "off" setting OwO AI Gateway wrote for this skill.
    #[serde(default)]
    pub disabled: BTreeSet<AppId>,
}

impl Managed {
    pub fn link(&self, app: AppId) -> Option<&Link> {
        self.links.iter().find(|l| l.app == app)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Source {
    /// Copied from a folder.
    Local { path: PathBuf },
    /// Unpacked from a zip file; `subdir` is the skill's folder inside it.
    Zip { path: PathBuf, subdir: String },
    Github {
        owner: String,
        repo: String,
        /// Branch, tag, or commit asked for; `None` follows the default branch.
        #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
        reference: Option<String>,
        subdir: String,
        commit: String,
    },
    /// Was already in `~/.agents/skills`; the user handed it to OwO AI Gateway.
    Adopted,
    /// Written (or edited) in OwO AI Gateway; `from` is the source it had before an edit.
    /// Nothing updates it from elsewhere.
    Authored {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        from: Option<String>,
    },
}

impl Source {
    pub fn label(&self) -> String {
        match self {
            Source::Local { path } => path.display().to_string(),
            Source::Zip { path, subdir } if subdir.is_empty() => path.display().to_string(),
            Source::Zip { path, subdir } => format!("{} ({subdir})", path.display()),
            Source::Github { owner, repo, subdir, commit, .. } => {
                let short: String = commit.chars().take(7).collect();
                let sub = if subdir.is_empty() { String::new() } else { format!("/{subdir}") };
                format!("github:{owner}/{repo}{sub}@{short}")
            }
            Source::Adopted => "adopted".into(),
            Source::Authored { from: None } => "authored".into(),
            Source::Authored { from: Some(from) } => format!("authored (was {from})"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Link {
    pub app: AppId,
    pub path: PathBuf,
    pub kind: LinkKind,
}

/// A GitHub repository to discover skills in: `owner/repo[/subdir][@ref]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RepoSpec {
    pub owner: String,
    pub repo: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subdir: Option<String>,
    #[serde(rename = "ref", default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

fn valid_github_name(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s != "." && s != ".." && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}

impl RepoSpec {
    /// `owner/repo[/sub/dir][@ref]`, also `github:` prefixed or a `https://github.com/…`
    /// URL (`/tree/<ref>/<subdir>` included).
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let s = s.strip_prefix("github:").unwrap_or(s);
        let (s, from_url) = match s.strip_prefix("https://github.com/").or_else(|| s.strip_prefix("http://github.com/")).or_else(|| s.strip_prefix("github.com/")) {
            Some(rest) => (rest.trim_end_matches('/').trim_end_matches(".git"), true),
            None => (s, false),
        };
        let (path, mut reference) = match s.rsplit_once('@') {
            Some((p, r)) if !r.is_empty() => (p, Some(r.to_string())),
            _ => (s, None),
        };
        let mut parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() < 2 {
            bail!("`{s}` is not a GitHub repository; write owner/repo[/subdir][@ref]");
        }
        let (owner, repo) = (parts[0].to_string(), parts[1].trim_end_matches(".git").to_string());
        let mut rest: Vec<&str> = parts.split_off(2);
        if from_url && rest.first().is_some_and(|p| *p == "tree" || *p == "blob") && rest.len() >= 2 {
            reference.get_or_insert_with(|| rest[1].to_string());
            rest = rest.split_off(2);
        }
        if !valid_github_name(&owner) || !valid_github_name(&repo) {
            bail!("`{owner}/{repo}` is not a valid GitHub repository name");
        }
        if rest.iter().any(|p| *p == ".." || *p == ".") {
            bail!("the folder inside the repository may not contain `.` or `..`");
        }
        if let Some(r) = &reference {
            if r.contains("..") || r.chars().any(|c| c.is_whitespace() || c.is_control() || "~^:?*[\\".contains(c)) {
                bail!("`{r}` is not a valid branch, tag, or commit");
            }
        }
        let subdir = Some(rest.join("/")).filter(|d| !d.is_empty());
        Ok(Self { owner, repo, subdir, reference })
    }

    /// The repository (and ref) whose archive holds this spec.
    pub fn repo_key(&self) -> String {
        match &self.reference {
            Some(r) => format!("{}/{}@{r}", self.owner, self.repo),
            None => format!("{}/{}", self.owner, self.repo),
        }
    }
}

impl std::fmt::Display for RepoSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.repo)?;
        if let Some(d) = &self.subdir {
            write!(f, "/{d}")?;
        }
        if let Some(r) = &self.reference {
            write!(f, "@{r}")?;
        }
        Ok(())
    }
}

/// Well-known public skill collections (each keeps its skills under `skills/`).
pub fn default_repos() -> Vec<RepoSpec> {
    ["anthropics/skills/skills", "openai/skills/skills", "vercel-labs/agent-skills/skills", "huggingface/skills/skills", "MiniMax-AI/skills/skills"]
        .iter()
        .map(|s| RepoSpec::parse(s).expect("built-in repository"))
        .collect()
}

pub fn load(state_dir: &Path) -> Result<State> {
    let path = state_dir.join(STATE_FILE);
    match std::fs::read_to_string(&path) {
        Ok(t) => serde_json::from_str(&t).with_context(|| format!("{} is corrupt; fix or remove it", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(State::default()),
        Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
    }
}

pub fn save(state_dir: &Path, state: &State) -> Result<()> {
    let mut text = serde_json::to_string_pretty(state)?;
    text.push('\n');
    owo_client_apps::managed::write_atomic(&state_dir.join(STATE_FILE), text.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repo_specs() {
        let r = RepoSpec::parse("github:anthropics/skills/skills/pdf@main").unwrap();
        assert_eq!((r.owner.as_str(), r.repo.as_str(), r.subdir.as_deref(), r.reference.as_deref()), ("anthropics", "skills", Some("skills/pdf"), Some("main")));
        assert_eq!(r.to_string(), "anthropics/skills/skills/pdf@main");
        assert_eq!(r.repo_key(), "anthropics/skills@main");
        let u = RepoSpec::parse("https://github.com/openai/skills/tree/main/skills/.curated").unwrap();
        assert_eq!((u.subdir.as_deref(), u.reference.as_deref()), (Some("skills/.curated"), Some("main")));
        assert_eq!(RepoSpec::parse("https://github.com/a/b.git").unwrap().repo, "b");
        for bad in ["onlyowner", "a/b/../c", "a b/c", "a/b@x..y", "../x/y"] {
            assert!(RepoSpec::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(default_repos().len(), 5);
    }

    #[test]
    fn state_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut s = State::default();
        s.skills.insert(
            "pdf".into(),
            Managed {
                source: Source::Github { owner: "a".into(), repo: "b".into(), reference: None, subdir: "skills/pdf".into(), commit: "0123456789".into() },
                installed_at: 1,
                updated_at: 1,
                hash: "h".into(),
                links: vec![Link { app: AppId::Claude, path: "/x".into(), kind: LinkKind::Junction }],
                disabled: [AppId::Codex].into(),
            },
        );
        save(tmp.path(), &s).unwrap();
        let back = load(tmp.path()).unwrap();
        assert_eq!(back.skills["pdf"].source, s.skills["pdf"].source);
        assert_eq!(back.skills["pdf"].source.label(), "github:a/b/skills/pdf@0123456");
        assert!(back.repos.is_none());
    }
}
