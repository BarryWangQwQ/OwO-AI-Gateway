//! Local management API (`/control/v1`). Responses carry credential *references* only,
//! never secret values. The one action, `shutdown`, sits behind the same admission
//! guard as everything else (token, browser-origin and DNS-rebinding checks).

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::AppState;

pub(crate) fn routes() -> Router<Arc<AppState>> {
    Router::new()
        .route("/status", get(status))
        .route("/providers", get(providers))
        .route("/models", get(models))
        .route("/shutdown", post(shutdown))
}

async fn shutdown(State(state): State<Arc<AppState>>) -> Json<Value> {
    tracing::info!("shutdown requested through the control API");
    state.shutdown.notify_one();
    Json(json!({ "stopping": true }))
}

async fn status(State(state): State<Arc<AppState>>) -> Json<Value> {
    let registry = state.router.registry();
    Json(json!({
        "name": "owo",
        "version": env!("CARGO_PKG_VERSION"),
        "listen": state.config.listen.to_string(),
        "uptime_secs": state.started.elapsed().as_secs(),
        "auth_required": state.config.auth_token.is_some(),
        "adapters": state.router.adapter_kinds(),
        "providers": {
            "configured": registry.providers().count(),
            "enabled": registry.providers().filter(|p| p.enabled).count(),
        },
        "models": {
            "configured": registry.models().count(),
            "available": registry.available_models().count(),
        },
    }))
}

async fn providers(State(state): State<Arc<AppState>>) -> Json<Value> {
    let list: Vec<_> = state.router.registry().providers().map(|p| p.as_ref().clone()).collect();
    Json(json!({ "data": list }))
}

async fn models(State(state): State<Arc<AppState>>) -> Json<Value> {
    let list: Vec<_> = state.router.registry().models().map(|m| m.as_ref().clone()).collect();
    Json(json!({ "data": list }))
}
