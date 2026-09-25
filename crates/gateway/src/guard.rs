//! Request admission: caller token, browser-origin and DNS-rebinding protection.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::middleware::Next;
use axum::response::Response;
use http::{header, HeaderMap};
use owo_core::{ErrorKind, ModelError};

use crate::reply::openai_error;
use crate::AppState;

pub(crate) async fn guard(State(state): State<Arc<AppState>>, req: Request, next: Next) -> Response {
    if let Err(err) = admit(&state, req.headers()) {
        tracing::warn!(path = %req.uri().path(), "rejected request: {}", err.message);
        return openai_error(&err);
    }
    next.run(req).await
}

fn admit(state: &AppState, headers: &HeaderMap) -> Result<(), ModelError> {
    // Native clients do not send Origin. A browser page does, and must not be able to
    // spend the user's provider credits or drive the control API.
    if let Some(origin) = headers.get(header::ORIGIN) {
        let ok = origin.to_str().ok().and_then(host_of_origin).is_some_and(is_loopback_name);
        if !ok {
            return Err(ModelError::new(ErrorKind::AuthorizationFailed, "cross-origin requests are not allowed"));
        }
    }
    // DNS rebinding: a loopback listener only answers to loopback host names.
    if state.config.listen.ip().is_loopback() {
        if let Some(host) = headers.get(header::HOST).and_then(|h| h.to_str().ok()) {
            if !is_loopback_name(strip_port(host)) {
                return Err(ModelError::new(ErrorKind::AuthorizationFailed, "unexpected Host header"));
            }
        }
    }
    if let Some(token) = &state.config.auth_token {
        let presented = headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer ").or_else(|| v.strip_prefix("bearer ")))
            .or_else(|| headers.get("x-api-key").and_then(|v| v.to_str().ok()));
        match presented {
            Some(p) if constant_time_eq(p.trim().as_bytes(), token.expose().as_bytes()) => {}
            _ => {
                return Err(ModelError::new(ErrorKind::AuthenticationFailed, "missing or invalid OwO AI Gateway access token"));
            }
        }
    }
    Ok(())
}

fn host_of_origin(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://")?.1;
    Some(strip_port(rest.split('/').next()?))
}

fn strip_port(host: &str) -> &str {
    if let Some(rest) = host.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(rest);
    }
    match host.rsplit_once(':') {
        Some((h, port)) if port.chars().all(|c| c.is_ascii_digit()) => h,
        _ => host,
    }
}

fn is_loopback_name(host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_parsing() {
        assert_eq!(strip_port("127.0.0.1:8787"), "127.0.0.1");
        assert_eq!(strip_port("[::1]:8787"), "::1");
        assert_eq!(strip_port("localhost"), "localhost");
        assert_eq!(host_of_origin("http://localhost:3000"), Some("localhost"));
        assert!(is_loopback_name("::1"));
        assert!(is_loopback_name("127.0.0.1"));
        assert!(!is_loopback_name("evil.example.com"));
        assert!(!is_loopback_name("127.0.0.1.evil.example.com"));
    }

    #[test]
    fn token_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
