//! Anthropic Messages API.
//!
//! Provider-facing side:
//! - [`encode::encode_request`]: canonical request → `POST /v1/messages` body.
//! - [`decode::AnthropicStreamDecoder`]: Messages SSE → canonical events.
//!
//! Client-facing side (Claude Code and other Anthropic clients):
//! - [`inbound::decode_request`]: `POST /v1/messages` body → canonical request.
//! - [`inbound::MessagesStreamEncoder`]: canonical events → Messages SSE.

pub mod decode;
pub mod encode;
pub mod inbound;
pub mod thinking;
mod tool_names;

pub use tool_names::ToolNames;
