//! Canonical request → Anthropic Messages request body.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use owo_core::{
    ContentBlock, FileSource, ImageInput, ImageSource, Message, ModelError, ModelRequest, OutputFormat, Role,
    ToolCallKind, ToolChoice, ToolDefinition, ToolResultContent,
};

use crate::thinking;
use crate::tool_names::ToolNames;

/// Notes the caller should log at warning level start with this.
pub const WARN_NOTE_PREFIX: &str = "omitted";

#[derive(Debug, Clone)]
pub struct EncodeOptions {
    /// `max_tokens` when the request sets none (Anthropic requires the field).
    pub default_max_tokens: u32,
    /// Place `cache_control` breakpoints (tools, system, last two user turns).
    pub prompt_caching: bool,
    /// Ask for thinking at the requested effort. Off for models with no declared efforts.
    pub thinking: bool,
}

impl Default for EncodeOptions {
    fn default() -> Self {
        Self { default_max_tokens: 32_000, prompt_caching: true, thinking: true }
    }
}

pub struct Encoded {
    pub body: Value,
    pub custom_tools: HashSet<String>,
    pub tool_names: ToolNames,
    pub notes: Vec<String>,
}

/// OpenAI `encrypted_content` blobs (Fernet tokens) are never Anthropic data.
fn is_foreign_blob(data: &str) -> bool {
    data.starts_with("gAAAA")
}

/// A server-tool definition in Anthropic wire form: a `type` with a date version suffix.
fn is_anthropic_server_tool(config: &Value) -> bool {
    config.get("type").and_then(Value::as_str).and_then(|t| t.rsplit_once('_')).is_some_and(|(name, version)| {
        !name.is_empty() && version.len() == 8 && version.chars().all(|c| c.is_ascii_digit())
    })
}

fn custom_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "input": { "type": "string", "description": "The raw tool input." } },
        "required": ["input"],
    })
}

/// Tool-use ids must match `^[a-zA-Z0-9_-]+$`; the same mapping is applied to results.
fn tool_id(id: &str) -> String {
    let cleaned: String = id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
    if cleaned.is_empty() { "toolu_empty".into() } else { cleaned }
}

fn image(img: &ImageInput) -> Value {
    match &img.source {
        ImageSource::Base64 { media_type, data } => {
            json!({ "type": "image", "source": { "type": "base64", "media_type": media_type, "data": data } })
        }
        ImageSource::Url { url } => json!({ "type": "image", "source": { "type": "url", "url": url } }),
    }
}

