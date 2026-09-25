//! `/v1/responses` request → canonical request.

use serde_json::{json, Value};
use owo_core::{
    ContentBlock, ErrorKind, FileInput, FileSource, ImageInput, ImageSource, InboundProtocol,
    Message, ModelError, ModelRequest, OutputFormat, ReasoningBlock, ReasoningConfig, Role,
    ToolCall, ToolCallKind, ToolChoice, ToolDefinition, ToolResult, ToolResultContent,
};
use owo_protocol_openai_chat::json::{invalid, str_field, take_bool, take_f64, take_string, take_u32};

/// Output-shaping preferences that are not part of the canonical request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ResponsesOptions {
    /// `include` contains `reasoning.encrypted_content`.
    pub include_encrypted_reasoning: bool,
    pub store: bool,
    /// Flat tool name → (namespace, name) for tools declared inside `namespace` groups.
    /// The canonical request carries the flat name; the encoder restores both parts.
    pub tool_namespaces: ToolNamespaces,
}

pub type ToolNamespaces = std::collections::BTreeMap<String, (String, String)>;

/// Codex's reserved group whose members are ordinary top-level tools.
const BUILTIN_FUNCTIONS_NAMESPACE: &str = "functions";

pub fn flat_tool_name(namespace: Option<&str>, name: &str) -> String {
    match namespace {
        Some(ns) if !ns.is_empty() && ns != BUILTIN_FUNCTIONS_NAMESPACE => format!("{ns}__{name}"),
        _ => name.to_string(),
    }
}

pub struct DecodedRequest {
    pub request: ModelRequest,
    pub options: ResponsesOptions,
}

pub fn decode_request(body: Value, request_id: impl Into<String>) -> Result<DecodedRequest, ModelError> {
    let Value::Object(mut obj) = body else {
        return Err(invalid("request body must be a JSON object"));
    };
    let model = take_string(&mut obj, "model")?.ok_or_else(|| invalid("`model` is required"))?;
    let mut req = ModelRequest::new(request_id, model);
    req.metadata.inbound = Some(InboundProtocol::OpenaiResponses);

    if obj.get("previous_response_id").is_some_and(|v| !v.is_null()) {
        return Err(ModelError::unsupported(
            "`previous_response_id` requires server-side response storage, which OwO AI Gateway does not provide; send the full input instead",
        ));
    }
    if take_bool(&mut obj, "background")? == Some(true) {
        return Err(ModelError::unsupported("background responses are not supported"));
    }
    if obj.get("conversation").is_some_and(|v| !v.is_null()) {
        return Err(ModelError::unsupported("server-side conversations are not supported"));
    }

    if let Some(instructions) = take_string(&mut obj, "instructions")? {
        if !instructions.is_empty() {
            req.system.push(ContentBlock::text(instructions));
        }
    }
    match obj.remove("input") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => req.messages.push(Message::user_text(s)),
        Some(Value::Array(items)) => {
            for (i, item) in items.into_iter().enumerate() {
                decode_item(item, i, &mut req)?;
            }
        }
        Some(_) => return Err(invalid("`input` must be a string or an array")),
    }

    let mut tool_namespaces = ToolNamespaces::new();
    if let Some(tools) = obj.remove("tools") {
        req.tools = decode_tools(tools, &mut tool_namespaces)?;
    }
    if let Some(choice) = obj.remove("tool_choice") {
        req.tool_choice = decode_tool_choice(choice)?;
    }
    if let Some(reasoning) = obj.remove("reasoning").filter(|v| !v.is_null()) {
        let effort = str_field(&reasoning, "effort").map(|s| s.to_ascii_lowercase());
        let summary = str_field(&reasoning, "summary").or_else(|| str_field(&reasoning, "generate_summary"));
        if effort.is_some() || summary.is_some() {
            req.reasoning = Some(ReasoningConfig { effort, summary: summary.map(str::to_string), budget_tokens: None });
        }
    }
    if let Some(text) = obj.remove("text").filter(|v| !v.is_null()) {
        if let Some(format) = text.get("format") {
            req.output_format = decode_format(format)?;
        }
        req.metadata.verbosity = str_field(&text, "verbosity").map(str::to_string);
    }

    req.max_output_tokens = take_u32(&mut obj, "max_output_tokens")?;
    req.sampling.temperature = take_f64(&mut obj, "temperature")?;
    req.sampling.top_p = take_f64(&mut obj, "top_p")?;
    req.stream = take_bool(&mut obj, "stream")?.unwrap_or(false);
    req.metadata.parallel_tool_calls = take_bool(&mut obj, "parallel_tool_calls")?;
    req.metadata.prompt_cache_key = take_string(&mut obj, "prompt_cache_key")?;
    req.metadata.user = take_string(&mut obj, "user")?.or(take_string(&mut obj, "safety_identifier")?);
    req.metadata.service_tier = take_string(&mut obj, "service_tier")?;

    let mut options = ResponsesOptions {
        store: take_bool(&mut obj, "store")?.unwrap_or(false),
        tool_namespaces,
        ..Default::default()
    };
    if let Some(Value::Array(include)) = obj.remove("include") {
        options.include_encrypted_reasoning = include.iter().any(|v| v == "reasoning.encrypted_content");
    }
    // Stream-shaping only; nothing to route.
    obj.remove("stream_options");

    req.metadata.extra = obj.into_iter().collect();
    Ok(DecodedRequest { request: req, options })
}

