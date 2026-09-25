//! End-to-end behaviour in a throwaway home directory.

use super::*;
use crate::github::{Cache, CachedRepo};
use crate::tree::tests::zip_of;

fn setup() -> (tempfile::TempDir, Skills) {
    let tmp = tempfile::tempdir().unwrap();
    let layout = Layout::for_home(&tmp.path().join("home"));
    for d in [&layout.codex_home, &layout.claude_home, &layout.cursor_home, &layout.opencode_home, &layout.grok_home, &layout.copilot_home] {
        std::fs::create_dir_all(d).unwrap();
    }
    let skills = Skills::new(layout, &tmp.path().join("owo/state"), &tmp.path().join("owo/backups"));
    (tmp, skills)
}

fn write_skill(dir: &Path, name: &str, description: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join(SKILL_FILE), format!("---\nname: {name}\ndescription: {description}\n---\n# {name}\n")).unwrap();
    std::fs::write(dir.join("scripts/run.sh"), "echo hi\n").unwrap();
}

fn hash(dir: &Path) -> String {
    Tree::from_dir(dir, SOURCE_LIMITS).unwrap().hash("").unwrap()
}

fn app(view: &SkillView, id: AppId) -> &AppState {
    view.apps.iter().find(|a| a.app == id).unwrap()
}

fn backups_named(s: &Skills, prefix: &str) -> Vec<PathBuf> {
    std::fs::read_dir(&s.backups).map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix)).collect()).unwrap_or_default()
}

#[test]
fn install_toggle_remove_round_trip() {
    let (tmp, s) = setup();
    let codex_config = s.layout.codex_home.join("config.toml");
    std::fs::write(&codex_config, "model = \"m\" # mine\n").unwrap();
    let src = tmp.path().join("src/pdf");
    write_skill(&src, "pdf", "PDF files");

    s.install_path(&src, &InstallOptions::default()).unwrap();
    let stored = s.store_path("pdf");
    assert_eq!(hash(&stored), hash(&src));
    let claude = s.layout.dir(Location::Claude).join("pdf");
    assert!(fsops::is_link(&claude), "Claude Code is installed, so it gets a link by default");

    let list = s.list().unwrap();
    let pdf = &list.skills[0];
    assert_eq!((pdf.name.as_str(), pdf.description.as_str(), pdf.managed, pdf.modified), ("pdf", "PDF files", true, false));
    assert_eq!(app(pdf, AppId::Claude).state, AppStateKind::Linked);
    assert_eq!(app(pdf, AppId::Codex).state, AppStateKind::On);
    assert!(app(pdf, AppId::Codex).can_toggle);
    assert_eq!(app(pdf, AppId::Cursor).state, AppStateKind::AlwaysOn);
    assert!(app(pdf, AppId::Cursor).duplicate, "Cursor also reads ~/.claude/skills");
    assert!(list.unmanaged.is_empty(), "OwO AI Gateway's own link is not an unmanaged skill: {:?}", list.unmanaged);

    s.set_enabled("pdf", AppId::Codex, false).unwrap();
    assert!(std::fs::read_to_string(&codex_config).unwrap().contains("enabled = false"));
    assert_eq!(app(&s.list().unwrap().skills[0], AppId::Codex).state, AppStateKind::OffByOwo);
    assert!(s.set_enabled("pdf", AppId::Cursor, false).is_err(), "Cursor has no switch OwO AI Gateway may use");

    s.set_enabled("pdf", AppId::Claude, false).unwrap();
    assert!(!fsops::exists(&claude));
    assert!(stored.join(SKILL_FILE).is_file(), "unlinking leaves the store alone");
    s.set_enabled("pdf", AppId::Claude, true).unwrap();
    assert!(fsops::is_link(&claude));

    s.remove("pdf").unwrap();
    assert!(!fsops::exists(&stored) && !fsops::exists(&claude));
    assert_eq!(std::fs::read_to_string(&codex_config).unwrap(), "model = \"m\" # mine\n", "OwO AI Gateway's entry is gone again");
    let backups = backups_named(&s, "pdf-");
    assert_eq!(backups.len(), 1);
    assert_eq!(hash(&backups[0]), hash(&src), "the removed skill is in the backups");
    assert!(src.join(SKILL_FILE).is_file(), "the source is never touched");
    assert!(s.list().unwrap().skills.is_empty());
}

