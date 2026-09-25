use std::io::{IsTerminal, Read};

use anyhow::{bail, Context, Result};
use owo_credentials::{CredentialBackend, CredentialRef, CredentialStore, Secret};

use crate::cli::GlobalArgs;
use crate::context;

fn store(global: &GlobalArgs) -> CredentialStore {
    // A missing or invalid config must not block storing the secret it will reference.
    let backend = context::paths(global)
        .ok()
        .and_then(|p| context::load_config(&p).ok())
        .map(|(c, _)| c.credentials.backend)
        .unwrap_or(CredentialBackend::System);
    CredentialStore::new(backend)
}

pub fn set(global: &GlobalArgs, name: &str) -> Result<()> {
    let value = if std::io::stdin().is_terminal() {
        rpassword::prompt_password(format!("secret for keyring:{name}: ")).context("cannot read secret")?
    } else {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf).context("cannot read secret from stdin")?;
        buf
    };
    let value = value.trim();
    if value.is_empty() {
        bail!("empty secret");
    }
    store(global).set_keyring(name, &Secret::new(value))?;
    println!("stored keyring:{name} — reference it with api_key = \"keyring:{name}\"");
    Ok(())
}

/// Every key config.toml refers to, who uses it, and whether it can be read right now.
pub fn list(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let store = CredentialStore::new(config.credentials.backend);
    let mut refs: Vec<(CredentialRef, Vec<String>)> = Vec::new();
    let mut note = |r: &CredentialRef, user: String| match refs.iter_mut().find(|(x, _)| x == r) {
        Some((_, users)) => users.push(user),
        None => refs.push((r.clone(), vec![user])),
    };
    for (id, p) in &config.providers {
        if let Some(c) = p.api_key.as_ref().filter(|c| **c != CredentialRef::None) {
            note(c, format!("provider {id}"));
        }
    }
    if let Some(t) = &config.server.auth_token {
        note(t, "server.auth_token".into());
    }
    for (name, s) in &config.mcp {
        for (field, values) in [("env", &s.env), ("headers", &s.headers)] {
            for (k, v) in values {
                if let Some(Ok(r)) = owo_config::mcp_value_ref(v) {
                    note(&r, format!("mcp {name} {field}.{k}"));
                }
            }
        }
    }
    if refs.is_empty() {
        println!("config.toml refers to no keys. Providers set up with `owo add` get one.");
        return Ok(());
    }
    println!("{:<30} {:<28} STATUS", "KEY", "USED BY");
    let mut missing = Vec::new();
    for (r, users) in &refs {
        let status = match store.resolve(r) {
            Ok(_) if matches!(r, CredentialRef::Inline(_)) => "ok (plain text in config.toml)".to_string(),
            Ok(_) => "ok".to_string(),
            Err(e) => {
                missing.push(r.clone());
                match r {
                    CredentialRef::Keyring(n) => format!("missing — owo key set {n}"),
                    CredentialRef::Env(v) => format!("missing — set the environment variable {v}"),
                    CredentialRef::Inline(_) | CredentialRef::None => e.to_string(),
                }
            }
        };
        println!("{:<30} {:<28} {status}", r.to_string(), users.join(", "));
    }
    if missing.is_empty() {
        println!("\nAll keys are available.");
    }
    Ok(())
}

pub fn delete(global: &GlobalArgs, name: &str) -> Result<()> {
    store(global).delete_keyring(name)?;
    println!("deleted keyring:{name}");
    Ok(())
}
