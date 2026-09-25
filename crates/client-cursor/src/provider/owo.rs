//! Bridges Cursor model invocations to OwO AI Gateway's shared router: the Cursor runtime's
//! provider-neutral request becomes an OwO AI Gateway canonical request, and OwO AI Gateway's canonical
//! events become the runtime's provider events. Providers, models, and credentials
//! are OwO AI Gateway's; nothing here is Cursor-specific configuration.
use std::{collections::HashMap, sync::Arc};

use async_stream::try_stream;
use base64::{engine::general_purpose::STANDARD, Engine};
use futures_util::StreamExt;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use owo_core as canonical;

use crate::{
    model::{ContentPart, ModelInvocation, ProjectedContent, ProviderReplayState, Role, Usage},
    Error, Result,
};

use super::{FinishReason, ModelEvent, Provider, ProviderStream};

/// Client id for OwO AI Gateway aliases (`models[].aliases.cursor`) and request metadata.
pub const CLIENT_ID: &str = "cursor";
/// `ProviderReplayState.provider_kind` for reasoning replay material produced by OwO AI Gateway.
pub const REPLAY_KIND: &str = "owo";

pub struct OwoProvider {
    router: Arc<owo_routing::Router>,
}

impl OwoProvider {
    pub fn new(router: Arc<owo_routing::Router>) -> Self {
        Self { router }
    }
}

impl Provider for OwoProvider {
    fn stream(&self, invocation: ModelInvocation, cancellation: CancellationToken) -> ProviderStream {
        let router = self.router.clone();
        Box::pin(try_stream! {
            let request = to_canonical(&invocation)?;
            let routed = tokio::select! {
                _ = cancellation.cancelled() => { return; }
                routed = router.execute(request) => routed,
            };
            let mut events = routed.map_err(|e| Error::Provider(e.message))?.events;
            let mut mapper = EventMapper::new(invocation.call_id.clone());
            loop {
                let next = tokio::select! {
                    _ = cancellation.cancelled() => { return; }
                    next = events.next() => next,
                };
                let Some(event) = next else { break };
                for out in mapper.push(event)? {
                    yield out;
                }
            }
            if !mapper.done {
                Err(Error::Provider("OwO AI Gateway stream ended before the response completed".into()))?;
            }
        })
    }
}

fn image(mime_type: &str, data: &[u8]) -> canonical::ImageInput {
    canonical::ImageInput {
        source: canonical::ImageSource::Base64 { media_type: mime_type.to_string(), data: STANDARD.encode(data) },
        detail: None,
    }
}

fn parts_to_blocks(parts: &[ContentPart]) -> Vec<canonical::ContentBlock> {
    parts
        .iter()
        .map(|p| match p {
            ContentPart::Text { text } => canonical::ContentBlock::text(text.clone()),
            ContentPart::Image { mime_type, data } => canonical::ContentBlock::Image(image(mime_type, data)),
        })
        .collect()
}