fn decode_item(item: Value, index: usize, req: &mut ModelRequest) -> Result<(), ModelError> {
    let Value::Object(mut item) = item else {
        return Err(invalid(format!("input[{index}] must be an object")));
    };
    let kind = match take_string(&mut item, "type")? {
        Some(t) => t,
        None if item.contains_key("role") => "message".to_string(),
        None => return Err(invalid(format!("input[{index}].type is required"))),
    };
    match kind.as_str() {
        "message" => {
            let role = take_string(&mut item, "role")?.ok_or_else(|| invalid(format!("input[{index}].role is required")))?;
            let content = message_content(item.remove("content"), index)?;
            match role.as_str() {
                "system" | "developer" => {
                    if content.iter().any(|b| b.as_text().is_none()) {
                        return Err(ModelError::unsupported(format!("input[{index}]: {role} messages support text only")));
                    }
                    if req.messages.is_empty() {
                        req.system.extend(content);
                    } else {
                        req.messages.push(Message::new(Role::System, content));
                    }
                }
                "user" => req.messages.push(Message::new(Role::User, content)),
                "assistant" => push_assistant(req, content),
                other => return Err(invalid(format!("input[{index}]: unknown role `{other}`"))),
            }
        }
        "function_call" | "custom_tool_call" => {
            let custom = kind == "custom_tool_call";
            let call_id = take_string(&mut item, "call_id")?
                .ok_or_else(|| invalid(format!("input[{index}].call_id is required")))?;
            let name = take_string(&mut item, "name")?.unwrap_or_default();
            let name = flat_tool_name(take_string(&mut item, "namespace")?.as_deref(), &name);
            let arguments = take_string(&mut item, if custom { "input" } else { "arguments" })?.unwrap_or_default();
            let kind = if custom { ToolCallKind::Custom } else { ToolCallKind::Function };
            push_assistant(req, vec![ContentBlock::ToolCall(ToolCall { id: call_id, name, arguments, kind })]);
        }
        "function_call_output" | "custom_tool_call_output" => {
            let custom = kind == "custom_tool_call_output";
            let call_id = take_string(&mut item, "call_id")?
                .ok_or_else(|| invalid(format!("input[{index}].call_id is required")))?;
            let content = tool_output(item.remove("output"), index)?;
            let kind = if custom { ToolCallKind::Custom } else { ToolCallKind::Function };
            push_tool_result(req, ContentBlock::ToolResult(ToolResult { call_id, content, is_error: false, kind }));
        }
        "reasoning" => {
            let summary: String = item
                .get("summary")
                .and_then(Value::as_array)
                .map(|parts| parts.iter().filter_map(|p| str_field(p, "text")).collect::<Vec<_>>().join("\n\n"))
                .unwrap_or_default();
            let raw: String = item
                .get("content")
                .and_then(Value::as_array)
                .map(|parts| parts.iter().filter_map(|p| str_field(p, "text")).collect::<Vec<_>>().join(""))
                .unwrap_or_default();
            let (signature, encrypted_content) = match take_string(&mut item, "encrypted_content")? {
                Some(blob) => match blob.strip_prefix(crate::SIGNATURE_PREFIX) {
                    Some(sig) => (Some(sig.to_string()), None),
                    None => (None, Some(blob)),
                },
                None => (None, None),
            };
            let block = ReasoningBlock {
                text: if raw.is_empty() { summary.clone() } else { raw },
                summary: (!summary.is_empty()).then_some(summary),
                signature,
                encrypted_content,
                id: take_string(&mut item, "id")?,
            };
            push_assistant(req, vec![ContentBlock::Reasoning(block)]);
        }
        other => {
            return Err(ModelError::new(
                ErrorKind::UnsupportedCapability,
                format!("input item type `{other}` cannot be routed to an external provider"),
            ));
        }
    }
    Ok(())
}

