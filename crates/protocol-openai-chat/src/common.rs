//! Shapes shared by the OpenAI protocol family (Chat and Responses).

use serde_json::{json, Value};
use owo_core::{ErrorKind, ModelError, StopReason, Usage};

use crate::json::u64_field;

/// OpenAI-style error `code`, which clients such as Codex use to decide on retries.
pub fn error_code(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::RateLimited => "rate_limit_exceeded",
        ErrorKind::ContextExceeded => "context_length_exceeded",
        ErrorKind::ModelNotFound => "model_not_found",
        ErrorKind::AuthenticationFailed => "invalid_api_key",
        other => other.as_str(),
    }
}

pub fn error_type(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::AuthenticationFailed => "authentication_error",
        ErrorKind::AuthorizationFailed => "permission_error",
        ErrorKind::RateLimited => "rate_limit_error",
        ErrorKind::ModelNotFound => "not_found_error",
        ErrorKind::InvalidRequest
        | ErrorKind::UnsupportedCapability
        | ErrorKind::ContextExceeded
        | ErrorKind::InvalidToolSchema
        | ErrorKind::ProtocolViolation => "invalid_request_error",
        _ => "api_error",
    }
}

/// `{"error": {...}}` body for an HTTP error response.
pub fn error_body(err: &ModelError) -> Value {
    json!({
        "error": {
            "message": err.message,
            "type": error_type(err.kind),
            "code": error_code(err.kind),
            "param": Value::Null,
        }
    })
}

pub fn finish_reason(stop: &StopReason) -> String {
    match stop {
        StopReason::EndTurn | StopReason::StopSequence => "stop".into(),
        StopReason::MaxTokens => "length".into(),
        StopReason::ToolUse => "tool_calls".into(),
        StopReason::ContentFilter | StopReason::Refusal => "content_filter".into(),
        StopReason::Other(s) => s.clone(),
    }
}

pub fn parse_finish_reason(reason: &str) -> StopReason {
    match reason {
        "stop" | "end_turn" => StopReason::EndTurn,
        "length" | "max_tokens" => StopReason::MaxTokens,
        "tool_calls" | "function_call" | "tool_use" => StopReason::ToolUse,
        "content_filter" => StopReason::ContentFilter,
        other => StopReason::Other(other.to_string()),
    }
}

/// Chat Completions `usage` object.
pub fn chat_usage(u: &Usage) -> Value {
    let mut v = json!({
        "prompt_tokens": u.input_tokens,
        "completion_tokens": u.output_tokens,
        "total_tokens": u.total_tokens(),
    });
    if let Some(c) = u.cached_input_tokens {
        v["prompt_tokens_details"] = json!({ "cached_tokens": c });
    }
    if let Some(r) = u.reasoning_tokens {
        v["completion_tokens_details"] = json!({ "reasoning_tokens": r });
    }
    v
}

/// Parses a Chat Completions `usage` object, including common vendor variants.
pub fn parse_chat_usage(v: &Value) -> Option<Usage> {
    if !v.is_object() {
        return None;
    }
    Some(Usage {
        input_tokens: u64_field(v, &["prompt_tokens"]).unwrap_or(0),
        output_tokens: u64_field(v, &["completion_tokens"]).unwrap_or(0),
        cached_input_tokens: u64_field(v, &["prompt_tokens_details", "cached_tokens"])
            .or_else(|| u64_field(v, &["prompt_cache_hit_tokens"])),
        cache_creation_input_tokens: None,
        reasoning_tokens: u64_field(v, &["completion_tokens_details", "reasoning_tokens"]),
    })
}

pub fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}{}", uuid::Uuid::new_v4().simple())
}
