//! Anthropic Messages SSE → canonical events.

use std::collections::HashSet;

use serde_json::Value;
use owo_core::{ErrorKind, ModelError, ModelEvent, StopReason, ToolCallKind, Usage};

use crate::tool_names::ToolNames;

enum Open {
    Text,
    Thinking { signature: Option<String> },
    Redacted { data: String },
    Tool { id: String, custom: bool, buf: String, streamed: bool },
    /// Server-side blocks (web search results, ...) with no canonical counterpart.
    Ignored,
}

pub struct AnthropicStreamDecoder {
    fallback_model: String,
    custom_tools: HashSet<String>,
    names: ToolNames,
    started: bool,
    open: Option<Open>,
    usage: Usage,
    stop: Option<StopReason>,
    ended: bool,
}

fn str_at<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn u64_at(v: &Value, key: &str) -> Option<u64> {
    v.get(key).and_then(Value::as_u64)
}

pub fn stop_reason(reason: &str) -> StopReason {
    match reason {
        "end_turn" => StopReason::EndTurn,
        "max_tokens" | "model_context_window_exceeded" => StopReason::MaxTokens,
        "tool_use" => StopReason::ToolUse,
        "stop_sequence" => StopReason::StopSequence,
        "refusal" => StopReason::Refusal,
        other => StopReason::Other(other.to_string()),
    }
}

fn error_kind(kind: &str) -> ErrorKind {
    match kind {
        "rate_limit_error" => ErrorKind::RateLimited,
        "authentication_error" => ErrorKind::AuthenticationFailed,
        "permission_error" => ErrorKind::AuthorizationFailed,
        "not_found_error" => ErrorKind::ModelNotFound,
        "invalid_request_error" => ErrorKind::InvalidRequest,
        "request_too_large" => ErrorKind::ContextExceeded,
        "timeout_error" => ErrorKind::Timeout,
        _ => ErrorKind::ProviderUnavailable,
    }
}

impl AnthropicStreamDecoder {
    pub fn new(fallback_model: impl Into<String>, custom_tools: HashSet<String>, names: ToolNames) -> Self {
        Self {
            fallback_model: fallback_model.into(),
            custom_tools,
            names,
            started: false,
            open: None,
            usage: Usage::default(),
            stop: None,
            ended: false,
        }
    }

    pub fn is_ended(&self) -> bool {
        self.ended
    }

    fn fail(&mut self, err: ModelError) -> Vec<ModelEvent> {
        self.ended = true;
        vec![ModelEvent::Error(err)]
    }

    fn absorb_usage(&mut self, u: &Value) {
        let read = u64_at(u, "cache_read_input_tokens").unwrap_or(0);
        let created = u64_at(u, "cache_creation_input_tokens").unwrap_or(0);
        if let Some(input) = u64_at(u, "input_tokens") {
            // Anthropic's `input_tokens` excludes cached tokens; canonical input includes them.
            self.usage.input_tokens = input + read + created;
            if read > 0 {
                self.usage.cached_input_tokens = Some(read);
            }
            if created > 0 {
                self.usage.cache_creation_input_tokens = Some(created);
            }
        }
        if let Some(out) = u64_at(u, "output_tokens") {
            self.usage.output_tokens = out;
        }
    }