/// Reasoning blocks OwO AI Gateway produced on an earlier turn (signatures included).
fn replayed_reasoning(state: Option<&ProviderReplayState>) -> Vec<canonical::ReasoningBlock> {
    state
        .filter(|s| s.provider_kind == REPLAY_KIND)
        .and_then(|s| s.value.get("reasoning").cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default()
}

pub(crate) fn to_canonical(invocation: &ModelInvocation) -> Result<canonical::ModelRequest> {
    let r = &invocation.request;
    let mut req = canonical::ModelRequest::new(invocation.call_id.clone(), r.model.model_id.clone());
    req.stream = true;
    req.metadata.inbound = Some(canonical::InboundProtocol::Cursor);
    req.metadata.client = Some(CLIENT_ID.into());
    req.metadata.session_id = Some(invocation.conversation_id.clone());
    if !r.prompt.instructions.is_empty() {
        req.system.push(canonical::ContentBlock::text(r.prompt.instructions.clone()));
    }
    req.tools = r
        .prompt
        .tools
        .iter()
        .map(|t| canonical::ToolDefinition::Function {
            name: t.name.clone(),
            description: (!t.description.is_empty()).then(|| t.description.clone()),
            parameters: t.parameters.clone(),
            strict: None,
        })
        .collect();
    req.max_output_tokens = r.model.max_output_tokens.map(|v| v.min(u32::MAX as u64) as u32);
    if let Some(effort) = &r.model.reasoning.effort {
        req.reasoning = Some(canonical::ReasoningConfig { effort: Some(effort.clone()), ..Default::default() });
    }

    for m in &r.history {
        match &m.content {
            ProjectedContent::Parts(parts) => {
                let role = match m.role {
                    Role::Assistant => canonical::Role::Assistant,
                    Role::System => canonical::Role::System,
                    Role::User | Role::Tool => canonical::Role::User,
                };
                let blocks = parts_to_blocks(parts);
                if blocks.is_empty() {
                    continue;
                }
                if role == canonical::Role::System && req.messages.is_empty() {
                    req.system.extend(blocks);
                } else {
                    req.messages.push(canonical::Message::new(role, blocks));
                }
            }
            ProjectedContent::Assistant { text, thinking, replay_state, calls } => {
                let mut blocks: Vec<canonical::ContentBlock> =
                    replayed_reasoning(replay_state.as_ref()).into_iter().map(canonical::ContentBlock::Reasoning).collect();
                if blocks.is_empty() && !thinking.is_empty() {
                    blocks.push(canonical::ContentBlock::Reasoning(canonical::ReasoningBlock {
                        text: thinking.clone(),
                        ..Default::default()
                    }));
                }
                if !text.is_empty() {
                    blocks.push(canonical::ContentBlock::text(text.clone()));
                }
                for call in calls {
                    blocks.push(canonical::ContentBlock::ToolCall(canonical::ToolCall {
                        id: call.call_id.clone(),
                        name: call.name.clone(),
                        arguments: serde_json::to_string(&call.arguments)?,
                        kind: canonical::ToolCallKind::Function,
                    }));
                }
                if !blocks.is_empty() {
                    req.messages.push(canonical::Message::new(canonical::Role::Assistant, blocks));
                }
            }
            ProjectedContent::ToolResult(result) => {
                let content = if result.provider_parts.is_empty() {
                    vec![canonical::ToolResultContent::Text { text: result.content.clone() }]
                } else {
                    result
                        .provider_parts
                        .iter()
                        .map(|p| match p {
                            ContentPart::Text { text } => canonical::ToolResultContent::Text { text: text.clone() },
                            ContentPart::Image { mime_type, data } => canonical::ToolResultContent::Image(image(mime_type, data)),
                        })
                        .collect()
                };
                let block = canonical::ContentBlock::ToolResult(canonical::ToolResult {
                    call_id: result.call_id.clone(),
                    content,
                    is_error: result.is_error,
                    kind: canonical::ToolCallKind::Function,
                });
                match req.messages.last_mut() {
                    Some(last)
                        if last.role == canonical::Role::User
                            && last.content.iter().all(|b| matches!(b, canonical::ContentBlock::ToolResult(_))) =>
                    {
                        last.content.push(block);
                    }
                    _ => req.messages.push(canonical::Message::new(canonical::Role::User, vec![block])),
                }
            }
        }
    }
    Ok(req)
}

/// Canonical events → Cursor runtime provider events.
pub(crate) struct EventMapper {
    call_id: String,
    started: bool,
    tool_index: HashMap<String, usize>,
    reasoning_text: String,
    replay: Vec<canonical::ReasoningBlock>,
    usage: Option<Usage>,
    saw_tool: bool,
    pub(crate) done: bool,
}

impl EventMapper {
    pub(crate) fn new(call_id: String) -> Self {
        Self {
            call_id,
            started: false,
            tool_index: HashMap::new(),
            reasoning_text: String::new(),
            replay: Vec::new(),
            usage: None,
            saw_tool: false,
            done: false,
        }
    }

    fn start(&mut self, out: &mut Vec<ModelEvent>) {
        if !self.started {
            self.started = true;
            out.push(ModelEvent::Start { model_call_id: self.call_id.clone() });
        }
    }

    pub(crate) fn push(&mut self, event: canonical::ModelEvent) -> Result<Vec<ModelEvent>> {
        use canonical::ModelEvent as E;
        let mut out = Vec::new();
        match event {
            E::ResponseStart { .. } | E::MessageStart => self.start(&mut out),
            E::TextStart => {
                self.start(&mut out);
                out.push(ModelEvent::TextStart);
            }
            E::TextDelta { text } => out.push(ModelEvent::TextDelta(text)),
            E::TextEnd => out.push(ModelEvent::TextEnd),
            E::ReasoningStart => {
                self.start(&mut out);
                self.reasoning_text.clear();
                out.push(ModelEvent::ThinkingStart);
            }
            E::ReasoningDelta { text } => {
                self.reasoning_text.push_str(&text);
                out.push(ModelEvent::ThinkingDelta(text));
            }
            E::ReasoningEnd { signature, encrypted_content } => {
                if signature.is_some() || encrypted_content.is_some() {
                    self.replay.push(canonical::ReasoningBlock {
                        text: std::mem::take(&mut self.reasoning_text),
                        signature,
                        encrypted_content,
                        ..Default::default()
                    });
                }
                out.push(ModelEvent::ThinkingEnd);
            }
            E::ToolCallStart { id, name, .. } => {
                self.start(&mut out);
                self.saw_tool = true;
                let index = self.tool_index.len();
                self.tool_index.insert(id.clone(), index);
                out.push(ModelEvent::ToolCallStart { index, call_id: id, name });
            }
            E::ToolCallDelta { id, arguments_delta } => {
                if let Some(&index) = self.tool_index.get(&id) {
                    out.push(ModelEvent::ToolCallArgumentsDelta { index, delta: arguments_delta });
                }
            }
            E::ToolCallEnd { id } => {
                if let Some(&index) = self.tool_index.get(&id) {
                    out.push(ModelEvent::ToolCallEnd { index });
                }
            }
            E::Usage(u) => {
                let cached = u.cached_input_tokens.unwrap_or(0);
                let created = u.cache_creation_input_tokens.unwrap_or(0);
                self.usage = Some(Usage {
                    input_tokens: Some(u.input_tokens.saturating_sub(cached + created)),
                    context_input_tokens: Some(u.input_tokens),
                    output_tokens: Some(u.output_tokens),
                    total_tokens: Some(u.total_tokens()),
                    cache_read_tokens: u.cached_input_tokens,
                    cache_write_tokens: u.cache_creation_input_tokens,
                    reasoning_tokens: u.reasoning_tokens,
                });
            }
            E::MessageEnd { stop_reason } => {
                self.start(&mut out);
                if !self.replay.is_empty() {
                    out.push(ModelEvent::ProviderReplayState(ProviderReplayState {
                        provider_kind: REPLAY_KIND.into(),
                        value: json!({ "reasoning": std::mem::take(&mut self.replay) }),
                    }));
                }
                if let Some(u) = self.usage.take() {
                    out.push(ModelEvent::Usage(u));
                }
                let finish = match stop_reason {
                    canonical::StopReason::MaxTokens => FinishReason::Length,
                    _ if self.saw_tool => FinishReason::ToolUse,
                    canonical::StopReason::ToolUse => FinishReason::ToolUse,
                    _ => FinishReason::Stop,
                };
                out.push(ModelEvent::Done(finish));
                self.done = true;
            }
            E::ResponseEnd => {}
            E::Error(err) => return Err(Error::Provider(err.message)),
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ModelRequest, ModelSpec, ProjectedMessage, PromptSpec, ToolCallContent, ToolDefinition, ToolResultContent};

    fn invocation(history: Vec<ProjectedMessage>) -> ModelInvocation {
        let mut model = ModelSpec::new("claude-sonnet-5");
        model.reasoning.effort = Some("high".into());
        model.max_output_tokens = Some(64_000);
        ModelInvocation {
            call_id: "call-1".into(),
            run_id: "run-1".into(),
            conversation_id: "conv-1".into(),
            provider_call_index: 0,
            request: ModelRequest {
                prompt: PromptSpec {
                    instructions: "You are Cursor's agent.".into(),
                    tools: vec![ToolDefinition { name: "read_file".into(), description: "Read".into(), parameters: json!({"type": "object"}) }],
                },
                model,
                history,
            },
        }
    }

    fn message(role: Role, content: ProjectedContent) -> ProjectedMessage {
        ProjectedMessage { message_id: "m".into(), role, content }
    }

    #[test]
    fn converts_a_tool_round_with_replayed_signature() {
        let replay = ProviderReplayState {
            provider_kind: REPLAY_KIND.into(),
            value: json!({"reasoning": [{"text": "plan", "signature": "SIG"}]}),
        };
        let inv = invocation(vec![
            message(Role::User, ProjectedContent::Parts(vec![
                ContentPart::Text { text: "fix it".into() },
                ContentPart::Image { mime_type: "image/png".into(), data: vec![1, 2, 3] },
            ])),
            message(Role::Assistant, ProjectedContent::Assistant {
                text: "Reading.".into(),
                thinking: "plan".into(),
                replay_state: Some(replay),
                calls: vec![ToolCallContent { index: 0, call_id: "toolu_1".into(), name: "read_file".into(), arguments: json!({"path": "a.rs"}) }],
            }),
            message(Role::Tool, ProjectedContent::ToolResult(ToolResultContent {
                call_id: "toolu_1".into(),
                name: "read_file".into(),
                content: "fn main() {}".into(),
                is_error: false,
                image: None,
                provider_parts: vec![],
            })),
        ]);
        let req = to_canonical(&inv).unwrap();
        assert_eq!(req.model.as_str(), "claude-sonnet-5");
        assert_eq!(req.metadata.client.as_deref(), Some("cursor"));
        assert_eq!(req.system_text(), "You are Cursor's agent.");
        assert_eq!(req.reasoning.as_ref().unwrap().effort.as_deref(), Some("high"));
        assert_eq!(req.max_output_tokens, Some(64_000));
        assert_eq!(req.messages.len(), 3);
        assert!(req.has_images());
        let assistant = &req.messages[1].content;
        assert!(matches!(&assistant[0], canonical::ContentBlock::Reasoning(r) if r.signature.as_deref() == Some("SIG") && r.text == "plan"));
        assert!(matches!(&assistant[2], canonical::ContentBlock::ToolCall(c) if c.arguments == "{\"path\":\"a.rs\"}"));
        assert!(matches!(&req.messages[2].content[0], canonical::ContentBlock::ToolResult(r) if r.text() == "fn main() {}"));
    }

    #[test]
    fn maps_events_and_carries_signatures_forward() {
        use canonical::ModelEvent as E;
        let mut m = EventMapper::new("call-1".into());
        let mut out = Vec::new();
        for e in [
            E::ResponseStart { id: "r".into(), model: "m".into() },
            E::MessageStart,
            E::ReasoningStart,
            E::ReasoningDelta { text: "pla".into() },
            E::ReasoningDelta { text: "n".into() },
            E::ReasoningEnd { signature: Some("SIG".into()), encrypted_content: None },
            E::TextStart,
            E::TextDelta { text: "ok".into() },
            E::TextEnd,
            E::ToolCallStart { id: "toolu_1".into(), name: "read_file".into(), kind: canonical::ToolCallKind::Function },
            E::ToolCallDelta { id: "toolu_1".into(), arguments_delta: "{}".into() },
            E::ToolCallEnd { id: "toolu_1".into() },
            E::Usage(canonical::Usage { input_tokens: 100, output_tokens: 5, cached_input_tokens: Some(80), ..Default::default() }),
            E::MessageEnd { stop_reason: canonical::StopReason::ToolUse },
            E::ResponseEnd,
        ] {
            out.extend(m.push(e).unwrap());
        }
        assert!(m.done);
        assert_eq!(out[0], ModelEvent::Start { model_call_id: "call-1".into() });
        assert!(out.contains(&ModelEvent::ToolCallStart { index: 0, call_id: "toolu_1".into(), name: "read_file".into() }));
        let replay = out.iter().find_map(|e| match e {
            ModelEvent::ProviderReplayState(s) => Some(s.clone()),
            _ => None,
        }).unwrap();
        assert_eq!(replay.provider_kind, REPLAY_KIND);
        assert_eq!(replay.value["reasoning"][0]["signature"], "SIG");
        assert_eq!(replay.value["reasoning"][0]["text"], "plan");
        let usage = out.iter().find_map(|e| match e { ModelEvent::Usage(u) => Some(*u), _ => None }).unwrap();
        assert_eq!((usage.input_tokens, usage.context_input_tokens, usage.cache_read_tokens), (Some(20), Some(100), Some(80)));
        assert_eq!(out.last(), Some(&ModelEvent::Done(FinishReason::ToolUse)));

        // The replay state round-trips into the next request's reasoning blocks.
        let blocks = replayed_reasoning(Some(&replay));
        assert_eq!(blocks[0].signature.as_deref(), Some("SIG"));
    }

    #[test]
    fn upstream_errors_surface_as_provider_errors() {
        let mut m = EventMapper::new("c".into());
        let err = m.push(canonical::ModelEvent::Error(canonical::ModelError::new(canonical::ErrorKind::RateLimited, "slow down"))).unwrap_err();
        assert!(err.to_string().contains("slow down"));
    }
}
