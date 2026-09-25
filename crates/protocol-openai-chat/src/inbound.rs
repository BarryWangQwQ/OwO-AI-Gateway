//! Client-facing Chat Completions: request decoding and response/stream encoding.

use std::collections::HashMap;

use bytes::Bytes;
use serde_json::{json, Map, Value};
use owo_core::{
    ContentBlock, FileInput, FileSource, ImageInput, ImageSource, InboundProtocol, Message,
    ModelError, ModelEvent, ModelRequest, ModelResponse, OutputFormat, ReasoningBlock,
    ReasoningConfig, Role, ToolCall, ToolCallKind, ToolChoice, ToolDefinition, ToolResult,
    ToolResultContent, Usage,
};

use crate::common::{chat_usage, error_body, finish_reason, new_id, unix_now};
use crate::json::{invalid, str_field, take_bool, take_f64, take_i64, take_string, take_u32};

pub struct DecodedRequest {
    pub request: ModelRequest,
    /// `stream_options.include_usage`: emit a final usage chunk.
    pub include_usage: bool,
}

pub fn decode_request(body: Value, request_id: impl Into<String>) -> Result<DecodedRequest, ModelError> {
    let Value::Object(mut obj) = body else {
        return Err(invalid("request body must be a JSON object"));
    };
    let model = take_string(&mut obj, "model")?.ok_or_else(|| invalid("`model` is required"))?;
    let messages = match obj.remove("messages") {
        Some(Value::Array(a)) => a,
        _ => return Err(invalid("`messages` must be an array")),
    };

    let mut req = ModelRequest::new(request_id, model);
    req.metadata.inbound = Some(InboundProtocol::OpenaiChat);
    for (i, m) in messages.into_iter().enumerate() {
        decode_message(m, i, &mut req)?;
    }

    if let Some(tools) = obj.remove("tools") {
        req.tools = decode_tools(tools)?;
    }
    if let Some(choice) = obj.remove("tool_choice") {
        req.tool_choice = decode_tool_choice(choice)?;
    }
    if let Some(format) = obj.remove("response_format") {
        req.output_format = decode_response_format(format)?;
    }

    req.max_output_tokens = match take_u32(&mut obj, "max_completion_tokens")? {
        Some(v) => {
            obj.remove("max_tokens");
            Some(v)
        }
        None => take_u32(&mut obj, "max_tokens")?,
    };
    req.sampling.temperature = take_f64(&mut obj, "temperature")?;
    req.sampling.top_p = take_f64(&mut obj, "top_p")?;
    req.sampling.top_k = take_u32(&mut obj, "top_k")?;
    req.sampling.frequency_penalty = take_f64(&mut obj, "frequency_penalty")?;
    req.sampling.presence_penalty = take_f64(&mut obj, "presence_penalty")?;
    req.sampling.seed = take_i64(&mut obj, "seed")?;
    req.sampling.stop = match obj.remove("stop") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::String(s)) => vec![s],
        Some(Value::Array(a)) => a
            .into_iter()
            .map(|v| v.as_str().map(str::to_string).ok_or_else(|| invalid("`stop` entries must be strings")))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(invalid("`stop` must be a string or an array of strings")),
    };

    req.stream = take_bool(&mut obj, "stream")?.unwrap_or(false);
    let include_usage = match obj.remove("stream_options") {
        Some(opts) => opts.get("include_usage").and_then(Value::as_bool).unwrap_or(false),
        None => false,
    };

    let effort = take_string(&mut obj, "reasoning_effort")?;
    let reasoning_obj = obj.remove("reasoning");
    let effort = effort.or_else(|| reasoning_obj.as_ref().and_then(|r| str_field(r, "effort")).map(str::to_string));
    if let Some(effort) = effort {
        req.reasoning = Some(ReasoningConfig { effort: Some(effort.to_ascii_lowercase()), ..Default::default() });
    }

    req.metadata.parallel_tool_calls = take_bool(&mut obj, "parallel_tool_calls")?;
    req.metadata.user = take_string(&mut obj, "user")?;
    req.metadata.prompt_cache_key = take_string(&mut obj, "prompt_cache_key")?;
    req.metadata.service_tier = take_string(&mut obj, "service_tier")?;
    req.metadata.verbosity = take_string(&mut obj, "verbosity")?;

    if let Some(n) = take_u32(&mut obj, "n")? {
        if n != 1 {
            return Err(ModelError::unsupported("`n` other than 1 is not supported"));
        }
    }
    if take_bool(&mut obj, "logprobs")? == Some(true) {
        return Err(ModelError::unsupported("`logprobs` is not supported"));
    }
    if obj.get("modalities").and_then(Value::as_array).is_some_and(|m| m.iter().any(|v| v != "text")) {
        return Err(ModelError::unsupported("only text output modality is supported"));
    }

    req.metadata.extra = obj.into_iter().collect();
    Ok(DecodedRequest { request: req, include_usage })
}