/// Responses items for one assistant turn (reasoning, text, calls) arrive as separate
/// items; they fold into a single canonical assistant message.
fn push_assistant(req: &mut ModelRequest, blocks: Vec<ContentBlock>) {
    if let Some(last) = req.messages.last_mut() {
        if last.role == Role::Assistant {
            last.content.extend(blocks);
            return;
        }
    }
    req.messages.push(Message::new(Role::Assistant, blocks));
}

fn push_tool_result(req: &mut ModelRequest, block: ContentBlock) {
    if let Some(last) = req.messages.last_mut() {
        if last.role == Role::User && last.content.iter().all(|b| matches!(b, ContentBlock::ToolResult(_))) {
            last.content.push(block);
            return;
        }
    }
    req.messages.push(Message::new(Role::User, vec![block]));
}

fn message_content(content: Option<Value>, index: usize) -> Result<Vec<ContentBlock>, ModelError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![ContentBlock::text(s)]),
        Some(Value::Array(parts)) => parts.into_iter().map(|p| content_part(p, index)).collect(),
        Some(_) => Err(invalid(format!("input[{index}].content must be a string or an array"))),
    }
}

fn content_part(part: Value, index: usize) -> Result<ContentBlock, ModelError> {
    match str_field(&part, "type") {
        Some("input_text" | "output_text" | "text") => Ok(ContentBlock::text(str_field(&part, "text").unwrap_or_default())),
        Some("refusal") => Ok(ContentBlock::text(str_field(&part, "refusal").unwrap_or_default())),
        Some("input_image") => Ok(ContentBlock::Image(image(&part, index)?)),
        Some("input_file") => {
            let filename = str_field(&part, "filename").map(str::to_string);
            let source = if let Some(id) = str_field(&part, "file_id") {
                FileSource::FileId { id: id.to_string() }
            } else if let Some(url) = str_field(&part, "file_url") {
                FileSource::Url { url: url.to_string() }
            } else if let Some(data) = str_field(&part, "file_data") {
                return Ok(ContentBlock::File(match ImageSource::from_url(data) {
                    ImageSource::Base64 { media_type, data } => {
                        FileInput { filename, media_type: Some(media_type), source: FileSource::Base64 { data } }
                    }
                    ImageSource::Url { url } => FileInput { filename, media_type: None, source: FileSource::Base64 { data: url } },
                }));
            } else {
                return Err(invalid(format!("input[{index}]: input_file needs file_id, file_url, or file_data")));
            };
            Ok(ContentBlock::File(FileInput { filename, media_type: None, source }))
        }
        other => Err(ModelError::unsupported(format!(
            "input[{index}]: content part type `{}` is not supported",
            other.unwrap_or("?")
        ))),
    }
}

fn image(part: &Value, index: usize) -> Result<ImageInput, ModelError> {
    let detail = str_field(part, "detail").map(str::to_string);
    if let Some(url) = str_field(part, "image_url") {
        return Ok(ImageInput { source: ImageSource::from_url(url), detail });
    }
    if str_field(part, "file_id").is_some() {
        return Err(ModelError::unsupported(format!("input[{index}]: image file_id references are provider-hosted and cannot be routed")));
    }
    Err(invalid(format!("input[{index}]: input_image needs image_url")))
}

