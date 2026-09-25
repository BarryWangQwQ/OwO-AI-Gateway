//! Client-facing Anthropic Messages API: request decoding and response encoding.
//!
//! Replay material survives the round trip through the client: Anthropic thinking
//! signatures are handed out as-is, and opaque reasoning from other providers is
//! carried in the signature field behind [`ENCRYPTED_PREFIX`].

use bytes::Bytes;
use serde_json::{json, Map, Value};
use owo_core::{
    ContentBlock, ErrorKind, FileInput, FileSource, ImageInput, ImageSource, InboundProtocol, Message,
    ModelError, ModelEvent, ModelRequest, ModelResponse, OutputFormat, ReasoningBlock, ReasoningConfig, Role,
    StopReason, ToolCall, ToolCallKind, ToolChoice, ToolDefinition, ToolResult, ToolResultContent, Usage,
};

/// Marks a thinking "signature" that is really another provider's opaque reasoning.
pub const ENCRYPTED_PREFIX: &str = "owo-enc:v1:";
/// Claude Code's first system block; attribution for Anthropic's own billing, not a prompt.
const BILLING_HEADER_PREFIX: &str = "x-anthropic-billing-header:";

pub struct Decoded {
    pub request: ModelRequest,
}

fn bad(msg: impl Into<String>) -> ModelError {
    ModelError::invalid_request(msg)
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn u32_at(v: &Value, key: &str) -> Option<u32> {
    v.get(key).and_then(Value::as_u64).map(|n| n.min(u64::from(u32::MAX)) as u32)
}

pub fn decode_request(body: Value, request_id: String) -> Result<Decoded, ModelError> {
    let Value::Object(mut obj) = body else { return Err(bad("request body must be a JSON object")) };
    let model = match obj.remove("model") {
        Some(Value::String(m)) if !m.trim().is_empty() => m,
        _ => return Err(bad("`model` is required")),
    };
    let mut req = ModelRequest::new(request_id, model);
    req.metadata.inbound = Some(InboundProtocol::Anthropic);

    if let Some(system) = obj.remove("system") {
        req.system = decode_system(&system, &mut req.metadata.extra)?;
    }
    let messages = match obj.remove("messages") {
        Some(Value::Array(m)) if !m.is_empty() => m,
        _ => return Err(bad("`messages` must be a non-empty array")),
    };
    for m in &messages {
        req.messages.push(decode_message(m)?);
    }

    if let Some(Value::Array(tools)) = obj.remove("tools") {
        for t in &tools {
            req.tools.push(decode_tool(t)?);
        }
    }
    if let Some(tc) = obj.remove("tool_choice") {
        let (choice, disable_parallel) = decode_tool_choice(&tc)?;
        req.tool_choice = choice;
        if disable_parallel {
            req.metadata.parallel_tool_calls = Some(false);
        }
    }

    let output_config = obj.remove("output_config");
    let effort = output_config.as_ref().and_then(|c| str_at(c, "effort")).map(str::to_ascii_lowercase);
    if let Some(format) = output_config.as_ref().and_then(|c| c.get("format")) {
        req.output_format = Some(decode_format(format)?);
    }
    req.reasoning = decode_thinking(obj.remove("thinking").as_ref(), effort)?;

    req.max_output_tokens = obj.remove("max_tokens").and_then(|v| v.as_u64()).map(|n| n.min(u64::from(u32::MAX)) as u32);
    if let Some(t) = obj.remove("temperature").and_then(|v| v.as_f64()) {
        req.sampling.temperature = Some(t);
    }
    if let Some(p) = obj.remove("top_p").and_then(|v| v.as_f64()) {
        req.sampling.top_p = Some(p);
    }
    if let Some(k) = obj.remove("top_k").and_then(|v| v.as_u64()) {
        req.sampling.top_k = Some(k.min(u64::from(u32::MAX)) as u32);
    }
    if let Some(Value::Array(stops)) = obj.remove("stop_sequences") {
        req.sampling.stop = stops.iter().filter_map(Value::as_str).map(str::to_string).collect();
    }
    req.stream = obj.remove("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    if let Some(meta) = obj.remove("metadata") {
        if let Some(user) = str_at(&meta, "user_id") {
            // Claude Code packs a JSON object (device id, account, session) into user_id.
            // Only the session is used; a device identifier is not passed on to other providers.
            match serde_json::from_str::<Value>(user).ok().as_ref().and_then(|u| str_at(u, "session_id")) {
                Some(session) => req.metadata.session_id = Some(session.to_string()),
                None => req.metadata.user = Some(user.to_string()),
            }
        }
        req.metadata.extra.insert("metadata".into(), meta);
    }
    if let Some(tier) = obj.remove("service_tier").as_ref().and_then(Value::as_str) {
        req.metadata.service_tier = Some(tier.to_string());
    }
    // Everything else (context_management, container, mcp_servers, ...) is kept, not dropped.
    for (k, v) in obj {
        req.metadata.extra.insert(k, v);
    }
    Ok(Decoded { request: req })
}

fn decode_system(system: &Value, extra: &mut std::collections::BTreeMap<String, Value>) -> Result<Vec<ContentBlock>, ModelError> {
    let blocks = match system {
        Value::String(s) => vec![json!({ "type": "text", "text": s })],
        Value::Array(a) => a.clone(),
        Value::Null => Vec::new(),
        _ => return Err(bad("`system` must be a string or an array of text blocks")),
    };
    let mut out = Vec::new();
    for b in blocks {
        match str_at(&b, "type") {
            Some("text") => {
                let text = str_at(&b, "text").unwrap_or_default();
                if text.starts_with(BILLING_HEADER_PREFIX) {
                    extra.insert("anthropic_billing_header".into(), json!(text));
                } else if !text.is_empty() {
                    out.push(ContentBlock::text(text));
                }
            }
            other => return Err(bad(format!("unsupported system block type `{}`", other.unwrap_or("?")))),
        }
    }
    Ok(out)
}

fn decode_image(source: &Value) -> Result<ImageInput, ModelError> {
    let source = match str_at(source, "type") {
        Some("base64") => ImageSource::Base64 {
            media_type: str_at(source, "media_type").unwrap_or("image/png").to_string(),
            data: str_at(source, "data").ok_or_else(|| bad("image source is missing `data`"))?.to_string(),
        },
        Some("url") => ImageSource::Url { url: str_at(source, "url").ok_or_else(|| bad("image source is missing `url`"))?.to_string() },
        other => return Err(bad(format!("unsupported image source `{}`", other.unwrap_or("?")))),
    };
    Ok(ImageInput { source, detail: None })
}

fn decode_document(block: &Value) -> Result<ContentBlock, ModelError> {
    let source = block.get("source").ok_or_else(|| bad("document block is missing `source`"))?;
    let (media_type, source) = match str_at(source, "type") {
        Some("base64") => (
            str_at(source, "media_type").map(str::to_string),
            FileSource::Base64 { data: str_at(source, "data").unwrap_or_default().to_string() },
        ),
        Some("url") => (None, FileSource::Url { url: str_at(source, "url").unwrap_or_default().to_string() }),
        Some("text") => return Ok(ContentBlock::text(str_at(source, "data").unwrap_or_default())),
        other => return Err(bad(format!("unsupported document source `{}`", other.unwrap_or("?")))),
    };
    Ok(ContentBlock::File(FileInput { filename: str_at(block, "title").map(str::to_string), media_type, source }))
}

fn content_blocks(content: &Value) -> Result<Vec<Value>, ModelError> {
    match content {
        Value::String(s) => Ok(vec![json!({ "type": "text", "text": s })]),
        Value::Array(a) => Ok(a.clone()),
        _ => Err(bad("message `content` must be a string or an array")),
    }
}

fn decode_tool_result(block: &Value) -> Result<ContentBlock, ModelError> {
    let call_id = str_at(block, "tool_use_id").filter(|s| !s.is_empty()).ok_or_else(|| bad("tool_result is missing `tool_use_id`"))?;
    let mut content = Vec::new();
    match block.get("content") {
        None | Some(Value::Null) => {}
        Some(Value::String(s)) => content.push(ToolResultContent::Text { text: s.clone() }),
        Some(Value::Array(parts)) => {
            for p in parts {
                match str_at(p, "type") {
                    Some("text") => content.push(ToolResultContent::Text { text: str_at(p, "text").unwrap_or_default().to_string() }),
                    Some("image") => {
                        let src = p.get("source").ok_or_else(|| bad("image block is missing `source`"))?;
                        content.push(ToolResultContent::Image(decode_image(src)?));
                    }
                    // Tool references, search results, documents: keep them readable to the model.
                    _ => content.push(ToolResultContent::Text { text: p.to_string() }),
                }
            }
        }
        Some(other) => content.push(ToolResultContent::Text { text: other.to_string() }),
    }
    Ok(ContentBlock::ToolResult(ToolResult {
        call_id: call_id.to_string(),
        content,
        is_error: block.get("is_error").and_then(Value::as_bool).unwrap_or(false),
        kind: ToolCallKind::Function,
    }))
}

fn decode_reasoning(block: &Value, redacted: bool) -> ContentBlock {
    if redacted {
        return ContentBlock::Reasoning(ReasoningBlock {
            encrypted_content: str_at(block, "data").map(str::to_string),
            ..Default::default()
        });
    }
    let text = str_at(block, "thinking").unwrap_or_default().to_string();
    let signature = str_at(block, "signature").filter(|s| !s.is_empty());
    let (signature, encrypted_content) = match signature {
        Some(s) => match s.strip_prefix(ENCRYPTED_PREFIX) {
            Some(blob) => (None, Some(blob.to_string())),
            None => (Some(s.to_string()), None),
        },
        None => (None, None),
    };
    ContentBlock::Reasoning(ReasoningBlock { text, signature, encrypted_content, ..Default::default() })
}

fn decode_message(m: &Value) -> Result<Message, ModelError> {
    let role = match str_at(m, "role") {
        Some("user") => Role::User,
        Some("assistant") => Role::Assistant,
        Some("system") => Role::System,
        other => return Err(bad(format!("unsupported message role `{}`", other.unwrap_or("?")))),
    };
    let blocks = content_blocks(m.get("content").unwrap_or(&Value::Null))?;
    let mut content = Vec::new();
    for b in &blocks {
        let ty = str_at(b, "type").unwrap_or("?");
        let block = match (role, ty) {
            (_, "text") => ContentBlock::text(str_at(b, "text").unwrap_or_default()),
            (Role::User, "image") => ContentBlock::Image(decode_image(b.get("source").ok_or_else(|| bad("image block is missing `source`"))?)?),
            (Role::User, "document") => decode_document(b)?,
            (Role::User, "tool_result") => decode_tool_result(b)?,
            (Role::Assistant, "thinking") => decode_reasoning(b, false),
            (Role::Assistant, "redacted_thinking") => decode_reasoning(b, true),
            (Role::Assistant, "tool_use") => {
                let id = str_at(b, "id").filter(|s| !s.is_empty()).ok_or_else(|| bad("tool_use is missing `id`"))?;
                let name = str_at(b, "name").filter(|s| !s.is_empty()).ok_or_else(|| bad("tool_use is missing `name`"))?;
                let arguments = match b.get("input") {
                    None | Some(Value::Null) => "{}".to_string(),
                    Some(v) => v.to_string(),
                };
                ContentBlock::ToolCall(ToolCall { id: id.into(), name: name.into(), arguments, kind: ToolCallKind::Function })
            }
            (_, other) => {
                return Err(ModelError::unsupported(format!("`{other}` content blocks in {role:?} messages are not supported")));
            }
        };
        content.push(block);
    }
    Ok(Message::new(role, content))
}

fn decode_tool(t: &Value) -> Result<ToolDefinition, ModelError> {
    let name = str_at(t, "name").unwrap_or_default().to_string();
    match (str_at(t, "type"), t.get("input_schema")) {
        (None | Some("custom"), Some(schema)) => {
            if name.is_empty() {
                return Err(bad("tool is missing `name`"));
            }
            Ok(ToolDefinition::Function {
                name,
                description: str_at(t, "description").map(str::to_string),
                parameters: schema.clone(),
                strict: t.get("strict").and_then(Value::as_bool),
            })
        }
        // Server tools (`web_search_20250305`, `web_fetch_…`, `code_execution_…`).
        (Some(ty), _) => {
            let kind = ty.rsplit_once('_').filter(|(_, v)| v.chars().all(|c| c.is_ascii_digit())).map_or(ty, |(k, _)| k);
            Ok(ToolDefinition::Hosted { kind: kind.to_string(), config: t.clone() })
        }
        (None, None) => Err(bad(format!("tool `{name}` is missing `input_schema`"))),
    }
}

fn decode_tool_choice(tc: &Value) -> Result<(Option<ToolChoice>, bool), ModelError> {
    let disable = tc.get("disable_parallel_tool_use").and_then(Value::as_bool).unwrap_or(false);
    let choice = match str_at(tc, "type") {
        Some("auto") => ToolChoice::Auto,
        Some("any") => ToolChoice::Required,
        Some("none") => ToolChoice::None,
        Some("tool") => ToolChoice::Tool {
            name: str_at(tc, "name").filter(|s| !s.is_empty()).ok_or_else(|| bad("tool_choice `tool` is missing `name`"))?.to_string(),
        },
        other => return Err(bad(format!("unsupported tool_choice `{}`", other.unwrap_or("?")))),
    };
    Ok((Some(choice), disable))
}

fn decode_format(format: &Value) -> Result<OutputFormat, ModelError> {
    match str_at(format, "type") {
        Some("json_schema") => Ok(OutputFormat::JsonSchema {
            name: str_at(format, "name").unwrap_or("response").to_string(),
            schema: format.get("schema").cloned().unwrap_or_else(|| json!({})),
            description: None,
            strict: None,
        }),
        other => Err(ModelError::unsupported(format!("output format `{}` is not supported", other.unwrap_or("?")))),
    }
}

fn effort_for_budget(budget: u32) -> &'static str {
    match budget {
        0..=4096 => "low",
        4097..=16_384 => "medium",
        _ => "high",
    }
}

