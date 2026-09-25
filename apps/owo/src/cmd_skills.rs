//! `owo skills`: Agent Skills kept once in `~/.agents/skills` and turned on or off per app
//! (see `owo-skills`).

use anyhow::{bail, Context, Result};
use clap::{Args, Subcommand};
use owo_skills::author::Change;
use owo_skills::{AppId, AppStateKind, InstallOptions, InstallSource, Listing, RepoSpec, Report, SkillView, Skills, Support};

use crate::cli::GlobalArgs;
use crate::{context, ui};

#[derive(Subcommand)]
pub enum SkillsCommand {
    /// Installed skills, other skills found on this machine, and each app's state (the default).
    List {
        /// Print as JSON (apps, managed and unmanaged skills).
        #[arg(long)]
        json: bool,
    },
    /// Install from a skill folder, a folder of skills, a .zip, or GitHub.
    Install(InstallArgs),
    /// Remove a skill OwO AI Gateway manages; its folder is moved to the backups.
    Remove { name: String },
    /// Turn a skill on for apps.
    Enable(ToggleArgs),
    /// Turn a skill off for apps.
    Disable(ToggleArgs),
    /// Take over a skill that is already in ~/.agents/skills (nothing on disk changes).
    Adopt { name: String },
    /// Write a new skill: its SKILL.md from a file, stdin, or a starter template, plus the
    /// Markdown files and folders of an --apply change list.
    New {
        name: String,
        #[command(flatten)]
        content: ContentArgs,
        /// Apps to turn it on for, comma-separated (default: on everywhere).
        #[arg(long, value_delimiter = ',', value_name = "APP")]
        app: Option<Vec<String>>,
    },
    /// Record the files of hand-written skills after changing them outside OwO AI Gateway,
    /// and refresh the copies apps got (default: every hand-written skill).
    Sync { names: Vec<String> },
    /// Change a managed skill's Markdown files and folders (backed up first; a skill from
    /// GitHub, a folder, or a zip becomes hand-written and is no longer updated).
    Edit {
        name: String,
        #[command(flatten)]
        content: ContentArgs,
    },
    /// Print a Markdown file of a managed skill (SKILL.md unless --path).
    Show {
        name: String,
        /// The file, relative to the skill folder.
        #[arg(long, value_name = "REL")]
        path: Option<String>,
        /// Print as JSON (with whether it can be edited).
        #[arg(long)]
        json: bool,
    },
    /// A managed skill's files and folders (links are listed, never followed).
    Files {
        name: String,
        #[arg(long)]
        json: bool,
    },
    /// Skills in GitHub repositories: the configured ones, or --repo.
    Discover {
        /// A repository to look in instead, `owner/repo[/subdir][@ref]` (repeatable).
        #[arg(long, value_name = "REPO")]
        repo: Vec<String>,
        /// Ask GitHub again even when the last listing is recent.
        #[arg(long)]
        refresh: bool,
        #[arg(long)]
        json: bool,
    },
    /// The GitHub repositories `discover` looks in.
    Repos {
        #[command(subcommand)]
        command: Option<ReposCommand>,
    },
    /// Refresh skills from their sources; the current files are backed up first.
    Update {
        /// Skills to update.
        names: Vec<String>,
        /// Every managed skill.
        #[arg(long, conflicts_with = "names")]
        all: bool,
        /// Only report what would change.
        #[arg(long)]
        check: bool,
    },
    /// How each app loads skills, and whether it is installed.
    Apps {
        #[arg(long)]
        json: bool,
    },
}

#[derive(Args)]
pub struct InstallArgs {
    /// A folder, a .zip, or `github:owner/repo[/subdir][@ref]` (a github.com URL works too).
    pub source: String,
    /// Apps to turn the skill on for, comma-separated; the others are turned off where the app
    /// allows it (`none`: off everywhere possible). Default: on everywhere.
    #[arg(long, value_delimiter = ',', value_name = "APP")]
    pub app: Option<Vec<String>>,
    /// The skill to take when the source holds several (repeatable).
    #[arg(long = "skill", value_name = "NAME")]
    pub skill: Vec<String>,
    /// Take every skill in the source.
    #[arg(long, conflicts_with = "skill")]
    pub all: bool,
    /// Replace a skill OwO AI Gateway already manages (its files are backed up first).
    #[arg(long, short)]
    pub force: bool,
}