fn tool_output(output: Option<Value>, index: usize) -> Result<Vec<ToolResultContent>, ModelError> {
    match output {
        None | Some(Value::Null) => Ok(vec![ToolResultContent::Text { text: String::new() }]),
        Some(Value::String(s)) => Ok(vec![ToolResultContent::Text { text: s }]),
        Some(Value::Array(parts)) => parts
            .into_iter()
            .map(|p| match str_field(&p, "type") {
                Some("input_text" | "output_text" | "text") => {
                    Ok(ToolResultContent::Text { text: str_field(&p, "text").unwrap_or_default().to_string() })
                }
                Some("input_image") => Ok(ToolResultContent::Image(image(&p, index)?)),
                other => Err(ModelError::unsupported(format!(
                    "input[{index}]: tool output part `{}` is not supported",
                    other.unwrap_or("?")
                ))),
            })
            .collect(),
        Some(_) => Err(invalid(format!("input[{index}].output must be a string or an array"))),
    }
}

fn decode_tools(tools: Value, namespaces: &mut ToolNamespaces) -> Result<Vec<ToolDefinition>, ModelError> {
    let Value::Array(tools) = tools else {
        return Err(invalid("`tools` must be an array"));
    };
    let mut out = Vec::new();
    for t in tools {
        if str_field(&t, "type") == Some("namespace") {
            let ns = str_field(&t, "name").filter(|n| !n.is_empty()).ok_or_else(|| {
                ModelError::new(ErrorKind::InvalidToolSchema, "namespace tool group is missing `name`")
            })?;
            let Some(children) = t.get("tools").and_then(Value::as_array) else { continue };
            for child in children {
                if let Some(def) = decode_tool(child, Some(ns))? {
                    if ns != BUILTIN_FUNCTIONS_NAMESPACE {
                        let child_name = str_field(child, "name").unwrap_or_default().to_string();
                        namespaces.insert(def.name().to_string(), (ns.to_string(), child_name));
                    }
                    out.push(def);
                }
            }
            continue;
        }
        if let Some(def) = decode_tool(&t, None)? {
            out.push(def);
        }
    }
    Ok(out)
}

fn decode_tool(t: &Value, namespace: Option<&str>) -> Result<Option<ToolDefinition>, ModelError> {
    Ok(Some(match str_field(t, "type").unwrap_or("function") {
        "function" => {
            let name = str_field(t, "name").filter(|n| !n.is_empty()).ok_or_else(|| {
                ModelError::new(ErrorKind::InvalidToolSchema, "function tool is missing `name`")
            })?;
            ToolDefinition::Function {
                name: flat_tool_name(namespace, name),
                description: str_field(t, "description").map(str::to_string),
                parameters: t.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                strict: t.get("strict").and_then(Value::as_bool),
            }
        }
        "custom" => ToolDefinition::Custom {
            name: flat_tool_name(namespace, str_field(t, "name").unwrap_or_default()),
            description: str_field(t, "description").map(str::to_string),
            format: t.get("format").cloned(),
        },
        "namespace" => return Err(ModelError::unsupported("nested tool namespaces are not supported")),
        other => ToolDefinition::Hosted { kind: other.to_string(), config: t.clone() },
    }))
}

fn decode_tool_choice(choice: Value) -> Result<Option<ToolChoice>, ModelError> {
    Ok(Some(match &choice {
        Value::Null => return Ok(None),
        Value::String(s) => match s.as_str() {
            "auto" => ToolChoice::Auto,
            "none" => ToolChoice::None,
            "required" => ToolChoice::Required,
            other => return Err(invalid(format!("unknown tool_choice `{other}`"))),
        },
        Value::Object(_) => match (str_field(&choice, "type"), str_field(&choice, "name")) {
            (Some("function" | "custom"), Some(name)) => {
                ToolChoice::Tool { name: flat_tool_name(str_field(&choice, "namespace"), name) }
            }
            (Some(kind), _) => return Err(ModelError::unsupported(format!("tool_choice type `{kind}` is not supported"))),
            _ => return Err(invalid("tool_choice object needs `type` and `name`")),
        },
        _ => return Err(invalid("`tool_choice` must be a string or an object")),
    }))
}

