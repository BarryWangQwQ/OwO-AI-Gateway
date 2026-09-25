use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolDefinition {
    Function {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        /// JSON Schema of the arguments object.
        parameters: Value,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        strict: Option<bool>,
    },
    /// Free-form input tool. `format` carries the inbound grammar/format description verbatim.
    Custom {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        format: Option<Value>,
    },
    /// Provider-hosted tool (web search, code interpreter, ...). Only providers that
    /// declare support for `kind` may accept it; everyone else must reject the request.
    Hosted { kind: String, config: Value },
}

impl ToolDefinition {
    pub fn name(&self) -> &str {
        match self {
            ToolDefinition::Function { name, .. } | ToolDefinition::Custom { name, .. } => name,
            ToolDefinition::Hosted { kind, .. } => kind,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool { name: String },
}