struct Turns {
    turns: Vec<(&'static str, Vec<Value>)>,
}

impl Turns {
    /// Appends blocks to the conversation, merging consecutive turns of the same role
    /// (Anthropic requires strict user/assistant alternation).
    fn push(&mut self, role: &'static str, blocks: Vec<Value>) {
        if blocks.is_empty() {
            return;
        }
        match self.turns.last_mut() {
            Some((last, content)) if *last == role => content.extend(blocks),
            _ => self.turns.push((role, blocks)),
        }
    }
}

pub fn encode_request(req: &ModelRequest, upstream_model: &str, opts: &EncodeOptions) -> Result<Encoded, ModelError> {
    let mut notes = Vec::new();
    let mut names = ToolNames::default();
    let mut body = Map::new();
    body.insert("model".into(), json!(upstream_model));

    // System prompt.
    let mut system = Vec::new();
    for b in &req.system {
        match b.as_text() {
            Some(t) if !t.is_empty() => system.push(json!({ "type": "text", "text": t })),
            Some(_) => {}
            None => return Err(ModelError::unsupported("non-text system content is not supported by Anthropic")),
        }
    }

    // Conversation.
    let mut turns = Turns { turns: Vec::new() };
    let mut dropped_reasoning = 0usize;
    for m in &req.messages {
        encode_message(m, &mut turns, &mut names, &mut dropped_reasoning)?;
    }
    if dropped_reasoning > 0 {
        notes.push(format!("{dropped_reasoning} reasoning block(s) from another provider not replayed"));
    }
    if turns.turns.first().is_some_and(|(role, _)| *role == "assistant") {
        turns.turns.insert(0, ("user", vec![json!({ "type": "text", "text": "(continue)" })]));
    }
    if turns.turns.is_empty() {
        return Err(ModelError::invalid_request("the request has no messages"));
    }

    // Tools.
    let mut custom_tools = HashSet::new();
    let mut tools = Vec::new();
    let mut hosted = Vec::new();
    for t in &req.tools {
        match t {
            ToolDefinition::Function { name, description, parameters, .. } => {
                // Anthropic requires a plain object root (no top-level oneOf/anyOf/allOf).
                let schema = owo_core::schema::object_root(parameters);
                let mut tool = json!({ "name": names.wire(name), "input_schema": schema });
                if let Some(d) = description.as_ref().filter(|d| !d.is_empty()) {
                    tool["description"] = json!(d);
                }
                tools.push(tool);
            }
            ToolDefinition::Custom { name, description, format } => {
                custom_tools.insert(name.clone());
                let mut desc = description.clone().unwrap_or_default();
                if let Some(def) = format.as_ref().and_then(|f| f.get("definition")).and_then(Value::as_str) {
                    desc.push_str("\n\nThe `input` string must follow this grammar:\n");
                    desc.push_str(def);
                }
                tools.push(json!({ "name": names.wire(name), "description": desc, "input_schema": custom_tool_schema() }));
            }
            // Anthropic's own server tools (`web_search_20250305`, ...) arrive from Anthropic
            // clients in wire form and go upstream unchanged.
            ToolDefinition::Hosted { config, .. } if is_anthropic_server_tool(config) => tools.push(config.clone()),
            ToolDefinition::Hosted { kind, .. } => hosted.push(kind.as_str()),
        }
    }
    if !hosted.is_empty() {
        notes.push(format!("{WARN_NOTE_PREFIX} hosted tool(s) unavailable on Anthropic: {}", hosted.join(", ")));
    }

    let mut tool_choice = match (&req.tool_choice, tools.is_empty()) {
        (_, true) => None,
        (None, false) => None,
        (Some(ToolChoice::Auto), false) => Some(json!({ "type": "auto" })),
        (Some(ToolChoice::None), false) => Some(json!({ "type": "none" })),
        (Some(ToolChoice::Required), false) => Some(json!({ "type": "any" })),
        (Some(ToolChoice::Tool { name }), false) => Some(json!({ "type": "tool", "name": names.wire(name) })),
    };
    if !tools.is_empty() && req.metadata.parallel_tool_calls == Some(false) {
        let tc = tool_choice.get_or_insert_with(|| json!({ "type": "auto" }));
        if tc["type"] != "none" {
            tc["disable_parallel_tool_use"] = json!(true);
        }
    }

    // Thinking and limits.
    let effort = req.reasoning.as_ref().and_then(|r| r.effort.as_deref());
    let effort = if opts.thinking {
        effort
    } else {
        if let Some(e) = effort {
            notes.push(format!("reasoning effort `{e}` not sent (model declares no reasoning_efforts)"));
        }
        None
    };
    let plan = thinking::plan(upstream_model, effort, req.max_output_tokens, opts.default_max_tokens);
    body.insert("max_tokens".into(), json!(plan.max_tokens));
    if let Some(t) = &plan.thinking {
        body.insert("thinking".into(), t.clone());
    }
    let mut output_config = Map::new();
    if let Some(e) = &plan.effort {
        output_config.insert("effort".into(), json!(e));
    }
    match &req.output_format {
        None | Some(OutputFormat::Text) => {}
        Some(OutputFormat::JsonSchema { schema, .. }) => {
            output_config.insert("format".into(), json!({ "type": "json_schema", "schema": schema }));
        }
        Some(OutputFormat::JsonObject) => {
            return Err(ModelError::unsupported("JSON-object mode is not supported by Anthropic; use a JSON schema"));
        }
    }
    if !output_config.is_empty() {
        body.insert("output_config".into(), Value::Object(output_config));
    }

    let s = &req.sampling;
    if plan.strip_sampling {
        if s.temperature.is_some() || s.top_p.is_some() || s.top_k.is_some() {
            notes.push("temperature/top_p/top_k not sent (not allowed with thinking)".into());
        }
    } else {
        if let Some(t) = s.temperature {
            body.insert("temperature".into(), json!(t.clamp(0.0, 1.0)));
        }
        if let Some(p) = s.top_p {
            body.insert("top_p".into(), json!(p));
        }
        if let Some(k) = s.top_k {
            body.insert("top_k".into(), json!(k));
        }
    }
    if !s.stop.is_empty() {
        body.insert("stop_sequences".into(), json!(s.stop));
    }

    // Prompt caching: most stable prefix first.
    let mut messages: Vec<Value> = turns.turns.into_iter().map(|(role, content)| json!({ "role": role, "content": content })).collect();
    if opts.prompt_caching {
        let cc = json!({ "type": "ephemeral" });
        if let Some(last) = tools.last_mut() {
            last["cache_control"] = cc.clone();
        }
        if let Some(last) = system.last_mut() {
            last["cache_control"] = cc.clone();
        }
        let user_turns: Vec<usize> =
            messages.iter().enumerate().filter(|(_, m)| m["role"] == "user").map(|(i, _)| i).rev().take(2).collect();
        for i in user_turns {
            if let Some(block) = messages[i]["content"].as_array_mut().and_then(|c| c.last_mut()) {
                block["cache_control"] = cc.clone();
            }
        }
    }

    if !system.is_empty() {
        body.insert("system".into(), Value::Array(system));
    }
    body.insert("messages".into(), Value::Array(messages));
    if !tools.is_empty() {
        body.insert("tools".into(), Value::Array(tools));
    }
    if let Some(tc) = tool_choice {
        body.insert("tool_choice".into(), tc);
    }
    body.insert("stream".into(), json!(true));

    Ok(Encoded { body: Value::Object(body), custom_tools, tool_names: names, notes })
}

fn encode_message(
    m: &Message,
    turns: &mut Turns,
    names: &mut ToolNames,
    dropped_reasoning: &mut usize,
) -> Result<(), ModelError> {
    match m.role {
        // No positional system role on this wire: keep the instruction, marked, as a user turn.
        Role::System => {
            let text: String = m.content.iter().filter_map(ContentBlock::as_text).collect::<Vec<_>>().join("\n");
            if !text.is_empty() {
                turns.push("user", vec![json!({ "type": "text", "text": format!("<instructions>\n{text}\n</instructions>") })]);
            }
        }
        Role::User => {
            let mut results = Vec::new();
            let mut rest = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::ToolResult(r) => {
                        let mut content = Vec::new();
                        for c in &r.content {
                            match c {
                                ToolResultContent::Text { text } if !text.is_empty() => {
                                    content.push(json!({ "type": "text", "text": text }));
                                }
                                ToolResultContent::Text { .. } => {}
                                ToolResultContent::Image(img) => content.push(image(img)),
                            }
                        }
                        let mut block = json!({ "type": "tool_result", "tool_use_id": tool_id(&r.call_id) });
                        if !content.is_empty() {
                            block["content"] = Value::Array(content);
                        }
                        if r.is_error {
                            block["is_error"] = json!(true);
                        }
                        results.push(block);
                    }
                    ContentBlock::Text { text } if !text.is_empty() => rest.push(json!({ "type": "text", "text": text })),
                    ContentBlock::Text { .. } => {}
                    ContentBlock::Image(img) => rest.push(image(img)),
                    ContentBlock::File(f) => {
                        let media = f.media_type.as_deref().unwrap_or("application/pdf");
                        let source = match &f.source {
                            FileSource::Base64 { data } if media == "application/pdf" => {
                                json!({ "type": "base64", "media_type": media, "data": data })
                            }
                            FileSource::Url { url } => json!({ "type": "url", "url": url }),
                            _ => return Err(ModelError::unsupported(format!("file input `{media}` is not supported by Anthropic"))),
                        };
                        rest.push(json!({ "type": "document", "source": source }));
                    }
                    ContentBlock::Reasoning(_) | ContentBlock::ToolCall(_) => {
                        return Err(ModelError::protocol("user messages cannot contain reasoning or tool calls"));
                    }
                }
            }
            // Tool results must lead the turn that answers the tool calls.
            results.extend(rest);
            turns.push("user", results);
        }
        Role::Assistant => {
            let mut blocks = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::Reasoning(r) => match (&r.signature, &r.encrypted_content) {
                        (Some(sig), _) => blocks.push(json!({ "type": "thinking", "thinking": r.text, "signature": sig })),
                        (None, Some(data)) if r.text.is_empty() && !is_foreign_blob(data) => {
                            blocks.push(json!({ "type": "redacted_thinking", "data": data }));
                        }
                        _ => *dropped_reasoning += 1,
                    },
                    ContentBlock::Text { text } if !text.is_empty() => blocks.push(json!({ "type": "text", "text": text })),
                    ContentBlock::Text { .. } => {}
                    ContentBlock::ToolCall(c) => {
                        let input = match c.kind {
                            ToolCallKind::Custom => json!({ "input": c.arguments }),
                            ToolCallKind::Function if c.arguments.trim().is_empty() => json!({}),
                            ToolCallKind::Function => match serde_json::from_str::<Value>(&c.arguments) {
                                Ok(v @ Value::Object(_)) => v,
                                _ => json!({ "arguments": c.arguments }),
                            },
                        };
                        blocks.push(json!({ "type": "tool_use", "id": tool_id(&c.id), "name": names.wire(&c.name), "input": input }));
                    }
                    ContentBlock::Image(_) | ContentBlock::File(_) | ContentBlock::ToolResult(_) => {
                        return Err(ModelError::protocol("assistant messages cannot contain images, files, or tool results"));
                    }
                }
            }
            turns.push("assistant", blocks);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use owo_core::{ReasoningBlock, ReasoningConfig, ToolCall, ToolResult};

    fn base_request() -> ModelRequest {
        let mut req = ModelRequest::new("r", "claude-sonnet-5");
        req.system.push(ContentBlock::text("You are Codex."));
        req.messages.push(Message::user_text("list files"));
        req.messages.push(Message::new(
            Role::Assistant,
            vec![
                ContentBlock::Reasoning(ReasoningBlock { text: "plan".into(), signature: Some("SIG".into()), ..Default::default() }),
                ContentBlock::Reasoning(ReasoningBlock { text: "gpt thoughts".into(), encrypted_content: Some("gAAAAxyz".into()), ..Default::default() }),
                ContentBlock::ToolCall(ToolCall { id: "call_1".into(), name: "exec_command".into(), arguments: "{\"cmd\":\"ls\"}".into(), kind: ToolCallKind::Function }),
                ContentBlock::ToolCall(ToolCall { id: "call.2".into(), name: "apply_patch".into(), arguments: "*** Begin Patch".into(), kind: ToolCallKind::Custom }),
            ],
        ));
        req.messages.push(Message::new(
            Role::User,
            vec![
                ContentBlock::ToolResult(ToolResult { call_id: "call_1".into(), content: vec![ToolResultContent::Text { text: "a.rs".into() }], is_error: false, kind: ToolCallKind::Function }),
                ContentBlock::ToolResult(ToolResult { call_id: "call.2".into(), content: vec![ToolResultContent::Text { text: String::new() }], is_error: true, kind: ToolCallKind::Custom }),
            ],
        ));
        req.messages.push(Message::new(Role::System, vec![ContentBlock::text("be brief")]));
        req.tools.push(ToolDefinition::Function { name: "exec_command".into(), description: Some("run".into()), parameters: json!({"type": "object"}), strict: None });
        req.tools.push(ToolDefinition::Custom { name: "apply_patch".into(), description: None, format: None });
        req.tools.push(ToolDefinition::Hosted { kind: "web_search".into(), config: json!({}) });
        req.tool_choice = Some(ToolChoice::Auto);
        req.metadata.parallel_tool_calls = Some(false);
        req.reasoning = Some(ReasoningConfig { effort: Some("high".into()), ..Default::default() });
        req.sampling.temperature = Some(0.2);
        req
    }

    #[test]
    fn encodes_codex_style_turn() {
        let enc = encode_request(&base_request(), "claude-sonnet-5", &EncodeOptions::default()).unwrap();
        let b = &enc.body;
        assert_eq!(b["model"], "claude-sonnet-5");
        assert_eq!(b["system"][0]["text"], "You are Codex.");
        assert_eq!(b["system"][0]["cache_control"]["type"], "ephemeral");

        let msgs = b["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "user, assistant, user (tool results + merged instruction)");
        let assistant = &msgs[1]["content"];
        assert_eq!(assistant[0], json!({"type": "thinking", "thinking": "plan", "signature": "SIG"}));
        assert_eq!(assistant[1]["type"], "tool_use", "foreign reasoning is dropped");
        assert_eq!(assistant[1]["input"], json!({"cmd": "ls"}));
        assert_eq!(assistant[2]["id"], "call_2");
        assert_eq!(assistant[2]["input"], json!({"input": "*** Begin Patch"}));

        let results = &msgs[2]["content"];
        assert_eq!(results[0]["tool_use_id"], "call_1");
        assert_eq!(results[0]["content"][0]["text"], "a.rs");
        assert_eq!(results[1]["tool_use_id"], "call_2");
        assert!(results[1].get("content").is_none());
        assert_eq!(results[1]["is_error"], true);
        assert!(results[2]["text"].as_str().unwrap().contains("be brief"));
        assert_eq!(results[2]["cache_control"]["type"], "ephemeral");

        assert_eq!(b["tools"].as_array().unwrap().len(), 2);
        assert_eq!(b["tools"][1]["cache_control"]["type"], "ephemeral");
        assert_eq!(b["tool_choice"], json!({"type": "auto", "disable_parallel_tool_use": true}));
        assert_eq!(b["thinking"], json!({"type": "adaptive"}));
        assert_eq!(b["output_config"]["effort"], "high");
        assert!(b.get("temperature").is_none(), "sampling stripped with thinking");
        assert_eq!(b["stream"], true);
        assert!(enc.custom_tools.contains("apply_patch"));
        assert!(enc.notes.iter().any(|n| n.starts_with(WARN_NOTE_PREFIX) && n.contains("web_search")));
    }

    #[test]
    fn anthropic_server_tools_pass_through() {
        let mut req = ModelRequest::new("r", "claude-sonnet-5");
        req.messages.push(Message::user_text("search"));
        let tool = json!({"type": "web_search_20250305", "name": "web_search", "max_uses": 5});
        req.tools.push(ToolDefinition::Hosted { kind: "web_search".into(), config: tool.clone() });
        req.tools.push(ToolDefinition::Hosted { kind: "web_search".into(), config: json!({"type": "web_search"}) });
        let enc = encode_request(&req, "claude-sonnet-5", &EncodeOptions { prompt_caching: false, ..Default::default() }).unwrap();
        assert_eq!(enc.body["tools"], json!([tool]));
        assert!(enc.notes.iter().any(|n| n.starts_with(WARN_NOTE_PREFIX)), "the OpenAI-shaped one is still omitted");
    }

    #[test]
    fn root_combinators_are_flattened() {
        let mut req = ModelRequest::new("r", "claude-sonnet-5");
        req.messages.push(Message::user_text("hi"));
        req.tools.push(ToolDefinition::Function {
            name: "browser".into(),
            description: None,
            parameters: json!({"anyOf": [
                {"type": "object", "properties": {"url": {"type": "string"}}, "required": ["url"]},
                {"type": "object", "properties": {"tab": {"type": "integer"}}, "required": ["tab"]}
            ]}),
            strict: None,
        });
        let enc = encode_request(&req, "claude-sonnet-5", &EncodeOptions::default()).unwrap();
        let schema = &enc.body["tools"][0]["input_schema"];
        assert_eq!(schema["type"], "object");
        assert!(schema.get("anyOf").is_none());
        assert_eq!(schema["properties"].as_object().unwrap().len(), 2);
    }

    #[test]
    fn legacy_budget_and_no_thinking() {
        let mut req = base_request();
        let enc = encode_request(&req, "claude-haiku-4-5", &EncodeOptions::default()).unwrap();
        assert_eq!(enc.body["thinking"]["type"], "enabled");
        assert!(enc.body.get("output_config").is_none());

        req.reasoning = None;
        let enc = encode_request(&req, "claude-haiku-4-5", &EncodeOptions { prompt_caching: false, ..Default::default() }).unwrap();
        assert!(enc.body.get("thinking").is_none());
        assert_eq!(enc.body["temperature"], 0.2);
        assert_eq!(enc.body["max_tokens"], 32_000);
        assert!(!enc.body.to_string().contains("cache_control"));
    }

    #[test]
    fn leading_assistant_turn_gets_a_user_opener() {
        let mut req = ModelRequest::new("r", "m");
        req.messages.push(Message::assistant_text("hi"));
        let enc = encode_request(&req, "m", &EncodeOptions::default()).unwrap();
        assert_eq!(enc.body["messages"][0]["role"], "user");
        assert!(encode_request(&ModelRequest::new("r", "m"), "m", &EncodeOptions::default()).is_err());
    }
}
