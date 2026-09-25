//! `owo usage` and `owo history`: what the gateway recorded about model calls.

use std::path::PathBuf;

use anyhow::{Context, Result};
use owo_usage::{CallFilter, GroupBy, StoredCall, Summary, UsageLog};

use crate::cli::{App, GlobalArgs, UsageBy};
use crate::context;

/// A usage-table column: heading and the figure it shows.
type Column = (&'static str, fn(&Summary) -> u64);

pub fn usage(global: &GlobalArgs, days: u32, by: UsageBy) -> Result<()> {
    let Some(path) = usage_db(global)? else { return Ok(()) };
    let days = days.max(1);
    let group = match by {
        UsageBy::Model => GroupBy::Model,
        UsageBy::App => GroupBy::App,
        UsageBy::Provider => GroupBy::Provider,
        UsageBy::Day => GroupBy::Day,
    };
    let (rows, since) = context::runtime()?.block_on(async {
        let log = open(&path).await?;
        anyhow::Ok((log.summary(days, group).await?, log.period_start(days).await?))
    })?;
    let period = if days == 1 { "today".to_string() } else { format!("last {days} days (since {since})") };
    if rows.is_empty() {
        println!("No model calls {period}.");
        return Ok(());
    }

    let mut total = Summary { key: "TOTAL".into(), ..Summary::default() };
    rows.iter().for_each(|r| total.add(r));
    let heading = match by {
        UsageBy::Model => "MODEL",
        UsageBy::App => "APP",
        UsageBy::Provider => "PROVIDER",
        UsageBy::Day => "DAY",
    };
    let mut columns: Vec<Column> = vec![("CALLS", |s| s.calls), ("FAILED", |s| s.failed), ("INPUT", |s| s.input_tokens)];
    if total.cached_input_tokens > 0 {
        columns.push(("CACHED", |s| s.cached_input_tokens));
    }
    if total.cache_creation_input_tokens > 0 {
        columns.push(("CACHE WRITE", |s| s.cache_creation_input_tokens));
    }
    columns.push(("OUTPUT", |s| s.output_tokens));
    if total.reasoning_tokens > 0 {
        columns.push(("REASONING", |s| s.reasoning_tokens));
    }
    columns.push(("TOTAL", Summary::total_tokens));

    let label = |s: &Summary| if by == UsageBy::App { app_name(&s.key).to_string() } else { s.key.clone() };
    let key_width = rows.iter().map(|r| label(r).chars().count()).chain([heading.len(), 5]).max().unwrap_or(5);
    let cell = |(name, value): &Column, s: &Summary| {
        let text = if matches!(*name, "CALLS" | "FAILED") { value(s).to_string() } else { compact(value(s)) };
        format!("{text:>width$}", width = name.len().max(7))
    };
    let priced = total.cost_usd.is_some();
    // A `*` marks totals that leave out calls to models without a price.
    let cost = |s: &Summary| {
        let text = match s.cost_usd {
            Some(usd) => format!("{}{}", money(usd), if s.unpriced > 0 { "*" } else { " " }),
            None => "- ".to_string(),
        };
        format!("{text:>10}")
    };
    let line = |label: &str, s: &Summary| {
        let cells: Vec<String> = columns.iter().map(|c| cell(c, s)).collect();
        let cost = if priced { format!("  {}", cost(s)) } else { String::new() };
        println!("{label:<key_width$}  {}{cost}", cells.join("  "));
    };

    println!("Token usage, {period}\n");
    let header: Vec<String> = columns.iter().map(|(name, _)| format!("{name:>width$}", width = name.len().max(7))).collect();
    let cost_header = if priced { format!("  {:>10}", "COST ") } else { String::new() };
    println!("{heading:<key_width$}  {}{cost_header}", header.join("  "));
    for row in &rows {
        line(&label(row), row);
    }
    if rows.len() > 1 {
        line(&total.key, &total);
    }
    println!("\nINPUT includes CACHED input; OUTPUT includes REASONING. `owo history` lists the calls.");
    if priced {
        println!("COST is an estimate from the prices in config.toml when each call was made.");
    }
    if total.unpriced > 0 {
        let marked = if priced { "* leaves out" } else { "No cost estimate for" };
        println!("{marked} {} call(s) to models without a price: add `price = {{ input = …, output = … }}` (USD per million tokens) to them in config.toml.", total.unpriced);
    }
    Ok(())
}

pub fn history(global: &GlobalArgs, limit: u32, failed: bool, model: Option<String>, app: Option<App>) -> Result<()> {
    let Some(path) = usage_db(global)? else { return Ok(()) };
    let filter = CallFilter { failed_only: failed, model, client: app.map(|a| a.client_id().to_string()), limit: limit.max(1) };
    let calls = context::runtime()?.block_on(async { anyhow::Ok(open(&path).await?.calls(&filter).await?) })?;
    if calls.is_empty() {
        println!("{}", if failed { "No failed calls recorded." } else { "No matching calls recorded." });
        return Ok(());
    }

    let model_width = calls.iter().map(|c| shown_model(c).chars().count()).chain([5]).max().unwrap_or(5);
    let app_width = calls.iter().map(|c| app_name(c.client.as_deref().unwrap_or("-")).len()).chain([3]).max().unwrap_or(3);
    let id_width = calls.iter().map(|c| c.id.to_string().len()).chain([2]).max().unwrap_or(2);
    let priced = calls.iter().any(|c| c.cost_usd.is_some());
    println!(
        "{:>id_width$}  {:<14}  {:<app_width$}  {:<model_width$}  {:<9}  {:>7}  {:>7}{}  {:>8}",
        "ID",
        "TIME",
        "APP",
        "MODEL",
        "STATUS",
        "INPUT",
        "OUTPUT",
        if priced { format!("  {:>9}", "COST") } else { String::new() },
        "DURATION"
    );
    for c in &calls {
        let status = match (c.status.as_str(), c.upstream_status) {
            ("error", Some(code)) => format!("error {code}"),
            (status, _) => status.to_string(),
        };
        let tokens = |n: Option<u64>| n.map_or_else(|| "-".to_string(), compact);
        let note = match (&c.error_kind, &c.error_message) {
            (Some(kind), Some(message)) => format!("  {kind}: {}", truncate(message, 60)),
            (Some(kind), None) => format!("  {kind}"),
            _ => String::new(),
        };
        let cost = if priced { format!("  {:>9}", c.cost_usd.map_or_else(|| "-".to_string(), money)) } else { String::new() };
        println!(
            "{:>id_width$}  {:<14}  {:<app_width$}  {:<model_width$}  {:<9}  {:>7}  {:>7}{cost}  {:>8}{note}",
            c.id,
            c.time.get(5..).unwrap_or(&c.time),
            app_name(c.client.as_deref().unwrap_or("-")),
            shown_model(c),
            status,
            tokens(c.input_tokens),
            tokens(c.output_tokens),
            duration(c.duration_ms),
        );
    }
    println!("\n`owo history <ID>` shows one call in full.");
    Ok(())
}

pub fn show(global: &GlobalArgs, id: i64) -> Result<()> {
    let Some(path) = usage_db(global)? else { return Ok(()) };
    let call = context::runtime()?.block_on(async { anyhow::Ok(open(&path).await?.call(id).await?) })?;
    let Some(c) = call else {
        anyhow::bail!("no call #{id} (records are kept for 400 days)");
    };
    let row = |label: &str, value: String| println!("  {label:<11} {value}");
    println!("Call #{}", c.id);
    row("time", c.time.clone());
    row("app", app_name(c.client.as_deref().unwrap_or("-")).to_string());
    let model = match &c.model {
        Some(model) if *model != c.requested_model => format!("{model}  (asked for {})", c.requested_model),
        Some(model) => model.clone(),
        None if c.error_kind.as_deref() == Some("model_not_found") => format!("{}  (not configured)", c.requested_model),
        None => c.requested_model.clone(),
    };
    row("model", model);
    if let Some(provider) = &c.provider {
        let upstream = c.upstream_model.as_deref().map(|m| format!(" · upstream model {m}")).unwrap_or_default();
        row("provider", format!("{provider}{upstream}"));
    }
    let mut status = c.status.clone();
    if let Some(kind) = &c.error_kind {
        status.push_str(&format!(" · {kind}"));
    }
    if let Some(code) = c.upstream_status {
        status.push_str(&format!(" · upstream HTTP {code}"));
    }
    row("status", status);
    if let Some(message) = &c.error_message {
        row("error", message.clone());
    }
    let first = c.first_token_ms.map(|ms| format!(" · first token after {}", duration(ms))).unwrap_or_default();
    row("duration", format!("{}{first}", duration(c.duration_ms)));
    if c.input_tokens.is_some() || c.output_tokens.is_some() {
        let input = c.input_tokens.unwrap_or(0);
        let output = c.output_tokens.unwrap_or(0);
        let mut input_parts = Vec::new();
        if let Some(n) = c.cached_input_tokens.filter(|n| *n > 0) {
            input_parts.push(format!("cached {}", exact(n)));
        }
        if let Some(n) = c.cache_creation_input_tokens.filter(|n| *n > 0) {
            input_parts.push(format!("cache write {}", exact(n)));
        }
        let input_detail = if input_parts.is_empty() { String::new() } else { format!(" ({})", input_parts.join(", ")) };
        let reasoning = c.reasoning_tokens.filter(|n| *n > 0).map(|n| format!(" (reasoning {})", exact(n))).unwrap_or_default();
        row("tokens", format!("input {}{input_detail} · output {}{reasoning} · total {}", exact(input), exact(output), exact(input + output)));
        row("cost", c.cost_usd.map_or_else(|| "no price configured for this model".into(), |usd| format!("≈ {} (estimate)", money(usd))));
    } else {
        row("tokens", "not reported".into());
    }
    if let Some(reason) = &c.stop_reason {
        row("stop", reason.clone());
    }
    row("streamed", if c.stream { "yes" } else { "no" }.into());
    row("request id", c.request_id.clone());
    Ok(())
}

/// Today's totals for the overview (`12 calls · 1.3M tokens · ≈ $0.42 · 1 failed`), once
/// anything has been recorded.
pub fn today_line(global: &GlobalArgs) -> Option<String> {
    let path = context::paths(global).ok()?.state.join(owo_usage::FILE_NAME);
    if !path.exists() {
        return None;
    }
    let today = context::runtime().ok()?.block_on(async { open(&path).await.ok()?.today().await.ok() })?;
    if today.calls == 0 {
        return Some("no calls yet".into());
    }
    let calls = if today.calls == 1 { "1 call".to_string() } else { format!("{} calls", today.calls) };
    let failed = if today.failed > 0 { format!(" · {} failed", today.failed) } else { String::new() };
    let cost = today.cost_usd.map(|usd| format!(" · ≈ {}", money(usd))).unwrap_or_default();
    Some(format!("{calls} · {} tokens{cost}{failed}", compact(today.total_tokens())))
}

fn usage_db(global: &GlobalArgs) -> Result<Option<PathBuf>> {
    let path = context::paths(global)?.state.join(owo_usage::FILE_NAME);
    if path.exists() {
        return Ok(Some(path));
    }
    println!("No calls recorded yet. OwO AI Gateway records every model call while it runs (`owo start`).");
    Ok(None)
}

async fn open(path: &std::path::Path) -> Result<UsageLog> {
    UsageLog::open(path).await.with_context(|| format!("cannot open {}", path.display()))
}

fn shown_model(call: &StoredCall) -> &str {
    call.model.as_deref().unwrap_or(&call.requested_model)
}

/// The `owo connect` name for a client integration id (`claude_code` → `claude`).
fn app_name(client: &str) -> &str {
    owo_config::APP_NAMES.iter().find(|(_, id)| *id == client).map_or(client, |(name, _)| name)
}

fn truncate(text: &str, max: usize) -> String {
    let line = text.lines().next().unwrap_or("");
    if line.chars().count() > max { line.chars().take(max).collect::<String>() + "…" } else { line.to_string() }
}

/// `812`, `95.3K`, `1.23M`.
fn compact(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => trim(format!("{:.1}", n as f64 / 1e3)) + "K",
        1_000_000..1_000_000_000 => trim(format!("{:.2}", n as f64 / 1e6)) + "M",
        _ => trim(format!("{:.2}", n as f64 / 1e9)) + "B",
    }
}