fn decode_message(msg: Value, index: usize, req: &mut ModelRequest) -> Result<(), ModelError> {
    let Value::Object(mut m) = msg else {
        return Err(invalid(format!("messages[{index}] must be an object")));
    };
    let role = take_string(&mut m, "role")?.ok_or_else(|| invalid(format!("messages[{index}].role is required")))?;
    match role.as_str() {
        "system" | "developer" => {
            let text = text_content(m.remove("content"), index)?;
            if req.messages.is_empty() {
                req.system.push(ContentBlock::text(text));
            } else {
                req.messages.push(Message::new(Role::System, vec![ContentBlock::text(text)]));
            }
        }
        "user" => {
            let content = user_content(m.remove("content"), index)?;
            req.messages.push(Message::new(Role::User, content));
        }
        "assistant" => {
            let mut blocks = Vec::new();
            let reasoning = take_string(&mut m, "reasoning_content")?.or(take_string(&mut m, "reasoning")?);
            if let Some(text) = reasoning.filter(|t| !t.is_empty()) {
                blocks.push(ContentBlock::Reasoning(ReasoningBlock { text, ..Default::default() }));
            }
            let text = text_content(m.remove("content"), index)?;
            if !text.is_empty() {
                blocks.push(ContentBlock::text(text));
            }
            if let Some(Value::Array(calls)) = m.remove("tool_calls") {
                for call in calls {
                    blocks.push(ContentBlock::ToolCall(decode_tool_call(call, index)?));
                }
            }
            if let Some(fc) = m.remove("function_call").filter(|v| !v.is_null()) {
                blocks.push(ContentBlock::ToolCall(ToolCall {
                    id: format!("call_legacy_{index}"),
                    name: str_field(&fc, "name").unwrap_or_default().to_string(),
                    arguments: str_field(&fc, "arguments").unwrap_or_default().to_string(),
                    kind: ToolCallKind::Function,
                }));
            }
            req.messages.push(Message::new(Role::Assistant, blocks));
        }
        "tool" => {
            let call_id = take_string(&mut m, "tool_call_id")?
                .ok_or_else(|| invalid(format!("messages[{index}].tool_call_id is required")))?;
            let text = text_content(m.remove("content"), index)?;
            let result = ContentBlock::ToolResult(ToolResult {
                call_id,
                content: vec![ToolResultContent::Text { text }],
                is_error: false,
                kind: ToolCallKind::Function,
            });
            push_tool_result(req, result);
        }
        "function" => return Err(ModelError::unsupported("legacy `function` role messages are not supported")),
        other => return Err(invalid(format!("messages[{index}]: unknown role `{other}`"))),
    }
    Ok(())
}

/// Consecutive tool results join one user turn, matching the canonical shape.
fn push_tool_result(req: &mut ModelRequest, block: ContentBlock) {
    if let Some(last) = req.messages.last_mut() {
        if last.role == Role::User && last.content.iter().all(|b| matches!(b, ContentBlock::ToolResult(_))) {
            last.content.push(block);
            return;
        }
    }
    req.messages.push(Message::new(Role::User, vec![block]));
}

