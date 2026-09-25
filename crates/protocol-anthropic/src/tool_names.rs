use std::collections::HashMap;

use sha2::{Digest, Sha256};

/// Anthropic tool names must match `^[a-zA-Z0-9_-]{1,64}$`. Names outside that set
/// (long flattened MCP names, dotted names) get a stable wire alias, mapped back on decode.
#[derive(Debug, Clone, Default)]
pub struct ToolNames {
    to_original: HashMap<String, String>,
}

const MAX_LEN: usize = 64;

fn valid(name: &str) -> bool {
    !name.is_empty() && name.len() <= MAX_LEN && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

impl ToolNames {
    pub fn wire(&mut self, original: &str) -> String {
        if valid(original) {
            return original.to_string();
        }
        let cleaned: String = original
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
            .collect();
        let hash: String = Sha256::digest(original.as_bytes()).iter().take(4).map(|b| format!("{b:02x}")).collect();
        let keep = cleaned.chars().take(MAX_LEN - 9).collect::<String>();
        let wire = format!("{keep}_{hash}");
        self.to_original.insert(wire.clone(), original.to_string());
        wire
    }

    pub fn original(&self, wire: &str) -> String {
        self.to_original.get(wire).cloned().unwrap_or_else(|| wire.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_only_invalid_names() {
        let mut names = ToolNames::default();
        assert_eq!(names.wire("mcp__node_repl__js"), "mcp__node_repl__js");
        let long = "mcp__some_very_long_server_name_here__and_an_even_longer_tool_name_value";
        let wire = names.wire(long);
        assert!(wire.len() <= 64 && valid(&wire));
        assert_eq!(names.original(&wire), long);
        let dotted = names.wire("server.tool");
        assert_eq!(names.original(&dotted), "server.tool");
    }
}
