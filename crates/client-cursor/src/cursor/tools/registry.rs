//! Owns the static Cursor Tool schema catalog and mode-specific selections.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::{model::ToolDefinition, Error, Result};

#[derive(Deserialize)]
struct Manifest {
    tools: Vec<ManifestTool>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum ManifestTool {
    Name(String),
    Variant { name: String, variant: String },
}

fn is_semble(entry: &ManifestTool) -> bool {
    let (ManifestTool::Name(name) | ManifestTool::Variant { name, .. }) = entry;
    crate::search::SEMBLE_TOOLS.contains(&name.as_str())
}

pub(crate) struct ToolRegistry {
    tools: HashMap<String, ToolDefinition>,
    variants: HashMap<String, ToolDefinition>,
}

impl ToolRegistry {
    pub(crate) fn parse(json: &str) -> Result<Self> {
        let value: Value = serde_json::from_str(json)?;
        let tools = value
            .get("tools")
            .and_then(Value::as_array)
            .ok_or_else(|| Error::Config("tools.json is missing tools".into()))?
            .iter()
            .map(parse_tool)
            .map(|result| result.map(|tool| (tool.name.clone(), tool)))
            .collect::<Result<HashMap<_, _>>>()?;
        let variants = value
            .get("variants")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|variants| variants.iter())
            .map(|(name, value)| parse_tool(value).map(|tool| (name.clone(), tool)))
            .collect::<Result<HashMap<_, _>>>()?;
        Ok(Self { tools, variants })
    }

    pub(crate) fn select_json(&self, manifest: &str) -> Result<Vec<ToolDefinition>> {
        let manifest: Manifest = serde_json::from_str(manifest)?;
        manifest
            .tools
            .iter()
            .filter(|entry| cfg!(feature = "semble") || !is_semble(entry))
            .map(|entry| match entry {
                ManifestTool::Name(name) => self.tools.get(name).cloned().ok_or_else(|| {
                    Error::Config(format!("tool manifest references unknown schema: {name}"))
                }),
                ManifestTool::Variant { name, variant } => self
                    .variants
                    .get(&format!("{name}.{variant}"))
                    .cloned()
                    .ok_or_else(|| {
                        Error::Config(format!(
                            "tool manifest references unknown variant: {name}.{variant}"
                        ))
                    }),
            })
            .collect()
    }
}

fn parse_tool(tool: &Value) -> Result<ToolDefinition> {
    let function = tool
        .get("function")
        .ok_or_else(|| Error::Config("tool is missing function".into()))?;
    Ok(ToolDefinition {
        name: function
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Config("tool is missing name".into()))?
            .into(),
        description: function
            .get("description")
            .and_then(Value::as_str)
            .ok_or_else(|| Error::Config("tool is missing description".into()))?
            .into(),
        parameters: function
            .get("parameters")
            .cloned()
            .ok_or_else(|| Error::Config("tool is missing parameters".into()))?,
    })
}

#[cfg(test)]
mod tests {
    use super::ToolRegistry;

    #[test]
    fn semble_tools_are_offered_only_when_compiled_in() {
        let registry = ToolRegistry::parse(include_str!("../../../prompt/cursor/tools.json")).unwrap();
        let tools = registry.select_json(include_str!("../../../prompt/cursor/modes/agent.json")).unwrap();
        assert_eq!(tools.iter().any(|t| t.name == "SembleSearch"), cfg!(feature = "semble"));
        assert!(tools.iter().any(|t| t.name == "Read"));
    }
}
