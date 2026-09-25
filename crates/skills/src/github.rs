//! Skills on GitHub. One API call resolves a branch or tag to a commit; the archive of that
//! commit comes from codeload.github.com (not rate limited like the API). Both are cached
//! in `<state>/skills-cache/`, so listing, discovering, and installing again cost nothing.
//! No token is used: unauthenticated clients get 60 API calls an hour, and when those run
//! out the archive is downloaded by ref and its commit read from the zip comment.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::frontmatter;
use crate::state::RepoSpec;
use crate::tree::{Tree, SKILL_FILE, SKILL_LIMITS, SOURCE_LIMITS};

pub const CACHE_DIR: &str = "skills-cache";
const INDEX_FILE: &str = "index.json";
const MAX_DOWNLOAD: usize = 200 << 20;
/// How long a resolved commit is trusted before discovery asks GitHub again.
pub const FRESH_SECS: u64 = 6 * 3600;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Cache {
    /// By `RepoSpec::repo_key` (`owner/repo[@ref]`).
    #[serde(default)]
    pub repos: BTreeMap<String, CachedRepo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CachedRepo {
    pub commit: String,
    pub fetched_at: u64,
    /// File name of the archive inside the cache directory.
    pub archive: String,
    /// Every skill in the repository (a spec's `subdir` filters when shown).
    pub skills: Vec<Found>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Found {
    /// The skill's folder inside the repository.
    pub dir: String,
    pub name: String,
    pub description: String,
    pub hash: String,
    /// Why it cannot be installed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub problem: Option<String>,
}

fn cache_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(CACHE_DIR)
}

pub fn load_cache(state_dir: &Path) -> Cache {
    std::fs::read_to_string(cache_dir(state_dir).join(INDEX_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub(crate) fn save_cache(state_dir: &Path, cache: &Cache) -> Result<()> {
    let mut text = serde_json::to_string_pretty(cache)?;
    text.push('\n');
    owo_client_apps::managed::write_atomic(&cache_dir(state_dir).join(INDEX_FILE), text.as_bytes())
}

/// Every skill in `tree`, with what keeps one from being installed.
pub fn scan(tree: &Tree) -> Vec<Found> {
    tree.skill_dirs()
        .into_iter()
        .map(|dir| {
            let leaf = dir.rsplit('/').next().unwrap_or("").to_string();
            let file = if dir.is_empty() { SKILL_FILE.to_string() } else { format!("{dir}/{SKILL_FILE}") };
            let text = tree.read(&file).map(|b| String::from_utf8_lossy(&b).into_owned());
            let meta = text.and_then(|t| frontmatter::inspect(&leaf, &t));
            let hash = tree.hash(&dir).unwrap_or_default();
            let checked = tree.check(&dir, SKILL_LIMITS);
            match meta {
                Ok(m) => Found { dir, name: m.name, description: m.description, hash, problem: checked.err().map(|e| format!("{e:#}")) },
                Err(e) => Found { dir, name: leaf, description: String::new(), hash, problem: Some(format!("{e:#}")) },
            }
        })
        .collect()
}

fn client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .user_agent(concat!("owo-skills/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(180))
        .build()
        .context("cannot start the HTTP client")
}

enum Resolved {
    Commit(String),
    RateLimited(String),
}

async fn resolve(client: &reqwest::Client, owner: &str, repo: &str, reference: Option<&str>) -> Result<Resolved> {
    let url = format!("https://api.github.com/repos/{owner}/{repo}/commits/{}", reference.unwrap_or("HEAD"));
    let resp = client.get(&url).header("Accept", "application/vnd.github.sha").send().await.with_context(|| format!("cannot reach GitHub for {owner}/{repo}"))?;
    let status = resp.status();
    let remaining = resp.headers().get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()).map(str::to_string);
    let reset = resp.headers().get("x-ratelimit-reset").and_then(|v| v.to_str().ok()).and_then(|v| v.parse::<u64>().ok());
    if status.is_success() {
        let sha = resp.text().await?.trim().to_string();
        if sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(Resolved::Commit(sha));
        }
        bail!("GitHub answered {owner}/{repo} with something that is not a commit id");
    }
    if status.as_u16() == 429 || (status.as_u16() == 403 && remaining.as_deref() == Some("0")) {
        let wait = reset.map(|r| r.saturating_sub(owo_client_apps::managed::now_unix()) / 60 + 1).map_or(String::new(), |m| format!(" (resets in about {m} min)"));
        return Ok(Resolved::RateLimited(format!("GitHub's limit of 60 unauthenticated API calls an hour is used up{wait}")));
    }
    if status.as_u16() == 404 || status.as_u16() == 422 {
        bail!("GitHub has no repository {owner}/{repo}{} (or it is private)", reference.map(|r| format!(" with ref `{r}`")).unwrap_or_default());
    }
    bail!("GitHub answered {status} for {owner}/{repo}")
}

async fn download(client: &reqwest::Client, url: &str) -> Result<Vec<u8>> {
    let mut resp = client.get(url).send().await.with_context(|| format!("cannot download {url}"))?;
    if !resp.status().is_success() {
        bail!("downloading {url} failed: {}", resp.status());
    }
    if resp.content_length().is_some_and(|n| n as usize > MAX_DOWNLOAD) {
        bail!("{url} is larger than {} MB", MAX_DOWNLOAD >> 20);
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.with_context(|| format!("downloading {url} was interrupted"))? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_DOWNLOAD {
            bail!("{url} is larger than {} MB", MAX_DOWNLOAD >> 20);
        }
    }
    Ok(body)
}

/// GitHub writes the commit id into the zip comment of every archive.
fn commit_of_archive(bytes: &[u8]) -> Option<String> {
    let zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).ok()?;
    let comment = String::from_utf8_lossy(zip.comment()).trim().to_string();
    (comment.len() == 40 && comment.bytes().all(|b| b.is_ascii_hexdigit())).then_some(comment)
}

fn archive_name(owner: &str, repo: &str, commit: &str) -> String {
    format!("{owner}__{repo}__{commit}.zip")
}

/// The cached listing of the repository behind `spec`, fetched again when older than
/// `max_age` seconds (0 always asks GitHub) or when its archive is gone.
pub async fn refresh(state_dir: &Path, spec: &RepoSpec, max_age: u64) -> Result<CachedRepo> {
    let key = spec.repo_key();
    let mut cache = load_cache(state_dir);
    let now = owo_client_apps::managed::now_unix();
    let dir = cache_dir(state_dir);
    let have = |c: &CachedRepo| dir.join(&c.archive).is_file();
    if let Some(c) = cache.repos.get(&key) {
        if now.saturating_sub(c.fetched_at) < max_age && have(c) {
            return Ok(c.clone());
        }
    }
    let client = client()?;
    let codeload = |x: &str| format!("https://codeload.github.com/{}/{}/zip/{x}", spec.owner, spec.repo);
    let (commit, bytes) = match resolve(&client, &spec.owner, &spec.repo, spec.reference.as_deref()).await? {
        Resolved::Commit(sha) => {
            if let Some(c) = cache.repos.get_mut(&key).filter(|c| c.commit == sha && have(c)) {
                c.fetched_at = now;
                let c = c.clone();
                save_cache(state_dir, &cache)?;
                return Ok(c);
            }
            let bytes = download(&client, &codeload(&sha)).await?;
            (sha, bytes)
        }
        Resolved::RateLimited(why) => {
            let bytes = download(&client, &codeload(spec.reference.as_deref().unwrap_or("HEAD"))).await.with_context(|| why.clone())?;
            let commit = commit_of_archive(&bytes).unwrap_or_else(|| format!("sha256-{}", owo_client_apps::managed::sha256(&bytes)));
            (commit, bytes)
        }
    };
    let tree = Tree::from_zip(&bytes, true, SOURCE_LIMITS).with_context(|| format!("the archive of {key} cannot be used"))?;
    let skills = scan(&tree);
    std::fs::create_dir_all(&dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let archive = archive_name(&spec.owner, &spec.repo, &commit);
    owo_client_apps::managed::write_atomic(&dir.join(&archive), &bytes)?;
    // Archives of this repository's earlier commits are no longer referenced by this key.
    if let Some(old) = cache.repos.get(&key) {
        if old.archive != archive && !cache.repos.iter().any(|(k, c)| k != &key && c.archive == old.archive) {
            let _ = std::fs::remove_file(dir.join(&old.archive));
        }
    }
    let entry = CachedRepo { commit, fetched_at: now, archive, skills };
    cache.repos.insert(key, entry.clone());
    save_cache(state_dir, &cache)?;
    Ok(entry)
}

/// The files of a cached archive.
pub fn open(state_dir: &Path, cached: &CachedRepo) -> Result<Tree> {
    let path = cache_dir(state_dir).join(&cached.archive);
    let bytes = std::fs::read(&path).with_context(|| format!("the cached archive {} is gone; discover again", path.display()))?;
    Tree::from_zip(&bytes, true, SOURCE_LIMITS)
}

/// Whether `dir` is `subdir` or below it (`None` matches everything).
pub fn within(dir: &str, subdir: Option<&str>) -> bool {
    match subdir {
        None | Some("") => true,
        Some(s) => dir == s || dir.strip_prefix(s).is_some_and(|r| r.starts_with('/')),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tree::tests::zip_of;

    #[test]
    fn scan_reports_problems() {
        let bytes = zip_of(&[
            ("r-main/skills/pdf/SKILL.md", "---\nname: pdf\ndescription: PDFs\n---\n"),
            ("r-main/skills/Bad_Name/SKILL.md", "---\nname: Bad_Name\ndescription: x\n---\n"),
            ("r-main/skills/nodesc/SKILL.md", "---\nname: nodesc\n---\n"),
        ]);
        let tree = Tree::from_zip(&bytes, true, SOURCE_LIMITS).unwrap();
        let found = scan(&tree);
        assert_eq!(found.iter().map(|f| f.dir.as_str()).collect::<Vec<_>>(), ["skills/Bad_Name", "skills/nodesc", "skills/pdf"]);
        assert!(found[0].problem.is_some() && found[1].problem.is_some());
        assert_eq!(found[2].name, "pdf");
        assert!(found[2].problem.is_none());
        assert!(within("skills/pdf", Some("skills")) && !within("skillsx/pdf", Some("skills")) && within("a", None));
    }

    /// Talks to GitHub: `cargo test -p owo-skills -- --ignored`.
    #[test]
    #[ignore]
    fn discovers_a_real_repository() {
        let tmp = tempfile::tempdir().unwrap();
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let spec = RepoSpec::parse("anthropics/skills/skills").unwrap();
        let first = rt.block_on(refresh(tmp.path(), &spec, 0)).unwrap();
        assert_eq!(first.commit.len(), 40);
        assert!(first.skills.iter().any(|s| s.name == "pdf" && s.problem.is_none()));
        let again = rt.block_on(refresh(tmp.path(), &spec, FRESH_SECS)).unwrap();
        assert_eq!(again.fetched_at, first.fetched_at, "served from the cache");
        assert!(open(tmp.path(), &again).unwrap().skill_dirs().contains(&"skills/pdf".to_string()));
    }
}
