//! Provider-facing Chat Completions: request encoding and response/stream decoding.

use std::collections::{BTreeMap, HashSet};

use serde_json::{json, Map, Value};
use owo_core::{
    ContentBlock, ErrorKind, FileSource, InboundProtocol, Message, ModelError, ModelEvent,
    ModelRequest, ModelResponse, OutputFormat, ReasoningBlock, Role, StopReason, ToolCall,
    ToolCallKind, ToolChoice, ToolDefinition, ToolResultContent, Usage,
};

use crate::common::{new_id, parse_chat_usage, parse_finish_reason};
use crate::json::str_field;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum MaxTokensField {
    /// `max_tokens` — accepted by nearly every OpenAI-compatible server.
    #[default]
    MaxTokens,
    /// `max_completion_tokens` — required by OpenAI reasoning models.
    MaxCompletionTokens,
}

#[derive(Debug, Clone, Default)]
pub struct EncodeOptions {
    /// Send prior assistant reasoning back as `reasoning_content`.
    pub replay_reasoning: bool,
    pub max_tokens_field: MaxTokensField,
    /// Forward `prompt_cache_key` (OpenAI-specific; strict servers reject it).
    pub prompt_cache_key: bool,
    /// Send `reasoning_effort`. Only models that declare reasoning efforts get it, because
    /// many Chat-compatible servers reject or misread the field.
    pub send_reasoning_effort: bool,
}

/// Notes that the caller should surface at warning level (content changed materially).
pub const WARN_NOTE_PREFIX: &str = "omitted";

pub struct EncodedRequest {
    pub body: Value,
    /// Names of custom tools that were wrapped as function tools; the decoder unwraps them.
    pub custom_tools: HashSet<String>,
    /// Human-readable notes about request content that could not be sent upstream
    /// as-is (logged by the caller; never includes content).
    pub notes: Vec<String>,
}

/// Parameters schema used to carry a free-form custom tool over a function-only wire.
fn custom_tool_schema() -> Value {
    json!({
        "type": "object",
        "properties": { "input": { "type": "string", "description": "The raw tool input." } },
        "required": ["input"],
        "additionalProperties": false,
    })
}