#[test]
fn unmanaged_skills_are_left_alone() {
    let (tmp, s) = setup();
    let theirs = s.store_path("theirs");
    write_skill(&theirs, "theirs", "someone else's");
    let mine = s.layout.dir(Location::Claude).join("mine");
    write_skill(&mine, "mine", "Claude only");
    let system = s.layout.dir(Location::Codex).join(".system/skill-creator");
    write_skill(&system, "skill-creator", "bundled");
    let before = (hash(&theirs), hash(&mine));

    assert!(s.remove("theirs").is_err());
    assert!(s.set_enabled("theirs", AppId::Codex, false).is_err());
    let clash = tmp.path().join("src/theirs");
    write_skill(&clash, "theirs", "same name");
    assert!(s.install_path(&clash, &InstallOptions::default()).is_err(), "never overwrites a skill it does not manage");

    let other = tmp.path().join("src/other");
    write_skill(&other, "other", "x");
    s.install_path(&other, &InstallOptions { apps: Some(vec![AppId::Codex]), ..Default::default() }).unwrap();
    let list = s.list().unwrap();
    let names: Vec<(&str, Location, bool)> = list.unmanaged.iter().map(|v| (v.name.as_str(), v.location, v.can_adopt)).collect();
    assert_eq!(names, [("theirs", Location::Agents, true), ("mine", Location::Claude, false)], "hidden folders such as .system are skipped");
    assert!(list.unmanaged[0].apps.iter().all(|a| !a.can_toggle));
    s.remove("other").unwrap();
    assert_eq!((hash(&theirs), hash(&mine)), before);

    s.adopt("theirs").unwrap();
    let list = s.list().unwrap();
    assert_eq!(list.skills[0].name, "theirs");
    assert_eq!(list.skills[0].source.as_ref().unwrap().kind, "adopted");
    assert_eq!(hash(&theirs), before.0, "adopting changes nothing on disk");
    assert!(s.adopt("mine").is_err(), "only skills in the store can be adopted");
}

#[test]
fn explicit_apps_turn_the_others_off() {
    let (tmp, s) = setup();
    let grok = s.layout.grok_home.join("config.toml");
    std::fs::write(&grok, "[ui]\ntheme = \"dark\"\n").unwrap();
    let src = tmp.path().join("src/pdf");
    write_skill(&src, "pdf", "x");
    let report = s.install_path(&src, &InstallOptions { apps: Some(vec![AppId::Codex]), ..Default::default() }).unwrap();
    assert!(report.warnings.iter().any(|w| w.contains("Cursor")), "{report:?}");
    let pdf = &s.list().unwrap().skills[0];
    for off in [AppId::Opencode, AppId::Grok, AppId::Copilot] {
        assert_eq!(app(pdf, off).state, AppStateKind::OffByOwo, "{off}");
    }
    assert_eq!(app(pdf, AppId::Codex).state, AppStateKind::On);
    assert_eq!(app(pdf, AppId::Claude).state, AppStateKind::NotLinked);
    assert!(!fsops::exists(&s.layout.dir(Location::Claude).join("pdf")));

    s.set_enabled("pdf", AppId::Opencode, true).unwrap();
    assert_eq!(app(&s.list().unwrap().skills[0], AppId::Opencode).state, AppStateKind::On);
    s.remove("pdf").unwrap();
    assert_eq!(std::fs::read_to_string(&grok).unwrap().trim_end(), "[ui]\ntheme = \"dark\"");
    let settings: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(s.layout.copilot_home.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings, serde_json::json!({}));
}