fn decode_thinking(thinking: Option<&Value>, effort: Option<String>) -> Result<Option<ReasoningConfig>, ModelError> {
    let Some(t) = thinking else {
        return Ok(effort.map(|e| ReasoningConfig { effort: Some(e), ..Default::default() }));
    };
    Ok(match str_at(t, "type") {
        Some("enabled") => {
            let budget = u32_at(t, "budget_tokens");
            Some(ReasoningConfig {
                effort: Some(effort.unwrap_or_else(|| effort_for_budget(budget.unwrap_or(8192)).to_string())),
                budget_tokens: budget,
                ..Default::default()
            })
        }
        // Anthropic's default effort for adaptive thinking is `high`.
        Some("adaptive") => Some(ReasoningConfig { effort: Some(effort.unwrap_or_else(|| "high".into())), ..Default::default() }),
        Some("disabled") => Some(ReasoningConfig { effort: Some("none".into()), ..Default::default() }),
        other => return Err(bad(format!("unsupported thinking type `{}`", other.unwrap_or("?")))),
    })
}

/// Rough prompt size for `count_tokens` (about four bytes per token).
pub fn estimate_input_tokens(body: &Value) -> u64 {
    let mut bytes = 0usize;
    for key in ["system", "messages", "tools"] {
        if let Some(v) = body.get(key) {
            bytes += v.to_string().len();
        }
    }
    (bytes as u64).div_ceil(4).max(1)
}

