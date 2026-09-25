//! Canonical events → Responses API events / response object.

use bytes::Bytes;
use serde_json::{json, Map, Value};
use owo_core::{ModelError, ModelEvent, StopReason, ToolCallKind, Usage};
use owo_protocol_openai_chat::common::{error_code, new_id, unix_now};

use crate::decode::ToolNamespaces;

#[derive(Debug, Clone)]
pub struct EncoderOptions {
    /// Present model reasoning as a reasoning *summary* (shown by Codex by default)
    /// rather than raw reasoning text (shown only with `show_raw_agent_reasoning`).
    pub reasoning_as_summary: bool,
    /// Echo `encrypted_content` on reasoning items (request `include` asked for it).
    pub include_encrypted_reasoning: bool,
    /// Tools the request declared inside `namespace` groups (from the decoder).
    pub tool_namespaces: ToolNamespaces,
}

impl Default for EncoderOptions {
    fn default() -> Self {
        Self { reasoning_as_summary: true, include_encrypted_reasoning: false, tool_namespaces: ToolNamespaces::new() }
    }
}

/// Adds `name` (and `namespace`, for namespaced tools) to a tool-call item.
fn set_tool_identity(item: &mut Value, flat: &str, namespaces: &ToolNamespaces) {
    match namespaces.get(flat) {
        Some((ns, name)) => {
            item["name"] = json!(name);
            item["namespace"] = json!(ns);
        }
        None => item["name"] = json!(flat),
    }
}

enum Item {
    Message { id: String, text: String },
    Reasoning { id: String, text: String, part_open: bool },
    Tool { id: String, call_id: String, name: String, args: String, kind: ToolCallKind },
}

/// Stateful encoder. Feed every event through [`ResponsesEncoder::push`]; for streaming
/// forward the returned frames, for non-streaming call [`ResponsesEncoder::finish`].
pub struct ResponsesEncoder {
    id: String,
    model: String,
    created_at: u64,
    seq: u64,
    opts: EncoderOptions,
    output: Vec<Value>,
    current: Option<Item>,
    usage: Option<Usage>,
    stop: Option<StopReason>,
    error: Option<ModelError>,
    done: bool,
}

impl ResponsesEncoder {
    pub fn new(model: impl Into<String>, opts: EncoderOptions) -> Self {
        Self {
            id: new_id("resp_"),
            model: model.into(),
            created_at: unix_now(),
            seq: 0,
            opts,
            output: Vec::new(),
            current: None,
            usage: None,
            stop: None,
            error: None,
            done: false,
        }
    }

    fn output_index(&self) -> usize {
        self.output.len()
    }

    fn emit(&mut self, out: &mut Vec<Bytes>, kind: &str, mut fields: Map<String, Value>) {
        fields.insert("type".into(), json!(kind));
        fields.insert("sequence_number".into(), json!(self.seq));
        self.seq += 1;
        out.push(owo_sse::encode(Some(kind), &Value::Object(fields).to_string()));
    }

    fn response(&self, status: &str) -> Value {
        let mut r = json!({
            "id": self.id,
            "object": "response",
            "created_at": self.created_at,
            "status": status,
            "model": self.model,
            "output": self.output,
            "error": Value::Null,
            "incomplete_details": Value::Null,
        });
        if status != "in_progress" {
            r["usage"] = usage_json(&self.usage.unwrap_or_default());
        }
        r
    }

