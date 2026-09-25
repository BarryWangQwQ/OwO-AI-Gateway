//! Shared HTTP layer for provider adapters. Protocol specifics (request bodies,
//! stream event shapes) stay in the protocol crates; this crate only moves bytes
//! safely: credentials in headers, sanitized errors, bounded SSE streams.

mod errors;
mod request;
mod stream;

use std::time::Duration;

pub use errors::{classify_error_body, redact, status_error, transport_error};
pub use request::{endpoint, headers};
pub use stream::{event_stream, StreamDecoder};

#[derive(Debug, Clone)]
pub struct AdapterSettings {
    pub connect_timeout: Duration,
    /// Maximum silence between stream chunks before the call is failed.
    pub stream_idle_timeout: Duration,
    /// Deadline for a non-streaming response.
    pub response_timeout: Duration,
}

impl Default for AdapterSettings {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(30),
            stream_idle_timeout: Duration::from_secs(300),
            response_timeout: Duration::from_secs(900),
        }
    }
}

pub fn client(settings: &AdapterSettings) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .connect_timeout(settings.connect_timeout)
        .user_agent(concat!("owo/", env!("CARGO_PKG_VERSION")))
        // Keep warm connections to providers across the pauses between agent turns, so
        // the next request skips a fresh TLS handshake, and notice half-open connections
        // (NAT, Wi-Fi changes) instead of waiting for a request to time out on them.
        .pool_idle_timeout(Duration::from_secs(300))
        .tcp_keepalive(Duration::from_secs(30))
        .http2_keep_alive_interval(Duration::from_secs(30))
        .http2_keep_alive_timeout(Duration::from_secs(20))
        .http2_keep_alive_while_idle(true)
        .build()
}