#[derive(Args)]
pub struct ContentArgs {
    /// Read SKILL.md from this file.
    #[arg(long, value_name = "PATH", conflicts_with = "stdin")]
    pub file: Option<std::path::PathBuf>,
    /// Read SKILL.md from standard input.
    #[arg(long)]
    pub stdin: bool,
    /// Apply a JSON change list: `{"changes": [{"op": "write", "path": "references/api.md",
    /// "file": "<file with the content>"}, {"op": "mkdir"|"delete", "path": …},
    /// {"op": "rename", "from": …, "to": …}]}`. Only Markdown files are written, renamed, or
    /// deleted one by one; deleting a folder removes everything in it.
    #[arg(long, value_name = "JSON")]
    pub apply: Option<std::path::PathBuf>,
}

impl ContentArgs {
    fn read(&self) -> Result<Option<String>> {
        if let Some(path) = &self.file {
            return Ok(Some(std::fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?));
        }
        if self.stdin {
            let mut text = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut text).context("cannot read standard input")?;
            return Ok(Some(text));
        }
        Ok(None)
    }

    /// The --apply list, with a SKILL.md from --file / --stdin first.
    fn changes(&self) -> Result<Vec<Change>> {
        let mut changes = Vec::new();
        if let Some(text) = self.read()? {
            changes.push(Change::Write { path: "SKILL.md".into(), content: text });
        }
        if let Some(path) = &self.apply {
            changes.extend(owo_skills::author::read_manifest(path)?);
        }
        Ok(changes)
    }
}

#[derive(Args)]
pub struct ToggleArgs {
    pub name: String,
    /// Apps, comma-separated (`all`: every app with a switch).
    #[arg(long, value_delimiter = ',', value_name = "APP", required = true)]
    pub app: Vec<String>,
}

#[derive(Subcommand)]
pub enum ReposCommand {
    /// The repositories (the default).
    List {
        #[arg(long)]
        json: bool,
    },
    /// Add `owner/repo[/subdir][@ref]`.
    Add { repo: String },
    /// Remove one (built-in ones too).
    Remove { repo: String },
    /// Go back to the built-in list.
    Reset,
}

fn skills(global: &GlobalArgs) -> Result<Skills> {
    let paths = context::paths(global)?;
    paths.ensure_dirs()?;
    Skills::from_env(&paths.state, &paths.backups)
}

fn apps_arg(list: &[String]) -> Result<Vec<AppId>> {
    let mut out = Vec::new();
    for a in list {
        match a.trim() {
            "none" => {}
            "all" => out.extend(AppId::ALL.into_iter().filter(|a| matches!(a.support(), Support::Switch | Support::Link))),
            name => {
                let Some(app) = AppId::parse(name) else {
                    let known: Vec<&str> = AppId::ALL.iter().map(|a| a.name()).collect();
                    bail!("unknown app `{name}` (apps: {})", known.join(", "));
                };
                out.push(app);
            }
        }
    }
    out.dedup();
    Ok(out)
}

fn print(report: &Report) {
    for line in &report.lines {
        println!("{line}");
    }
    for w in &report.warnings {
        eprintln!("warning: {w}");
    }
}