#[test]
fn zip_sources() {
    let (tmp, s) = setup();
    let zip = tmp.path().join("pack.zip");
    std::fs::write(
        &zip,
        zip_of(&[
            ("pack/skills/a/SKILL.md", "---\nname: a\ndescription: A\n---\n"),
            ("pack/skills/b/SKILL.md", "---\nname: b\ndescription: B\n---\n"),
            ("pack/skills/b/ref/notes.md", "notes"),
        ]),
    )
    .unwrap();
    assert!(s.install_path(&zip, &InstallOptions::default()).unwrap_err().to_string().contains("--skill"));
    s.install_path(&zip, &InstallOptions { only: vec!["b".into()], ..Default::default() }).unwrap();
    assert!(s.store_path("b").join("ref/notes.md").is_file());
    let r = s.install_path(&zip, &InstallOptions { all: true, ..Default::default() }).unwrap();
    assert!(r.lines.iter().any(|l| l == "b: already installed"), "{r:?}");
    assert!(s.store_path("a").join(SKILL_FILE).is_file());

    let evil = tmp.path().join("evil.zip");
    std::fs::write(&evil, zip_of(&[("x/SKILL.md", "---\nname: x\ndescription: X\n---\n"), ("../../outside.txt", "gotcha")])).unwrap();
    assert!(s.install_path(&evil, &InstallOptions::default()).is_err());
    assert!(!s.store_path("x").exists() && !tmp.path().join("outside.txt").exists());
}

#[test]
fn update_from_a_folder_backs_up_first() {
    let (tmp, s) = setup();
    let src = tmp.path().join("src/pdf");
    write_skill(&src, "pdf", "v1");
    s.install_path(&src, &InstallOptions { apps: Some(vec![]), ..Default::default() }).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    assert!(rt.block_on(s.update(&[], false)).unwrap().lines.iter().any(|l| l == "pdf: up to date"));
    write_skill(&src, "pdf", "v2");
    std::fs::write(s.store_path("pdf").join("mine.txt"), "local edit").unwrap();
    assert!(s.list().unwrap().skills[0].modified);
    let r = rt.block_on(s.update(&["pdf".into()], false)).unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("local changes")), "{r:?}");
    assert_eq!(s.list().unwrap().skills[0].description, "v2");
    let backup = &backups_named(&s, "pdf-")[0];
    assert!(backup.join("mine.txt").is_file(), "the local edit is kept in the backup");
}

#[test]
fn github_installs_and_update_hints_from_the_cache() {
    let (_tmp, s) = setup();
    let spec = RepoSpec::parse("github:acme/skills/skills").unwrap();
    let put = |commit: &str, body: &str| {
        let bytes = zip_of(&[("skills-x/skills/pdf/SKILL.md", &format!("---\nname: pdf\ndescription: {body}\n---\n")), ("skills-x/README.md", "r")]);
        let tree = Tree::from_zip(&bytes, true, SOURCE_LIMITS).unwrap();
        let archive = format!("acme__skills__{commit}.zip");
        let dir = s.state_dir.join(github::CACHE_DIR);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(&archive), &bytes).unwrap();
        let mut cache = Cache::default();
        cache.repos.insert(spec.repo_key(), CachedRepo { commit: commit.into(), fetched_at: owo_client_apps::managed::now_unix(), archive, skills: github::scan(&tree) });
        github::save_cache(&s.state_dir, &cache).unwrap();
    };
    put(&"a".repeat(40), "v1");
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(s.install_github(&spec, &InstallOptions::default())).unwrap();
    let pdf = &s.list().unwrap().skills[0];
    assert_eq!(pdf.source.as_ref().unwrap().label, "github:acme/skills/skills/pdf@aaaaaaa");
    assert!(!pdf.update.as_ref().unwrap().available);

    s.add_repo(spec.clone()).unwrap();
    let found = &s.discover_cached().unwrap().into_iter().find(|r| r.repo == "acme/skills/skills").unwrap().skills[0];
    assert!(found.installed && !found.update_available);
    assert_eq!(found.install, "github:acme/skills/skills/pdf");

    put(&"b".repeat(40), "v2");
    assert!(s.list().unwrap().skills[0].update.as_ref().unwrap().available);
    let found = &s.discover_cached().unwrap().into_iter().find(|r| r.repo == "acme/skills/skills").unwrap().skills[0];
    assert!(found.update_available);
}

