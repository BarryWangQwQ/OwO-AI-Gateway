//! Canonical core of OwO AI Gateway.
//!
//! Every inbound protocol decodes into [`ModelRequest`] and every provider
//! adapter emits [`ModelEvent`]s. Changes to these types require explicit
//! migration review (spec §8): they are the boundary that keeps the system
//! N+M instead of N×M.

pub mod content;
pub mod errors;
pub mod events;
pub mod message;
pub mod request;
pub mod response;
pub mod schema;
pub mod tools;
pub mod usage;

pub use content::{
    ContentBlock, FileInput, FileSource, ImageInput, ImageSource, ReasoningBlock, ToolCall,
    ToolCallKind, ToolResult, ToolResultContent,
};
pub use errors::{ErrorKind, ModelError};
pub use events::{ModelEvent, ModelEventStream, StopReason};
pub use message::{Message, Role};
pub use request::{
    InboundProtocol, ModelRef, ModelRequest, OutputFormat, ReasoningConfig, RequestMetadata,
    SamplingConfig,
};
pub use response::{ModelResponse, ResponseAccumulator};
pub use tools::{ToolChoice, ToolDefinition};
pub use usage::Usage;
