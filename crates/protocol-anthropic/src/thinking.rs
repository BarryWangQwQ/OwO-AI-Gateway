//! Extended-thinking request shape per Claude family.
//!
//! Two wire shapes coexist and each family rejects the other (verified by OpenCodex
//! against api.anthropic.com, 2026-09):
//! - adaptive: `thinking: {type: "adaptive"}` + `output_config.effort`
//!   — Sonnet ≥ 5, Opus ≥ 4.7, every Fable.
//! - budget:   `thinking: {type: "enabled", budget_tokens}` — older families.
//!
//! `max_tokens` caps thinking plus visible output, so it is sized to keep output room.

use serde_json::{json, Value};

pub const MIN_THINKING_BUDGET: u32 = 1024;
const OUTPUT_HEADROOM: u32 = 8192;
const OUTPUT_FLOOR: u32 = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    pub major: u32,
    pub minor: u32,
}

/// Parses `claude-<family>-<major>[-<minor>]` anywhere after a `/`-separated prefix.
/// A date suffix (`claude-opus-4-20250514`) is not a minor version.
pub fn family(model: &str) -> Option<Family> {
    let lower = model.to_ascii_lowercase();
    for (i, _) in lower.match_indices("claude-") {
        if i != 0 && lower.as_bytes()[i - 1] != b'/' {
            continue;
        }
        let rest = &lower[i + "claude-".len()..];
        let name: String = rest.chars().take_while(|c| c.is_ascii_alphabetic()).collect();
        let rest = rest[name.len()..].strip_prefix('-')?;
        let major_str: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        let major = major_str.parse().ok()?;
        let rest = &rest[major_str.len()..];
        let minor = match rest.strip_prefix(['-', '.']) {
            Some(tail) => {
                let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
                if (1..=2).contains(&digits.len()) { digits.parse().unwrap_or(0) } else { 0 }
            }
            None => 0,
        };
        if name.is_empty() {
            return None;
        }
        return Some(Family { name, major, minor });
    }
    None
}

fn at_least(model: &str, table: &[(&str, u32, u32)]) -> bool {
    let Some(f) = family(model) else { return false };
    table.iter().any(|(name, major, minor)| f.name == *name && (f.major > *major || (f.major == *major && f.minor >= *minor)))
}

pub fn uses_adaptive(model: &str) -> bool {
    at_least(model, &[("sonnet", 5, 0), ("opus", 4, 7), ("fable", 0, 0)])
}

/// Families that think by default and accept `thinking: {type: "disabled"}`.
pub fn supports_explicit_disable(model: &str) -> bool {
    at_least(model, &[("sonnet", 5, 0)])
}

pub fn budget_for(effort: &str) -> u32 {
    match effort {
        "minimal" => 1024,
        "low" => 4096,
        "high" => 16_384,
        "xhigh" => 24_576,
        "max" => 32_000,
        _ => 8192,
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plan {
    pub thinking: Option<Value>,
    pub effort: Option<String>,
    pub max_tokens: u32,
    /// Thinking forbids `temperature`/`top_p`/`top_k` overrides.
    pub strip_sampling: bool,
}

/// `effort`: the requested reasoning effort (`None` = do not ask for thinking).
pub fn plan(model: &str, effort: Option<&str>, requested_max: Option<u32>, default_max: u32) -> Plan {
    let plain = |thinking: Option<Value>| Plan {
        thinking,
        effort: None,
        max_tokens: requested_max.unwrap_or(default_max),
        strip_sampling: false,
    };
    match effort {
        None | Some("") => plain(None),
        Some("none") => plain(supports_explicit_disable(model).then(|| json!({ "type": "disabled" }))),
        Some(effort) if uses_adaptive(model) => {
            let effort = if effort == "minimal" { "low" } else { effort };
            let floor = budget_for(effort) + OUTPUT_HEADROOM;
            Plan {
                thinking: Some(json!({ "type": "adaptive" })),
                effort: Some(effort.to_string()),
                max_tokens: requested_max.unwrap_or(default_max.max(floor)),
                strip_sampling: true,
            }
        }
        Some(effort) => {
            let want = budget_for(effort);
            let max_tokens = requested_max.unwrap_or(default_max).max(want + OUTPUT_HEADROOM);
            let budget = want.min(max_tokens - OUTPUT_FLOOR).max(MIN_THINKING_BUDGET);
            Plan {
                thinking: Some(json!({ "type": "enabled", "budget_tokens": budget })),
                effort: None,
                max_tokens,
                strip_sampling: true,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_families() {
        assert_eq!(family("claude-sonnet-5"), Some(Family { name: "sonnet".into(), major: 5, minor: 0 }));
        assert_eq!(family("claude-opus-4-8"), Some(Family { name: "opus".into(), major: 4, minor: 8 }));
        assert_eq!(family("claude-opus-4-20250514").unwrap().minor, 0);
        assert_eq!(family("anthropic/claude-opus-5-5").unwrap().minor, 5);
        assert_eq!(family("claude-sonnet-4.6").unwrap().minor, 6);
        assert_eq!(family("my-claude-opus-5"), None);
        assert_eq!(family("gpt-5"), None);
    }

    #[test]
    fn adaptive_boundaries() {
        for m in ["claude-sonnet-5", "claude-opus-4-7", "claude-opus-5-5", "claude-fable-5-1"] {
            assert!(uses_adaptive(m), "{m}");
        }
        for m in ["claude-sonnet-4-6", "claude-opus-4-6", "claude-haiku-4-5", "glm-5"] {
            assert!(!uses_adaptive(m), "{m}");
        }
        assert!(supports_explicit_disable("claude-sonnet-5"));
        assert!(!supports_explicit_disable("claude-fable-5-1"));
    }

    #[test]
    fn adaptive_plan() {
        let p = plan("claude-sonnet-5", Some("high"), None, 64_000);
        assert_eq!(p.thinking, Some(json!({"type": "adaptive"})));
        assert_eq!(p.effort.as_deref(), Some("high"));
        assert_eq!(p.max_tokens, 64_000);
        assert!(p.strip_sampling);
        assert_eq!(plan("claude-opus-5-5", Some("minimal"), None, 8000).effort.as_deref(), Some("low"));
        assert_eq!(plan("claude-opus-5-5", Some("max"), None, 8000).max_tokens, 40_192);
    }

    #[test]
    fn budget_plan_keeps_output_room() {
        let p = plan("claude-haiku-4-5", Some("max"), Some(8192), 64_000);
        assert_eq!(p.max_tokens, 40_192);
        assert_eq!(p.thinking, Some(json!({"type": "enabled", "budget_tokens": 32_000})));
        let p = plan("claude-haiku-4-5", Some("medium"), None, 64_000);
        assert_eq!(p.thinking.unwrap()["budget_tokens"], 8192);
        assert_eq!(p.max_tokens, 64_000);
    }

    #[test]
    fn disable_only_where_accepted() {
        assert_eq!(plan("claude-sonnet-5", Some("none"), None, 1000).thinking, Some(json!({"type": "disabled"})));
        assert_eq!(plan("claude-fable-5-1", Some("none"), None, 1000).thinking, None);
        assert_eq!(plan("claude-sonnet-5", None, None, 1000), Plan { thinking: None, effort: None, max_tokens: 1000, strip_sampling: false });
    }
}