pub fn encode_request(
    req: &ModelRequest,
    upstream_model: &str,
    opts: &EncodeOptions,
) -> Result<EncodedRequest, ModelError> {
    let mut notes = Vec::new();
    let mut body = Map::new();
    body.insert("model".into(), json!(upstream_model));

    let mut messages = Vec::new();
    if !req.system.is_empty() {
        if req.system.iter().any(|b| b.as_text().is_none()) {
            return Err(ModelError::unsupported("non-text system content is not supported by this provider"));
        }
        messages.push(json!({ "role": "system", "content": req.system_text() }));
    }
    let mut dropped_reasoning = 0usize;
    for m in &req.messages {
        encode_message(m, opts, &mut messages, &mut dropped_reasoning)?;
    }
    if dropped_reasoning > 0 {
        notes.push(format!("{dropped_reasoning} prior reasoning block(s) not replayed (provider has no reasoning replay)"));
    }
    body.insert("messages".into(), Value::Array(messages));

    let mut custom_tools = HashSet::new();
    let mut hosted = Vec::new();
    if !req.tools.is_empty() {
        let mut tools = Vec::new();
        for t in &req.tools {
            tools.push(match t {
                ToolDefinition::Function { name, description, parameters, strict } => {
                    // Many Chat-compatible servers also reject a non-object root schema.
                    let mut f = json!({ "name": name, "parameters": owo_core::schema::object_root(parameters) });
                    if let Some(d) = description {
                        f["description"] = json!(d);
                    }
                    if let Some(s) = strict {
                        f["strict"] = json!(s);
                    }
                    json!({ "type": "function", "function": f })
                }
                ToolDefinition::Custom { name, description, format } => {
                    custom_tools.insert(name.clone());
                    let mut desc = description.clone().unwrap_or_default();
                    if let Some(def) = format.as_ref().and_then(|f| f.get("definition")).and_then(Value::as_str) {
                        desc.push_str("\n\nThe `input` string must follow this grammar:\n");
                        desc.push_str(def);
                    }
                    json!({
                        "type": "function",
                        "function": { "name": name, "description": desc, "parameters": custom_tool_schema() },
                    })
                }
                // Provider-hosted tools (web search, ...) have no Chat equivalent. The model
                // simply does not get them; the omission is reported at warning level.
                ToolDefinition::Hosted { kind, .. } => {
                    hosted.push(kind.as_str());
                    continue;
                }
            });
        }
        if !hosted.is_empty() {
            notes.push(format!("{WARN_NOTE_PREFIX} hosted tool(s) unavailable on a Chat provider: {}", hosted.join(", ")));
        }
        if !tools.is_empty() {
            body.insert("tools".into(), Value::Array(tools));
            if let Some(p) = req.metadata.parallel_tool_calls {
                body.insert("parallel_tool_calls".into(), json!(p));
            }
        }
    }
    if let Some(choice) = req.tool_choice.as_ref().filter(|_| body.contains_key("tools")) {
        body.insert(
            "tool_choice".into(),
            match choice {
                ToolChoice::Auto => json!("auto"),
                ToolChoice::None => json!("none"),
                ToolChoice::Required => json!("required"),
                ToolChoice::Tool { name } => json!({ "type": "function", "function": { "name": name } }),
            },
        );
    }

    match &req.output_format {
        None | Some(OutputFormat::Text) => {}
        Some(OutputFormat::JsonObject) => {
            body.insert("response_format".into(), json!({ "type": "json_object" }));
        }
        Some(OutputFormat::JsonSchema { name, schema, description, strict }) => {
            let mut js = json!({ "name": name, "schema": schema });
            if let Some(d) = description {
                js["description"] = json!(d);
            }
            if let Some(s) = strict {
                js["strict"] = json!(s);
            }
            body.insert("response_format".into(), json!({ "type": "json_schema", "json_schema": js }));
        }
    }

    if let Some(max) = req.max_output_tokens {
        let key = match opts.max_tokens_field {
            MaxTokensField::MaxTokens => "max_tokens",
            MaxTokensField::MaxCompletionTokens => "max_completion_tokens",
        };
        body.insert(key.into(), json!(max));
    }
    let s = &req.sampling;
    for (key, value) in [
        ("temperature", s.temperature),
        ("top_p", s.top_p),
        ("frequency_penalty", s.frequency_penalty),
        ("presence_penalty", s.presence_penalty),
    ] {
        if let Some(v) = value {
            body.insert(key.into(), json!(v));
        }
    }
    if let Some(k) = s.top_k {
        body.insert("top_k".into(), json!(k));
    }
    if let Some(seed) = s.seed {
        body.insert("seed".into(), json!(seed));
    }
    if !s.stop.is_empty() {
        body.insert("stop".into(), json!(s.stop));
    }
    if let Some(effort) = req.reasoning.as_ref().and_then(|r| r.effort.as_ref()) {
        if opts.send_reasoning_effort {
            body.insert("reasoning_effort".into(), json!(effort));
        } else {
            notes.push(format!("reasoning effort `{effort}` not sent (model declares no reasoning_efforts)"));
        }
    }
    if let Some(user) = &req.metadata.user {
        body.insert("user".into(), json!(user));
    }
    if opts.prompt_cache_key {
        if let Some(key) = &req.metadata.prompt_cache_key {
            body.insert("prompt_cache_key".into(), json!(key));
        }
    }

    // Unknown Chat fields are vendor parameters meant for a Chat upstream; forward them
    // without letting them override anything OwO AI Gateway set. Other protocols' extras are not Chat fields.
    if !req.metadata.extra.is_empty() {
        if req.metadata.inbound == Some(InboundProtocol::OpenaiChat) {
            for (k, v) in &req.metadata.extra {
                body.entry(k.clone()).or_insert_with(|| v.clone());
            }
        } else {
            let keys: Vec<_> = req.metadata.extra.keys().map(String::as_str).collect();
            notes.push(format!("inbound fields without a Chat equivalent: {}", keys.join(", ")));
        }
    }

    body.insert("stream".into(), json!(req.stream));
    if req.stream {
        body.insert("stream_options".into(), json!({ "include_usage": true }));
    }

    Ok(EncodedRequest { body: Value::Object(body), custom_tools, notes })
}