    pub fn push(&mut self, event: ModelEvent) -> Vec<Bytes> {
        let mut out = Vec::new();
        if self.done {
            return out;
        }
        match event {
            ModelEvent::ResponseStart { .. } => {
                let r = self.response("in_progress");
                self.emit(&mut out, "response.created", obj([("response", r.clone())]));
                self.emit(&mut out, "response.in_progress", obj([("response", r)]));
            }
            ModelEvent::MessageStart => {}
            ModelEvent::TextStart => {
                self.close_current(&mut out);
                self.open(&mut out, Item::Message { id: new_id("msg_"), text: String::new() });
            }
            ModelEvent::TextDelta { text } => {
                if !matches!(self.current, Some(Item::Message { .. })) {
                    self.close_current(&mut out);
                    self.open(&mut out, Item::Message { id: new_id("msg_"), text: String::new() });
                }
                let index = self.output_index();
                if let Some(Item::Message { id, text: buf }) = &mut self.current {
                    buf.push_str(&text);
                    let id = id.clone();
                    self.emit(
                        &mut out,
                        "response.output_text.delta",
                        obj([("item_id", json!(id)), ("output_index", json!(index)), ("content_index", json!(0)), ("delta", json!(text))]),
                    );
                }
            }
            ModelEvent::ReasoningStart => {
                self.close_current(&mut out);
                self.open(&mut out, Item::Reasoning { id: new_id("rs_"), text: String::new(), part_open: false });
            }
            ModelEvent::ReasoningDelta { text } => {
                if !matches!(self.current, Some(Item::Reasoning { .. })) {
                    self.close_current(&mut out);
                    self.open(&mut out, Item::Reasoning { id: new_id("rs_"), text: String::new(), part_open: false });
                }
                self.reasoning_delta(&mut out, text);
            }
            ModelEvent::ReasoningEnd { signature, encrypted_content } => {
                let blob = signature.map(|s| format!("{}{s}", crate::SIGNATURE_PREFIX)).or(encrypted_content);
                self.close_reasoning(&mut out, blob)
            }
            ModelEvent::ToolCallStart { id, name, kind } => {
                self.close_current(&mut out);
                let item_id = new_id(if kind == ToolCallKind::Custom { "ctc_" } else { "fc_" });
                self.open(&mut out, Item::Tool { id: item_id, call_id: id, name, args: String::new(), kind });
            }
            ModelEvent::ToolCallDelta { id: call, arguments_delta } => {
                let index = self.output_index();
                if let Some(Item::Tool { id, call_id, args, kind, .. }) = &mut self.current {
                    if *call_id == call {
                        args.push_str(&arguments_delta);
                        let (id, kind) = (id.clone(), *kind);
                        let event = match kind {
                            ToolCallKind::Function => "response.function_call_arguments.delta",
                            ToolCallKind::Custom => "response.custom_tool_call_input.delta",
                        };
                        self.emit(&mut out, event, obj([("item_id", json!(id)), ("output_index", json!(index)), ("delta", json!(arguments_delta))]));
                    }
                }
            }
            ModelEvent::TextEnd | ModelEvent::ToolCallEnd { .. } => self.close_current(&mut out),
            ModelEvent::Usage(u) => match &mut self.usage {
                Some(existing) => existing.merge(&u),
                None => self.usage = Some(u),
            },
            ModelEvent::MessageEnd { stop_reason } => self.stop = Some(stop_reason),
            ModelEvent::ResponseEnd => {
                self.close_current(&mut out);
                self.done = true;
                let (status, reason) = match &self.stop {
                    Some(StopReason::MaxTokens) => ("incomplete", Some("max_output_tokens")),
                    Some(StopReason::ContentFilter) => ("incomplete", Some("content_filter")),
                    _ => ("completed", None),
                };
                let mut r = self.response(status);
                let kind = if let Some(reason) = reason {
                    r["incomplete_details"] = json!({ "reason": reason });
                    "response.incomplete"
                } else {
                    "response.completed"
                };
                self.emit(&mut out, kind, obj([("response", r)]));
            }
            ModelEvent::Error(err) => {
                self.done = true;
                let mut r = self.response("failed");
                r["error"] = json!({ "code": error_code(err.kind), "message": err.message });
                self.error = Some(err);
                self.emit(&mut out, "response.failed", obj([("response", r)]));
            }
        }
        out
    }