fn decode_tool_call(call: Value, index: usize) -> Result<ToolCall, ModelError> {
    let id = str_field(&call, "id").unwrap_or_default().to_string();
    if id.is_empty() {
        return Err(invalid(format!("messages[{index}].tool_calls[].id is required")));
    }
    match str_field(&call, "type").unwrap_or("function") {
        "function" => {
            let f = call.get("function").ok_or_else(|| invalid("tool call is missing `function`"))?;
            Ok(ToolCall {
                id,
                name: str_field(f, "name").unwrap_or_default().to_string(),
                arguments: str_field(f, "arguments").unwrap_or_default().to_string(),
                kind: ToolCallKind::Function,
            })
        }
        "custom" => {
            let c = call.get("custom").ok_or_else(|| invalid("tool call is missing `custom`"))?;
            Ok(ToolCall {
                id,
                name: str_field(c, "name").unwrap_or_default().to_string(),
                arguments: str_field(c, "input").unwrap_or_default().to_string(),
                kind: ToolCallKind::Custom,
            })
        }
        other => Err(ModelError::unsupported(format!("tool call type `{other}` is not supported"))),
    }
}

/// Text-only content: a string or an array of `text`/`refusal` parts.
fn text_content(content: Option<Value>, index: usize) -> Result<String, ModelError> {
    match content {
        None | Some(Value::Null) => Ok(String::new()),
        Some(Value::String(s)) => Ok(s),
        Some(Value::Array(parts)) => {
            let mut out = String::new();
            for part in parts {
                match str_field(&part, "type") {
                    Some("text") => out.push_str(str_field(&part, "text").unwrap_or_default()),
                    Some("refusal") => out.push_str(str_field(&part, "refusal").unwrap_or_default()),
                    other => {
                        return Err(ModelError::unsupported(format!(
                            "messages[{index}]: content part `{}` is only allowed in user messages",
                            other.unwrap_or("?")
                        )));
                    }
                }
            }
            Ok(out)
        }
        Some(_) => Err(invalid(format!("messages[{index}].content must be a string or an array"))),
    }
}

fn user_content(content: Option<Value>, index: usize) -> Result<Vec<ContentBlock>, ModelError> {
    match content {
        None | Some(Value::Null) => Ok(Vec::new()),
        Some(Value::String(s)) => Ok(vec![ContentBlock::text(s)]),
        Some(Value::Array(parts)) => parts.into_iter().map(|p| user_part(p, index)).collect(),
        Some(_) => Err(invalid(format!("messages[{index}].content must be a string or an array"))),
    }
}

fn user_part(part: Value, index: usize) -> Result<ContentBlock, ModelError> {
    match str_field(&part, "type") {
        Some("text") => Ok(ContentBlock::text(str_field(&part, "text").unwrap_or_default())),
        Some("image_url") => {
            let img = part.get("image_url").ok_or_else(|| invalid("`image_url` part is missing `image_url`"))?;
            let (url, detail) = match img {
                Value::String(s) => (s.as_str(), None),
                _ => (str_field(img, "url").unwrap_or_default(), str_field(img, "detail").map(str::to_string)),
            };
            if url.is_empty() {
                return Err(invalid(format!("messages[{index}]: image_url.url is required")));
            }
            Ok(ContentBlock::Image(ImageInput { source: ImageSource::from_url(url), detail }))
        }
        Some("file") => {
            let f = part.get("file").ok_or_else(|| invalid("`file` part is missing `file`"))?;
            let filename = str_field(f, "filename").map(str::to_string);
            if let Some(id) = str_field(f, "file_id") {
                return Ok(ContentBlock::File(FileInput {
                    filename,
                    media_type: None,
                    source: FileSource::FileId { id: id.to_string() },
                }));
            }
            let data = str_field(f, "file_data").ok_or_else(|| invalid("`file` needs `file_data` or `file_id`"))?;
            let (media_type, data) = match ImageSource::from_url(data) {
                ImageSource::Base64 { media_type, data } => (Some(media_type), data),
                ImageSource::Url { url } => (None, url),
            };
            Ok(ContentBlock::File(FileInput { filename, media_type, source: FileSource::Base64 { data } }))
        }
        Some("input_audio") => Err(ModelError::unsupported("audio input is not supported")),
        other => Err(ModelError::unsupported(format!(
            "messages[{index}]: content part type `{}` is not supported",
            other.unwrap_or("?")
        ))),
    }
}