#[test]
fn authoring_round_trip() {
    let (_tmp, s) = setup();
    let body = author::template("notes");
    s.create("notes", &body, Some(&[AppId::Codex]), &[]).unwrap();
    let file = s.store_path("notes").join(SKILL_FILE);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), body);
    let read = s.read_skill("notes", None).unwrap();
    assert_eq!((read.content.as_str(), read.source, read.editable), (body.as_str(), "authored", true));
    assert!(s.create("notes", &body, None, &[]).is_err(), "names are unique");

    std::fs::write(s.store_path("notes").join("extra.md"), "kept").unwrap();
    let edited = body.replace("What this skill does", "Take meeting notes");
    let r = s.edit("notes", &edited).unwrap();
    assert!(r.lines.iter().any(|l| l.starts_with("backup:")), "{r:?}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), edited);
    assert_eq!(std::fs::read_to_string(s.store_path("notes").join("extra.md")).unwrap(), "kept", "only SKILL.md is written");
    let view = s.list().unwrap().skills.into_iter().find(|v| v.name == "notes").unwrap();
    assert!(!view.modified, "an in-app edit is not a local change");
    assert!(view.description.starts_with("Take meeting notes"));
    let backup = &backups_named(&s, "notes-")[0];
    assert_eq!(std::fs::read_to_string(backup.join(SKILL_FILE)).unwrap(), body);
    assert!(s.edit("notes", &edited).unwrap().lines.iter().any(|l| l.ends_with("unchanged")));
}

fn w(path: &str, content: &str) -> author::Change {
    author::Change::Write { path: path.into(), content: content.into() }
}

fn del(path: &str) -> author::Change {
    author::Change::Delete { path: path.into() }
}

#[test]
fn paths_stay_inside_the_skill() {
    for bad in ["../x.md", "a/../../x.md", "/etc/x.md", "C:/x.md", "C:\\x.md", "con.md", "a/aux/b.md", "x:y.md", "", "."] {
        assert!(author::rel_path(bad).is_err(), "{bad:?}");
    }
    assert_eq!(author::rel_path("references\\api.md").unwrap(), "references/api.md");
    let (_tmp, s) = setup();
    s.create("kit", &author::template("kit"), Some(&[]), &[]).unwrap();
    for change in [w("notes.txt", "x"), w("../evil.md", "x"), del("SKILL.md"), del("skill.md"), author::Change::Rename { from: "SKILL.md".into(), to: "a.md".into() }] {
        assert!(s.apply("kit", std::slice::from_ref(&change)).is_err(), "{change:?}");
    }
    std::fs::write(s.store_path("kit").join("data.json"), "{}").unwrap();
    assert!(s.apply("kit", &[del("data.json")]).is_err(), "other files are the file manager's");
    assert!(s.read_skill("kit", Some("data.json")).is_err());
}

#[test]
fn links_are_never_followed() {
    let (tmp, s) = setup();
    s.create("kit", &author::template("kit"), Some(&[]), &[]).unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.md"), "secret").unwrap();
    let link = s.store_path("kit").join("elsewhere");
    fsops::link_dir(&outside, &link).unwrap();
    assert!(s.read_skill("kit", Some("elsewhere/secret.md")).is_err());
    assert!(s.apply("kit", &[w("elsewhere/new.md", "x")]).is_err());
    assert!(!outside.join("new.md").exists());
    // Deleting the link removes the link itself, not what it points at.
    s.apply("kit", &[del("elsewhere")]).unwrap();
    assert!(!fsops::exists(&link) && outside.join("secret.md").is_file());
}