fn trim(number: String) -> String {
    if number.contains('.') { number.trim_end_matches('0').trim_end_matches('.').to_string() } else { number }
}

/// `1,234,567`.
fn exact(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

/// `$0.0042` below a cent, else `$1.23`.
fn money(usd: f64) -> String {
    if usd == 0.0 {
        "$0".into()
    } else if usd < 0.01 {
        format!("${usd:.4}")
    } else {
        format!("${usd:.2}")
    }
}

/// `850ms`, `4.2s`, `2m05s`.
fn duration(ms: u64) -> String {
    match ms {
        0..1_000 => format!("{ms}ms"),
        1_000..60_000 => format!("{:.1}s", ms as f64 / 1e3),
        _ => format!("{}m{:02}s", ms / 60_000, ms % 60_000 / 1000),
    }
}

#[cfg(test)]
mod tests {
    use super::{app_name, compact, duration, exact, money};

    #[test]
    fn formats_numbers() {
        assert_eq!([compact(812), compact(95_300), compact(1_000), compact(1_234_567)], ["812", "95.3K", "1K", "1.23M"]);
        assert_eq!([exact(7), exact(1_234), exact(1_234_567)], ["7", "1,234", "1,234,567"]);
        assert_eq!([duration(850), duration(4_200), duration(125_000)], ["850ms", "4.2s", "2m05s"]);
        assert_eq!([money(0.0), money(0.00423), money(1.234), money(1234.5)], ["$0", "$0.0042", "$1.23", "$1234.50"]);
        assert_eq!([app_name("claude_code"), app_name("codex"), app_name("-")], ["claude", "codex", "-"]);
    }
}
