use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::body::Body;
use http::{Request, StatusCode};
use serde_json::{json, Value};
use tower::ServiceExt;
use owo_config::Config;
use owo_core::{ContentBlock, ModelError, ModelEventStream, ModelRequest, ModelResponse, StopReason};
use owo_credentials::{CredentialBackend, CredentialStore, Secret};
use owo_registry::{Model, PresetCatalog, Registry};
use owo_routing::{DiscoveredModel, ProviderAccess, ProviderAdapter, Router};

use crate::{app, AppState, GatewayConfig};

/// Answers with the upstream model id so tests can see how routing resolved.
struct Fake;

#[async_trait::async_trait]
impl ProviderAdapter for Fake {
    fn kind(&self) -> &'static str {
        "openai-chat"
    }

    async fn execute(&self, _: &ProviderAccess, model: &Model, _: ModelRequest) -> Result<ModelEventStream, ModelError> {
        let resp = ModelResponse {
            id: "x".into(),
            model: model.upstream_model.clone(),
            content: vec![ContentBlock::text(format!("via {}", model.upstream_model))],
            stop_reason: StopReason::EndTurn,
            usage: None,
        };
        Ok(Box::pin(futures::stream::iter(resp.into_events())))
    }

    async fn discover_models(&self, _: &ProviderAccess) -> Result<Vec<DiscoveredModel>, ModelError> {
        Ok(vec![])
    }
}

fn state(token: Option<&str>) -> Arc<AppState> {
    let (config, _) = Config::from_toml_str(
        r#"
version = 1
[providers.local]
preset = "ollama"
[[models]]
id = "qwen"
display_name = "Qwen"
provider = "local"
upstream_model = "qwen3:8b"
aliases = { codex = "gpt-qwen", codex-desktop = "qwen-app" }
"#,
        "test",
    )
    .unwrap();
    let (registry, _) = Registry::build(&config, PresetCatalog::builtin(), &["openai-chat"]).unwrap();
    let router = Router::new(Arc::new(registry), CredentialStore::new(CredentialBackend::Env), vec![Arc::new(Fake)]);
    Arc::new(AppState {
        router: Arc::new(router),
        config: GatewayConfig {
            listen: "127.0.0.1:8787".parse().unwrap(),
            auth_token: token.map(Secret::new),
            max_body_bytes: 1024 * 1024,
            request_timeout: Duration::from_secs(5),
            max_concurrent_requests: 8,
            control_api: true,
        },
        started: Instant::now(),
        codex_template: None,
        codex_native_aliases: vec![owo_client_codex::catalog::NativeAlias {
            native: "gpt-5.5".into(),
            model: "qwen-app".into(),
        }],
        shutdown: Arc::default(),
    })
}

async fn call(state: Arc<AppState>, req: Request<Body>) -> (StatusCode, Value) {
    let resp = app(state).oneshot(req).await.unwrap();
    let status = resp.status();
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
}