    /// The final response object for a non-streaming call.
    pub fn finish(mut self) -> Result<Value, ModelError> {
        if !self.done {
            self.push(ModelEvent::Error(ModelError::upstream_invalid("response ended without completion")));
        }
        if let Some(err) = self.error.take() {
            return Err(err);
        }
        let (status, reason) = match &self.stop {
            Some(StopReason::MaxTokens) => ("incomplete", Some("max_output_tokens")),
            Some(StopReason::ContentFilter) => ("incomplete", Some("content_filter")),
            _ => ("completed", None),
        };
        let mut r = self.response(status);
        if let Some(reason) = reason {
            r["incomplete_details"] = json!({ "reason": reason });
        }
        Ok(r)
    }

    fn open(&mut self, out: &mut Vec<Bytes>, item: Item) {
        let index = self.output_index();
        let (added, content_part) = match &item {
            Item::Message { id, .. } => (
                json!({ "id": id, "type": "message", "status": "in_progress", "role": "assistant", "content": [] }),
                Some((id.clone(), json!({ "type": "output_text", "text": "", "annotations": [] }))),
            ),
            Item::Reasoning { id, .. } => (json!({ "id": id, "type": "reasoning", "summary": [] }), None),
            Item::Tool { id, call_id, name, kind, .. } => {
                let mut v = match kind {
                    ToolCallKind::Function => json!({
                        "id": id, "type": "function_call", "status": "in_progress",
                        "call_id": call_id, "arguments": "",
                    }),
                    ToolCallKind::Custom => json!({
                        "id": id, "type": "custom_tool_call", "status": "in_progress",
                        "call_id": call_id, "input": "",
                    }),
                };
                set_tool_identity(&mut v, name, &self.opts.tool_namespaces);
                (v, None)
            }
        };
        self.emit(out, "response.output_item.added", obj([("output_index", json!(index)), ("item", added)]));
        if let Some((id, part)) = content_part {
            self.emit(
                out,
                "response.content_part.added",
                obj([("item_id", json!(id)), ("output_index", json!(index)), ("content_index", json!(0)), ("part", part)]),
            );
        }
        self.current = Some(item);
    }

    fn reasoning_delta(&mut self, out: &mut Vec<Bytes>, delta: String) {
        let index = self.output_index();
        let summary = self.opts.reasoning_as_summary;
        let Some(Item::Reasoning { id, text, part_open }) = &mut self.current else { return };
        text.push_str(&delta);
        let id = id.clone();
        let first = !*part_open;
        *part_open = true;
        if summary {
            if first {
                self.emit(
                    out,
                    "response.reasoning_summary_part.added",
                    obj([
                        ("item_id", json!(id)),
                        ("output_index", json!(index)),
                        ("summary_index", json!(0)),
                        ("part", json!({ "type": "summary_text", "text": "" })),
                    ]),
                );
            }
            self.emit(
                out,
                "response.reasoning_summary_text.delta",
                obj([("item_id", json!(id)), ("output_index", json!(index)), ("summary_index", json!(0)), ("delta", json!(delta))]),
            );
        } else {
            self.emit(
                out,
                "response.reasoning_text.delta",
                obj([("item_id", json!(id)), ("output_index", json!(index)), ("content_index", json!(0)), ("delta", json!(delta))]),
            );
        }
    }

    fn close_reasoning(&mut self, out: &mut Vec<Bytes>, encrypted: Option<String>) {
        if !matches!(self.current, Some(Item::Reasoning { .. })) {
            return;
        }
        let Some(Item::Reasoning { id, text, part_open }) = self.current.take() else { return };
        let index = self.output_index();
        let mut item = json!({ "id": id, "type": "reasoning", "summary": [] });
        if self.opts.reasoning_as_summary {
            if part_open {
                let part = json!({ "type": "summary_text", "text": text });
                self.emit(
                    out,
                    "response.reasoning_summary_text.done",
                    obj([("item_id", json!(id)), ("output_index", json!(index)), ("summary_index", json!(0)), ("text", json!(text))]),
                );
                self.emit(
                    out,
                    "response.reasoning_summary_part.done",
                    obj([("item_id", json!(id)), ("output_index", json!(index)), ("summary_index", json!(0)), ("part", part.clone())]),
                );
                item["summary"] = json!([part]);
            }
        } else if part_open {
            self.emit(
                out,
                "response.reasoning_text.done",
                obj([("item_id", json!(id)), ("output_index", json!(index)), ("content_index", json!(0)), ("text", json!(text))]),
            );
            item["content"] = json!([{ "type": "reasoning_text", "text": text }]);
        }
        if self.opts.include_encrypted_reasoning {
            if let Some(enc) = encrypted {
                item["encrypted_content"] = json!(enc);
            }
        }
        self.emit(out, "response.output_item.done", obj([("output_index", json!(index)), ("item", item.clone())]));
        self.output.push(item);
    }

