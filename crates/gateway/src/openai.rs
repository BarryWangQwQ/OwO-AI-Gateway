//! `/v1/chat/completions` and `/v1/responses`.

use std::sync::Arc;
use std::time::Instant;

use axum::extract::{Path, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures::StreamExt;
use owo_core::{ModelError, ModelEvent, ModelEventStream, ResponseAccumulator};
use owo_protocol_openai_chat::common::new_id;
use owo_protocol_openai_chat::inbound as chat;
use owo_protocol_openai_responses::decode as responses_decode;
use owo_protocol_openai_responses::encode::{EncoderOptions, ResponsesEncoder};
use owo_routing::RoutedStream;

use crate::reply::{openai_error, sse};
use crate::AppState;

pub(crate) async fn chat_default(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    chat(state, None, body).await
}

pub(crate) async fn chat_client(State(state): State<Arc<AppState>>, Path(client): Path<String>, body: Bytes) -> Response {
    match valid_client(client) {
        Ok(client) => chat(state, Some(client), body).await,
        Err(e) => openai_error(&e),
    }
}

pub(crate) async fn responses_default(State(state): State<Arc<AppState>>, body: Bytes) -> Response {
    responses(state, None, body).await
}

pub(crate) async fn responses_client(
    State(state): State<Arc<AppState>>,
    Path(client): Path<String>,
    body: Bytes,
) -> Response {
    match valid_client(client) {
        Ok(client) => responses(state, Some(client), body).await,
        Err(e) => openai_error(&e),
    }
}

pub(crate) fn valid_client(client: String) -> Result<String, ModelError> {
    let ok = !client.is_empty()
        && client.len() <= 64
        && client.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
    if ok { Ok(client) } else { Err(ModelError::invalid_request("invalid client id in path")) }
}

pub(crate) fn parse_json(body: &Bytes) -> Result<serde_json::Value, ModelError> {
    serde_json::from_slice(body).map_err(|e| ModelError::invalid_request(format!("request body is not valid JSON: {e}")))
}

/// Per-request log context. Logs metadata only, never prompt or response content.
pub(crate) struct Ctx {
    pub(crate) request_id: String,
    protocol: &'static str,
    client: Option<String>,
    started: Instant,
}

impl Ctx {
    pub(crate) fn new(protocol: &'static str, client: Option<String>) -> Self {
        Self { request_id: new_id("req_"), protocol, client, started: Instant::now() }
    }

    fn fail(&self, model: Option<&str>, err: &ModelError) -> Response {
        self.log_failure(model, err);
        openai_error(err)
    }

    pub(crate) fn log_failure(&self, model: Option<&str>, err: &ModelError) {
        tracing::warn!(
            request_id = %self.request_id,
            protocol = self.protocol,
            client = self.client.as_deref().unwrap_or("-"),
            model = model.unwrap_or("-"),
            status = err.http_status(),
            kind = err.kind.as_str(),
            elapsed_ms = self.started.elapsed().as_millis() as u64,
            "request failed: {}",
            err.message
        );
    }

    /// Wraps a stream so that its outcome is logged when it finishes.
    pub(crate) fn log_stream(self, routed: RoutedStream) -> ModelEventStream {
        let model = routed.target.model.id.clone();
        let provider = routed.target.provider.id.clone();
        Box::pin(routed.events.inspect(move |ev| match ev {
            ModelEvent::ResponseEnd => tracing::info!(
                request_id = %self.request_id,
                protocol = self.protocol,
                client = self.client.as_deref().unwrap_or("-"),
                model = %model,
                provider = %provider,
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "stream completed"
            ),
            ModelEvent::Error(err) => tracing::warn!(
                request_id = %self.request_id,
                protocol = self.protocol,
                model = %model,
                provider = %provider,
                kind = err.kind.as_str(),
                elapsed_ms = self.started.elapsed().as_millis() as u64,
                "stream failed: {}",
                err.message
            ),
            _ => {}
        }))
    }
}

/// Structure of a request (counts, tool kinds, unmapped field names) at debug level.
/// Never logs content.
pub(crate) fn log_shape(ctx: &Ctx, req: &owo_core::ModelRequest) {
    if !tracing::enabled!(tracing::Level::DEBUG) {
        return;
    }
    let (mut function, mut custom, mut hosted) = (0, 0, Vec::new());
    for t in &req.tools {
        match t {
            owo_core::ToolDefinition::Function { .. } => function += 1,
            owo_core::ToolDefinition::Custom { .. } => custom += 1,
            owo_core::ToolDefinition::Hosted { kind, .. } => hosted.push(kind.as_str()),
        }
    }
    let extra: Vec<&str> = req.metadata.extra.keys().map(String::as_str).collect();
    tracing::debug!(
        request_id = %ctx.request_id,
        model = %req.model,
        messages = req.messages.len(),
        system_blocks = req.system.len(),
        function_tools = function,
        custom_tools = custom,
        hosted_tools = ?hosted,
        reasoning = ?req.reasoning.as_ref().and_then(|r| r.effort.as_deref()),
        stream = req.stream,
        unmapped_fields = ?extra,
        "request shape"
    );
}

pub(crate) async fn collect(events: ModelEventStream) -> ResponseAccumulator {
    events
        .fold(ResponseAccumulator::new(), |mut acc, ev| async move {
            acc.push(ev);
            acc
        })
        .await
}

async fn chat(state: Arc<AppState>, client: Option<String>, body: Bytes) -> Response {
    let ctx = Ctx::new("openai-chat", client.clone());
    let decoded = match parse_json(&body).and_then(|v| chat::decode_request(v, ctx.request_id.clone())) {
        Ok(d) => d,
        Err(e) => return ctx.fail(None, &e),
    };
    let mut request = decoded.request;
    request.metadata.client = client;
    log_shape(&ctx, &request);
    let requested = request.model.0.clone();
    // Responses echo the id the client sent; routing uses the model it stands for.
    request.model.0 = state.client_model(request.metadata.client.as_deref(), &requested).to_string();
    let stream = request.stream;

    let routed = match state.router.execute(request).await {
        Ok(r) => r,
        Err(e) => return ctx.fail(Some(&requested), &e),
    };
    let events = ctx.log_stream(routed);

    if stream {
        let mut encoder = chat::ChatStreamEncoder::new(requested, decoded.include_usage);
        return sse(events.flat_map(move |ev| futures::stream::iter(encoder.push(ev))));
    }
    match collect(events).await.finish() {
        Ok(resp) => Json(chat::encode_response(&resp, &requested)).into_response(),
        Err(e) => openai_error(&e),
    }
}

async fn responses(state: Arc<AppState>, client: Option<String>, body: Bytes) -> Response {
    let ctx = Ctx::new("openai-responses", client.clone());
    let decoded = match parse_json(&body).and_then(|v| responses_decode::decode_request(v, ctx.request_id.clone())) {
        Ok(d) => d,
        Err(e) => return ctx.fail(None, &e),
    };
    let mut request = decoded.request;
    request.metadata.client = client;
    log_shape(&ctx, &request);
    let requested = request.model.0.clone();
    // Responses echo the id the client sent; routing uses the model it stands for.
    request.model.0 = state.client_model(request.metadata.client.as_deref(), &requested).to_string();
    let stream = request.stream;
    let options = EncoderOptions {
        include_encrypted_reasoning: decoded.options.include_encrypted_reasoning,
        tool_namespaces: decoded.options.tool_namespaces,
        ..Default::default()
    };

    let routed = match state.router.execute(request).await {
        Ok(r) => r,
        Err(e) => return ctx.fail(Some(&requested), &e),
    };
    let events = ctx.log_stream(routed);

    let mut encoder = ResponsesEncoder::new(requested, options);
    if stream {
        return sse(events.flat_map(move |ev| futures::stream::iter(encoder.push(ev))));
    }
    let mut events = events;
    while let Some(ev) = events.next().await {
        encoder.push(ev);
    }
    match encoder.finish() {
        Ok(body) => Json(body).into_response(),
        Err(e) => openai_error(&e),
    }
}
