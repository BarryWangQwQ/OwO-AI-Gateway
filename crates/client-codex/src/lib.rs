//! Codex integration (spec §19).
//!
//! Two independent integrations share one Codex home:
//!
//! - `codex` (CLI): OwO AI Gateway owns one file, `<codex_home>/owo.config.toml`, a Codex profile
//!   layer selected with `codex -p owo`. The user's `config.toml` is not touched, so
//!   regular Codex sessions and their history are unaffected.
//! - `codex-desktop`: Codex Desktop has no profile switch, so OwO AI Gateway sets
//!   `model_provider`/`model`/`model_catalog_json` and `[model_providers.owo]` in
//!   `config.toml`, after backing the file up and recording exactly what it changed.
//!
//! Connecting or disconnecting one never touches the other's files.
//!
//! The ChatGPT login (`auth.json`) is never read or modified.

pub mod catalog;
pub mod detect;
mod desktop;
mod files;
pub mod profile;
pub mod state;

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde_json::Value;

pub use catalog::{catalog_models, CatalogModel};
pub use profile::ProviderSettings;
pub use state::{CliEdit, CodexState, DesktopEdit};

/// Codex CLI's client id: its aliases (`models[].aliases.codex`) and the `/c/codex/v1` route.
pub const CLIENT_ID: &str = "codex";
/// Codex Desktop's client id (`aliases.codex-desktop`, `/c/codex_desktop/v1`).
pub const DESKTOP_CLIENT_ID: &str = "codex_desktop";
/// Codex provider id and profile name.
pub const PROVIDER_ID: &str = "owo";

pub struct Environment {
    pub state_dir: PathBuf,
    pub backups_dir: PathBuf,
    pub codex_home: PathBuf,
    pub codex_bin: Option<PathBuf>,
}

impl Environment {
    fn codex_state_dir(&self) -> PathBuf {
        self.state_dir.join("codex")
    }

    /// Each surface has its own catalog file: Desktop's may carry native aliases.
    pub fn catalog_path(&self, surface: Surface) -> PathBuf {
        self.codex_state_dir().join(match surface {
            Surface::Cli => "models.json",
            Surface::Desktop => "desktop-models.json",
        })
    }

    fn template_path(&self) -> PathBuf {
        self.codex_state_dir().join("template.json")
    }

    pub fn profile_path(&self) -> PathBuf {
        self.codex_home.join(format!("{PROVIDER_ID}.config.toml"))
    }

    pub fn config_path(&self) -> PathBuf {
        self.codex_home.join("config.toml")
    }

    fn backups(&self) -> PathBuf {
        self.backups_dir.join("codex")
    }
}

/// Which Codex surface to connect: two independent integrations of the same Codex home.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    /// The CLI profile layer `<codex_home>/owo.config.toml` (`codex -p owo`).
    Cli,
    /// `config.toml` root keys and `[model_providers.owo]`, which Codex Desktop reads.
    Desktop,
}

impl Surface {
    pub fn app(self) -> &'static str {
        match self {
            Surface::Cli => "codex",
            Surface::Desktop => "codex-desktop",
        }
    }

    pub fn client_id(self) -> &'static str {
        match self {
            Surface::Cli => CLIENT_ID,
            Surface::Desktop => DESKTOP_CLIENT_ID,
        }
    }
}

pub struct EnableRequest {
    pub provider: ProviderSettings,
    /// Slug Codex starts with; must be one of `models`.
    pub model: String,
    pub models: Vec<CatalogModel>,
    pub surface: Surface,
    pub force: bool,
    /// Desktop only: publish OwO AI Gateway models under native GPT slugs, for a signed-out Codex
    /// Desktop whose picker only lists slugs on OpenAI's native allowlist.
    pub native_aliases: bool,
}
#[derive(Debug, Default)]
pub struct Report {
    pub lines: Vec<String>,
    pub warnings: Vec<String>,
}