pub fn run(global: &GlobalArgs, command: Option<SkillsCommand>) -> Result<()> {
    let s = skills(global)?;
    match command.unwrap_or(SkillsCommand::List { json: false }) {
        SkillsCommand::List { json: true } => println!("{}", serde_json::to_string_pretty(&s.list()?)?),
        SkillsCommand::List { json: false } => list(&s.list()?),
        SkillsCommand::Install(args) => {
            let source = InstallSource::parse(&args.source)?;
            let opts = InstallOptions { apps: args.app.as_deref().map(apps_arg).transpose()?, only: args.skill, all: args.all, force: args.force };
            print(&context::runtime()?.block_on(s.install(&source, &opts))?);
        }
        SkillsCommand::Remove { name } => print(&s.remove(&name)?),
        SkillsCommand::Enable(args) => toggle(&s, &args, true)?,
        SkillsCommand::Disable(args) => toggle(&s, &args, false)?,
        SkillsCommand::Adopt { name } => print(&s.adopt(&name)?),
        SkillsCommand::New { name, content, app } => {
            let mut changes = content.changes()?;
            // The entry point comes from the change list when it has one, else the template.
            let entry = changes.iter().position(|c| matches!(c, Change::Write { path, .. } if path.eq_ignore_ascii_case("SKILL.md")));
            let text = match entry.map(|i| changes.remove(i)) {
                Some(Change::Write { content, .. }) => content,
                _ => owo_skills::author::template(&name),
            };
            let apps = app.as_deref().map(apps_arg).transpose()?;
            print(&s.create(&name, &text, apps.as_deref(), &changes)?);
        }
        SkillsCommand::Sync { names } => print(&s.sync(&names)?),
        SkillsCommand::Edit { name, content } => {
            let changes = content.changes()?;
            if changes.is_empty() {
                bail!("give the new SKILL.md with --file PATH or --stdin, or a change list with --apply JSON");
            }
            print(&s.apply(&name, &changes)?);
        }
        SkillsCommand::Show { name, path, json: true } => println!("{}", serde_json::to_string_pretty(&s.read_skill(&name, path.as_deref())?)?),
        SkillsCommand::Show { name, path, json: false } => print!("{}", s.read_skill(&name, path.as_deref())?.content),
        SkillsCommand::Files { name, json: true } => println!("{}", serde_json::to_string_pretty(&s.tree(&name)?)?),
        SkillsCommand::Files { name, json: false } => {
            let tree = s.tree(&name)?;
            for e in &tree.entries {
                let mark = if e.link { " -> (link)" } else if e.dir { "/" } else { "" };
                println!("{}{}{mark}", "  ".repeat(e.depth), e.path.rsplit('/').next().unwrap_or(&e.path));
            }
            if tree.more > 0 {
                println!("... and {} more", tree.more);
            }
        }
        SkillsCommand::Discover { repo, refresh, json } => {
            let specs = if repo.is_empty() { None } else { Some(repo.iter().map(|r| RepoSpec::parse(r)).collect::<Result<Vec<_>>>()?) };
            let max_age = if refresh { 0 } else { owo_skills::github::FRESH_SECS };
            let repos = context::runtime()?.block_on(s.discover(specs, max_age))?;
            if json {
                println!("{}", serde_json::to_string_pretty(&repos)?);
            } else {
                discovered(&repos);
            }
        }
        SkillsCommand::Repos { command } => match command.unwrap_or(ReposCommand::List { json: false }) {
            ReposCommand::List { json: true } => println!("{}", serde_json::to_string_pretty(&s.repos()?)?),
            ReposCommand::List { json: false } => {
                for r in s.repos()? {
                    println!("{r}");
                }
            }
            ReposCommand::Add { repo } => print(&s.add_repo(RepoSpec::parse(&repo)?)?),
            ReposCommand::Remove { repo } => print(&s.remove_repo(&RepoSpec::parse(&repo)?)?),
            ReposCommand::Reset => print(&s.reset_repos()?),
        },
        SkillsCommand::Update { names, all, check } => {
            if names.is_empty() && !all && !check {
                bail!("name the skills to update, or pass --all (--check only reports)");
            }
            print(&context::runtime()?.block_on(s.update(&names, check))?);
        }
        SkillsCommand::Apps { json: true } => println!("{}", serde_json::to_string_pretty(&s.apps())?),
        SkillsCommand::Apps { json: false } => {
            for a in s.apps() {
                let installed = if a.detected { "" } else { " (not installed)" };
                println!("{:<16} {}{installed}", a.id.name(), a.note);
            }
        }
    }
    Ok(())
}

