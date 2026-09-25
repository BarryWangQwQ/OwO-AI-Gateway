//! What OwO AI Gateway changed in Codex's files, so restore can undo exactly that (spec §33).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::files::write_atomic;

/// One Codex home, with up to two independent integrations: the CLI profile
/// (`owo connect codex`) and the Desktop edit of `config.toml` (`owo connect codex-desktop`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodexState {
    pub codex_home: PathBuf,
    pub owo_version: String,
    pub updated_at_unix: u64,
    /// Where the catalog template came from (`codex-cli 0.155.0` or `builtin`).
    pub template_source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli: Option<CliEdit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desktop: Option<DesktopEdit>,
}

/// `<codex_home>/owo.config.toml` (a profile layer, `codex -p owo`) and its catalog.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CliEdit {
    pub base_url: String,
    pub model: String,
    pub catalog_path: PathBuf,
    pub profile: OwnedFile,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedFile {
    pub path: PathBuf,
    pub written_sha256: String,
    /// A pre-existing file OwO AI Gateway replaced (only with `--force`), kept for restore.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced_backup: Option<PathBuf>,
}

/// Root keys and the `[model_providers.owo]` table set in `config.toml` for Codex Desktop.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DesktopEdit {
    pub base_url: String,
    pub model: String,
    pub catalog_path: PathBuf,
    /// Native GPT slugs lent to OwO AI Gateway models (signed-out Desktop picker support).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub native_aliases: Vec<crate::catalog::NativeAlias>,
    pub config_path: PathBuf,
    /// Copy of the file before OwO AI Gateway's first edit; `None` when the file did not exist.
    pub backup_path: Option<PathBuf>,
    pub original_sha256: Option<String>,
    pub written_sha256: String,
    /// Root keys OwO AI Gateway set: previous value (TOML source, `None` = absent) and value written.
    pub root_keys: BTreeMap<String, OwnedValue>,
    /// TOML source of the `[model_providers.owo]` table as written.
    pub provider_table: String,
    /// OwO AI Gateway created the `[model_providers]` table and should remove it if left empty.
    pub created_providers_table: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedValue {
    pub previous: Option<String>,
    pub written: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct IntegrationsFile {
    #[serde(default)]
    clients: BTreeMap<String, serde_json::Value>,
}

fn path(state_dir: &Path) -> PathBuf {
    state_dir.join("integrations.json")
}

fn read_all(state_dir: &Path) -> Result<IntegrationsFile> {
    match std::fs::read_to_string(path(state_dir)) {
        Ok(text) => serde_json::from_str(&text).context("state/integrations.json is corrupt"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(IntegrationsFile::default()),
        Err(e) => Err(e).context("cannot read state/integrations.json"),
    }
}

fn write_all(state_dir: &Path, file: &IntegrationsFile) -> Result<()> {
    write_atomic(&path(state_dir), serde_json::to_string_pretty(file)?.as_bytes())
}

pub fn load(state_dir: &Path) -> Result<Option<CodexState>> {
    let file = read_all(state_dir)?;
    file.clients
        .get(crate::CLIENT_ID)
        .map(|v| serde_json::from_value(v.clone()).context("Codex integration state is corrupt"))
        .transpose()
}

pub fn save(state_dir: &Path, state: &CodexState) -> Result<()> {
    let mut file = read_all(state_dir)?;
    file.clients.insert(crate::CLIENT_ID.to_string(), serde_json::to_value(state)?);
    write_all(state_dir, &file)
}

pub fn clear(state_dir: &Path) -> Result<()> {
    let mut file = read_all(state_dir)?;
    if file.clients.remove(crate::CLIENT_ID).is_some() {
        write_all(state_dir, &file)?;
    }
    Ok(())
}
