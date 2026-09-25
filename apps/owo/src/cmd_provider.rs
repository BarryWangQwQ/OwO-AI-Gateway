use anyhow::Result;
use owo_registry::PresetCatalog;

use crate::cli::GlobalArgs;
use crate::context;

pub fn list(global: &GlobalArgs) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, warnings) = context::build_router(&config)?;
    context::print_diagnostics(&warnings);
    println!("{:<18} {:<16} {:<8} {:<28} BASE URL", "ID", "ADAPTER", "ENABLED", "API KEY");
    for p in router.registry().providers() {
        println!(
            "{:<18} {:<16} {:<8} {:<28} {}",
            p.id,
            p.adapter,
            if p.enabled { "yes" } else { "no" },
            p.api_key.to_string(),
            p.base_url
        );
    }
    Ok(())
}

pub fn presets() -> Result<()> {
    let catalog = PresetCatalog::builtin();
    let implemented: Vec<&str> = context::adapters()?.iter().map(|a| a.kind()).collect();
    println!("{:<16} {:<18} {:<12} {:<30} BASE URL", "PRESET", "ADAPTER", "AVAILABLE", "DEFAULT API KEY");
    for p in catalog.iter() {
        let credential = p.api_key.as_ref().map(ToString::to_string).unwrap_or_else(|| "none".into());
        println!(
            "{:<16} {:<18} {:<12} {:<30} {}",
            p.id,
            p.adapter,
            if implemented.contains(&p.adapter.as_str()) { "yes" } else { "not yet" },
            credential,
            p.base_url
        );
    }
    println!("\n{} presets. Add one with `owo add <preset>`.", catalog.len());
    Ok(())
}

/// Lists what the provider serves right now, marks the models config.toml already
/// exposes, and adds the chosen ones (`add`, or every one with `all`) to its `models`.
pub fn discover(global: &GlobalArgs, id: &str, add: &[String], all: bool) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, _) = context::build_router(&config)?;
    let found = context::runtime()?.block_on(router.discover(id)).map_err(|e| anyhow::anyhow!(e.message))?;
    let configured: Vec<&str> = router.registry().models().filter(|m| m.provider == id).map(|m| m.upstream_model.as_str()).collect();

    for m in &found {
        let mark = if configured.contains(&m.id.as_str()) { "✓" } else { " " };
        match m.context_window {
            Some(ctx) => println!("{mark} {}  (context {ctx})", m.id),
            None => println!("{mark} {}", m.id),
        }
    }
    println!("\n{} model(s) from `{id}`; ✓ = already in config.toml", found.len());

    let wanted: Vec<String> = if all { found.iter().map(|m| m.id.clone()).collect() } else { add.to_vec() };
    if wanted.is_empty() {
        println!("Add some with:  owo providers discover {id} --add <model> [<model> …]   (or --all)");
        return Ok(());
    }
    let unknown: Vec<&String> = wanted.iter().filter(|w| !found.iter().any(|m| &m.id == *w)).collect();
    if !unknown.is_empty() {
        anyhow::bail!("`{id}` does not list: {}", unknown.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", "));
    }
    let preset_id = config.providers.get(id).and_then(|p| p.preset.clone()).unwrap_or_else(|| id.to_string());
    let preset_models = PresetCatalog::builtin().get(&preset_id).map(|p| p.models.clone()).unwrap_or_default();
    let mut added = Vec::new();
    crate::cmd_config::edit_config(&paths, |doc| {
        added = crate::cmd_add::add_models(doc, id, &wanted, &preset_models)?;
        Ok(())
    })?;
    if added.is_empty() {
        println!("Nothing to add: those models are already configured.");
    } else {
        println!("Added to [providers.{id}]: {}  (restart `owo start` to serve them)", added.join(", "));
    }
    Ok(())
}
pub fn models(global: &GlobalArgs, client: Option<&str>) -> Result<()> {
    let paths = context::paths(global)?;
    let (config, _) = context::load_config(&paths)?;
    let (router, warnings) = context::build_router(&config)?;
    context::print_diagnostics(&warnings);
    let registry = router.registry();
    let priced = registry.models().any(|m| m.price.is_some());
    let price_header = if priced { format!("{:<18} ", "PRICE IN/OUT") } else { String::new() };
    println!("{:<24} {:<24} {:<14} {:<10} {price_header}UPSTREAM", "ID", "CLIENT-FACING ID", "PROVIDER", "AVAILABLE");
    for m in registry.models() {
        let available = m.enabled && registry.provider(&m.provider).is_some_and(|p| p.enabled);
        let price = match (priced, m.price) {
            (false, _) => String::new(),
            (true, Some(p)) => format!("{:<18} ", format!("${} / ${}", number(p.input), number(p.output))),
            (true, None) => format!("{:<18} ", "-"),
        };
        println!(
            "{:<24} {:<24} {:<14} {:<10} {price}{}",
            m.id,
            m.exposed_id(client),
            m.provider,
            if available { "yes" } else { "no" },
            m.upstream_model
        );
    }
    if priced {
        println!("\nPrices are USD per million tokens, from config.toml; they estimate costs in `owo usage`.");
    }
    Ok(())
}

/// `3`, `0.3`, `1.25`: no trailing zeros.
fn number(n: f64) -> String {
    let s = format!("{n:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
