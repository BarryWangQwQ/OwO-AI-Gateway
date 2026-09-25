use std::time::Duration;

use reqwest::header::RETRY_AFTER;
use serde_json::Value;
use owo_core::{ErrorKind, ModelError};
use owo_routing::ProviderAccess;

const MAX_ERROR_BODY: usize = 64 * 1024;
const MAX_ERROR_MESSAGE: usize = 500;

/// Maps a transport error without exposing the URL (which may carry a query-string key).
pub fn transport_error(provider: &str, e: reqwest::Error) -> ModelError {
    let kind = if e.is_timeout() { ErrorKind::Timeout } else { ErrorKind::ProviderUnavailable };
    let what = if e.is_connect() {
        "could not connect"
    } else if e.is_timeout() {
        "timed out"
    } else {
        "request failed"
    };
    let detail = e.without_url().to_string();
    ModelError::new(kind, format!("provider `{provider}`: {what}: {}", truncate(&detail))).with_provider(provider)
}

/// Reads an error object in the shapes OpenAI-compatible and Anthropic servers use:
/// `{"error": {"message", "code"|"type"}}`, `{"error": "text"}`, or `{"message": ...}`.
/// Returns the message and, when recognizable, a more specific error kind.
pub fn classify_error_body(body: &Value) -> (Option<String>, Option<ErrorKind>) {
    let err = body.get("error").unwrap_or(body);
    let message = match err {
        Value::String(s) => Some(s.clone()),
        _ => err.get("message").and_then(Value::as_str).map(str::to_string),
    };
    let code = ["code", "type"]
        .iter()
        .find_map(|k| err.get(*k).and_then(Value::as_str))
        .unwrap_or_default()
        .to_ascii_lowercase();
    let text = message.as_deref().unwrap_or_default().to_ascii_lowercase();
    let kind = if code.contains("rate_limit") {
        Some(ErrorKind::RateLimited)
    } else if code.contains("context_length") || text.contains("prompt is too long") || text.contains("context length") {
        Some(ErrorKind::ContextExceeded)
    } else if code.contains("overloaded") {
        Some(ErrorKind::ProviderUnavailable)
    } else if code.contains("authentication") || code == "invalid_api_key" {
        Some(ErrorKind::AuthenticationFailed)
    } else if code.contains("permission") {
        Some(ErrorKind::AuthorizationFailed)
    } else if code.contains("not_found") {
        Some(ErrorKind::ModelNotFound)
    } else {
        None
    };
    (message, kind)
}

/// Builds a sanitized error from a non-2xx upstream response.
pub async fn status_error(access: &ProviderAccess, resp: reqwest::Response) -> ModelError {
    let status = resp.status().as_u16();
    let retry_after = resp
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok());
    let body = read_limited(resp).await;
    let (message, kind) = match serde_json::from_slice::<Value>(&body) {
        Ok(v) => classify_error_body(&v),
        Err(_) => (None, None),
    };
    let message = message.unwrap_or_else(|| String::from_utf8_lossy(&body).trim().to_string());
    let message = redact(&truncate(&message), access);
    let provider = &access.provider.id;
    let mut err = ModelError::from_upstream_status(status, format!("provider `{provider}` returned HTTP {status}: {message}"));
    if let Some(kind @ (ErrorKind::RateLimited | ErrorKind::ContextExceeded)) = kind {
        err.kind = kind;
    }
    err.retry_after_secs = retry_after;
    err.with_provider(provider)
}

async fn read_limited(mut resp: reqwest::Response) -> Vec<u8> {
    let mut out = Vec::new();
    let read = async {
        while let Ok(Some(chunk)) = resp.chunk().await {
            out.extend_from_slice(&chunk);
            if out.len() >= MAX_ERROR_BODY {
                out.truncate(MAX_ERROR_BODY);
                break;
            }
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(10), read).await;
    out
}

fn truncate(s: &str) -> String {
    if s.chars().count() <= MAX_ERROR_MESSAGE {
        return s.to_string();
    }
    let cut: String = s.chars().take(MAX_ERROR_MESSAGE).collect();
    format!("{cut}…")
}

/// Upstreams sometimes echo the presented key back in error text.
pub fn redact(s: &str, access: &ProviderAccess) -> String {
    match &access.secret {
        Some(secret) if secret.expose().len() >= 4 => s.replace(secret.expose(), "***"),
        _ => s.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::tests::access;
    use serde_json::json;
    use owo_config::AuthMode;

    #[test]
    fn redacts_echoed_secret() {
        let a = access("https://x", AuthMode::Bearer);
        assert_eq!(redact("bad key sk-secret-value", &a), "bad key ***");
    }

    #[test]
    fn classifies_openai_and_anthropic_errors() {
        let (m, k) = classify_error_body(&json!({"error": {"message": "slow", "code": "rate_limit_exceeded"}}));
        assert_eq!((m.as_deref(), k), (Some("slow"), Some(ErrorKind::RateLimited)));
        let (_, k) = classify_error_body(&json!({"type": "error", "error": {"type": "overloaded_error", "message": "busy"}}));
        assert_eq!(k, Some(ErrorKind::ProviderUnavailable));
        let (_, k) = classify_error_body(&json!({"error": {"type": "invalid_request_error", "message": "prompt is too long: 250000 tokens"}}));
        assert_eq!(k, Some(ErrorKind::ContextExceeded));
        let (m, _) = classify_error_body(&json!({"error": "plain"}));
        assert_eq!(m.as_deref(), Some("plain"));
    }
}
