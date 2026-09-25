use std::pin::Pin;

use futures::Stream;
use serde::{Deserialize, Serialize};

use crate::content::ToolCallKind;
use crate::errors::ModelError;
use crate::usage::Usage;

/// Canonical streaming event.
///
/// Ordering contract for producers:
/// `ResponseStart`, `MessageStart`, then content blocks, then optional `Usage`,
/// `MessageEnd`, `ResponseEnd`. Content blocks are emitted one at a time
/// (start, deltas, end) and do not interleave, so encoders for strictly
/// sequential protocols (Anthropic) need no buffering. `Error` may appear at
/// any point and terminates the stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum ModelEvent {
    ResponseStart { id: String, model: String },
    MessageStart,

    TextStart,
    TextDelta { text: String },
    TextEnd,

    ReasoningStart,
    ReasoningDelta { text: String },
    ReasoningEnd {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        encrypted_content: Option<String>,
    },

    ToolCallStart { id: String, name: String, kind: ToolCallKind },
    ToolCallDelta { id: String, arguments_delta: String },
    ToolCallEnd { id: String },

    Usage(Usage),
    MessageEnd { stop_reason: StopReason },
    ResponseEnd,
    Error(ModelError),
}

pub type ModelEventStream = Pin<Box<dyn Stream<Item = ModelEvent> + Send + 'static>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    MaxTokens,
    ToolUse,
    StopSequence,
    ContentFilter,
    Refusal,
    /// Upstream reason with no canonical equivalent, kept verbatim.
    Other(String),
}