#[test]
fn multi_file_changes() {
    let (tmp, s) = setup();
    let body = author::template("kit");
    s.create("kit", &body, Some(&[]), &[author::Change::Mkdir { path: "scripts".into() }, w("references/api.md", "# API\n")]).unwrap();
    let dir = s.store_path("kit");
    assert!(dir.join("scripts").is_dir() && std::fs::read_dir(dir.join("scripts")).unwrap().count() == 0, "an empty folder, no placeholder");
    assert_eq!(std::fs::read_to_string(dir.join("references/api.md")).unwrap(), "# API\n");
    assert_eq!(s.read_skill("kit", Some("references/api.md")).unwrap().content, "# API\n");

    let edited = body.replace("What this skill does", "Build kits");
    let r = s
        .apply("kit", &[w("SKILL.md", &edited), author::Change::Rename { from: "references/api.md".into(), to: "references/http.md".into() }, w("guides/start.md", "go\n"), del("scripts")])
        .unwrap();
    assert!(r.lines.iter().any(|l| l.starts_with("backup:")), "{r:?}");
    assert!(!dir.join("references/api.md").exists() && dir.join("references/http.md").is_file() && dir.join("guides/start.md").is_file() && !dir.join("scripts").exists());
    let view = s.list().unwrap().skills.into_iter().find(|v| v.name == "kit").unwrap();
    assert!(!view.modified && view.description.starts_with("Build kits"), "the hash follows in-app edits");
    let backup = &backups_named(&s, "kit-")[0];
    assert!(backup.join("references/api.md").is_file(), "the files before the save are in the backup");
    assert!(s.apply("kit", &[w("SKILL.md", &edited)]).unwrap().lines.iter().any(|l| l.ends_with("unchanged")));

    // A folder goes with everything in it, links as links; the backup keeps the rest.
    let assets = dir.join("assets");
    std::fs::create_dir_all(assets.join("img")).unwrap();
    std::fs::write(assets.join("img/logo.png"), "png").unwrap();
    std::fs::write(assets.join("data.csv"), "a,b").unwrap();
    let target = tmp.path().join("shared");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join("keep.txt"), "keep").unwrap();
    fsops::link_dir(&target, &assets.join("shared")).unwrap();
    s.apply("kit", &[del("assets")]).unwrap();
    assert!(!assets.exists());
    assert_eq!(std::fs::read_to_string(target.join("keep.txt")).unwrap(), "keep", "a junction's target is untouched");
    assert!(backups_named(&s, "kit-").iter().any(|b| b.join("assets/img/logo.png").is_file()));
    assert!(s.apply("kit", &[del("")]).is_err() && s.apply("kit", &[del(".")]).is_err(), "the root is not deletable");
    assert!(dir.join(SKILL_FILE).is_file());
}

#[test]
fn authored_tree_and_sync() {
    let (tmp, s) = setup();
    s.create("kit", &author::template("kit"), Some(&[]), &[author::Change::Mkdir { path: "scripts".into() }]).unwrap();
    let dir = s.store_path("kit");

    // The user adds files in the file manager, and a link pointing outside the skill.
    std::fs::write(dir.join("scripts/run.py"), "print(1)\n").unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("secret.txt"), "x").unwrap();
    fsops::link_dir(&outside, &dir.join("elsewhere")).unwrap();
    let tree = s.tree("kit").unwrap();
    let paths: Vec<(&str, bool, bool)> = tree.entries.iter().map(|e| (e.path.as_str(), e.dir, e.link)).collect();
    assert_eq!(paths, [("elsewhere", true, true), ("scripts", true, false), ("scripts/run.py", false, false), ("SKILL.md", false, false)]);
    assert_eq!(tree.entries[2].size, 9);
    fsops::unlink_dir(&dir.join("elsewhere")).unwrap();

    let view = s.list().unwrap().skills.into_iter().find(|v| v.name == "kit").unwrap();
    assert!(!view.modified, "files the user adds to a hand-written skill are not a warning");

    // A copy an app got follows after a sync.
    let copy = s.layout.dir(Location::Claude).join("kit");
    let mut st = state::load(&s.state_dir).unwrap();
    st.skills.get_mut("kit").unwrap().links.push(state::Link { app: AppId::Claude, path: copy.clone(), kind: fsops::LinkKind::Copy });
    state::save(&s.state_dir, &st).unwrap();
    std::fs::create_dir_all(&copy).unwrap();
    let r = s.sync(&["kit".into()]).unwrap();
    assert!(r.lines.iter().any(|l| l == "kit: synced"), "{r:?}");
    assert!(copy.join("scripts/run.py").is_file());
    assert!(s.sync(&[]).unwrap().lines.is_empty(), "nothing changed since");
    assert!(s.sync(&["nope".into()]).is_err());

    std::fs::create_dir_all(dir.join("assets")).unwrap();
    for i in 0..author::TREE_MAX + 10 {
        std::fs::write(dir.join(format!("assets/{i:03}.txt")), "a").unwrap();
    }
    let big = s.tree("kit").unwrap();
    assert_eq!((big.entries.len(), big.entries.len() + big.more), (author::TREE_MAX, author::TREE_MAX + 14));
}

