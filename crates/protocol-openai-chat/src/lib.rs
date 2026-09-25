//! OpenAI Chat Completions protocol.
//!
//! - [`inbound`]: client-facing ? decode `/v1/chat/completions` requests into the
//!   canonical core and encode canonical events back as Chat responses/chunks.
//! - [`upstream`]: provider-facing ? encode canonical requests for an
//!   OpenAI-compatible upstream and decode its responses/chunks into events.

pub mod common;
pub mod inbound;
pub mod upstream;

/// JSON accessors shared with the Responses protocol crate.
pub mod json;