fn encode_message(
    m: &Message,
    opts: &EncodeOptions,
    out: &mut Vec<Value>,
    dropped_reasoning: &mut usize,
) -> Result<(), ModelError> {
    match m.role {
        Role::System => {
            let text = text_only(&m.content, "system")?;
            out.push(json!({ "role": "system", "content": text }));
        }
        Role::User => {
            let mut parts = Vec::new();
            let mut tool_images = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::ToolResult(r) => {
                        let mut text = r.text();
                        if r.is_error && !text.starts_with("Error") {
                            text = format!("Error: {text}");
                        }
                        out.push(json!({ "role": "tool", "tool_call_id": r.call_id, "content": text }));
                        for c in &r.content {
                            if let ToolResultContent::Image(img) = c {
                                tool_images.push((r.call_id.clone(), img.clone()));
                            }
                        }
                    }
                    ContentBlock::Text { text } => parts.push(json!({ "type": "text", "text": text })),
                    ContentBlock::Image(img) => parts.push(image_part(img)),
                    ContentBlock::File(f) => {
                        let mut file = Map::new();
                        if let Some(name) = &f.filename {
                            file.insert("filename".into(), json!(name));
                        }
                        match &f.source {
                            FileSource::Base64 { data } => {
                                let media = f.media_type.as_deref().unwrap_or("application/octet-stream");
                                file.insert("file_data".into(), json!(format!("data:{media};base64,{data}")));
                            }
                            FileSource::FileId { id } => {
                                file.insert("file_id".into(), json!(id));
                            }
                            FileSource::Url { .. } => {
                                return Err(ModelError::unsupported("file URLs are not supported by this provider"));
                            }
                        }
                        parts.push(json!({ "type": "file", "file": file }));
                    }
                    ContentBlock::Reasoning(_) | ContentBlock::ToolCall(_) => {
                        return Err(ModelError::protocol("user messages cannot contain reasoning or tool calls"));
                    }
                }
            }
            // Tool messages are text-only on this wire; images move to a user turn right after.
            if !tool_images.is_empty() {
                let mut content = Vec::new();
                for (call_id, img) in &tool_images {
                    content.push(json!({ "type": "text", "text": format!("Image returned by tool call {call_id}:") }));
                    content.push(image_part(img));
                }
                out.push(json!({ "role": "user", "content": content }));
            }
            if !parts.is_empty() {
                let content = if parts.len() == 1 && parts[0]["type"] == "text" {
                    parts[0]["text"].clone()
                } else {
                    Value::Array(parts)
                };
                out.push(json!({ "role": "user", "content": content }));
            }
        }
        Role::Assistant => {
            let mut text = String::new();
            let mut reasoning = String::new();
            let mut calls = Vec::new();
            for b in &m.content {
                match b {
                    ContentBlock::Text { text: t } => text.push_str(t),
                    ContentBlock::Reasoning(r) => {
                        if opts.replay_reasoning {
                            reasoning.push_str(&r.text);
                        } else {
                            *dropped_reasoning += 1;
                        }
                    }
                    ContentBlock::ToolCall(c) => {
                        let arguments = match c.kind {
                            ToolCallKind::Function => c.arguments.clone(),
                            ToolCallKind::Custom => json!({ "input": c.arguments }).to_string(),
                        };
                        calls.push(json!({
                            "id": c.id, "type": "function",
                            "function": { "name": c.name, "arguments": arguments },
                        }));
                    }
                    ContentBlock::Image(_) | ContentBlock::File(_) | ContentBlock::ToolResult(_) => {
                        return Err(ModelError::protocol("assistant messages cannot contain images, files, or tool results"));
                    }
                }
            }
            let mut msg = Map::new();
            msg.insert("role".into(), json!("assistant"));
            msg.insert("content".into(), if text.is_empty() && !calls.is_empty() { Value::Null } else { json!(text) });
            if !reasoning.is_empty() {
                msg.insert("reasoning_content".into(), json!(reasoning));
            }
            if !calls.is_empty() {
                msg.insert("tool_calls".into(), Value::Array(calls));
            }
            out.push(Value::Object(msg));
        }
    }
    Ok(())
}

