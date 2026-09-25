use serde::{Deserialize, Serialize};

/// Normalized token usage. `input_tokens` includes cached tokens; the cache fields
/// break that total down when the upstream reports it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,
}

impl Usage {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Merges a later usage report into this one, keeping the most complete figures.
    /// Upstreams sometimes report input at stream start and output at the end.
    pub fn merge(&mut self, other: &Usage) {
        self.input_tokens = self.input_tokens.max(other.input_tokens);
        self.output_tokens = self.output_tokens.max(other.output_tokens);
        self.cached_input_tokens = other.cached_input_tokens.or(self.cached_input_tokens);
        self.cache_creation_input_tokens =
            other.cache_creation_input_tokens.or(self.cache_creation_input_tokens);
        self.reasoning_tokens = other.reasoning_tokens.or(self.reasoning_tokens);
    }
}