    /// One SSE event. The `type` inside `data` is authoritative; `event:` is only a hint.
    pub fn push(&mut self, data: &str) -> Vec<ModelEvent> {
        if self.ended || data.trim().is_empty() {
            return Vec::new();
        }
        let v: Value = match serde_json::from_str(data) {
            Ok(v) => v,
            Err(_) => return self.fail(ModelError::upstream_invalid("upstream sent a non-JSON stream event")),
        };
        let mut out = Vec::new();
        match str_at(&v, "type").unwrap_or_default() {
            "message_start" => {
                let msg = v.get("message").cloned().unwrap_or(Value::Null);
                if let Some(u) = msg.get("usage") {
                    self.absorb_usage(u);
                }
                self.start(&msg, &mut out);
            }
            "content_block_start" => {
                self.start(&Value::Null, &mut out);
                self.close(&mut out);
                let block = v.get("content_block").cloned().unwrap_or(Value::Null);
                match str_at(&block, "type").unwrap_or_default() {
                    "text" => {
                        out.push(ModelEvent::TextStart);
                        if let Some(t) = str_at(&block, "text").filter(|t| !t.is_empty()) {
                            out.push(ModelEvent::TextDelta { text: t.to_string() });
                        }
                        self.open = Some(Open::Text);
                    }
                    "thinking" => {
                        out.push(ModelEvent::ReasoningStart);
                        if let Some(t) = str_at(&block, "thinking").filter(|t| !t.is_empty()) {
                            out.push(ModelEvent::ReasoningDelta { text: t.to_string() });
                        }
                        let signature = str_at(&block, "signature").filter(|s| !s.is_empty()).map(str::to_string);
                        self.open = Some(Open::Thinking { signature });
                    }
                    "redacted_thinking" => {
                        out.push(ModelEvent::ReasoningStart);
                        self.open = Some(Open::Redacted { data: str_at(&block, "data").unwrap_or_default().to_string() });
                    }
                    "tool_use" => {
                        let id = str_at(&block, "id").unwrap_or_default().to_string();
                        let name = self.names.original(str_at(&block, "name").unwrap_or_default());
                        let custom = self.custom_tools.contains(&name);
                        let kind = if custom { ToolCallKind::Custom } else { ToolCallKind::Function };
                        out.push(ModelEvent::ToolCallStart { id: id.clone(), name, kind });
                        // Non-streamed servers put the whole input on the start block.
                        let buf = match block.get("input") {
                            Some(Value::Object(m)) if !m.is_empty() => Value::Object(m.clone()).to_string(),
                            _ => String::new(),
                        };
                        self.open = Some(Open::Tool { id, custom, buf, streamed: false });
                    }
                    _ => self.open = Some(Open::Ignored),
                }
            }
            "content_block_delta" => {
                let delta = v.get("delta").cloned().unwrap_or(Value::Null);
                match (str_at(&delta, "type").unwrap_or_default(), self.open.as_mut()) {
                    ("text_delta", Some(Open::Text)) => {
                        if let Some(t) = str_at(&delta, "text").filter(|t| !t.is_empty()) {
                            out.push(ModelEvent::TextDelta { text: t.to_string() });
                        }
                    }
                    ("thinking_delta", Some(Open::Thinking { .. })) => {
                        if let Some(t) = str_at(&delta, "thinking").filter(|t| !t.is_empty()) {
                            out.push(ModelEvent::ReasoningDelta { text: t.to_string() });
                        }
                    }
                    ("signature_delta", Some(Open::Thinking { signature })) => {
                        let piece = str_at(&delta, "signature").unwrap_or_default();
                        signature.get_or_insert_with(String::new).push_str(piece);
                    }
                    ("input_json_delta", Some(Open::Tool { id, custom, buf, streamed })) => {
                        let piece = str_at(&delta, "partial_json").unwrap_or_default();
                        if *custom || !buf.is_empty() {
                            buf.push_str(piece);
                        } else if !piece.is_empty() {
                            *streamed = true;
                            out.push(ModelEvent::ToolCallDelta { id: id.clone(), arguments_delta: piece.to_string() });
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => self.close(&mut out),
            "message_delta" => {
                if let Some(reason) = v.get("delta").and_then(|d| str_at(d, "stop_reason")) {
                    self.stop = Some(stop_reason(reason));
                }
                if let Some(u) = v.get("usage") {
                    self.absorb_usage(u);
                }
            }
            "message_stop" => out.extend(self.finish()),
            "error" => {
                let err = v.get("error").cloned().unwrap_or(Value::Null);
                let kind = error_kind(str_at(&err, "type").unwrap_or_default());
                let message = str_at(&err, "message").unwrap_or("upstream error").to_string();
                out.extend(self.fail(ModelError::new(kind, message)));
            }
            _ => {} // ping and future event types
        }
        out
    }

    fn start(&mut self, msg: &Value, out: &mut Vec<ModelEvent>) {
        if self.started {
            return;
        }
        self.started = true;
        out.push(ModelEvent::ResponseStart {
            id: str_at(msg, "id").map(str::to_string).unwrap_or_else(|| format!("msg_{}", uuid::Uuid::new_v4().simple())),
            model: str_at(msg, "model").unwrap_or(&self.fallback_model).to_string(),
        });
        out.push(ModelEvent::MessageStart);
    }

    fn close(&mut self, out: &mut Vec<ModelEvent>) {
        match self.open.take() {
            None | Some(Open::Ignored) => {}
            Some(Open::Text) => out.push(ModelEvent::TextEnd),
            Some(Open::Thinking { signature }) => out.push(ModelEvent::ReasoningEnd { signature, encrypted_content: None }),
            Some(Open::Redacted { data }) => {
                out.push(ModelEvent::ReasoningEnd { signature: None, encrypted_content: Some(data) });
            }
            Some(Open::Tool { id, custom, buf, streamed }) => {
                let args = if custom {
                    serde_json::from_str::<Value>(&buf)
                        .ok()
                        .and_then(|v| v.get("input").and_then(Value::as_str).map(str::to_string))
                        .unwrap_or(buf)
                } else if !buf.is_empty() {
                    buf
                } else if !streamed {
                    "{}".to_string()
                } else {
                    String::new()
                };
                if !args.is_empty() {
                    out.push(ModelEvent::ToolCallDelta { id: id.clone(), arguments_delta: args });
                }
                out.push(ModelEvent::ToolCallEnd { id });
            }
        }
    }

    /// Ends the response (on `message_stop`, or at EOF).
    pub fn finish(&mut self) -> Vec<ModelEvent> {
        if self.ended {
            return Vec::new();
        }
        if !self.started {
            return self.fail(ModelError::upstream_invalid("upstream stream ended without any data"));
        }
        let Some(stop) = self.stop.take() else {
            return self.fail(ModelError::upstream_invalid("upstream stream ended before completion"));
        };
        let mut out = Vec::new();
        self.close(&mut out);
        self.ended = true;
        out.push(ModelEvent::Usage(self.usage));
        out.push(ModelEvent::MessageEnd { stop_reason: stop });
        out.push(ModelEvent::ResponseEnd);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use owo_core::{ContentBlock, ResponseAccumulator};

    fn run(events: &[Value], custom: &[&str]) -> Vec<ModelEvent> {
        let mut d = AnthropicStreamDecoder::new("m", custom.iter().map(|s| s.to_string()).collect(), ToolNames::default());
        let mut out = Vec::new();
        for e in events {
            out.extend(d.push(&e.to_string()));
        }
        out.extend(d.finish());
        out
    }

    fn collect(events: Vec<ModelEvent>) -> owo_core::ModelResponse {
        let mut acc = ResponseAccumulator::new();
        for e in events {
            acc.push(e);
        }
        acc.finish().unwrap()
    }

    #[test]
    fn thinking_text_and_tools() {
        let events = run(
            &[
                json!({"type": "message_start", "message": {"id": "msg_1", "model": "claude-sonnet-5",
                    "usage": {"input_tokens": 10, "cache_read_input_tokens": 90, "cache_creation_input_tokens": 5, "output_tokens": 1}}}),
                json!({"type": "content_block_start", "index": 0, "content_block": {"type": "thinking", "thinking": ""}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "Let me "}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "thinking_delta", "thinking": "look."}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "signature_delta", "signature": "EqQB"}}),
                json!({"type": "content_block_stop", "index": 0}),
                json!({"type": "ping"}),
                json!({"type": "content_block_start", "index": 1, "content_block": {"type": "text", "text": ""}}),
                json!({"type": "content_block_delta", "index": 1, "delta": {"type": "text_delta", "text": "Running it."}}),
                json!({"type": "content_block_stop", "index": 1}),
                json!({"type": "content_block_start", "index": 2, "content_block": {"type": "tool_use", "id": "toolu_1", "name": "exec_command", "input": {}}}),
                json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "{\"cmd\":"}}),
                json!({"type": "content_block_delta", "index": 2, "delta": {"type": "input_json_delta", "partial_json": "\"ls\"}"}}),
                json!({"type": "content_block_stop", "index": 2}),
                json!({"type": "content_block_start", "index": 3, "content_block": {"type": "tool_use", "id": "toolu_2", "name": "apply_patch", "input": {}}}),
                json!({"type": "content_block_delta", "index": 3, "delta": {"type": "input_json_delta", "partial_json": "{\"input\":\"*** Begin\"}"}}),
                json!({"type": "content_block_stop", "index": 3}),
                json!({"type": "content_block_start", "index": 4, "content_block": {"type": "tool_use", "id": "toolu_3", "name": "get_goal", "input": {}}}),
                json!({"type": "content_block_stop", "index": 4}),
                json!({"type": "message_delta", "delta": {"stop_reason": "tool_use"}, "usage": {"output_tokens": 42}}),
                json!({"type": "message_stop"}),
            ],
            &["apply_patch"],
        );
        let resp = collect(events);
        assert_eq!(resp.id, "msg_1");
        assert!(matches!(&resp.content[0], ContentBlock::Reasoning(r) if r.text == "Let me look." && r.signature.as_deref() == Some("EqQB")));
        assert_eq!(resp.content[1], ContentBlock::text("Running it."));
        assert!(matches!(&resp.content[2], ContentBlock::ToolCall(c) if c.arguments == "{\"cmd\":\"ls\"}" && c.kind == ToolCallKind::Function));
        assert!(matches!(&resp.content[3], ContentBlock::ToolCall(c) if c.arguments == "*** Begin" && c.kind == ToolCallKind::Custom));
        assert!(matches!(&resp.content[4], ContentBlock::ToolCall(c) if c.arguments == "{}"));
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
        let u = resp.usage.unwrap();
        assert_eq!((u.input_tokens, u.cached_input_tokens, u.output_tokens), (105, Some(90), 42));
    }

    #[test]
    fn redacted_thinking_is_carried_as_opaque_data() {
        let resp = collect(run(
            &[
                json!({"type": "message_start", "message": {"id": "m", "usage": {"input_tokens": 1}}}),
                json!({"type": "content_block_start", "index": 0, "content_block": {"type": "redacted_thinking", "data": "OPAQUE"}}),
                json!({"type": "content_block_stop", "index": 0}),
                json!({"type": "message_delta", "delta": {"stop_reason": "end_turn"}}),
                json!({"type": "message_stop"}),
            ],
            &[],
        ));
        assert!(matches!(&resp.content[0], ContentBlock::Reasoning(r) if r.encrypted_content.as_deref() == Some("OPAQUE") && r.text.is_empty()));
    }

    #[test]
    fn errors_and_truncation() {
        let events = run(
            &[
                json!({"type": "message_start", "message": {"id": "m"}}),
                json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}}),
            ],
            &[],
        );
        assert!(matches!(events.last(), Some(ModelEvent::Error(e)) if e.kind == ErrorKind::ProviderUnavailable));

        let events = run(
            &[
                json!({"type": "message_start", "message": {"id": "m"}}),
                json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
                json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "par"}}),
            ],
            &[],
        );
        assert!(matches!(events.last(), Some(ModelEvent::Error(e)) if e.kind == ErrorKind::UpstreamInvalidResponse));
    }

    #[test]
    fn aliased_tool_names_are_restored() {
        let mut names = ToolNames::default();
        let long = "mcp__a_really_long_server_name_for_testing_purposes__and_a_long_tool_name";
        let wire = names.wire(long);
        let mut d = AnthropicStreamDecoder::new("m", HashSet::new(), names);
        let mut out = d.push(&json!({"type": "message_start", "message": {"id": "m"}}).to_string());
        out.extend(d.push(&json!({"type": "content_block_start", "index": 0, "content_block": {"type": "tool_use", "id": "t", "name": wire}}).to_string()));
        assert!(out.iter().any(|e| matches!(e, ModelEvent::ToolCallStart { name, .. } if name == long)));
    }
}