fn image_part(img: &owo_core::ImageInput) -> Value {
    let mut image_url = json!({ "url": img.source.to_url() });
    if let Some(d) = &img.detail {
        image_url["detail"] = json!(d);
    }
    json!({ "type": "image_url", "image_url": image_url })
}

fn text_only(blocks: &[ContentBlock], role: &str) -> Result<String, ModelError> {
    let mut out = String::new();
    for b in blocks {
        match b.as_text() {
            Some(t) => out.push_str(t),
            None => return Err(ModelError::unsupported(format!("{role} messages support text only"))),
        }
    }
    Ok(out)
}

#[derive(Debug, Default)]
struct PendingTool {
    id: String,
    name: String,
    args: String,
    custom_wire: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Open {
    None,
    Text,
    Reasoning,
}

/// Decodes a Chat Completions SSE stream (one `data:` payload at a time) into events.
///
/// Tool calls are buffered until the message finishes and then emitted in index
/// order, so interleaved upstream deltas never violate the canonical
/// "blocks do not interleave" contract.
pub struct ChatStreamDecoder {
    fallback_model: String,
    custom_tools: HashSet<String>,
    started: bool,
    open: Open,
    tools: BTreeMap<u64, PendingTool>,
    finish: Option<StopReason>,
    usage: Option<Usage>,
    ended: bool,
}

impl ChatStreamDecoder {
    pub fn new(fallback_model: impl Into<String>, custom_tools: HashSet<String>) -> Self {
        Self {
            fallback_model: fallback_model.into(),
            custom_tools,
            started: false,
            open: Open::None,
            tools: BTreeMap::new(),
            finish: None,
            usage: None,
            ended: false,
        }
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    /// Handles one SSE `data:` payload.
    pub fn push_data(&mut self, data: &str) -> Vec<ModelEvent> {
        if self.ended {
            return Vec::new();
        }
        let data = data.trim();
        if data.is_empty() {
            return Vec::new();
        }
        if data == "[DONE]" {
            return self.finish(true);
        }
        let chunk: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return self.fail(ModelError::upstream_invalid("upstream sent a non-JSON stream chunk")),
        };
        if let Some(err) = chunk.get("error").filter(|e| !e.is_null()) {
            return self.fail(upstream_error_from_body(err));
        }

        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            out.push(ModelEvent::ResponseStart {
                id: str_field(&chunk, "id").map(str::to_string).unwrap_or_else(|| new_id("chatcmpl-")),
                model: str_field(&chunk, "model").unwrap_or(&self.fallback_model).to_string(),
            });
            out.push(ModelEvent::MessageStart);
        }

        if let Some(choice) = chunk.get("choices").and_then(Value::as_array).and_then(|c| c.first()) {
            if let Some(delta) = choice.get("delta") {
                let reasoning = str_field(delta, "reasoning_content").or_else(|| str_field(delta, "reasoning"));
                if let Some(text) = reasoning.filter(|t| !t.is_empty()) {
                    self.switch(Open::Reasoning, &mut out);
                    out.push(ModelEvent::ReasoningDelta { text: text.to_string() });
                }
                if let Some(text) = str_field(delta, "content").filter(|t| !t.is_empty()) {
                    self.switch(Open::Text, &mut out);
                    out.push(ModelEvent::TextDelta { text: text.to_string() });
                }
                if let Some(calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for (pos, call) in calls.iter().enumerate() {
                        self.absorb_tool_delta(call, pos as u64);
                    }
                }
            }
            if let Some(reason) = str_field(choice, "finish_reason") {
                self.finish = Some(parse_finish_reason(reason));
                self.switch(Open::None, &mut out);
                self.flush_tools(&mut out);
            }
        }
        if let Some(usage) = chunk.get("usage").and_then(parse_chat_usage) {
            match &mut self.usage {
                Some(u) => u.merge(&usage),
                None => self.usage = Some(usage),
            }
        }
        out
    }

