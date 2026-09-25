//! Anthropic Messages API: `/v1/messages` and `/v1/messages/count_tokens`.

use std::sync::Arc;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures::StreamExt;
use http::{HeaderMap, StatusCode};
use serde_json::json;
use owo_core::ModelError;
use owo_protocol_anthropic::inbound;

use crate::openai::{collect, log_shape, parse_json, valid_client, Ctx};
use crate::reply::sse;
use crate::AppState;

/// An Anthropic-shaped error response.
pub(crate) fn anthropic_error(err: &ModelError) -> Response {
    let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut resp = (status, Json(inbound::error_body(err))).into_response();
    if let Some(secs) = err.retry_after_secs {
        if let Ok(v) = http::HeaderValue::from_str(&secs.to_string()) {
            resp.headers_mut().insert(http::header::RETRY_AFTER, v);
        }
    }
    resp
}

pub(crate) async fn messages_default(State(state): State<Arc<AppState>>, headers: HeaderMap, body: Bytes) -> Response {
    messages(state, None, headers, body).await
}

pub(crate) async fn messages_client(
    State(state): State<Arc<AppState>>,
    Path(client): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    match valid_client(client) {
        Ok(client) => messages(state, Some(client), headers, body).await,
        Err(e) => anthropic_error(&e),
    }
}

pub(crate) async fn count_tokens_default(body: Bytes) -> Response {
    count_tokens(body)
}

pub(crate) async fn count_tokens_client(Path(_client): Path<String>, body: Bytes) -> Response {
    count_tokens(body)
}

/// Claude Code's connectivity probe (`HEAD /api/hello`).
pub(crate) async fn hello() -> Response {
    Json(json!({ "status": "ok" })).into_response()
}

fn count_tokens(body: Bytes) -> Response {
    match parse_json(&body) {
        Ok(v) => Json(json!({ "input_tokens": inbound::estimate_input_tokens(&v) })).into_response(),
        Err(e) => anthropic_error(&e),
    }
}

async fn messages(state: Arc<AppState>, client: Option<String>, headers: HeaderMap, body: Bytes) -> Response {
    let ctx = Ctx::new("anthropic", client.clone());
    let decoded = match parse_json(&body).and_then(|v| inbound::decode_request(v, ctx.request_id.clone())) {
        Ok(d) => d,
        Err(e) => {
            ctx.log_failure(None, &e);
            return anthropic_error(&e);
        }
    };
    let mut request = decoded.request;
    request.metadata.client = client;
    if request.metadata.session_id.is_none() {
        request.metadata.session_id =
            headers.get("x-claude-code-session-id").and_then(|v| v.to_str().ok()).map(str::to_string);
    }
    if let Some(beta) = headers.get("anthropic-beta").and_then(|v| v.to_str().ok()) {
        request.metadata.extra.insert("anthropic_beta".into(), json!(beta));
    }
    log_shape(&ctx, &request);
    let requested = request.model.0.clone();
    request.model.0 = state.claude_model(request.metadata.client.as_deref(), &requested);
    let stream = request.stream;

    let routed = match state.router.execute(request).await {
        Ok(r) => r,
        Err(e) => {
            ctx.log_failure(Some(&requested), &e);
            return anthropic_error(&e);
        }
    };
    let events = ctx.log_stream(routed);

    if stream {
        let mut encoder = inbound::MessagesStreamEncoder::new(requested);
        return sse(events.flat_map(move |ev| futures::stream::iter(encoder.push(ev))));
    }
    match collect(events).await.finish() {
        Ok(resp) => Json(inbound::encode_response(&resp, &requested)).into_response(),
        Err(e) => anthropic_error(&e),
    }
}