// ---------------------------------------------------------------------------
// Responses

pub fn stop_reason(reason: &StopReason) -> &str {
    match reason {
        StopReason::EndTurn => "end_turn",
        StopReason::MaxTokens => "max_tokens",
        StopReason::ToolUse => "tool_use",
        StopReason::StopSequence => "stop_sequence",
        StopReason::ContentFilter | StopReason::Refusal => "refusal",
        StopReason::Other(s) => s,
    }
}

/// Anthropic reports `input_tokens` without the cached part; canonical usage includes it.
pub fn usage_json(u: &Usage) -> Value {
    let cached = u.cached_input_tokens.unwrap_or(0);
    let created = u.cache_creation_input_tokens.unwrap_or(0);
    json!({
        "input_tokens": u.input_tokens.saturating_sub(cached + created),
        "output_tokens": u.output_tokens,
        "cache_read_input_tokens": cached,
        "cache_creation_input_tokens": created,
    })
}

pub fn error_type(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::InvalidRequest
        | ErrorKind::InvalidToolSchema
        | ErrorKind::ProtocolViolation
        | ErrorKind::ContextExceeded
        | ErrorKind::UnsupportedCapability
        | ErrorKind::ConfigurationError => "invalid_request_error",
        ErrorKind::AuthenticationFailed => "authentication_error",
        ErrorKind::AuthorizationFailed => "permission_error",
        ErrorKind::ModelNotFound => "not_found_error",
        ErrorKind::ClientIntegrationConflict => "conflict_error",
        ErrorKind::RateLimited => "rate_limit_error",
        ErrorKind::Timeout => "timeout_error",
        ErrorKind::ProviderUnavailable => "overloaded_error",
        ErrorKind::UpstreamInvalidResponse | ErrorKind::Cancelled | ErrorKind::Internal => "api_error",
    }
}