    /// Ends the stream. `saw_done` is true when the upstream sent `[DONE]`.
    pub fn finish(&mut self, saw_done: bool) -> Vec<ModelEvent> {
        if self.ended {
            return Vec::new();
        }
        if !self.started {
            return self.fail(ModelError::upstream_invalid("upstream stream ended without any data"));
        }
        if self.finish.is_none() && !saw_done {
            return self.fail(ModelError::upstream_invalid("upstream stream ended before completion"));
        }
        self.ended = true;
        let mut out = Vec::new();
        self.switch(Open::None, &mut out);
        let had_tools = !self.tools.is_empty();
        self.flush_tools(&mut out);
        if let Some(u) = self.usage.take() {
            out.push(ModelEvent::Usage(u));
        }
        let stop = self.finish.take().unwrap_or(if had_tools { StopReason::ToolUse } else { StopReason::EndTurn });
        out.push(ModelEvent::MessageEnd { stop_reason: stop });
        out.push(ModelEvent::ResponseEnd);
        out
    }

    fn fail(&mut self, err: ModelError) -> Vec<ModelEvent> {
        self.ended = true;
        vec![ModelEvent::Error(err)]
    }

    fn switch(&mut self, to: Open, out: &mut Vec<ModelEvent>) {
        if self.open == to {
            return;
        }
        match self.open {
            Open::Text => out.push(ModelEvent::TextEnd),
            Open::Reasoning => out.push(ModelEvent::ReasoningEnd { signature: None, encrypted_content: None }),
            Open::None => {}
        }
        match to {
            Open::Text => out.push(ModelEvent::TextStart),
            Open::Reasoning => out.push(ModelEvent::ReasoningStart),
            Open::None => {}
        }
        self.open = to;
    }

    fn absorb_tool_delta(&mut self, call: &Value, pos: u64) {
        let index = call.get("index").and_then(Value::as_u64).unwrap_or(pos);
        let entry = self.tools.entry(index).or_default();
        if let Some(id) = str_field(call, "id").filter(|s| !s.is_empty()) {
            entry.id = id.to_string();
        }
        let (fields, arg_key) = match call.get("custom") {
            Some(c) => {
                entry.custom_wire = true;
                (c, "input")
            }
            None => match call.get("function") {
                Some(f) => (f, "arguments"),
                None => return,
            },
        };
        if let Some(name) = str_field(fields, "name").filter(|s| !s.is_empty()) {
            if entry.name.is_empty() {
                entry.name = name.to_string();
            }
        }
        if let Some(args) = str_field(fields, arg_key) {
            entry.args.push_str(args);
        }
    }

    fn flush_tools(&mut self, out: &mut Vec<ModelEvent>) {
        for (_, t) in std::mem::take(&mut self.tools) {
            let call = finish_tool_call(t, &self.custom_tools);
            let id = call.id.clone();
            out.push(ModelEvent::ToolCallStart { id: id.clone(), name: call.name, kind: call.kind });
            if !call.arguments.is_empty() {
                out.push(ModelEvent::ToolCallDelta { id: id.clone(), arguments_delta: call.arguments });
            }
            out.push(ModelEvent::ToolCallEnd { id });
        }
    }
}

fn finish_tool_call(t: PendingTool, custom_tools: &HashSet<String>) -> ToolCall {
    let id = if t.id.is_empty() { new_id("call_") } else { t.id };
    if t.custom_wire {
        return ToolCall { id, name: t.name, arguments: t.args, kind: ToolCallKind::Custom };
    }
    if custom_tools.contains(&t.name) {
        // Unwrap `{"input": "..."}`; if the model ignored the schema keep its raw output.
        let input = serde_json::from_str::<Value>(&t.args)
            .ok()
            .and_then(|v| v.get("input").and_then(Value::as_str).map(str::to_string))
            .unwrap_or(t.args);
        return ToolCall { id, name: t.name, arguments: input, kind: ToolCallKind::Custom };
    }
    ToolCall { id, name: t.name, arguments: t.args, kind: ToolCallKind::Function }
}

/// Classifies an in-band `{"error": {...}}` object.
pub fn upstream_error_from_body(err: &Value) -> ModelError {
    let message = str_field(err, "message").unwrap_or("upstream error").to_string();
    let code = str_field(err, "code").or_else(|| str_field(err, "type")).unwrap_or_default();
    let kind = match code {
        c if c.contains("rate_limit") => ErrorKind::RateLimited,
        c if c.contains("context_length") => ErrorKind::ContextExceeded,
        c if c.contains("auth") || c == "invalid_api_key" => ErrorKind::AuthenticationFailed,
        c if c.contains("not_found") => ErrorKind::ModelNotFound,
        _ => ErrorKind::ProviderUnavailable,
    };
    ModelError::new(kind, message)
}