    fn close_current(&mut self, out: &mut Vec<Bytes>) {
        let index = self.output_index();
        let item = match self.current.take() {
            None => return,
            Some(r @ Item::Reasoning { .. }) => {
                self.current = Some(r);
                self.close_reasoning(out, None);
                return;
            }
            Some(Item::Message { id, text }) => {
                let part = json!({ "type": "output_text", "text": text, "annotations": [] });
                self.emit(
                    out,
                    "response.output_text.done",
                    obj([("item_id", json!(id)), ("output_index", json!(index)), ("content_index", json!(0)), ("text", json!(text))]),
                );
                self.emit(
                    out,
                    "response.content_part.done",
                    obj([("item_id", json!(id)), ("output_index", json!(index)), ("content_index", json!(0)), ("part", part.clone())]),
                );
                json!({ "id": id, "type": "message", "status": "completed", "role": "assistant", "content": [part] })
            }
            Some(Item::Tool { id, call_id, name, args, kind }) => {
                let mut v = match kind {
                    ToolCallKind::Function => {
                        self.emit(
                            out,
                            "response.function_call_arguments.done",
                            obj([("item_id", json!(id)), ("output_index", json!(index)), ("arguments", json!(args))]),
                        );
                        json!({ "id": id, "type": "function_call", "status": "completed", "call_id": call_id, "arguments": args })
                    }
                    ToolCallKind::Custom => {
                        self.emit(
                            out,
                            "response.custom_tool_call_input.done",
                            obj([("item_id", json!(id)), ("output_index", json!(index)), ("input", json!(args))]),
                        );
                        json!({ "id": id, "type": "custom_tool_call", "status": "completed", "call_id": call_id, "input": args })
                    }
                };
                set_tool_identity(&mut v, &name, &self.opts.tool_namespaces);
                v
            }
        };
        self.emit(out, "response.output_item.done", obj([("output_index", json!(index)), ("item", item.clone())]));
        self.output.push(item);
    }
}