pub fn error_body(err: &ModelError) -> Value {
    json!({ "type": "error", "error": { "type": error_type(err.kind), "message": err.message } })
}

fn signature_for(signature: Option<String>, encrypted: Option<String>) -> String {
    match (signature, encrypted) {
        (Some(s), _) => s,
        (None, Some(e)) => format!("{ENCRYPTED_PREFIX}{e}"),
        (None, None) => String::new(),
    }
}

/// Encodes a complete response as a Messages API `message` object.
pub fn encode_response(resp: &ModelResponse, model: &str) -> Value {
    let mut content = Vec::new();
    for b in &resp.content {
        match b {
            ContentBlock::Text { text } => content.push(json!({ "type": "text", "text": text })),
            ContentBlock::Reasoning(r) if r.text.is_empty() && r.signature.is_none() && r.encrypted_content.is_some() => {
                content.push(json!({ "type": "redacted_thinking", "data": r.encrypted_content }));
            }
            ContentBlock::Reasoning(r) => content.push(json!({
                "type": "thinking",
                "thinking": r.text,
                "signature": signature_for(r.signature.clone(), r.encrypted_content.clone()),
            })),
            ContentBlock::ToolCall(c) => content.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": tool_input(c) })),
            ContentBlock::Image(_) | ContentBlock::ToolResult(_) | ContentBlock::File(_) => {}
        }
    }
    json!({
        "id": message_id(&resp.id),
        "type": "message",
        "role": "assistant",
        "model": model,
        "content": content,
        "stop_reason": stop_reason(&resp.stop_reason),
        "stop_sequence": null,
        "usage": usage_json(&resp.usage.unwrap_or_default()),
    })
}

