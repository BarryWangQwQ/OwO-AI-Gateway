use serde::{Deserialize, Serialize};

/// Canonical error classes (spec §38). Client adapters map these onto their own
/// protocol's error shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    InvalidRequest,
    AuthenticationFailed,
    AuthorizationFailed,
    ProviderUnavailable,
    ModelNotFound,
    UnsupportedCapability,
    RateLimited,
    ContextExceeded,
    InvalidToolSchema,
    ProtocolViolation,
    UpstreamInvalidResponse,
    Timeout,
    Cancelled,
    ConfigurationError,
    ClientIntegrationConflict,
    Internal,
}

impl ErrorKind {
    pub fn http_status(self) -> u16 {
        match self {
            ErrorKind::InvalidRequest
            | ErrorKind::InvalidToolSchema
            | ErrorKind::ProtocolViolation
            | ErrorKind::ContextExceeded
            | ErrorKind::UnsupportedCapability => 400,
            ErrorKind::AuthenticationFailed => 401,
            ErrorKind::AuthorizationFailed => 403,
            ErrorKind::ModelNotFound => 404,
            ErrorKind::ClientIntegrationConflict => 409,
            ErrorKind::RateLimited => 429,
            ErrorKind::Cancelled => 499,
            ErrorKind::ConfigurationError | ErrorKind::Internal => 500,
            ErrorKind::UpstreamInvalidResponse | ErrorKind::ProviderUnavailable => 502,
            ErrorKind::Timeout => 504,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ErrorKind::InvalidRequest => "invalid_request",
            ErrorKind::AuthenticationFailed => "authentication_failed",
            ErrorKind::AuthorizationFailed => "authorization_failed",
            ErrorKind::ProviderUnavailable => "provider_unavailable",
            ErrorKind::ModelNotFound => "model_not_found",
            ErrorKind::UnsupportedCapability => "unsupported_capability",
            ErrorKind::RateLimited => "rate_limited",
            ErrorKind::ContextExceeded => "context_exceeded",
            ErrorKind::InvalidToolSchema => "invalid_tool_schema",
            ErrorKind::ProtocolViolation => "protocol_violation",
            ErrorKind::UpstreamInvalidResponse => "upstream_invalid_response",
            ErrorKind::Timeout => "timeout",
            ErrorKind::Cancelled => "cancelled",
            ErrorKind::ConfigurationError => "configuration_error",
            ErrorKind::ClientIntegrationConflict => "client_integration_conflict",
            ErrorKind::Internal => "internal_error",
        }
    }
}

/// A sanitized error. `message` must never contain credentials or raw
/// secret-bearing upstream payloads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{kind:?}: {message}")]
pub struct ModelError {
    pub kind: ErrorKind,
    pub message: String,
    /// Upstream HTTP status, when the error came from a provider response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_after_secs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

impl ModelError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self { kind, message: message.into(), upstream_status: None, retry_after_secs: None, provider: None }
    }

    pub fn invalid_request(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::InvalidRequest, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::UnsupportedCapability, message)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::ProtocolViolation, message)
    }

    pub fn upstream_invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::UpstreamInvalidResponse, message)
    }

    pub fn with_provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Classifies an upstream HTTP failure. `detail` should already be sanitized.
    pub fn from_upstream_status(status: u16, detail: impl Into<String>) -> Self {
        let kind = match status {
            400 | 422 => ErrorKind::InvalidRequest,
            401 => ErrorKind::AuthenticationFailed,
            403 => ErrorKind::AuthorizationFailed,
            404 => ErrorKind::ModelNotFound,
            408 | 504 => ErrorKind::Timeout,
            413 => ErrorKind::ContextExceeded,
            429 => ErrorKind::RateLimited,
            500..=599 => ErrorKind::ProviderUnavailable,
            _ => ErrorKind::UpstreamInvalidResponse,
        };
        let mut err = Self::new(kind, detail);
        err.upstream_status = Some(status);
        err
    }

    pub fn http_status(&self) -> u16 {
        self.kind.http_status()
    }
}