impl Report {
    fn line(&mut self, s: impl Into<String>) {
        self.lines.push(s.into());
    }

    fn warn(&mut self, s: impl Into<String>) {
        self.warnings.push(s.into());
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct StoredTemplate {
    source: String,
    entry: Value,
    /// Picker-visible native slugs of the same Codex build.
    #[serde(default)]
    native_slugs: Vec<String>,
}

struct Template {
    entry: Value,
    source: String,
    native_slugs: Vec<String>,
}

/// The catalog template captured at enable time, used by the gateway to answer
/// `GET /models?client_version=` with the same entries as the file on disk.
pub fn stored_template(state_dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(state_dir.join("codex").join("template.json")).ok()?;
    serde_json::from_str::<StoredTemplate>(&text).ok().map(|t| t.entry)
}

fn aliases_path(state_dir: &Path) -> PathBuf {
    state_dir.join("codex").join("native-aliases.json")
}

/// Native slugs lent to OwO AI Gateway models (empty unless enabled with native aliases).
pub fn stored_native_aliases(state_dir: &Path) -> Vec<catalog::NativeAlias> {
    std::fs::read_to_string(aliases_path(state_dir))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

fn resolve_template(env: &Environment, report: &mut Report) -> Result<Template> {
    if let Some(bin) = &env.codex_bin {
        let attempt = (|| -> Result<Template> {
            let bundled = detect::bundled_catalog(bin, &env.codex_home)?;
            let entry = catalog::pick_template(&bundled).context("the bundled Codex catalog has no models")?;
            let source = detect::codex_version(bin).unwrap_or_else(|_| "codex (unknown version)".into());
            Ok(Template { entry, source, native_slugs: catalog::native_slugs(&bundled) })
        })();
        match attempt {
            Ok(t) => {
                let stored = StoredTemplate { source: t.source.clone(), entry: t.entry.clone(), native_slugs: t.native_slugs.clone() };
                files::write_atomic(&env.template_path(), serde_json::to_string_pretty(&stored)?.as_bytes())?;
                return Ok(t);
            }
            Err(e) => report.warn(format!("could not read the catalog template from {}: {e:#}", bin.display())),
        }
    } else {
        report.warn("no Codex executable found (pass --codex-bin); using OwO AI Gateway's built-in catalog template");
    }
    if let Ok(text) = std::fs::read_to_string(env.template_path()) {
        if let Ok(stored) = serde_json::from_str::<StoredTemplate>(&text) {
            return Ok(Template {
                entry: stored.entry,
                source: format!("{} (cached)", stored.source),
                native_slugs: stored.native_slugs,
            });
        }
    }
    Ok(Template { entry: catalog::builtin_template(), source: "builtin".into(), native_slugs: Vec::new() })
}

fn load_home_state(env: &Environment) -> Result<Option<CodexState>> {
    let prior = state::load(&env.state_dir)?;
    if let Some(p) = &prior {
        if p.codex_home != env.codex_home {
            bail!(
                "OwO AI Gateway is already connected to Codex at {}; disconnect it (`owo disconnect codex` / `owo disconnect codex-desktop`) before using another Codex home",
                p.codex_home.display()
            );
        }
    }
    Ok(prior)
}

fn save_or_clear(env: &Environment, cli: Option<state::CliEdit>, desktop: Option<state::DesktopEdit>, template_source: String) -> Result<()> {
    if cli.is_none() && desktop.is_none() {
        return state::clear(&env.state_dir);
    }
    state::save(
        &env.state_dir,
        &CodexState {
            codex_home: env.codex_home.clone(),
            owo_version: env!("CARGO_PKG_VERSION").to_string(),
            updated_at_unix: files::now_unix(),
            template_source,
            cli,
            desktop,
        },
    )
}

pub fn enable(env: &Environment, req: &EnableRequest) -> Result<Report> {
    let mut report = Report::default();
    if req.models.is_empty() {
        bail!("no models are available to expose to Codex; add models to OwO AI Gateway's config.toml first");
    }
    if !req.models.iter().any(|m| m.slug == req.model) {
        let known: Vec<_> = req.models.iter().map(|m| m.slug.as_str()).collect();
        bail!("model `{}` is not available (available: {})", req.model, known.join(", "));
    }
    if !env.codex_home.is_dir() {
        bail!("Codex home {} does not exist (run Codex once, or set CODEX_HOME)", env.codex_home.display());
    }
    if req.native_aliases && req.surface == Surface::Cli {
        bail!("native aliases only apply to Codex Desktop (`owo connect codex-desktop`)");
    }
    let prior = load_home_state(env)?;

    // 1. Catalog for this surface (Desktop's may lend native slugs to OwO AI Gateway models).
    let template = resolve_template(env, &mut report)?;
    let template_source = template.source.clone();
    let aliases = if req.native_aliases {
        if template.native_slugs.is_empty() {
            bail!("native aliases need the model list of an installed Codex build; pass --codex-bin");
        }
        catalog::assign_native_aliases(&template.native_slugs, &req.models)
    } else {
        Vec::new()
    };
    let catalog = if aliases.is_empty() {
        catalog::build_catalog(&template.entry, &req.models)
    } else {
        catalog::build_aliased_catalog(&template.entry, &req.models, &aliases)
    };
    let catalog_path = env.catalog_path(req.surface);
    files::write_atomic(&catalog_path, serde_json::to_string_pretty(&catalog)?.as_bytes())?;
    report.line(format!("catalog:  {} ({} models, template: {template_source})", catalog_path.display(), req.models.len()));
    if req.surface == Surface::Desktop {
        if aliases.is_empty() {
            let _ = std::fs::remove_file(aliases_path(&env.state_dir));
        } else {
            files::write_atomic(&aliases_path(&env.state_dir), serde_json::to_string_pretty(&aliases)?.as_bytes())?;
            for a in &aliases {
                report.line(format!("alias:    {} → {}", a.native, a.model));
            }
            let unplaced = req.models.len().saturating_sub(aliases.len());
            if unplaced > 0 {
                report.warn(format!("{unplaced} model(s) had no free native slot and are hidden from a signed-out Desktop picker"));
            }
        }
    }
    // The slug Codex starts with: the native slot of the requested model, if it has one.
    let start_model = aliases.iter().find(|a| a.model == req.model).map(|a| a.native.clone()).unwrap_or_else(|| req.model.clone());

    let (mut cli, mut desktop) = prior.map(|p| (p.cli, p.desktop)).unwrap_or((None, None));
    match req.surface {
        // 2a. Profile layer (OwO AI Gateway-owned file).
        Surface::Cli => {
            let profile_path = env.profile_path();
            let mut replaced_backup = cli.as_ref().and_then(|c| c.profile.replaced_backup.clone());
            if let Some(current) = files::sha256_file(&profile_path)? {
                let ours = cli.as_ref().is_some_and(|c| c.profile.written_sha256 == current);
                if !ours {
                    if !req.force {
                        bail!("{} exists and was not written by OwO AI Gateway (use --force; the file is backed up first)", profile_path.display());
                    }
                    let b = files::backup(&profile_path, &env.backups(), "owo.config.toml")?;
                    report.line(format!("backup:   {}", b.display()));
                    replaced_backup = Some(b);
                }
            }
            let profile_text = profile::render(&start_model, &catalog_path, &req.provider);
            files::write_atomic(&profile_path, profile_text.as_bytes())?;
            report.line(format!("profile:  {}  (use: codex -p {PROVIDER_ID})", profile_path.display()));
            cli = Some(state::CliEdit {
                base_url: req.provider.base_url.clone(),
                model: req.model.clone(),
                catalog_path: catalog_path.clone(),
                profile: state::OwnedFile { path: profile_path, written_sha256: files::sha256(profile_text.as_bytes()), replaced_backup },
            });
        }
        // 2b. Codex Desktop edit of config.toml.
        Surface::Desktop => {
            let config_path = env.config_path();
            let original = match std::fs::read(&config_path) {
                Ok(b) => Some(b),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e).with_context(|| format!("cannot read {}", config_path.display())),
            };
            let text = match &original {
                Some(b) => String::from_utf8(b.clone()).context("config.toml is not UTF-8")?,
                None => String::new(),
            };
            let applied = desktop::apply(&text, &start_model, &catalog_path, &req.provider, desktop.as_ref(), req.force)?;
            let (backup_path, original_sha256) = match &desktop {
                Some(p) => (p.backup_path.clone(), p.original_sha256.clone()),
                None => match &original {
                    Some(bytes) => {
                        let b = files::backup(&config_path, &env.backups(), "config.toml")?;
                        report.line(format!("backup:   {}", b.display()));
                        (Some(b), Some(files::sha256(bytes)))
                    }
                    None => (None, None),
                },
            };
            files::write_atomic(&config_path, applied.text.as_bytes())?;
            report.line(format!("desktop:  {} now selects OwO AI Gateway (restart Codex Desktop)", config_path.display()));
            desktop = Some(state::DesktopEdit {
                base_url: req.provider.base_url.clone(),
                model: req.model.clone(),
                catalog_path: catalog_path.clone(),
                native_aliases: aliases,
                config_path,
                backup_path,
                original_sha256,
                written_sha256: files::sha256(applied.text.as_bytes()),
                root_keys: applied.root_keys,
                provider_table: applied.provider_table,
                created_providers_table: applied.created_providers_table,
            });
        }
    }

    // 3. Let Codex itself confirm it accepts what was written.
    // The CLI profile is checked as the equivalent `-c` layers (`-p` only applies to runtime
    // commands); Desktop's settings are already in config.toml.
    if let Some(bin) = &env.codex_bin {
        let overrides = match req.surface {
            Surface::Cli => profile::overrides(&start_model, &catalog_path, &req.provider),
            Surface::Desktop => Vec::new(),
        };
        match detect::verify_config(bin, &env.codex_home, &overrides) {
            Ok(slugs) => {
                let missing: Vec<_> = req.models.iter().filter(|m| !slugs.contains(&m.slug)).map(|m| m.slug.clone()).collect();
                if missing.is_empty() {
                    report.line("verified: Codex accepts the settings and lists the OwO AI Gateway models");
                } else {
                    report.warn(format!("Codex did not list: {}", missing.join(", ")));
                }
            }
            Err(e) => report.warn(format!("{e:#}")),
        }
    }

    save_or_clear(env, cli, desktop, template_source)?;
    Ok(report)
}

/// Undoes what `enable` did for `surface`, leaving the other surface connected. Plans
/// first and writes nothing if a conflict is found (unless `force`).
pub fn restore(env: &Environment, surface: Surface, force: bool) -> Result<Report> {
    let mut report = Report::default();
    let Some(st) = state::load(&env.state_dir)? else {
        bail!("{} is not connected", surface.app());
    };
    let (mut cli, mut desktop) = (st.cli, st.desktop);
    match surface {
        Surface::Desktop => {
            let Some(edit) = desktop.take() else { bail!("codex-desktop is not connected") };
            let current = std::fs::read(&edit.config_path).ok();
            let current_sha = current.as_deref().map(files::sha256);
            if current_sha.as_deref() == Some(edit.written_sha256.as_str()) {
                // Untouched since enable: put the original bytes back exactly.
                match &edit.backup_path {
                    Some(b) => {
                        let original = std::fs::read(b).with_context(|| format!("backup {} is missing", b.display()))?;
                        files::write_atomic(&edit.config_path, &original)?;
                    }
                    None => {
                        let _ = std::fs::remove_file(&edit.config_path);
                    }
                }
                report.line(format!("config:   {} restored byte-for-byte from backup", edit.config_path.display()));
            } else if let Some(bytes) = current {
                let text = String::from_utf8(bytes).context("config.toml is not UTF-8")?;
                let reverted = desktop::revert(&text, &edit, force)?;
                if !reverted.conflicts.is_empty() && !force {
                    bail!(
                        "config.toml changed after OwO AI Gateway edited it; nothing was restored:\n  - {}\nResolve these by hand or re-run with --force to revert them too.",
                        reverted.conflicts.join("\n  - ")
                    );
                }
                files::write_atomic(&edit.config_path, reverted.text.as_bytes())?;
                report.line(format!("config:   {} — removed only OwO AI Gateway's keys (other edits kept)", edit.config_path.display()));
            }
            let _ = std::fs::remove_file(&edit.catalog_path);
            let _ = std::fs::remove_file(aliases_path(&env.state_dir));
            report.line("codex-desktop disconnected. Restart Codex Desktop.");
        }
        Surface::Cli => {
            let Some(edit) = cli.take() else { bail!("codex is not connected") };
            let profile_sha = files::sha256_file(&edit.profile.path)?;
            if let Some(sha) = &profile_sha {
                if *sha != edit.profile.written_sha256 && !force {
                    bail!("{} was edited after OwO AI Gateway wrote it; nothing was restored (re-run with --force)", edit.profile.path.display());
                }
                match &edit.profile.replaced_backup {
                    Some(b) => {
                        std::fs::copy(b, &edit.profile.path).with_context(|| format!("cannot restore {}", edit.profile.path.display()))?;
                        report.line(format!("profile:  {} restored from {}", edit.profile.path.display(), b.display()));
                    }
                    None => {
                        std::fs::remove_file(&edit.profile.path).with_context(|| format!("cannot remove {}", edit.profile.path.display()))?;
                        report.line(format!("profile:  {} removed", edit.profile.path.display()));
                    }
                }
            }
            let _ = std::fs::remove_file(&edit.catalog_path);
            report.line("codex disconnected.");
        }
    }
    save_or_clear(env, cli, desktop, st.template_source)?;
    Ok(report)
}

pub fn status(env: &Environment) -> Result<Option<CodexState>> {
    state::load(&env.state_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("owo-codex-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn env(root: &Path) -> Environment {
        let codex_home = root.join("codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        Environment { state_dir: root.join("state"), backups_dir: root.join("backups"), codex_home, codex_bin: None }
    }

    fn request(surface: Surface) -> EnableRequest {
        EnableRequest {
            provider: ProviderSettings { name: "OwO".into(), base_url: "http://127.0.0.1:8787/c/codex/v1".into(), env_key: None },
            model: "deepseek-chat".into(),
            models: vec![CatalogModel {
                slug: "deepseek-chat".into(),
                display_name: "DeepSeek Chat".into(),
                description: "DeepSeek via OwO AI Gateway".into(),
                context_window: None,
                reasoning_efforts: vec![],
                default_reasoning_effort: None,
                vision: false,
                parallel_tools: false,
            }],
            surface,
            force: false,
            native_aliases: false,
        }
    }

    const USER_CONFIG: &str = "model = \"gpt-5.6-sol\"\n\n[desktop]\nsansFontSize = 14\n";

    #[test]
    fn native_aliases_start_desktop_on_a_native_slot() {
        let root = temp_dir("aliases");
        let env = env(&root);
        std::fs::write(env.config_path(), USER_CONFIG).unwrap();
        // A template cached by an earlier enable (no Codex binary in tests).
        let stored = StoredTemplate {
            source: "codex-cli test".into(),
            entry: catalog::builtin_template(),
            native_slugs: vec!["gpt-6-astra".into(), "gpt-5.5".into()],
        };
        files::write_atomic(&env.template_path(), serde_json::to_string(&stored).unwrap().as_bytes()).unwrap();

        let mut req = request(Surface::Desktop);
        req.native_aliases = true;
        let report = enable(&env, &req).unwrap();
        assert!(report.lines.iter().any(|l| l.contains("gpt-6-astra → deepseek-chat")), "{:?}", report.lines);
        let config = std::fs::read_to_string(env.config_path()).unwrap();
        assert!(config.contains("model = \"gpt-6-astra\""), "{config}");
        assert_eq!(stored_native_aliases(&env.state_dir)[0].model, "deepseek-chat");
        let catalog: Value = serde_json::from_str(&std::fs::read_to_string(env.catalog_path(Surface::Desktop)).unwrap()).unwrap();
        assert_eq!(catalog["models"][0]["slug"], "gpt-6-astra");

        // The CLI never uses native aliases, and connecting it leaves Desktop's alone.
        let mut cli = request(Surface::Cli);
        cli.native_aliases = true;
        assert!(enable(&env, &cli).unwrap_err().to_string().contains("codex-desktop"));
        enable(&env, &request(Surface::Cli)).unwrap();
        assert_eq!(stored_native_aliases(&env.state_dir).len(), 1);
        let catalog: Value = serde_json::from_str(&std::fs::read_to_string(env.catalog_path(Surface::Cli)).unwrap()).unwrap();
        assert_eq!(catalog["models"][0]["slug"], "deepseek-chat");

        // Re-connecting Desktop without aliases drops the mapping; disconnecting clears it.
        enable(&env, &request(Surface::Desktop)).unwrap();
        assert!(stored_native_aliases(&env.state_dir).is_empty());
        enable(&env, &req).unwrap();
        restore(&env, Surface::Desktop, false).unwrap();
        assert!(stored_native_aliases(&env.state_dir).is_empty());
        assert!(status(&env).unwrap().unwrap().cli.is_some(), "the CLI stays connected");
    }

    #[test]
    fn native_aliases_need_native_slugs() {
        let root = temp_dir("aliases-missing");
        let env = env(&root);
        let mut req = request(Surface::Desktop);
        req.native_aliases = true;
        assert!(enable(&env, &req).unwrap_err().to_string().contains("native aliases"));
    }

    #[test]
    fn cli_profile_enable_and_restore_leave_config_untouched() {
        let root = temp_dir("cli");
        let env = env(&root);
        std::fs::write(env.config_path(), USER_CONFIG).unwrap();

        enable(&env, &request(Surface::Cli)).unwrap();
        assert!(env.profile_path().is_file());
        assert!(env.catalog_path(Surface::Cli).is_file());
        assert_eq!(std::fs::read_to_string(env.config_path()).unwrap(), USER_CONFIG);
        let st = status(&env).unwrap().unwrap();
        assert!(st.cli.is_some() && st.desktop.is_none());

        // Re-enable is idempotent over OwO AI Gateway's own file.
        enable(&env, &request(Surface::Cli)).unwrap();

        assert!(restore(&env, Surface::Desktop, false).is_err(), "Desktop is not connected");
        restore(&env, Surface::Cli, false).unwrap();
        assert!(!env.profile_path().exists());
        assert!(!env.catalog_path(Surface::Cli).exists());
        assert!(status(&env).unwrap().is_none());
        assert_eq!(std::fs::read_to_string(env.config_path()).unwrap(), USER_CONFIG);
    }

    #[test]
    fn desktop_enable_and_byte_exact_restore() {
        let root = temp_dir("desktop");
        let env = env(&root);
        std::fs::write(env.config_path(), USER_CONFIG).unwrap();

        let report = enable(&env, &request(Surface::Desktop)).unwrap();
        assert!(report.lines.iter().any(|l| l.starts_with("backup:")));
        let edited = std::fs::read_to_string(env.config_path()).unwrap();
        assert!(edited.contains("model_provider = \"owo\""), "{edited}");
        assert!(edited.contains("[desktop]\nsansFontSize = 14"));
        assert!(!env.profile_path().exists(), "Desktop does not need the profile");

        restore(&env, Surface::Desktop, false).unwrap();
        assert_eq!(std::fs::read_to_string(env.config_path()).unwrap(), USER_CONFIG);
        assert!(status(&env).unwrap().is_none());
    }

    #[test]
    fn surfaces_are_independent() {
        let root = temp_dir("both");
        let env = env(&root);
        std::fs::write(env.config_path(), USER_CONFIG).unwrap();
        enable(&env, &request(Surface::Cli)).unwrap();
        let mut desktop = request(Surface::Desktop);
        desktop.model = "deepseek-chat".into();
        enable(&env, &desktop).unwrap();
        let st = status(&env).unwrap().unwrap();
        assert!(st.cli.is_some() && st.desktop.is_some());

        restore(&env, Surface::Cli, false).unwrap();
        assert!(!env.profile_path().exists());
        assert!(std::fs::read_to_string(env.config_path()).unwrap().contains("model_provider = \"owo\""), "Desktop keeps working");
        let st = status(&env).unwrap().unwrap();
        assert!(st.cli.is_none() && st.desktop.is_some());

        restore(&env, Surface::Desktop, false).unwrap();
        assert_eq!(std::fs::read_to_string(env.config_path()).unwrap(), USER_CONFIG);
        assert!(status(&env).unwrap().is_none());
    }

    #[test]
    fn desktop_restore_keeps_later_user_edits() {
        let root = temp_dir("desktop-edits");
        let env = env(&root);
        std::fs::write(env.config_path(), USER_CONFIG).unwrap();
        enable(&env, &request(Surface::Desktop)).unwrap();

        let mut text = std::fs::read_to_string(env.config_path()).unwrap();
        text.push_str("\n[projects.'d:\\new']\ntrust_level = \"trusted\"\n");
        std::fs::write(env.config_path(), &text).unwrap();

        restore(&env, Surface::Desktop, false).unwrap();
        let after = std::fs::read_to_string(env.config_path()).unwrap();
        assert!(after.contains("trust_level = \"trusted\""), "{after}");
        assert!(after.contains("model = \"gpt-5.6-sol\""), "{after}");
        assert!(!after.contains("owo"), "{after}");
    }

    #[test]
    fn foreign_profile_file_requires_force() {
        let root = temp_dir("foreign");
        let env = env(&root);
        std::fs::write(env.profile_path(), "model = \"mine\"\n").unwrap();
        assert!(enable(&env, &request(Surface::Cli)).is_err());

        let mut req = request(Surface::Cli);
        req.force = true;
        enable(&env, &req).unwrap();
        restore(&env, Surface::Cli, false).unwrap();
        assert_eq!(std::fs::read_to_string(env.profile_path()).unwrap(), "model = \"mine\"\n");
    }

    #[test]
    fn one_codex_home_at_a_time() {
        let root = temp_dir("two-homes");
        let first = env(&root);
        enable(&first, &request(Surface::Cli)).unwrap();
        let other = Environment { codex_home: root.join("other-codex"), ..env(&root) };
        std::fs::create_dir_all(&other.codex_home).unwrap();
        assert!(enable(&other, &request(Surface::Desktop)).unwrap_err().to_string().contains("already connected"));
        restore(&first, Surface::Cli, false).unwrap();
        enable(&other, &request(Surface::Cli)).unwrap();
    }

    #[test]
    fn rejects_unknown_default_model() {
        let root = temp_dir("unknown");
        let env = env(&root);
        let mut req = request(Surface::Cli);
        req.model = "nope".into();
        assert!(enable(&env, &req).unwrap_err().to_string().contains("not available"));
    }
}