pub(crate) fn decode_tools(tools: Value) -> Result<Vec<ToolDefinition>, ModelError> {
    let Value::Array(tools) = tools else {
        return Err(invalid("`tools` must be an array"));
    };
    tools
        .into_iter()
        .map(|t| match str_field(&t, "type").unwrap_or("function") {
            "function" => {
                let f = t.get("function").ok_or_else(|| invalid("function tool is missing `function`"))?;
                let name = str_field(f, "name").filter(|n| !n.is_empty()).ok_or_else(|| {
                    ModelError::new(owo_core::ErrorKind::InvalidToolSchema, "function tool is missing `name`")
                })?;
                Ok(ToolDefinition::Function {
                    name: name.to_string(),
                    description: str_field(f, "description").map(str::to_string),
                    parameters: f.get("parameters").cloned().unwrap_or_else(|| json!({"type": "object", "properties": {}})),
                    strict: f.get("strict").and_then(Value::as_bool),
                })
            }
            "custom" => {
                let c = t.get("custom").ok_or_else(|| invalid("custom tool is missing `custom`"))?;
                Ok(ToolDefinition::Custom {
                    name: str_field(c, "name").unwrap_or_default().to_string(),
                    description: str_field(c, "description").map(str::to_string),
                    format: c.get("format").cloned(),
                })
            }
            other => Ok(ToolDefinition::Hosted { kind: other.to_string(), config: t.clone() }),
        })
        .collect()
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
        Value::Object(_) => {
            let name = choice
                .get("function")
                .or_else(|| choice.get("custom"))
                .and_then(|f| str_field(f, "name"))
                .ok_or_else(|| invalid("tool_choice object needs function.name"))?;
            ToolChoice::Tool { name: name.to_string() }
        }
        _ => return Err(invalid("`tool_choice` must be a string or an object")),
    }))
}

fn decode_response_format(format: Value) -> Result<Option<OutputFormat>, ModelError> {
    Ok(match str_field(&format, "type") {
        None if format.is_null() => None,
        Some("text") => Some(OutputFormat::Text),
        Some("json_object") => Some(OutputFormat::JsonObject),
        Some("json_schema") => {
            let js = format.get("json_schema").ok_or_else(|| invalid("json_schema format needs `json_schema`"))?;
            Some(OutputFormat::JsonSchema {
                name: str_field(js, "name").unwrap_or("response").to_string(),
                schema: js.get("schema").cloned().unwrap_or_else(|| json!({})),
                description: str_field(js, "description").map(str::to_string),
                strict: js.get("strict").and_then(Value::as_bool),
            })
        }
        other => return Err(ModelError::unsupported(format!("response_format `{}` is not supported", other.unwrap_or("?")))),
    })
}

/// Encodes canonical events as `chat.completion.chunk` SSE frames.
pub struct ChatStreamEncoder {
    id: String,
    model: String,
    created: u64,
    include_usage: bool,
    tool_index: HashMap<String, (usize, ToolCallKind)>,
    usage: Option<Usage>,
    done: bool,
}

impl ChatStreamEncoder {
    pub fn new(model: impl Into<String>, include_usage: bool) -> Self {
        Self {
            id: new_id("chatcmpl-"),
            model: model.into(),
            created: unix_now(),
            include_usage,
            tool_index: HashMap::new(),
            usage: None,
            done: false,
        }
    }