fn toggle(s: &Skills, args: &ToggleArgs, on: bool) -> Result<()> {
    let apps = apps_arg(&args.app)?;
    if apps.is_empty() {
        bail!("name at least one app with --app");
    }
    let mut failed = 0;
    for app in apps {
        match s.set_enabled(&args.name, app, on) {
            Ok(r) if r.lines.is_empty() && r.warnings.is_empty() => println!("{}: already {}", app.title(), if on { "on" } else { "off" }),
            Ok(r) => print(&r),
            Err(e) => {
                eprintln!("error: {}: {e:#}", app.title());
                failed += 1;
            }
        }
    }
    if failed > 0 {
        bail!("{failed} app(s) could not be changed");
    }
    Ok(())
}

fn app_word(state: AppStateKind) -> Option<&'static str> {
    Some(match state {
        AppStateKind::On | AppStateKind::AlwaysOn | AppStateKind::OnByUser | AppStateKind::InFolder => "on",
        AppStateKind::Linked => "linked",
        AppStateKind::Foreign => "own copy",
        AppStateKind::OffByOwo => "off",
        AppStateKind::OffByUser => "off (your setting)",
        AppStateKind::NotLinked => "off",
        AppStateKind::Error => "unreadable settings",
        AppStateKind::Unsupported => return None,
    })
}

fn apps_line(v: &SkillView) -> String {
    v.apps.iter().filter_map(|a| app_word(a.state).map(|w| format!("{} {w}", a.app.name()))).collect::<Vec<_>>().join(" · ")
}

fn one_line(text: &str, max: usize) -> String {
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max { flat } else { format!("{}...", flat.chars().take(max - 3).collect::<String>()) }
}

fn list(l: &Listing) {
    println!("Skills folder: {}\n", ui::tilde(&l.store));
    if l.skills.is_empty() {
        println!("No skills installed through OwO AI Gateway yet.");
        println!("  owo skills discover                   skills in well-known GitHub repositories");
        println!("  owo skills install <folder|zip|github:owner/repo/path>");
    }
    for v in &l.skills {
        let mut flags = Vec::new();
        if v.update.as_ref().is_some_and(|u| u.available) {
            flags.push("update available");
        }
        if v.modified {
            flags.push("changed locally");
        }
        let flags = if flags.is_empty() { String::new() } else { format!("  [{}]", flags.join(", ")) };
        println!("{}  {}{flags}", v.name, v.source.as_ref().map(|s| s.label.as_str()).unwrap_or(""));
        if !v.description.is_empty() {
            println!("  {}", one_line(&v.description, 100));
        }
        println!("  {}", apps_line(v));
        for w in &v.warnings {
            println!("  warning: {w}");
        }
    }
    if !l.unmanaged.is_empty() {
        println!("\nOther skills on this machine (OwO AI Gateway leaves them alone; `owo skills adopt <name>` takes over one in the skills folder):");
        for v in &l.unmanaged {
            let hint = if v.can_adopt { "  (can adopt)" } else { "" };
            println!("  {:<28} {}{hint}", v.name, ui::tilde(&v.path));
        }
    }
}

fn discovered(repos: &[owo_skills::RepoView]) {
    for r in repos {
        let commit = r.commit.as_deref().map(|c| format!(" @{}", &c[..c.len().min(7)])).unwrap_or_default();
        println!("{}{commit}", r.repo);
        if let Some(e) = &r.error {
            println!("  error: {e}");
        }
        for s in &r.skills {
            let state = if s.update_available {
                "  [update available]"
            } else if s.installed {
                "  [installed]"
            } else if s.conflict {
                "  [name taken]"
            } else if s.problem.is_some() {
                "  [cannot install]"
            } else {
                ""
            };
            println!("  {:<32} {}{state}", s.name, one_line(&s.description, 80));
        }
        println!();
    }
    println!("Install one with:  owo skills install <github:owner/repo/path>   (see --json for the exact sources)");
}
