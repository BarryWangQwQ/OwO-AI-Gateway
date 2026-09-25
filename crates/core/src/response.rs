use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::content::{ContentBlock, ReasoningBlock, ToolCall};
use crate::errors::ModelError;
use crate::events::{ModelEvent, StopReason};
use crate::usage::Usage;

/// A complete (non-streaming) model response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelResponse {
    pub id: String,
    pub model: String,
    pub content: Vec<ContentBlock>,
    pub stop_reason: StopReason,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl ModelResponse {
    /// Replays the response as a canonical event sequence.
    pub fn into_events(self) -> Vec<ModelEvent> {
        let mut events = vec![
            ModelEvent::ResponseStart { id: self.id, model: self.model },
            ModelEvent::MessageStart,
        ];
        for block in self.content {
            match block {
                ContentBlock::Text { text } => {
                    events.push(ModelEvent::TextStart);
                    if !text.is_empty() {
                        events.push(ModelEvent::TextDelta { text });
                    }
                    events.push(ModelEvent::TextEnd);
                }
                ContentBlock::Reasoning(r) => {
                    events.push(ModelEvent::ReasoningStart);
                    if !r.text.is_empty() {
                        events.push(ModelEvent::ReasoningDelta { text: r.text });
                    }
                    events.push(ModelEvent::ReasoningEnd {
                        signature: r.signature,
                        encrypted_content: r.encrypted_content,
                    });
                }
                ContentBlock::ToolCall(call) => {
                    events.push(ModelEvent::ToolCallStart {
                        id: call.id.clone(),
                        name: call.name,
                        kind: call.kind,
                    });
                    if !call.arguments.is_empty() {
                        events.push(ModelEvent::ToolCallDelta {
                            id: call.id.clone(),
                            arguments_delta: call.arguments,
                        });
                    }
                    events.push(ModelEvent::ToolCallEnd { id: call.id });
                }
                // Responses never carry these output kinds; nothing to replay.
                ContentBlock::Image(_) | ContentBlock::ToolResult(_) | ContentBlock::File(_) => {}
            }
        }
        if let Some(usage) = self.usage {
            events.push(ModelEvent::Usage(usage));
        }
        events.push(ModelEvent::MessageEnd { stop_reason: self.stop_reason });
        events.push(ModelEvent::ResponseEnd);
        events
    }
}

/// Folds a canonical event stream into a [`ModelResponse`].
#[derive(Debug, Default)]
pub struct ResponseAccumulator {
    id: String,
    model: String,
    content: Vec<ContentBlock>,
    open_text: Option<usize>,
    open_reasoning: Option<usize>,
    tool_calls: HashMap<String, usize>,
    stop_reason: Option<StopReason>,
    usage: Option<Usage>,
    error: Option<ModelError>,
}