    fn chunk(&self, delta: Value, finish: Option<String>) -> Bytes {
        let body = json!({
            "id": self.id,
            "object": "chat.completion.chunk",
            "created": self.created,
            "model": self.model,
            "choices": [{ "index": 0, "delta": delta, "finish_reason": finish }],
        });
        owo_sse::encode(None, &body.to_string())
    }

    pub fn push(&mut self, event: ModelEvent) -> Vec<Bytes> {
        if self.done {
            return Vec::new();
        }
        match event {
            ModelEvent::MessageStart => vec![self.chunk(json!({"role": "assistant", "content": ""}), None)],
            ModelEvent::TextDelta { text } => vec![self.chunk(json!({ "content": text }), None)],
            ModelEvent::ReasoningDelta { text } => vec![self.chunk(json!({ "reasoning_content": text }), None)],
            ModelEvent::ToolCallStart { id, name, kind } => {
                let index = self.tool_index.len();
                self.tool_index.insert(id.clone(), (index, kind));
                let call = match kind {
                    ToolCallKind::Function => json!({
                        "index": index, "id": id, "type": "function",
                        "function": { "name": name, "arguments": "" },
                    }),
                    ToolCallKind::Custom => json!({
                        "index": index, "id": id, "type": "custom",
                        "custom": { "name": name, "input": "" },
                    }),
                };
                vec![self.chunk(json!({ "tool_calls": [call] }), None)]
            }
            ModelEvent::ToolCallDelta { id, arguments_delta } => {
                let Some(&(index, kind)) = self.tool_index.get(&id) else { return Vec::new() };
                let call = match kind {
                    ToolCallKind::Function => json!({ "index": index, "function": { "arguments": arguments_delta } }),
                    ToolCallKind::Custom => json!({ "index": index, "custom": { "input": arguments_delta } }),
                };
                vec![self.chunk(json!({ "tool_calls": [call] }), None)]
            }
            ModelEvent::Usage(u) => {
                match &mut self.usage {
                    Some(existing) => existing.merge(&u),
                    None => self.usage = Some(u),
                }
                Vec::new()
            }
            ModelEvent::MessageEnd { stop_reason } => vec![self.chunk(json!({}), Some(finish_reason(&stop_reason)))],
            ModelEvent::ResponseEnd => {
                self.done = true;
                let mut out = Vec::new();
                if self.include_usage {
                    let usage = self.usage.unwrap_or_default();
                    let body = json!({
                        "id": self.id,
                        "object": "chat.completion.chunk",
                        "created": self.created,
                        "model": self.model,
                        "choices": [],
                        "usage": chat_usage(&usage),
                    });
                    out.push(owo_sse::encode(None, &body.to_string()));
                }
                out.push(owo_sse::encode(None, "[DONE]"));
                out
            }
            ModelEvent::Error(err) => {
                self.done = true;
                vec![owo_sse::encode(None, &error_body(&err).to_string()), owo_sse::encode(None, "[DONE]")]
            }
            ModelEvent::ResponseStart { .. }
            | ModelEvent::TextStart
            | ModelEvent::TextEnd
            | ModelEvent::ReasoningStart
            | ModelEvent::ReasoningEnd { .. }
            | ModelEvent::ToolCallEnd { .. } => Vec::new(),
        }
    }
}

