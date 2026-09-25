//! The local gateway. Client-facing endpoints decode into the canonical core and
//! hand requests to the shared [`owo_routing::Router`].
//!
//! Routes:
//! - `/v1/...` and `/c/{client}/v1/...` — data plane (OpenAI Chat, OpenAI Responses,
//!   Anthropic Messages). The client prefix selects that client's model aliases
//!   (`models[].aliases.<client>`).
//! - `/control/v1/...` — local management API.
//! - `/healthz`.

mod anthropic;
mod control;
mod guard;
mod models;
mod openai;
mod reply;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::Router as AxumRouter;
use http::StatusCode;
use tower::limit::ConcurrencyLimitLayer;
use tower_http::timeout::TimeoutLayer;
use owo_credentials::Secret;
use owo_routing::Router;

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub listen: SocketAddr,
    /// Token required from every caller. Mandatory for non-loopback binds (enforced by config validation).
    pub auth_token: Option<Secret>,
    pub max_body_bytes: usize,
    /// Deadline until response headers are sent. Streams are bounded by the adapters' idle timeout.
    pub request_timeout: Duration,
    pub max_concurrent_requests: usize,
    pub control_api: bool,
}

pub struct AppState {
    pub router: Arc<Router>,
    pub config: GatewayConfig,
    pub started: Instant,
    /// Codex catalog template captured by `owo connect codex`; the built-in
    /// template is used when absent.
    pub codex_template: Option<serde_json::Value>,
    /// Native GPT slugs lent to OwO AI Gateway models for a signed-out Codex Desktop.
    pub codex_native_aliases: Vec<owo_client_codex::catalog::NativeAlias>,
    /// Signalled by `POST /control/v1/shutdown` (`owo stop`).
    pub shutdown: Arc<tokio::sync::Notify>,
}

impl AppState {
    /// The model a client actually means: a Codex Desktop native-slug alias resolves to its
    /// OwO AI Gateway model.
    pub(crate) fn client_model<'a>(&'a self, client: Option<&str>, requested: &'a str) -> &'a str {
        if client == Some(owo_client_codex::DESKTOP_CLIENT_ID) {
            if let Some(a) = self.codex_native_aliases.iter().find(|a| a.native == requested) {
                return &a.model;
            }
        }
        requested
    }

    /// The model an Anthropic client means: Claude Code picker ids (`claude-owo--…`,
    /// `[1m]` markers) and dated snapshot ids resolve to the OwO AI Gateway model they stand for.
    /// Unresolvable ids pass through so routing reports them.
    pub(crate) fn claude_model(&self, client: Option<&str>, requested: &str) -> String {
        let candidates = owo_client_claude_code::ids::candidates(requested);
        let registry = self.router.registry();
        if client == Some(owo_client_claude_code::desktop::CLIENT_ID) {
            let name = candidates.first().map(String::as_str).unwrap_or(requested);
            let aliased = registry.available_models().map(|m| m.exposed_id(client)).find(|id| {
                *id != name && owo_client_claude_code::desktop::model_name(id) == name
            });
            if let Some(id) = aliased {
                return id.to_string();
            }
        }
        candidates
            .iter()
            .find(|c| registry.resolve(c, client).is_ok())
            .or(candidates.first())
            .cloned()
            .unwrap_or_else(|| requested.to_string())
    }
}

pub fn app(state: Arc<AppState>) -> AxumRouter {
    let data = AxumRouter::new()
        .route("/models", get(models::list_default))
        .route("/chat/completions", post(openai::chat_default))
        .route("/responses", post(openai::responses_default))
        .route("/messages", post(anthropic::messages_default))
        .route("/messages/count_tokens", post(anthropic::count_tokens_default));
    let client_data = AxumRouter::new()
        .route("/models", get(models::list_client))
        .route("/chat/completions", post(openai::chat_client))
        .route("/responses", post(openai::responses_client))
        .route("/messages", post(anthropic::messages_client))
        .route("/messages/count_tokens", post(anthropic::count_tokens_client));

    let mut app = AxumRouter::new()
        .route("/healthz", get(|| async { axum::Json(serde_json::json!({ "status": "ok" })) }))
        .route("/api/hello", get(anthropic::hello).head(anthropic::hello))
        .route("/c/{client}/api/hello", get(anthropic::hello).head(anthropic::hello))
        // MiniMax CLI (`mmx`) fixes the Messages path under its base URL.
        .route("/c/{client}/anthropic/v1/messages", post(anthropic::messages_client))
        .route("/c/{client}/anthropic/v1/messages/count_tokens", post(anthropic::count_tokens_client))
        .nest("/v1", data)
        .nest("/c/{client}/v1", client_data);
    if state.config.control_api {
        app = app.nest("/control/v1", control::routes());
    }

    app.fallback(reply::not_found)
        .layer(axum::middleware::from_fn_with_state(state.clone(), guard::guard))
        .layer(DefaultBodyLimit::max(state.config.max_body_bytes))
        .layer(TimeoutLayer::with_status_code(StatusCode::GATEWAY_TIMEOUT, state.config.request_timeout))
        .layer(ConcurrencyLimitLayer::new(state.config.max_concurrent_requests))
        .with_state(state)
}

/// Binds and serves until `shutdown` resolves.
pub async fn serve(
    state: Arc<AppState>,
    shutdown: impl std::future::Future<Output = ()> + Send + 'static,
) -> std::io::Result<()> {
    let listener = tokio::net::TcpListener::bind(state.config.listen).await?;
    tracing::info!(listen = %state.config.listen, "listening");
    axum::serve(listener, app(state)).with_graceful_shutdown(shutdown).await
}

#[cfg(test)]
mod tests;
