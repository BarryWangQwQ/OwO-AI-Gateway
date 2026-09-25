//! Shared loading of paths, config, registry, and router for every command.

use std::sync::Arc;

use anyhow::{Context, Result};
use owo_config::{Config, Diagnostic, OwoPaths};
use owo_credentials::CredentialStore;
use owo_provider_anthropic::AnthropicAdapter;
use owo_provider_openai_compat::{AdapterSettings, OpenAiChatAdapter};
use owo_registry::{PresetCatalog, Registry};
use owo_routing::{ProviderAdapter, Router};

use crate::cli::GlobalArgs;

pub fn paths(global: &GlobalArgs) -> Result<OwoPaths> {
    let paths = if global.portable {
        OwoPaths::portable_from_exe().context("cannot locate the executable directory for portable mode")?
    } else {
        OwoPaths::home().context("cannot determine the home directory; set OWO_HOME")?
    };
    Ok(match &global.config {
        Some(file) => paths.with_config_file(file.clone()),
        None => paths,
    })
}

pub fn load_config(paths: &OwoPaths) -> Result<(Config, Vec<Diagnostic>)> {
    Ok(Config::load(&paths.config)?)
}

/// Every adapter compiled into this build.
pub fn adapters() -> Result<Vec<Arc<dyn ProviderAdapter>>> {
    let settings = AdapterSettings::default();
    Ok(vec![
        Arc::new(OpenAiChatAdapter::new(settings.clone()).context("failed to initialize HTTP client")?),
        Arc::new(AnthropicAdapter::new(settings).context("failed to initialize HTTP client")?),
    ])
}

pub fn build_router(config: &Config) -> Result<(Router, Vec<Diagnostic>)> {
    let adapters = adapters()?;
    let kinds: Vec<&str> = adapters.iter().map(|a| a.kind()).collect();
    let (registry, warnings) = Registry::build(config, PresetCatalog::builtin(), &kinds).map_err(|errors| {
        let text = errors.iter().map(|d| format!("  - {d}")).collect::<Vec<_>>().join("\n");
        anyhow::anyhow!("provider/model registry is invalid:\n{text}")
    })?;
    let store = CredentialStore::new(config.credentials.backend);
    Ok((Router::new(Arc::new(registry), store, adapters), warnings))
}

pub fn print_diagnostics(diags: &[Diagnostic]) {
    for d in diags {
        eprintln!("{d}");
    }
}

pub fn runtime() -> Result<tokio::runtime::Runtime> {
    tokio::runtime::Builder::new_multi_thread().enable_all().build().context("failed to start async runtime")
}