/// Encodes a complete response as a `chat.completion` object.
pub fn encode_response(resp: &ModelResponse, model: &str) -> Value {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for block in &resp.content {
        match block {
            ContentBlock::Text { text: t } => text.push_str(t),
            ContentBlock::Reasoning(r) => reasoning.push_str(&r.text),
            ContentBlock::ToolCall(c) => tool_calls.push(match c.kind {
                ToolCallKind::Function => json!({
                    "id": c.id, "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments },
                }),
                ToolCallKind::Custom => json!({
                    "id": c.id, "type": "custom",
                    "custom": { "name": c.name, "input": c.arguments },
                }),
            }),
            _ => {}
        }
    }
    let mut message = Map::new();
    message.insert("role".into(), json!("assistant"));
    message.insert("content".into(), if text.is_empty() && !tool_calls.is_empty() { Value::Null } else { json!(text) });
    if !reasoning.is_empty() {
        message.insert("reasoning_content".into(), json!(reasoning));
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".into(), Value::Array(tool_calls));
    }
    json!({
        "id": new_id("chatcmpl-"),
        "object": "chat.completion",
        "created": unix_now(),
        "model": model,
        "choices": [{ "index": 0, "message": message, "finish_reason": finish_reason(&resp.stop_reason) }],
        "usage": chat_usage(&resp.usage.unwrap_or_default()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use owo_core::StopReason;

    fn decode(v: Value) -> ModelRequest {
        decode_request(v, "req").unwrap().request
    }

    #[test]
    fn decodes_full_conversation() {
        let req = decode(json!({
            "model": "m",
            "messages": [
                {"role": "system", "content": "be brief"},
                {"role": "user", "content": [
                    {"type": "text", "text": "look"},
                    {"type": "image_url", "image_url": {"url": "data:image/png;base64,AAA", "detail": "high"}}
                ]},
                {"role": "assistant", "content": null, "reasoning_content": "hmm",
                 "tool_calls": [{"id": "c1", "type": "function", "function": {"name": "f", "arguments": "{\"a\":1}"}}]},
                {"role": "tool", "tool_call_id": "c1", "content": "result"},
                {"role": "developer", "content": "late instruction"}
            ],
            "tools": [{"type": "function", "function": {"name": "f", "parameters": {"type": "object"}}}],
            "tool_choice": {"type": "function", "function": {"name": "f"}},
            "max_tokens": 100,
            "stop": "END",
            "stream": true,
            "reasoning_effort": "HIGH",
            "custom_vendor_flag": 7
        }));
        assert_eq!(req.system_text(), "be brief");
        assert_eq!(req.messages.len(), 4);
        assert!(matches!(&req.messages[1].content[0], ContentBlock::Reasoning(r) if r.text == "hmm"));
        assert!(matches!(&req.messages[1].content[1], ContentBlock::ToolCall(c) if c.arguments == "{\"a\":1}"));
        assert!(matches!(&req.messages[2].content[0], ContentBlock::ToolResult(r) if r.text() == "result"));
        assert_eq!(req.messages[3].role, Role::System);
        assert!(req.has_images());
        assert_eq!(req.tool_choice, Some(ToolChoice::Tool { name: "f".into() }));
        assert_eq!(req.max_output_tokens, Some(100));
        assert_eq!(req.sampling.stop, vec!["END"]);
        assert_eq!(req.reasoning.unwrap().effort.as_deref(), Some("high"));
        assert_eq!(req.metadata.extra.get("custom_vendor_flag"), Some(&json!(7)));
    }

    #[test]
    fn consecutive_tool_results_share_a_turn() {
        let req = decode(json!({
            "model": "m",
            "messages": [
                {"role": "user", "content": "hi"},
                {"role": "assistant", "tool_calls": [
                    {"id": "a", "type": "function", "function": {"name": "f", "arguments": "{}"}},
                    {"id": "b", "type": "function", "function": {"name": "f", "arguments": "{}"}}
                ]},
                {"role": "tool", "tool_call_id": "a", "content": "1"},
                {"role": "tool", "tool_call_id": "b", "content": "2"}
            ]
        }));
        assert_eq!(req.messages.len(), 3);
        assert_eq!(req.messages[2].content.len(), 2);
    }

    #[test]
    fn rejects_unsupported_inputs_explicitly() {
        let err = decode_request(json!({"model": "m", "messages": [], "n": 2}), "r").err().unwrap();
        assert_eq!(err.kind, owo_core::ErrorKind::UnsupportedCapability);
        let err = decode_request(
            json!({"model": "m", "messages": [{"role": "user", "content": [{"type": "input_audio"}]}]}),
            "r",
        )
        .err()
        .unwrap();
        assert_eq!(err.kind, owo_core::ErrorKind::UnsupportedCapability);
        assert!(decode_request(json!({"messages": []}), "r").is_err());
    }

    fn frames(bytes: Vec<Bytes>) -> Vec<Value> {
        bytes
            .iter()
            .map(|b| {
                let s = std::str::from_utf8(b).unwrap();
                let data = s.trim_start_matches("data: ").trim_end();
                if data == "[DONE]" { json!("[DONE]") } else { serde_json::from_str(data).unwrap() }
            })
            .collect()
    }

    #[test]
    fn stream_encoding() {
        let mut enc = ChatStreamEncoder::new("alias", true);
        let events = vec![
            ModelEvent::ResponseStart { id: "x".into(), model: "up".into() },
            ModelEvent::MessageStart,
            ModelEvent::ReasoningStart,
            ModelEvent::ReasoningDelta { text: "r".into() },
            ModelEvent::ReasoningEnd { signature: None, encrypted_content: None },
            ModelEvent::TextStart,
            ModelEvent::TextDelta { text: "hi".into() },
            ModelEvent::TextEnd,
            ModelEvent::ToolCallStart { id: "c1".into(), name: "f".into(), kind: ToolCallKind::Function },
            ModelEvent::ToolCallDelta { id: "c1".into(), arguments_delta: "{}".into() },
            ModelEvent::ToolCallEnd { id: "c1".into() },
            ModelEvent::Usage(Usage { input_tokens: 3, output_tokens: 2, ..Default::default() }),
            ModelEvent::MessageEnd { stop_reason: StopReason::ToolUse },
            ModelEvent::ResponseEnd,
        ];
        let out = frames(events.into_iter().flat_map(|e| enc.push(e)).collect());
        assert_eq!(out[0]["choices"][0]["delta"]["role"], "assistant");
        assert_eq!(out[0]["model"], "alias");
        assert_eq!(out[1]["choices"][0]["delta"]["reasoning_content"], "r");
        assert_eq!(out[2]["choices"][0]["delta"]["content"], "hi");
        assert_eq!(out[3]["choices"][0]["delta"]["tool_calls"][0]["function"]["name"], "f");
        assert_eq!(out[4]["choices"][0]["delta"]["tool_calls"][0]["function"]["arguments"], "{}");
        assert_eq!(out[5]["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(out[6]["usage"]["total_tokens"], 5);
        assert_eq!(out[7], "[DONE]");
    }

    #[test]
    fn stream_error_terminates() {
        let mut enc = ChatStreamEncoder::new("m", false);
        let out = frames(enc.push(ModelEvent::Error(ModelError::new(owo_core::ErrorKind::RateLimited, "slow"))));
        assert_eq!(out[0]["error"]["code"], "rate_limit_exceeded");
        assert_eq!(out[1], "[DONE]");
        assert!(enc.push(ModelEvent::ResponseEnd).is_empty());
    }

    #[test]
    fn non_stream_response() {
        let resp = ModelResponse {
            id: "x".into(),
            model: "up".into(),
            content: vec![ContentBlock::ToolCall(ToolCall {
                id: "c".into(),
                name: "f".into(),
                arguments: "{}".into(),
                kind: ToolCallKind::Function,
            })],
            stop_reason: StopReason::ToolUse,
            usage: None,
        };
        let v = encode_response(&resp, "alias");
        assert_eq!(v["choices"][0]["message"]["content"], Value::Null);
        assert_eq!(v["choices"][0]["message"]["tool_calls"][0]["id"], "c");
        assert_eq!(v["choices"][0]["finish_reason"], "tool_calls");
    }
}