fn decode_format(format: &Value) -> Result<Option<OutputFormat>, ModelError> {
    Ok(match str_field(format, "type") {
        None | Some("text") => None,
        Some("json_object") => Some(OutputFormat::JsonObject),
        Some("json_schema") => Some(OutputFormat::JsonSchema {
            name: str_field(format, "name").unwrap_or("response").to_string(),
            schema: format.get("schema").cloned().unwrap_or_else(|| json!({})),
            description: str_field(format, "description").map(str::to_string),
            strict: format.get("strict").and_then(Value::as_bool),
        }),
        Some(other) => return Err(ModelError::unsupported(format!("text.format `{other}` is not supported"))),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Shape of a Codex turn: instructions, developer/user context, a prior tool round trip.
    fn codex_like() -> Value {
        json!({
            "model": "gpt-ds",
            "instructions": "You are Codex.",
            "input": [
                {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "sandbox: workspace-write"}]},
                {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "fix the bug"}]},
                {"type": "reasoning", "id": "rs_1", "summary": [{"type": "summary_text", "text": "look at main"}], "encrypted_content": "enc"},
                {"type": "function_call", "id": "fc_1", "call_id": "call_1", "name": "shell", "arguments": "{\"command\":[\"ls\"]}"},
                {"type": "function_call_output", "call_id": "call_1", "output": "main.rs"},
                {"type": "custom_tool_call", "call_id": "call_2", "name": "apply_patch", "input": "*** Begin Patch"},
                {"type": "custom_tool_call_output", "call_id": "call_2", "output": "Done"},
                {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "Fixed."}]}
            ],
            "tools": [
                {"type": "function", "name": "shell", "parameters": {"type": "object"}, "strict": false},
                {"type": "custom", "name": "apply_patch", "format": {"type": "grammar", "syntax": "lark", "definition": "start: x"}}
            ],
            "tool_choice": "auto",
            "parallel_tool_calls": false,
            "reasoning": {"effort": "medium", "summary": "auto"},
            "store": false,
            "stream": true,
            "include": ["reasoning.encrypted_content"],
            "prompt_cache_key": "session-1",
            "text": {"verbosity": "low"}
        })
    }

    #[test]
    fn decodes_codex_turn() {
        let d = decode_request(codex_like(), "r").unwrap();
        let req = d.request;
        assert!(d.options.include_encrypted_reasoning);
        assert_eq!(req.system.len(), 2, "instructions + leading developer message");
        assert_eq!(req.messages.len(), 6);
        assert_eq!(req.messages[0].role, Role::User);

        let a1 = &req.messages[1];
        assert_eq!(a1.role, Role::Assistant);
        assert!(matches!(&a1.content[0], ContentBlock::Reasoning(r)
            if r.text == "look at main" && r.encrypted_content.as_deref() == Some("enc") && r.id.as_deref() == Some("rs_1")));
        assert!(matches!(&a1.content[1], ContentBlock::ToolCall(c) if c.id == "call_1" && c.kind == ToolCallKind::Function));

        assert!(matches!(&req.messages[2].content[0], ContentBlock::ToolResult(r) if r.text() == "main.rs"));
        // Custom call folds into a new assistant turn, followed by its result and the final text.
        assert!(matches!(&req.messages[3].content[0], ContentBlock::ToolCall(c) if c.kind == ToolCallKind::Custom));
        assert!(matches!(&req.messages[4].content[0], ContentBlock::ToolResult(r) if r.kind == ToolCallKind::Custom));
        assert_eq!(req.messages[4].content.len(), 1);

        assert!(matches!(&req.tools[1], ToolDefinition::Custom { name, .. } if name == "apply_patch"));
        assert_eq!(req.reasoning.as_ref().unwrap().effort.as_deref(), Some("medium"));
        assert_eq!(req.metadata.parallel_tool_calls, Some(false));
        assert_eq!(req.metadata.prompt_cache_key.as_deref(), Some("session-1"));
        assert_eq!(req.metadata.verbosity.as_deref(), Some("low"));
        assert!(req.metadata.extra.is_empty(), "{:?}", req.metadata.extra);
    }

    #[test]
    fn assistant_text_after_tool_output_is_a_new_turn() {
        let req = decode_request(codex_like(), "r").unwrap().request;
        let last = req.messages.last().unwrap();
        assert_eq!(last.role, Role::Assistant);
        assert_eq!(last.content, vec![ContentBlock::text("Fixed.")]);
        let v = decode_request(
            json!({"model": "m", "input": [
                {"role": "user", "content": "q"},
                {"role": "assistant", "content": "a"}
            ]}),
            "r",
        )
        .unwrap()
        .request;
        assert_eq!(v.messages[1].role, Role::Assistant);
    }

    #[test]
    fn string_input_and_hosted_tools() {
        let req = decode_request(
            json!({"model": "m", "input": "hello", "tools": [{"type": "web_search"}], "max_output_tokens": 10}),
            "r",
        )
        .unwrap()
        .request;
        assert_eq!(req.messages[0], Message::user_text("hello"));
        assert!(matches!(&req.tools[0], ToolDefinition::Hosted { kind, .. } if kind == "web_search"));
        assert_eq!(req.max_output_tokens, Some(10));
    }

    #[test]
    fn namespace_groups_are_flattened() {
        let d = decode_request(
            json!({"model": "m",
                "input": [
                    {"role": "user", "content": "go"},
                    {"type": "function_call", "call_id": "c1", "name": "spawn_agent", "namespace": "multi_agent_v1", "arguments": "{}"},
                    {"type": "function_call_output", "call_id": "c1", "output": "ok"}
                ],
                "tools": [
                    {"type": "namespace", "name": "multi_agent_v1", "description": "Sub-agents", "tools": [
                        {"type": "function", "name": "spawn_agent", "parameters": {"type": "object"}},
                        {"type": "function", "name": "close_agent", "parameters": {"type": "object"}}
                    ]},
                    {"type": "namespace", "name": "functions", "tools": [
                        {"type": "function", "name": "plain", "parameters": {"type": "object"}}
                    ]},
                    {"type": "web_search", "external_web_access": false}
                ],
                "tool_choice": {"type": "function", "name": "close_agent", "namespace": "multi_agent_v1"}
            }),
            "r",
        )
        .unwrap();
        let names: Vec<_> = d.request.tools.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(names, vec!["multi_agent_v1__spawn_agent", "multi_agent_v1__close_agent", "plain", "web_search"]);
        assert_eq!(
            d.options.tool_namespaces.get("multi_agent_v1__close_agent"),
            Some(&("multi_agent_v1".to_string(), "close_agent".to_string()))
        );
        assert!(!d.options.tool_namespaces.contains_key("plain"));
        assert!(matches!(&d.request.messages[1].content[0], ContentBlock::ToolCall(c) if c.name == "multi_agent_v1__spawn_agent"));
        assert_eq!(d.request.tool_choice, Some(ToolChoice::Tool { name: "multi_agent_v1__close_agent".into() }));
    }

    #[test]
    fn stateful_features_are_rejected() {
        for body in [
            json!({"model": "m", "input": "x", "previous_response_id": "resp_1"}),
            json!({"model": "m", "input": "x", "background": true}),
            json!({"model": "m", "input": [{"type": "item_reference", "id": "x"}]}),
        ] {
            let err = decode_request(body, "r").err().unwrap();
            assert_eq!(err.kind, ErrorKind::UnsupportedCapability, "{}", err.message);
        }
    }

    #[test]
    fn images_and_structured_output() {
        let req = decode_request(
            json!({"model": "m",
                "input": [{"role": "user", "content": [
                    {"type": "input_text", "text": "what is this"},
                    {"type": "input_image", "image_url": "data:image/jpeg;base64,/9j", "detail": "low"}
                ]}],
                "text": {"format": {"type": "json_schema", "name": "out", "schema": {"type": "object"}, "strict": true}}
            }),
            "r",
        )
        .unwrap()
        .request;
        assert!(req.has_images());
        assert!(matches!(req.output_format, Some(OutputFormat::JsonSchema { strict: Some(true), .. })));
    }
}