impl ResponseAccumulator {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, event: ModelEvent) {
        if self.error.is_some() {
            return;
        }
        match event {
            ModelEvent::ResponseStart { id, model } => {
                self.id = id;
                self.model = model;
            }
            ModelEvent::MessageStart | ModelEvent::ResponseEnd | ModelEvent::ToolCallEnd { .. } => {}
            ModelEvent::TextStart => {
                self.content.push(ContentBlock::text(""));
                self.open_text = Some(self.content.len() - 1);
            }
            ModelEvent::TextDelta { text } => {
                let idx = match self.open_text {
                    Some(idx) => idx,
                    None => {
                        self.content.push(ContentBlock::text(""));
                        let idx = self.content.len() - 1;
                        self.open_text = Some(idx);
                        idx
                    }
                };
                if let ContentBlock::Text { text: buf } = &mut self.content[idx] {
                    buf.push_str(&text);
                }
            }
            ModelEvent::TextEnd => self.open_text = None,
            ModelEvent::ReasoningStart => {
                self.content.push(ContentBlock::Reasoning(ReasoningBlock::default()));
                self.open_reasoning = Some(self.content.len() - 1);
            }
            ModelEvent::ReasoningDelta { text } => {
                let idx = match self.open_reasoning {
                    Some(idx) => idx,
                    None => {
                        self.content.push(ContentBlock::Reasoning(ReasoningBlock::default()));
                        let idx = self.content.len() - 1;
                        self.open_reasoning = Some(idx);
                        idx
                    }
                };
                if let ContentBlock::Reasoning(r) = &mut self.content[idx] {
                    r.text.push_str(&text);
                }
            }
            ModelEvent::ReasoningEnd { signature, encrypted_content } => {
                if let Some(idx) = self.open_reasoning.take() {
                    if let ContentBlock::Reasoning(r) = &mut self.content[idx] {
                        r.signature = signature;
                        r.encrypted_content = encrypted_content;
                    }
                }
            }
            ModelEvent::ToolCallStart { id, name, kind } => {
                self.content.push(ContentBlock::ToolCall(ToolCall {
                    id: id.clone(),
                    name,
                    arguments: String::new(),
                    kind,
                }));
                self.tool_calls.insert(id, self.content.len() - 1);
            }
            ModelEvent::ToolCallDelta { id, arguments_delta } => match self.tool_calls.get(&id) {
                Some(&idx) => {
                    if let ContentBlock::ToolCall(call) = &mut self.content[idx] {
                        call.arguments.push_str(&arguments_delta);
                    }
                }
                None => {
                    self.error = Some(ModelError::upstream_invalid(format!(
                        "tool call delta for unknown call id `{id}`"
                    )));
                }
            },
            ModelEvent::Usage(usage) => match &mut self.usage {
                Some(existing) => existing.merge(&usage),
                None => self.usage = Some(usage),
            },
            ModelEvent::MessageEnd { stop_reason } => self.stop_reason = Some(stop_reason),
            ModelEvent::Error(err) => self.error = Some(err),
        }
    }

    pub fn finish(self) -> Result<ModelResponse, ModelError> {
        if let Some(err) = self.error {
            return Err(err);
        }
        let has_tool_calls = self.content.iter().any(|b| matches!(b, ContentBlock::ToolCall(_)));
        let stop_reason = self.stop_reason.unwrap_or(if has_tool_calls {
            StopReason::ToolUse
        } else {
            StopReason::EndTurn
        });
        Ok(ModelResponse {
            id: self.id,
            model: self.model,
            content: self.content,
            stop_reason,
            usage: self.usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ToolCallKind;

    fn sample() -> ModelResponse {
        ModelResponse {
            id: "resp_1".into(),
            model: "m".into(),
            content: vec![
                ContentBlock::Reasoning(ReasoningBlock {
                    text: "think".into(),
                    signature: Some("sig".into()),
                    ..Default::default()
                }),
                ContentBlock::text("hello"),
                ContentBlock::ToolCall(ToolCall {
                    id: "call_1".into(),
                    name: "lookup".into(),
                    arguments: "{\"q\":1}".into(),
                    kind: ToolCallKind::Function,
                }),
            ],
            stop_reason: StopReason::ToolUse,
            usage: Some(Usage { input_tokens: 10, output_tokens: 5, ..Default::default() }),
        }
    }

    #[test]
    fn events_round_trip_through_accumulator() {
        let original = sample();
        let mut acc = ResponseAccumulator::new();
        for event in original.clone().into_events() {
            acc.push(event);
        }
        assert_eq!(acc.finish().unwrap(), original);
    }

    #[test]
    fn implicit_text_block_is_created() {
        let mut acc = ResponseAccumulator::new();
        acc.push(ModelEvent::TextDelta { text: "a".into() });
        acc.push(ModelEvent::TextDelta { text: "b".into() });
        let resp = acc.finish().unwrap();
        assert_eq!(resp.content, vec![ContentBlock::text("ab")]);
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn unknown_tool_delta_is_an_error() {
        let mut acc = ResponseAccumulator::new();
        acc.push(ModelEvent::ToolCallDelta { id: "x".into(), arguments_delta: "{}".into() });
        assert_eq!(acc.finish().unwrap_err().kind, crate::ErrorKind::UpstreamInvalidResponse);
    }

    #[test]
    fn error_event_wins() {
        let mut acc = ResponseAccumulator::new();
        acc.push(ModelEvent::TextDelta { text: "a".into() });
        acc.push(ModelEvent::Error(ModelError::new(crate::ErrorKind::RateLimited, "slow down")));
        assert_eq!(acc.finish().unwrap_err().kind, crate::ErrorKind::RateLimited);
    }
}
