//! Model ids as Claude Code sees them.
//!
//! Claude Code's `/model` picker only lists gateway models whose id starts with
//! `claude` or `anthropic`, so other OwO AI Gateway models are published under a reversible
//! prefix. Ids chosen in the picker may also carry a `[1m]` context marker.

/// Prefix for OwO AI Gateway models whose own id would be hidden by the picker.
pub const PICKER_PREFIX: &str = "claude-owo--";
const CONTEXT_MARKER: &str = "[1m]";

fn picker_visible(id: &str) -> bool {
    let lower = id.to_ascii_lowercase();
    lower.starts_with("claude") || lower.starts_with("anthropic")
}

/// The id published to Claude Code for an OwO AI Gateway model id.
pub fn picker_id(id: &str) -> String {
    if picker_visible(id) { id.to_string() } else { format!("{PICKER_PREFIX}{id}") }
}

/// Candidate OwO AI Gateway model ids for an id Claude Code sent, most specific first:
/// the id without markers and picker prefix, then the same without a date suffix
/// (`claude-haiku-4-5-20251001` → `claude-haiku-4-5`).
pub fn candidates(requested: &str) -> Vec<String> {
    let mut id = requested.trim();
    if id.len() >= CONTEXT_MARKER.len() && id[id.len() - CONTEXT_MARKER.len()..].eq_ignore_ascii_case(CONTEXT_MARKER) {
        id = &id[..id.len() - CONTEXT_MARKER.len()];
    }
    let id = id.strip_prefix(PICKER_PREFIX).unwrap_or(id);
    let mut out = vec![id.to_string()];
    if let Some((base, date)) = id.rsplit_once('-') {
        if date.len() == 8 && date.chars().all(|c| c.is_ascii_digit()) && !base.is_empty() {
            out.push(base.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_ids_round_trip() {
        assert_eq!(picker_id("claude-sonnet-5"), "claude-sonnet-5");
        assert_eq!(picker_id("deepseek-chat"), "claude-owo--deepseek-chat");
        assert_eq!(picker_id("openrouter/qwen3"), "claude-owo--openrouter/qwen3");
        assert_eq!(candidates("claude-owo--deepseek-chat"), ["deepseek-chat"]);
        assert_eq!(candidates("claude-owo--openrouter/qwen3[1M]"), ["openrouter/qwen3"]);
    }

    #[test]
    fn date_suffix_is_a_fallback() {
        assert_eq!(candidates("claude-haiku-4-5-20251001"), ["claude-haiku-4-5-20251001", "claude-haiku-4-5"]);
        assert_eq!(candidates("claude-opus-5-5[1m]"), ["claude-opus-5-5"]);
        assert_eq!(candidates("gpt-5-2025"), ["gpt-5-2025"]);
    }
}