/// Decodes a non-streaming `chat.completion` object.
pub fn decode_response(
    body: &Value,
    fallback_model: &str,
    custom_tools: &HashSet<String>,
) -> Result<ModelResponse, ModelError> {
    if let Some(err) = body.get("error").filter(|e| !e.is_null()) {
        return Err(upstream_error_from_body(err));
    }
    let choice = body
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|c| c.first())
        .ok_or_else(|| ModelError::upstream_invalid("upstream response has no choices"))?;
    let message = choice.get("message").ok_or_else(|| ModelError::upstream_invalid("upstream choice has no message"))?;

    let mut content = Vec::new();
    let reasoning = str_field(message, "reasoning_content").or_else(|| str_field(message, "reasoning"));
    if let Some(text) = reasoning.filter(|t| !t.is_empty()) {
        content.push(ContentBlock::Reasoning(ReasoningBlock { text: text.to_string(), ..Default::default() }));
    }
    match message.get("content") {
        Some(Value::String(s)) if !s.is_empty() => content.push(ContentBlock::text(s.clone())),
        Some(Value::Array(parts)) => {
            let text: String = parts.iter().filter_map(|p| str_field(p, "text")).collect();
            if !text.is_empty() {
                content.push(ContentBlock::text(text));
            }
        }
        _ => {}
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for call in calls {
            let mut pending = PendingTool { id: str_field(call, "id").unwrap_or_default().to_string(), ..Default::default() };
            let fields = match call.get("custom") {
                Some(c) => {
                    pending.custom_wire = true;
                    pending.args = str_field(c, "input").unwrap_or_default().to_string();
                    c
                }
                None => {
                    let f = call.get("function").ok_or_else(|| ModelError::upstream_invalid("tool call without function"))?;
                    pending.args = str_field(f, "arguments").unwrap_or_default().to_string();
                    f
                }
            };
            pending.name = str_field(fields, "name").unwrap_or_default().to_string();
            content.push(ContentBlock::ToolCall(finish_tool_call(pending, custom_tools)));
        }
    }
    let has_tools = content.iter().any(|b| matches!(b, ContentBlock::ToolCall(_)));
    let stop_reason = str_field(choice, "finish_reason")
        .map(parse_finish_reason)
        .unwrap_or(if has_tools { StopReason::ToolUse } else { StopReason::EndTurn });

    Ok(ModelResponse {
        id: str_field(body, "id").map(str::to_string).unwrap_or_else(|| new_id("chatcmpl-")),
        model: str_field(body, "model").unwrap_or(fallback_model).to_string(),
        content,
        stop_reason,
        usage: body.get("usage").and_then(parse_chat_usage),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use owo_core::{ImageInput, ImageSource, ResponseAccumulator, ToolResult};

    fn request() -> ModelRequest {
        let mut req = ModelRequest::new("r", "alias");
        req.system.push(ContentBlock::text("sys"));
        req.messages.push(Message::user_text("hi"));
        req.messages.push(Message::new(
            Role::Assistant,
            vec![
                ContentBlock::Reasoning(ReasoningBlock { text: "think".into(), ..Default::default() }),
                ContentBlock::ToolCall(ToolCall {
                    id: "c1".into(),
                    name: "apply_patch".into(),
                    arguments: "*** Begin Patch".into(),
                    kind: ToolCallKind::Custom,
                }),
            ],
        ));
        req.messages.push(Message::new(
            Role::User,
            vec![ContentBlock::ToolResult(ToolResult {
                call_id: "c1".into(),
                content: vec![
                    ToolResultContent::Text { text: "ok".into() },
                    ToolResultContent::Image(ImageInput {
                        source: ImageSource::Url { url: "https://x/img.png".into() },
                        detail: None,
                    }),
                ],
                is_error: false,
                kind: ToolCallKind::Custom,
            })],
        ));
        req.tools.push(ToolDefinition::Custom {
            name: "apply_patch".into(),
            description: Some("Patch files".into()),
            format: Some(json!({"type": "grammar", "definition": "start: patch"})),
        });
        req.max_output_tokens = Some(64);
        req.stream = true;
        req
    }

    #[test]
    fn encodes_request_for_chat_upstream() {
        let enc = encode_request(&request(), "deepseek-chat", &EncodeOptions::default()).unwrap();
        let b = &enc.body;
        assert_eq!(b["model"], "deepseek-chat");
        assert_eq!(b["messages"][0], json!({"role": "system", "content": "sys"}));
        assert_eq!(b["messages"][1], json!({"role": "user", "content": "hi"}));
        let call = &b["messages"][2]["tool_calls"][0]["function"];
        assert_eq!(call["arguments"], json!({"input": "*** Begin Patch"}).to_string());
        assert!(b["messages"][2].get("reasoning_content").is_none());
        assert_eq!(b["messages"][3], json!({"role": "tool", "tool_call_id": "c1", "content": "ok"}));
        assert_eq!(b["messages"][4]["content"][1]["image_url"]["url"], "https://x/img.png");
        assert!(b["tools"][0]["function"]["description"].as_str().unwrap().contains("start: patch"));
        assert_eq!(b["max_tokens"], 64);
        assert_eq!(b["stream_options"]["include_usage"], true);
        assert!(enc.custom_tools.contains("apply_patch"));
        assert_eq!(enc.notes.len(), 1);
    }

    #[test]
    fn replays_reasoning_when_enabled() {
        let opts = EncodeOptions { replay_reasoning: true, ..Default::default() };
        let enc = encode_request(&request(), "m", &opts).unwrap();
        assert_eq!(enc.body["messages"][2]["reasoning_content"], "think");
        assert!(enc.notes.is_empty());
    }

    #[test]
    fn hosted_tools_are_omitted_with_a_warning_note() {
        let mut req = request();
        req.tools.push(ToolDefinition::Hosted { kind: "web_search".into(), config: json!({}) });
        let enc = encode_request(&req, "m", &EncodeOptions::default()).unwrap();
        assert_eq!(enc.body["tools"].as_array().unwrap().len(), 1);
        assert!(enc.notes.iter().any(|n| n.starts_with(WARN_NOTE_PREFIX) && n.contains("web_search")));

        // Only hosted tools: no `tools`, and no dangling tool_choice.
        req.tools = vec![ToolDefinition::Hosted { kind: "web_search".into(), config: json!({}) }];
        req.tool_choice = Some(ToolChoice::Auto);
        let enc = encode_request(&req, "m", &EncodeOptions::default()).unwrap();
        assert!(enc.body.get("tools").is_none());
        assert!(enc.body.get("tool_choice").is_none());
    }

    #[test]
    fn reasoning_effort_only_when_declared() {
        let mut req = request();
        req.reasoning = Some(owo_core::ReasoningConfig { effort: Some("high".into()), ..Default::default() });
        let enc = encode_request(&req, "m", &EncodeOptions::default()).unwrap();
        assert!(enc.body.get("reasoning_effort").is_none());
        let opts = EncodeOptions { send_reasoning_effort: true, ..Default::default() };
        assert_eq!(encode_request(&req, "m", &opts).unwrap().body["reasoning_effort"], "high");
    }

    fn run(decoder: &mut ChatStreamDecoder, chunks: &[&str]) -> Vec<ModelEvent> {
        let mut out = Vec::new();
        for c in chunks {
            out.extend(decoder.push_data(c));
        }
        out.extend(decoder.finish(false));
        out
    }

    #[test]
    fn decodes_stream_with_reasoning_text_and_interleaved_tools() {
        let mut d = ChatStreamDecoder::new("m", HashSet::from(["apply_patch".to_string()]));
        let events = run(
            &mut d,
            &[
                r#"{"id":"u1","model":"up","choices":[{"index":0,"delta":{"role":"assistant","reasoning_content":"th"}}]}"#,
                r#"{"choices":[{"delta":{"reasoning_content":"ink"}}]}"#,
                r#"{"choices":[{"delta":{"content":"Hi"}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"id":"a","function":{"name":"f","arguments":"{\"x\""}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"id":"b","function":{"name":"apply_patch","arguments":"{\"input\":"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":0,"function":{"arguments":":1}"}}]}}]}"#,
                r#"{"choices":[{"delta":{"tool_calls":[{"index":1,"function":{"arguments":"\"P\"}"}}]}}]}"#,
                r#"{"choices":[{"delta":{},"finish_reason":"tool_calls"}]}"#,
                r#"{"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":4,"prompt_cache_hit_tokens":6}}"#,
                "[DONE]",
            ],
        );
        let mut acc = ResponseAccumulator::new();
        for e in events.clone() {
            acc.push(e);
        }
        let resp = acc.finish().unwrap();
        assert_eq!(resp.id, "u1");
        assert_eq!(resp.content.len(), 4);
        assert!(matches!(&resp.content[0], ContentBlock::Reasoning(r) if r.text == "think"));
        assert_eq!(resp.content[1], ContentBlock::text("Hi"));
        assert!(matches!(&resp.content[2], ContentBlock::ToolCall(c) if c.arguments == "{\"x\":1}" && c.kind == ToolCallKind::Function));
        assert!(matches!(&resp.content[3], ContentBlock::ToolCall(c) if c.arguments == "P" && c.kind == ToolCallKind::Custom));
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.usage.unwrap().cached_input_tokens, Some(6));
        assert!(matches!(events.last(), Some(ModelEvent::ResponseEnd)));
        // Blocks never interleave: every Start is followed by its own End before the next Start.
        let mut depth = 0i32;
        for e in &events {
            match e {
                ModelEvent::TextStart | ModelEvent::ReasoningStart | ModelEvent::ToolCallStart { .. } => depth += 1,
                ModelEvent::TextEnd | ModelEvent::ReasoningEnd { .. } | ModelEvent::ToolCallEnd { .. } => depth -= 1,
                _ => {}
            }
            assert!((0..=1).contains(&depth));
        }
    }

    #[test]
    fn truncated_stream_is_an_error() {
        let mut d = ChatStreamDecoder::new("m", HashSet::new());
        let events = run(&mut d, &[r#"{"choices":[{"delta":{"content":"par"}}]}"#]);
        assert!(matches!(events.last(), Some(ModelEvent::Error(e)) if e.kind == ErrorKind::UpstreamInvalidResponse));
    }

    #[test]
    fn malformed_and_error_chunks() {
        let mut d = ChatStreamDecoder::new("m", HashSet::new());
        assert!(matches!(&d.push_data("{not json")[..], [ModelEvent::Error(_)]));
        assert!(d.push_data("[DONE]").is_empty());

        let mut d = ChatStreamDecoder::new("m", HashSet::new());
        let ev = d.push_data(r#"{"error":{"message":"Rate limit reached","code":"rate_limit_exceeded"}}"#);
        assert!(matches!(&ev[..], [ModelEvent::Error(e)] if e.kind == ErrorKind::RateLimited));
    }

    #[test]
    fn done_without_finish_reason_is_accepted() {
        let mut d = ChatStreamDecoder::new("m", HashSet::new());
        let events = run(&mut d, &[r#"{"choices":[{"delta":{"content":"x"}}]}"#, "[DONE]"]);
        assert!(matches!(events[events.len() - 2], ModelEvent::MessageEnd { stop_reason: StopReason::EndTurn }));
    }

    #[test]
    fn decodes_non_stream_response() {
        let body = json!({
            "id": "c", "model": "up",
            "choices": [{"message": {"role": "assistant", "content": "hello", "reasoning": "r",
                "tool_calls": [{"id": "t", "type": "function", "function": {"name": "f", "arguments": "{}"}}]},
                "finish_reason": "tool_calls"}],
            "usage": {"prompt_tokens": 1, "completion_tokens": 2,
                      "completion_tokens_details": {"reasoning_tokens": 1}}
        });
        let resp = decode_response(&body, "m", &HashSet::new()).unwrap();
        assert_eq!(resp.content.len(), 3);
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        assert_eq!(resp.usage.unwrap().reasoning_tokens, Some(1));
        assert!(decode_response(&json!({"choices": []}), "m", &HashSet::new()).is_err());
    }
}