fn obj<const N: usize>(pairs: [(&str, Value); N]) -> Map<String, Value> {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn usage_json(u: &Usage) -> Value {
    json!({
        "input_tokens": u.input_tokens,
        "input_tokens_details": { "cached_tokens": u.cached_input_tokens.unwrap_or(0) },
        "output_tokens": u.output_tokens,
        "output_tokens_details": { "reasoning_tokens": u.reasoning_tokens.unwrap_or(0) },
        "total_tokens": u.total_tokens(),
    })
}

/// `{"error": {...}}` body for an HTTP error response.
pub fn error_body(err: &ModelError) -> Value {
    owo_protocol_openai_chat::common::error_body(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use owo_core::{ContentBlock, ErrorKind, ModelResponse, ReasoningBlock, ToolCall};

    fn parse(frames: &[Bytes]) -> Vec<(String, Value)> {
        let mut dec = owo_sse::SseDecoder::new();
        let mut out = Vec::new();
        for f in frames {
            for ev in dec.feed(f).unwrap() {
                out.push((ev.event.unwrap(), serde_json::from_str(&ev.data).unwrap()));
            }
        }
        out
    }

    fn sample() -> ModelResponse {
        ModelResponse {
            id: "up".into(),
            model: "up".into(),
            content: vec![
                ContentBlock::Reasoning(ReasoningBlock { text: "plan".into(), ..Default::default() }),
                ContentBlock::text("Hi"),
                ContentBlock::ToolCall(ToolCall { id: "call_1".into(), name: "shell".into(), arguments: "{}".into(), kind: ToolCallKind::Function }),
                ContentBlock::ToolCall(ToolCall { id: "call_2".into(), name: "apply_patch".into(), arguments: "P".into(), kind: ToolCallKind::Custom }),
            ],
            stop_reason: StopReason::ToolUse,
            usage: Some(Usage { input_tokens: 10, output_tokens: 5, cached_input_tokens: Some(4), ..Default::default() }),
        }
    }

    #[test]
    fn stream_event_sequence() {
        let mut enc = ResponsesEncoder::new("gpt-ds", EncoderOptions::default());
        let frames: Vec<Bytes> = sample().into_events().into_iter().flat_map(|e| enc.push(e)).collect();
        let events = parse(&frames);
        let kinds: Vec<&str> = events.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(
            kinds,
            vec![
                "response.created",
                "response.in_progress",
                "response.output_item.added",
                "response.reasoning_summary_part.added",
                "response.reasoning_summary_text.delta",
                "response.reasoning_summary_text.done",
                "response.reasoning_summary_part.done",
                "response.output_item.done",
                "response.output_item.added",
                "response.content_part.added",
                "response.output_text.delta",
                "response.output_text.done",
                "response.content_part.done",
                "response.output_item.done",
                "response.output_item.added",
                "response.function_call_arguments.delta",
                "response.function_call_arguments.done",
                "response.output_item.done",
                "response.output_item.added",
                "response.custom_tool_call_input.delta",
                "response.custom_tool_call_input.done",
                "response.output_item.done",
                "response.completed",
            ]
        );
        for (i, (_, v)) in events.iter().enumerate() {
            assert_eq!(v["sequence_number"], i as u64);
        }
        let (_, completed) = events.last().unwrap();
        let r = &completed["response"];
        assert_eq!(r["status"], "completed");
        assert_eq!(r["model"], "gpt-ds");
        assert_eq!(r["output"].as_array().unwrap().len(), 4);
        assert_eq!(r["output"][2]["call_id"], "call_1");
        assert_eq!(r["output"][3]["type"], "custom_tool_call");
        assert_eq!(r["output"][3]["input"], "P");
        assert_eq!(r["usage"]["input_tokens_details"]["cached_tokens"], 4);
        assert_eq!(r["usage"]["total_tokens"], 15);
        // output_index increments per item.
        assert_eq!(events[14].1["output_index"], 2);
    }

    #[test]
    fn raw_reasoning_mode() {
        let mut enc = ResponsesEncoder::new(
            "m",
            EncoderOptions { reasoning_as_summary: false, include_encrypted_reasoning: true, ..Default::default() },
        );
        let events = vec![
            ModelEvent::ResponseStart { id: "x".into(), model: "m".into() },
            ModelEvent::ReasoningStart,
            ModelEvent::ReasoningDelta { text: "t".into() },
            ModelEvent::ReasoningEnd { signature: None, encrypted_content: Some("enc".into()) },
            ModelEvent::MessageEnd { stop_reason: StopReason::EndTurn },
            ModelEvent::ResponseEnd,
        ];
        let frames: Vec<Bytes> = events.into_iter().flat_map(|e| enc.push(e)).collect();
        let events = parse(&frames);
        assert!(events.iter().any(|(k, _)| k == "response.reasoning_text.delta"));
        let item = &events.last().unwrap().1["response"]["output"][0];
        assert_eq!(item["content"][0]["text"], "t");
        assert_eq!(item["encrypted_content"], "enc");
    }

    #[test]
    fn provider_signature_round_trips_through_the_client() {
        let mut enc = ResponsesEncoder::new("m", EncoderOptions { include_encrypted_reasoning: true, ..Default::default() });
        let resp = ModelResponse {
            id: "x".into(),
            model: "m".into(),
            content: vec![ContentBlock::Reasoning(ReasoningBlock { text: "plan".into(), signature: Some("EqQB".into()), ..Default::default() })],
            stop_reason: StopReason::EndTurn,
            usage: None,
        };
        for e in resp.into_events() {
            enc.push(e);
        }
        let item = enc.finish().unwrap()["output"][0].clone();
        assert_eq!(item["encrypted_content"], format!("{}EqQB", crate::SIGNATURE_PREFIX));

        // The client replays the item verbatim on the next turn.
        let replay = json!({"model": "m", "input": [{"role": "user", "content": "q"}, item]});
        let req = crate::decode::decode_request(replay, "r").unwrap().request;
        assert!(matches!(&req.messages[1].content[0], ContentBlock::Reasoning(r)
            if r.signature.as_deref() == Some("EqQB") && r.encrypted_content.is_none() && r.text == "plan"));
    }

    #[test]
    fn namespaced_tool_calls_get_their_identity_back() {
        let mut namespaces = ToolNamespaces::new();
        namespaces.insert("multi_agent_v1__spawn_agent".into(), ("multi_agent_v1".into(), "spawn_agent".into()));
        let mut enc = ResponsesEncoder::new("m", EncoderOptions { tool_namespaces: namespaces, ..Default::default() });
        let resp = ModelResponse {
            id: "x".into(),
            model: "m".into(),
            content: vec![ContentBlock::ToolCall(ToolCall {
                id: "call_1".into(),
                name: "multi_agent_v1__spawn_agent".into(),
                arguments: "{}".into(),
                kind: ToolCallKind::Function,
            })],
            stop_reason: StopReason::ToolUse,
            usage: None,
        };
        let frames: Vec<Bytes> = resp.into_events().into_iter().flat_map(|e| enc.push(e)).collect();
        let events = parse(&frames);
        let added = &events.iter().find(|(k, _)| k == "response.output_item.added").unwrap().1["item"];
        assert_eq!((added["name"].as_str(), added["namespace"].as_str()), (Some("spawn_agent"), Some("multi_agent_v1")));
        let done = &events.last().unwrap().1["response"]["output"][0];
        assert_eq!((done["name"].as_str(), done["namespace"].as_str()), (Some("spawn_agent"), Some("multi_agent_v1")));
    }

    #[test]
    fn failure_and_incomplete() {
        let mut enc = ResponsesEncoder::new("m", EncoderOptions::default());
        enc.push(ModelEvent::ResponseStart { id: "x".into(), model: "m".into() });
        let frames = enc.push(ModelEvent::Error(ModelError::new(ErrorKind::ContextExceeded, "too long")));
        let (kind, v) = &parse(&frames)[0];
        assert_eq!(kind, "response.failed");
        assert_eq!(v["response"]["error"]["code"], "context_length_exceeded");
        assert!(enc.push(ModelEvent::ResponseEnd).is_empty());

        let mut enc = ResponsesEncoder::new("m", EncoderOptions::default());
        let mut resp = sample();
        resp.stop_reason = StopReason::MaxTokens;
        let frames: Vec<Bytes> = resp.into_events().into_iter().flat_map(|e| enc.push(e)).collect();
        let (kind, v) = parse(&frames).pop().unwrap();
        assert_eq!(kind, "response.incomplete");
        assert_eq!(v["response"]["incomplete_details"]["reason"], "max_output_tokens");
    }

    #[test]
    fn non_stream_object() {
        let mut enc = ResponsesEncoder::new("m", EncoderOptions::default());
        for e in sample().into_events() {
            enc.push(e);
        }
        let r = enc.finish().unwrap();
        assert_eq!(r["object"], "response");
        assert_eq!(r["output"][1]["content"][0]["text"], "Hi");

        let mut enc = ResponsesEncoder::new("m", EncoderOptions::default());
        enc.push(ModelEvent::Error(ModelError::new(ErrorKind::RateLimited, "x")));
        assert_eq!(enc.finish().unwrap_err().kind, ErrorKind::RateLimited);
    }
}