fn tool_input(c: &ToolCall) -> Value {
    match c.kind {
        ToolCallKind::Custom => json!({ "input": c.arguments }),
        ToolCallKind::Function if c.arguments.trim().is_empty() => json!({}),
        ToolCallKind::Function => match serde_json::from_str::<Value>(&c.arguments) {
            Ok(v @ Value::Object(_)) => v,
            _ => json!({ "arguments": c.arguments }),
        },
    }
}

fn message_id(upstream: &str) -> String {
    if upstream.starts_with("msg_") {
        upstream.to_string()
    } else {
        format!("msg_{}", uuid::Uuid::new_v4().simple())
    }
}

enum Open {
    Text,
    /// Thinking whose `content_block_start` is deferred until its kind is known:
    /// redacted reasoning has no text, only opaque data at the end.
    PendingReasoning,
    Thinking,
    Tool { custom: bool, buf: String },
}

/// Encodes canonical events as Messages API SSE frames.
pub struct MessagesStreamEncoder {
    model: String,
    id: Option<String>,
    started: bool,
    index: usize,
    open: Option<Open>,
    usage: Usage,
    stop: Option<StopReason>,
    done: bool,
}

fn frame(event: &str, data: Value) -> Bytes {
    owo_sse::encode(Some(event), &data.to_string())
}