fn post(path: &str, body: Value) -> Request<Body> {
    Request::post(path)
        .header("host", "127.0.0.1:8787")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn get(path: &str) -> Request<Body> {
    Request::get(path).header("host", "127.0.0.1:8787").body(Body::empty()).unwrap()
}

#[tokio::test]
async fn chat_non_stream_with_client_alias() {
    let body = json!({"model": "gpt-qwen", "messages": [{"role": "user", "content": "hi"}]});
    let (status, v) = call(state(None), post("/c/codex/v1/chat/completions", body.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["model"], "gpt-qwen");
    assert_eq!(v["choices"][0]["message"]["content"], "via qwen3:8b");

    // Without the codex prefix the alias is not visible.
    let (status, v) = call(state(None), post("/v1/chat/completions", body)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(v["error"]["code"], "model_not_found");
}

#[tokio::test]
async fn responses_non_stream() {
    let (status, v) = call(state(None), post("/v1/responses", json!({"model": "qwen", "input": "hi"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["status"], "completed");
    assert_eq!(v["output"][0]["content"][0]["text"], "via qwen3:8b");
}

#[tokio::test]
async fn models_are_listed_per_client_and_protocol() {
    let (_, v) = call(state(None), get("/c/codex/v1/models")).await;
    assert_eq!(v["data"][0]["id"], "gpt-qwen");
    let (_, v) = call(state(None), get("/v1/models")).await;
    assert_eq!(v["data"][0]["id"], "qwen");
    let req = Request::get("/v1/models")
        .header("host", "localhost:8787")
        .header("anthropic-version", "2023-06-01")
        .body(Body::empty())
        .unwrap();
    let (_, v) = call(state(None), req).await;
    assert_eq!(v["data"][0]["type"], "model");
    assert_eq!(v["data"][0]["display_name"], "Qwen");
}

#[tokio::test]
async fn codex_catalog_for_client_version_queries() {
    let (status, v) = call(state(None), get("/c/codex_desktop/v1/models?client_version=0.155.0")).await;
    assert_eq!(status, StatusCode::OK);
    let entry = &v["models"][0];
    assert_eq!(entry["slug"], "gpt-5.5", "native alias first");
    assert_eq!(entry["display_name"], "Qwen");
    assert_eq!(entry["supports_search_tool"], false);
    assert!(entry["base_instructions"].is_string());
    assert_eq!(v["models"][1]["slug"], "qwen-app");
    assert_eq!(v["models"][1]["visibility"], "hide");

    // The CLI has its own aliases and never lends native slots.
    let (_, v) = call(state(None), get("/c/codex/v1/models?client_version=0.155.0")).await;
    assert_eq!(v["models"].as_array().map(Vec::len), Some(1));
    assert_eq!(v["models"][0]["slug"], "gpt-qwen");
}

#[tokio::test]
async fn native_alias_routes_to_its_owo_model_for_codex_desktop_only() {
    let body = json!({"model": "gpt-5.5", "input": "hi"});
    let (status, v) = call(state(None), post("/c/codex_desktop/v1/responses", body.clone())).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["model"], "gpt-5.5", "the client's id is echoed");
    assert_eq!(v["output"][0]["content"][0]["text"], "via qwen3:8b");

    for path in ["/c/codex/v1/responses", "/v1/responses"] {
        let (status, _) = call(state(None), post(path, body.clone())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path} does not see Desktop's native aliases");
    }
}

#[tokio::test]
async fn anthropic_messages_for_claude_code() {
    let body = json!({"model": "claude-owo--qwen[1m]", "max_tokens": 64, "messages": [{"role": "user", "content": "hi"}]});
    let (status, v) = call(state(None), post("/c/claude_code/v1/messages", body)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["type"], "message");
    assert_eq!(v["model"], "claude-owo--qwen[1m]", "the client's id is echoed");
    assert_eq!(v["content"][0]["text"], "via qwen3:8b");
    assert_eq!(v["stop_reason"], "end_turn");

    let body = json!({"model": "qwen", "stream": true, "messages": [{"role": "user", "content": "hi"}]});
    let resp = app(state(None)).oneshot(post("/v1/messages", body)).await.unwrap();
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let text = String::from_utf8(axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap().to_vec()).unwrap();
    let events: Vec<&str> = text.lines().filter_map(|l| l.strip_prefix("event: ")).collect();
    assert_eq!(events, ["message_start", "content_block_start", "content_block_delta", "content_block_stop", "message_delta", "message_stop"]);

    let (status, v) = call(state(None), post("/v1/messages", json!({"model": "nope", "messages": [{"role": "user", "content": "hi"}]}))).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(v["type"], "error");
    assert_eq!(v["error"]["type"], "not_found_error");

    let (status, v) = call(state(None), post("/c/claude_code/v1/messages/count_tokens", json!({"model": "qwen", "messages": [{"role": "user", "content": "hello there"}]}))).await;
    assert_eq!(status, StatusCode::OK);
    assert!(v["input_tokens"].as_u64().unwrap() > 0);

    let body = json!({"model": "qwen", "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]});
    let (status, v) = call(state(None), post("/c/minimax_cli/anthropic/v1/messages", body)).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["content"][0]["text"], "via qwen3:8b");

    let req = Request::head("/c/claude_code/api/hello").header("host", "127.0.0.1:8787").body(Body::empty()).unwrap();
    assert_eq!(app(state(None)).oneshot(req).await.unwrap().status(), StatusCode::OK);
}

#[tokio::test]
async fn claude_desktop_aliases_resolve() {
    let alias = owo_client_claude_code::desktop::model_name("qwen");
    assert_ne!(alias, "qwen");
    let body = json!({"model": alias, "max_tokens": 16, "messages": [{"role": "user", "content": "hi"}]});
    let (status, v) = call(state(None), post("/c/claude_desktop/v1/messages", body.clone())).await;
    assert_eq!(status, StatusCode::OK, "{v}");
    assert_eq!(v["content"][0]["text"], "via qwen3:8b");
    let (status, _) = call(state(None), post("/c/claude_code/v1/messages", body)).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "Desktop aliases are Desktop's own");
}

#[tokio::test]
async fn claude_code_picker_ids() {
    let req = Request::get("/c/claude_code/v1/models?limit=1000")
        .header("host", "127.0.0.1:8787")
        .header("anthropic-version", "2023-06-01")
        .body(Body::empty())
        .unwrap();
    let (_, v) = call(state(None), req).await;
    assert_eq!(v["data"][0]["id"], "claude-owo--qwen", "the picker only lists claude-prefixed ids");
}

#[tokio::test]
async fn browser_origins_and_rebinding_are_rejected() {
    let mut req = post("/v1/chat/completions", json!({}));
    req.headers_mut().insert("origin", "https://evil.example.com".parse().unwrap());
    assert_eq!(call(state(None), req).await.0, StatusCode::FORBIDDEN);

    let req = Request::get("/control/v1/status").header("host", "evil.example.com").body(Body::empty()).unwrap();
    assert_eq!(call(state(None), req).await.0, StatusCode::FORBIDDEN);

    let mut req = get("/healthz");
    req.headers_mut().insert("origin", "http://localhost:3000".parse().unwrap());
    assert_eq!(call(state(None), req).await.0, StatusCode::OK);
}

#[tokio::test]
async fn access_token_is_enforced() {
    assert_eq!(call(state(Some("t0ken")), get("/v1/models")).await.0, StatusCode::UNAUTHORIZED);
    let mut req = get("/v1/models");
    req.headers_mut().insert("authorization", "Bearer t0ken".parse().unwrap());
    assert_eq!(call(state(Some("t0ken")), req).await.0, StatusCode::OK);
    let mut req = get("/v1/models");
    req.headers_mut().insert("x-api-key", "t0ken".parse().unwrap());
    assert_eq!(call(state(Some("t0ken")), req).await.0, StatusCode::OK);
}

#[tokio::test]
async fn shutdown_is_signalled_and_guarded() {
    let s = state(Some("t0ken"));
    assert_eq!(call(s.clone(), post("/control/v1/shutdown", json!({}))).await.0, StatusCode::UNAUTHORIZED);
    let mut req = post("/control/v1/shutdown", json!({}));
    req.headers_mut().insert("authorization", "Bearer t0ken".parse().unwrap());
    let notified = s.shutdown.clone();
    let wait = tokio::spawn(async move { notified.notified().await });
    assert_eq!(call(s, req).await.0, StatusCode::OK);
    tokio::time::timeout(Duration::from_secs(1), wait).await.unwrap().unwrap();
}

#[tokio::test]
async fn control_api_never_exposes_secrets() {
    let (status, v) = call(state(None), get("/control/v1/providers")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(v["data"][0]["id"], "local");
    assert_eq!(v["data"][0]["api_key"], "none");
    let (_, v) = call(state(None), get("/control/v1/status")).await;
    assert_eq!(v["models"]["available"], 1);
}

#[tokio::test]
async fn bad_requests_map_to_openai_errors() {
    let req = Request::post("/v1/responses")
        .header("host", "127.0.0.1")
        .body(Body::from("{not json"))
        .unwrap();
    let (status, v) = call(state(None), req).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(v["error"]["type"], "invalid_request_error");

    let (status, _) = call(state(None), get("/nope")).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(state(None), post("/c/bad%20id/v1/responses", json!({}))).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
