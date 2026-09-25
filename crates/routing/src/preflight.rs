use owo_core::{ModelError, ModelRequest, OutputFormat, ReasoningConfig};
use owo_registry::Model;

/// Applies model defaults and rejects requests needing a capability the model is
/// known to lack. Unknown capabilities (`None`) are allowed through: the upstream
/// is the authority, and OwO AI Gateway only refuses what it knows cannot work.
pub fn preflight(req: &mut ModelRequest, model: &Model) -> Result<(), ModelError> {
    let caps = &model.capabilities;
    let deny = |what: &str| ModelError::unsupported(format!("model `{}` does not support {what}", model.id));

    if caps.vision == Some(false) && req.has_images() {
        return Err(deny("image input"));
    }
    if caps.tools == Some(false) && !req.tools.is_empty() {
        return Err(deny("tools"));
    }
    if caps.streaming == Some(false) && req.stream {
        return Err(deny("streaming"));
    }
    if caps.structured_output == Some(false)
        && matches!(req.output_format, Some(OutputFormat::JsonSchema { .. } | OutputFormat::JsonObject))
    {
        return Err(deny("structured output"));
    }
    if caps.parallel_tools == Some(false) && !req.tools.is_empty() {
        req.metadata.parallel_tool_calls = Some(false);
    }

    let requested = req.reasoning.as_ref().and_then(|r| r.effort.clone());
    match requested {
        Some(effort) => {
            if caps.reasoning == Some(false) {
                if effort != "none" {
                    return Err(deny("reasoning effort"));
                }
            } else if !model.reasoning_efforts.is_empty() && !model.reasoning_efforts.contains(&effort) {
                return Err(ModelError::unsupported(format!(
                    "model `{}` does not support reasoning effort `{effort}` (supported: {})",
                    model.id,
                    model.reasoning_efforts.join(", ")
                )));
            }
        }
        None => {
            if let Some(default) = &model.default_reasoning_effort {
                let r = req.reasoning.get_or_insert_with(ReasoningConfig::default);
                r.effort = Some(default.clone());
            }
        }
    }

    if let (Some(requested), Some(limit)) = (req.max_output_tokens, model.max_output_tokens) {
        req.max_output_tokens = Some(requested.min(limit));
    }
    Ok(())
}
