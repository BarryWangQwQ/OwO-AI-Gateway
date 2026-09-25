//! OpenAI Responses API, client-facing side (Codex CLI/App/SDK and generic clients).
//!
//! - [`decode::decode_request`]: `/v1/responses` body → [`owo_core::ModelRequest`].
//! - [`encode::ResponsesEncoder`]: canonical events → Responses SSE events, or a
//!   complete response object for non-streaming calls.

pub mod decode;
pub mod encode;

/// Marks a provider reasoning signature (Anthropic `thinking.signature`) carried in a
/// Responses reasoning item's `encrypted_content`, so the client hands it back and the
/// next turn can replay the thinking block the provider requires.
pub const SIGNATURE_PREFIX: &str = "owo-sig:v1:";