impl MessagesStreamEncoder {
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            id: None,
            started: false,
            index: 0,
            open: None,
            usage: Usage::default(),
            stop: None,
            done: false,
        }
    }

    fn start(&mut self, out: &mut Vec<Bytes>) {
        if self.started {
            return;
        }
        self.started = true;
        let id = message_id(self.id.as_deref().unwrap_or(""));
        out.push(frame(
            "message_start",
            json!({
                "type": "message_start",
                "message": {
                    "id": id, "type": "message", "role": "assistant", "model": self.model,
                    "content": [], "stop_reason": null, "stop_sequence": null,
                    "usage": { "input_tokens": 0, "output_tokens": 0 },
                },
            }),
        ));
    }

    fn open_block(&mut self, out: &mut Vec<Bytes>, block: Value, open: Open) {
        self.close(out);
        self.start(out);
        out.push(frame("content_block_start", json!({ "type": "content_block_start", "index": self.index, "content_block": block })));
        self.open = Some(open);
    }

    fn delta(&self, delta: Value) -> Bytes {
        frame("content_block_delta", json!({ "type": "content_block_delta", "index": self.index, "delta": delta }))
    }

    fn stop_block(&mut self, out: &mut Vec<Bytes>) {
        out.push(frame("content_block_stop", json!({ "type": "content_block_stop", "index": self.index })));
        self.index += 1;
        self.open = None;
    }

    fn close(&mut self, out: &mut Vec<Bytes>) {
        match self.open.take() {
            None | Some(Open::PendingReasoning) => {}
            Some(Open::Tool { custom: true, buf }) => {
                out.push(self.delta(json!({ "type": "input_json_delta", "partial_json": json!({ "input": buf }).to_string() })));
                self.stop_block(out);
            }
            Some(_) => self.stop_block(out),
        }
    }

    pub fn push(&mut self, event: ModelEvent) -> Vec<Bytes> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        match event {
            ModelEvent::ResponseStart { id, .. } => self.id = Some(id),
            ModelEvent::MessageStart => self.start(&mut out),
            ModelEvent::TextStart => self.open_block(&mut out, json!({ "type": "text", "text": "" }), Open::Text),
            ModelEvent::TextDelta { text } => {
                if !matches!(self.open, Some(Open::Text)) {
                    self.open_block(&mut out, json!({ "type": "text", "text": "" }), Open::Text);
                }
                out.push(self.delta(json!({ "type": "text_delta", "text": text })));
            }
            ModelEvent::TextEnd => {
                if matches!(self.open, Some(Open::Text)) {
                    self.stop_block(&mut out);
                }
            }
            ModelEvent::ReasoningStart => {
                self.close(&mut out);
                self.start(&mut out);
                self.open = Some(Open::PendingReasoning);
            }
            ModelEvent::ReasoningDelta { text } => {
                if !matches!(self.open, Some(Open::Thinking)) {
                    self.open_block(&mut out, json!({ "type": "thinking", "thinking": "", "signature": "" }), Open::Thinking);
                }
                out.push(self.delta(json!({ "type": "thinking_delta", "thinking": text })));
            }
            ModelEvent::ReasoningEnd { signature, encrypted_content } => {
                let redacted = signature.is_none() && encrypted_content.is_some() && !matches!(self.open, Some(Open::Thinking));
                if redacted {
                    self.open_block(&mut out, json!({ "type": "redacted_thinking", "data": encrypted_content }), Open::Thinking);
                } else {
                    if !matches!(self.open, Some(Open::Thinking)) {
                        self.open_block(&mut out, json!({ "type": "thinking", "thinking": "", "signature": "" }), Open::Thinking);
                    }
                    let sig = signature_for(signature, encrypted_content);
                    if !sig.is_empty() {
                        out.push(self.delta(json!({ "type": "signature_delta", "signature": sig })));
                    }
                }
                self.stop_block(&mut out);
            }
            ModelEvent::ToolCallStart { id, name, kind } => {
                let custom = kind == ToolCallKind::Custom;
                self.open_block(
                    &mut out,
                    json!({ "type": "tool_use", "id": id, "name": name, "input": {} }),
                    Open::Tool { custom, buf: String::new() },
                );
            }
            ModelEvent::ToolCallDelta { arguments_delta, .. } => match &mut self.open {
                Some(Open::Tool { custom: true, buf }) => buf.push_str(&arguments_delta),
                Some(Open::Tool { custom: false, .. }) => {
                    out.push(self.delta(json!({ "type": "input_json_delta", "partial_json": arguments_delta })));
                }
                _ => {}
            },
            ModelEvent::ToolCallEnd { .. } => {
                if matches!(self.open, Some(Open::Tool { .. })) {
                    self.close(&mut out);
                }
            }
            ModelEvent::Usage(u) => self.usage.merge(&u),
            ModelEvent::MessageEnd { stop_reason } => {
                self.close(&mut out);
                self.stop = Some(stop_reason);
            }
            ModelEvent::ResponseEnd => {
                self.close(&mut out);
                self.start(&mut out);
                let stop = self.stop.take().unwrap_or(StopReason::EndTurn);
                let mut delta = Map::new();
                delta.insert("stop_reason".into(), json!(stop_reason(&stop)));
                delta.insert("stop_sequence".into(), Value::Null);
                out.push(frame("message_delta", json!({ "type": "message_delta", "delta": delta, "usage": usage_json(&self.usage) })));
                out.push(frame("message_stop", json!({ "type": "message_stop" })));
                self.done = true;
            }
            ModelEvent::Error(err) => {
                self.done = true;
                out.push(frame("error", error_body(&err)));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_code_request() -> Value {
        json!({
            "model": "claude-opus-5-5",
            "max_tokens": 128000,
            "stream": true,
            "system": [
                {"type": "text", "text": "x-anthropic-billing-header: cc_version=2.1.280; cc_entrypoint=cli;"},
                {"type": "text", "text": "You are Claude Code.", "cache_control": {"type": "ephemeral"}}
            ],
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "list files"}]},
                {"role": "system", "content": [{"type": "text", "text": "# Environment"}]},
                {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "plan", "signature": "SIG"},
                    {"type": "redacted_thinking", "data": "OPAQUE"},
                    {"type": "thinking", "thinking": "gpt", "signature": "owo-enc:v1:gAAAAblob"},
                    {"type": "tool_use", "id": "toolu_1", "name": "Bash", "input": {"command": "ls"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": [{"type": "text", "text": "a.rs"}]},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AAAA"}}
                ]}
            ],
            "tools": [
                {"name": "Bash", "description": "run", "input_schema": {"type": "object"}},
                {"type": "web_search_20250305", "name": "web_search", "max_uses": 5}
            ],
            "tool_choice": {"type": "auto", "disable_parallel_tool_use": true},
            "thinking": {"type": "adaptive", "display": "omitted"},
            "output_config": {"effort": "medium"},
            "context_management": {"edits": [{"type": "clear_thinking_20251015", "keep": "all"}]},
            "metadata": {"user_id": "{\"device_id\":\"d\",\"session_id\":\"sess-1\"}"}
        })
    }

    #[test]
    fn decodes_a_claude_code_request() {
        let req = decode_request(claude_code_request(), "r".into()).unwrap().request;
        assert_eq!(req.model.as_str(), "claude-opus-5-5");
        assert_eq!(req.system, vec![ContentBlock::text("You are Claude Code.")]);
        assert!(req.metadata.extra.contains_key("anthropic_billing_header"));
        assert!(req.metadata.extra.contains_key("context_management"));
        assert_eq!(req.metadata.session_id.as_deref(), Some("sess-1"));
        assert_eq!(req.metadata.user, None, "the device id is not forwarded");
        assert_eq!(req.max_output_tokens, Some(128_000));
        assert!(req.stream);
        assert_eq!(req.reasoning.as_ref().unwrap().effort.as_deref(), Some("medium"));
        assert_eq!(req.messages[1].role, Role::System);

        let assistant = &req.messages[2].content;
        assert!(matches!(&assistant[0], ContentBlock::Reasoning(r) if r.signature.as_deref() == Some("SIG") && r.text == "plan"));
        assert!(matches!(&assistant[1], ContentBlock::Reasoning(r) if r.encrypted_content.as_deref() == Some("OPAQUE") && r.text.is_empty()));
        assert!(matches!(&assistant[2], ContentBlock::Reasoning(r) if r.signature.is_none() && r.encrypted_content.as_deref() == Some("gAAAAblob")));
        assert!(matches!(&assistant[3], ContentBlock::ToolCall(c) if c.arguments == "{\"command\":\"ls\"}"));
        assert!(matches!(&req.messages[3].content[0], ContentBlock::ToolResult(r) if r.text() == "a.rs"));
        assert!(matches!(&req.messages[3].content[1], ContentBlock::Image(_)));

        assert!(matches!(&req.tools[0], ToolDefinition::Function { name, .. } if name == "Bash"));
        assert!(matches!(&req.tools[1], ToolDefinition::Hosted { kind, .. } if kind == "web_search"));
        assert_eq!(req.tool_choice, Some(ToolChoice::Auto));
        assert_eq!(req.metadata.parallel_tool_calls, Some(false));
    }

    #[test]
    fn thinking_shapes() {
        let effort = |thinking: Value, cfg: Option<&str>| {
            decode_thinking(Some(&thinking), cfg.map(str::to_string)).unwrap().unwrap().effort.unwrap()
        };
        assert_eq!(effort(json!({"type": "enabled", "budget_tokens": 2000}), None), "low");
        assert_eq!(effort(json!({"type": "enabled", "budget_tokens": 31999}), None), "high");
        assert_eq!(effort(json!({"type": "adaptive"}), None), "high");
        assert_eq!(effort(json!({"type": "disabled"}), None), "none");
        assert_eq!(decode_thinking(None, None).unwrap(), None);
    }

    #[test]
    fn rejects_malformed_requests() {
        assert!(decode_request(json!({"messages": [{"role": "user", "content": "hi"}]}), "r".into()).is_err());
        assert!(decode_request(json!({"model": "m", "messages": []}), "r".into()).is_err());
        let bad_result = json!({"model": "m", "messages": [{"role": "user", "content": [{"type": "tool_result"}]}]});
        assert!(decode_request(bad_result, "r".into()).is_err());
    }

    fn frames(bytes: Vec<Bytes>) -> Vec<(String, Value)> {
        let text: String = bytes.iter().map(|b| String::from_utf8_lossy(b).into_owned()).collect();
        text.split("\n\n")
            .filter(|f| !f.trim().is_empty())
            .map(|f| {
                let event = f.lines().find_map(|l| l.strip_prefix("event: ")).unwrap_or_default().to_string();
                let data = f.lines().find_map(|l| l.strip_prefix("data: ")).unwrap();
                (event, serde_json::from_str(data).unwrap())
            })
            .collect()
    }

    #[test]
    fn stream_encoding_round_trips_replay_material() {
        let mut enc = MessagesStreamEncoder::new("claude-opus-5-5");
        let mut out = Vec::new();
        for ev in [
            ModelEvent::ResponseStart { id: "resp_1".into(), model: "x".into() },
            ModelEvent::MessageStart,
            ModelEvent::ReasoningStart,
            ModelEvent::ReasoningDelta { text: "think".into() },
            ModelEvent::ReasoningEnd { signature: Some("SIG".into()), encrypted_content: None },
            ModelEvent::ReasoningStart,
            ModelEvent::ReasoningEnd { signature: None, encrypted_content: Some("OPAQUE".into()) },
            ModelEvent::ReasoningStart,
            ModelEvent::ReasoningDelta { text: "gpt".into() },
            ModelEvent::ReasoningEnd { signature: None, encrypted_content: Some("gAAAAblob".into()) },
            ModelEvent::TextStart,
            ModelEvent::TextDelta { text: "hi".into() },
            ModelEvent::TextEnd,
            ModelEvent::ToolCallStart { id: "call_1".into(), name: "Bash".into(), kind: ToolCallKind::Function },
            ModelEvent::ToolCallDelta { id: "call_1".into(), arguments_delta: "{\"command\":".into() },
            ModelEvent::ToolCallDelta { id: "call_1".into(), arguments_delta: "\"ls\"}".into() },
            ModelEvent::ToolCallEnd { id: "call_1".into() },
            ModelEvent::Usage(Usage { input_tokens: 100, output_tokens: 7, cached_input_tokens: Some(60), cache_creation_input_tokens: Some(10), reasoning_tokens: None }),
            ModelEvent::MessageEnd { stop_reason: StopReason::ToolUse },
            ModelEvent::ResponseEnd,
        ] {
            out.extend(enc.push(ev));
        }
        let f = frames(out);
        let names: Vec<&str> = f.iter().map(|(e, _)| e.as_str()).collect();
        assert_eq!(names[0], "message_start");
        assert!(f[0].1["message"]["id"].as_str().unwrap().starts_with("msg_"));
        let starts: Vec<&Value> = f.iter().filter(|(e, _)| e == "content_block_start").map(|(_, d)| &d["content_block"]).collect();
        assert_eq!(starts.iter().map(|b| b["type"].as_str().unwrap()).collect::<Vec<_>>(), ["thinking", "redacted_thinking", "thinking", "text", "tool_use"]);
        assert_eq!(starts[1]["data"], "OPAQUE");
        let sigs: Vec<&str> = f.iter().filter(|(_, d)| d["delta"]["type"] == "signature_delta").map(|(_, d)| d["delta"]["signature"].as_str().unwrap()).collect();
        assert_eq!(sigs, ["SIG", "owo-enc:v1:gAAAAblob"]);
        let indexes: Vec<u64> = f.iter().filter(|(e, _)| e == "content_block_start").map(|(_, d)| d["index"].as_u64().unwrap()).collect();
        assert_eq!(indexes, [0, 1, 2, 3, 4]);
        let json: String = f.iter().filter(|(_, d)| d["delta"]["type"] == "input_json_delta").map(|(_, d)| d["delta"]["partial_json"].as_str().unwrap()).collect();
        assert_eq!(json, "{\"command\":\"ls\"}");
        let (_, delta) = f.iter().find(|(e, _)| e == "message_delta").unwrap();
        assert_eq!(delta["delta"]["stop_reason"], "tool_use");
        assert_eq!(delta["usage"], json!({"input_tokens": 30, "output_tokens": 7, "cache_read_input_tokens": 60, "cache_creation_input_tokens": 10}));
        assert_eq!(names.last(), Some(&"message_stop"));

        // What the client sends back decodes to the original replay material.
        let replay = json!({"role": "assistant", "content": [
            {"type": "thinking", "thinking": "think", "signature": sigs[0]},
            {"type": "redacted_thinking", "data": "OPAQUE"},
            {"type": "thinking", "thinking": "gpt", "signature": sigs[1]},
        ]});
        let m = decode_message(&replay).unwrap();
        assert!(matches!(&m.content[2], ContentBlock::Reasoning(r) if r.encrypted_content.as_deref() == Some("gAAAAblob") && r.signature.is_none()));
    }

    #[test]
    fn errors_mid_stream_and_non_stream_response() {
        let mut enc = MessagesStreamEncoder::new("m");
        let f = frames(enc.push(ModelEvent::Error(ModelError::new(ErrorKind::RateLimited, "slow down"))));
        assert_eq!(f[0].0, "error");
        assert_eq!(f[0].1["error"]["type"], "rate_limit_error");
        assert!(enc.push(ModelEvent::ResponseEnd).is_empty());

        let resp = ModelResponse {
            id: "x".into(),
            model: "up".into(),
            content: vec![ContentBlock::text("hello"), ContentBlock::ToolCall(ToolCall { id: "c".into(), name: "Read".into(), arguments: "{\"p\":1}".into(), kind: ToolCallKind::Function })],
            stop_reason: StopReason::ToolUse,
            usage: Some(Usage { input_tokens: 5, output_tokens: 2, ..Default::default() }),
        };
        let v = encode_response(&resp, "claude-sonnet-5");
        assert_eq!(v["model"], "claude-sonnet-5");
        assert_eq!(v["content"][1]["input"], json!({"p": 1}));
        assert_eq!(v["stop_reason"], "tool_use");
        assert_eq!(v["usage"]["input_tokens"], 5);
    }
}