#[test]
fn authoring_validation() {
    let (_tmp, s) = setup();
    for (name, text) in [
        ("Bad_Name", "---\nname: Bad_Name\ndescription: d\n---\n"),
        ("good", "---\nname: other\ndescription: d\n---\n"),
        ("good", "---\ndescription: d\n---\n"),
        ("good", "---\nname: good\n---\n"),
        ("good", "no frontmatter"),
    ] {
        assert!(s.create(name, text, None, &[]).is_err(), "{name}: {text:?}");
    }
    assert!(!s.store_path("good").exists());
    let theirs = s.store_path("theirs");
    write_skill(&theirs, "theirs", "x");
    assert!(s.edit("theirs", "---\nname: theirs\ndescription: y\n---\n").is_err(), "unmanaged skills are adopted first");
    assert!(s.read_skill("theirs", None).is_err());
    assert!(s.create("theirs", &author::template("theirs"), None, &[]).is_err(), "never overwrites a folder");
}

#[test]
fn editing_refreshes_copies_and_detaches_from_the_source() {
    let (_tmp, s) = setup();
    let spec = RepoSpec::parse("github:acme/skills/skills").unwrap();
    let bytes = zip_of(&[("r/skills/pdf/SKILL.md", "---\nname: pdf\ndescription: v1\n---\n")]);
    let tree = Tree::from_zip(&bytes, true, SOURCE_LIMITS).unwrap();
    let dir = s.state_dir.join(github::CACHE_DIR);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("a.zip"), &bytes).unwrap();
    let mut cache = Cache::default();
    cache.repos.insert(spec.repo_key(), CachedRepo { commit: "c".repeat(40), fetched_at: owo_client_apps::managed::now_unix(), archive: "a.zip".into(), skills: github::scan(&tree) });
    github::save_cache(&s.state_dir, &cache).unwrap();
    let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
    rt.block_on(s.install_github(&spec, &InstallOptions { apps: Some(vec![]), ..Default::default() })).unwrap();

    // Pretend Claude Code got a copy instead of a link.
    let copy = s.layout.dir(Location::Claude).join("pdf");
    tree.write("skills/pdf", &copy).unwrap();
    let mut st = state::load(&s.state_dir).unwrap();
    st.skills.get_mut("pdf").unwrap().links.push(state::Link { app: AppId::Claude, path: copy.clone(), kind: fsops::LinkKind::Copy });
    state::save(&s.state_dir, &st).unwrap();

    let r = s.edit("pdf", "---\nname: pdf\ndescription: v2 mine\n---\n").unwrap();
    assert!(r.warnings.iter().any(|w| w.contains("hand-written")), "{r:?}");
    assert!(std::fs::read_to_string(copy.join(SKILL_FILE)).unwrap().contains("v2 mine"), "the copy follows the edit");
    let view = &s.list().unwrap().skills[0];
    assert_eq!(view.source.as_ref().unwrap().kind, "authored");
    assert!(view.source.as_ref().unwrap().label.contains("github:acme/skills/skills/pdf"));
    assert!(view.update.is_none() && !view.modified);
    assert!(rt.block_on(s.update(&["pdf".into()], false)).unwrap().lines.iter().any(|l| l.contains("no source to update from")), "an update never overwrites the edit");
}

#[test]
fn repo_list_edits() {
    let (_tmp, s) = setup();
    assert_eq!(s.repos().unwrap(), state::default_repos());
    let first = s.repos().unwrap()[0].clone();
    s.remove_repo(&first).unwrap();
    assert!(!s.repos().unwrap().contains(&first));
    assert!(s.remove_repo(&first).is_err());
    s.add_repo(RepoSpec::parse("me/mine").unwrap()).unwrap();
    assert!(s.add_repo(RepoSpec::parse("ME/mine").unwrap()).is_err());
    s.reset_repos().unwrap();
    assert_eq!(s.repos().unwrap(), state::default_repos());
}
