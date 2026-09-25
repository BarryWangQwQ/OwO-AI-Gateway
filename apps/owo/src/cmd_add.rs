//! `owo add` — add a provider in one step: store its key, write `[providers.<name>]`.

use anyhow::{bail, Context, Result};
use toml_edit::{value, Array, DocumentMut, Item, Table};
use owo_credentials::{CredentialBackend, CredentialRef, CredentialStore};
use owo_registry::PresetCatalog;

use crate::cli::{AddArgs, GlobalArgs};
use crate::{cmd_config, cmd_credential, context};

fn providers_table(doc: &mut DocumentMut) -> Result<&mut Table> {
    if doc.get("providers").is_none() {
        let mut t = Table::new();
        t.set_implicit(true);
        doc.insert("providers", Item::Table(t));
    }
    doc["providers"].as_table_mut().context("`providers` in config.toml is not a table")
}

fn string_array(items: &[String]) -> Array {
    items.iter().map(String::as_str).collect()
}

/// Appends `models` to `[providers.<id>] models`. A provider without its own list was
/// exposing its preset's models, so that list is written out first to keep exposing them.
pub fn add_models(doc: &mut DocumentMut, id: &str, models: &[String], preset_models: &[String]) -> Result<Vec<String>> {
    let provider = providers_table(doc)?
        .get_mut(id)
        .and_then(Item::as_table_mut)
        .with_context(|| format!("provider `{id}` is not in config.toml (add it with `owo add {id}`)"))?;
    if provider.get("models").is_none() {
        provider["models"] = value(string_array(preset_models));
    }
    let list = provider["models"].as_array_mut().context("`models` is not a list")?;
    let mut added = Vec::new();
    for m in models {
        if !list.iter().any(|v| v.as_str() == Some(m.as_str())) {
            list.push(m.as_str());
            added.push(m.clone());
        }
    }
    Ok(added)
}

pub fn add(global: &GlobalArgs, args: AddArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let catalog = PresetCatalog::builtin();
    let preset_id = args.preset.clone().unwrap_or_else(|| args.name.clone());
    let preset = catalog.get(&preset_id);
    if args.preset.is_some() && preset.is_none() {
        bail!("no preset `{preset_id}` (see `owo providers presets`)");
    }
    if preset.is_none() && args.url.is_none() {
        bail!("`{}` is not a built-in preset; give its endpoint with --url (and --adapter), or pick one from `owo providers presets`", args.name);
    }

    let backend = context::load_config(&paths).map(|(c, _)| c.credentials.backend).unwrap_or(CredentialBackend::System);
    let preset_env = preset.and_then(|p| match &p.api_key {
        Some(CredentialRef::Env(v)) => Some(v.clone()),
        _ => None,
    });
    let needs_key = !args.no_key && preset.is_none_or(|p| p.api_key.as_ref().is_some_and(|c| *c != CredentialRef::None));
    let credential = match (&args.env, needs_key, backend) {
        (Some(var), _, _) => Some(CredentialRef::Env(var.clone())),
        (None, false, _) => None,
        (None, true, CredentialBackend::Env) => {
            Some(CredentialRef::Env(preset_env.clone().unwrap_or_else(|| format!("{}_API_KEY", args.name.to_ascii_uppercase().replace('-', "_")))))
        }
        (None, true, CredentialBackend::System) => Some(CredentialRef::Keyring(args.name.clone())),
    };

    let models: Vec<String> = match &args.models {
        Some(list) => list.clone(),
        None => preset.map(|p| p.models.clone()).unwrap_or_default(),
    };

    // Store the key before touching config.toml, so the config never points at a key that is not there.
    if let Some(CredentialRef::Keyring(name)) = &credential {
        let store = CredentialStore::new(backend);
        if store.resolve(&CredentialRef::Keyring(name.clone())).is_ok() {
            println!("key:      keyring:{name} is already stored (replace it with `owo key set {name}`)");
        } else {
            cmd_credential::set(global, name)?;
        }
    }

    let name = args.name.clone();
    cmd_config::edit_config(&paths, |doc| {
        let providers = providers_table(doc)?;
        if providers.contains_key(&name) && !args.force {
            bail!("provider `{name}` is already in config.toml (use --force to replace it)");
        }
        let mut t = Table::new();
        if let Some(p) = &args.preset {
            if *p != name {
                t["preset"] = value(p.as_str());
            }
        }
        if let Some(a) = &args.adapter {
            t["adapter"] = value(a.as_str());
        } else if preset.is_none() {
            t["adapter"] = value("openai-chat");
        }
        if let Some(u) = &args.url {
            t["base_url"] = value(u.as_str());
        }
        match &credential {
            Some(c) => t["api_key"] = value(c.to_string()),
            None if preset.is_some_and(|p| p.api_key.is_some()) => t["api_key"] = value("none"),
            None => {}
        }
        if !models.is_empty() {
            t["models"] = value(string_array(&models));
        }
        providers.insert(&name, Item::Table(t));
        Ok(())
    })?;

    println!("config:   [providers.{name}] added to {}", paths.config.display());
    match &credential {
        Some(CredentialRef::Env(var)) => println!("key:      read from the environment variable {var} (set it where `owo start` runs)"),
        Some(_) => {}
        None => println!("key:      none"),
    }
    if models.is_empty() {
        println!("models:   none yet — list what `{name}` offers with `owo providers discover {name}`, then add some with `--add`");
    } else {
        println!("models:   {}", models.join(", "));
    }
    println!("\nNext:  owo start   then   owo connect <app>");
    Ok(())
}
