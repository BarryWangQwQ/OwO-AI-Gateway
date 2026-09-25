//! End to end over real sockets: client → OwO AI Gateway → `openai-chat` adapter →
//! mock OpenAI-compatible upstream. No network access or API keys required.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::Json;
use serde_json::{json, Value};
use owo_config::Config;
use owo_credentials::{CredentialBackend, CredentialStore};
use owo_gateway::{AppState, GatewayConfig};
use owo_provider_openai_compat::{AdapterSettings, OpenAiChatAdapter};
use owo_registry::{PresetCatalog, Registry};
use owo_routing::Router;

#[derive(Default)]
struct Upstream {
    last_body: Mutex<Option<Value>>,
    last_auth: Mutex<Option<String>>,
}

async fn upstream_chat(State(up): State<Arc<Upstream>>, headers: http::HeaderMap, Json(body): Json<Value>) -> Response {
    *up.last_auth.lock().unwrap() = headers.get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
    let stream = body["stream"].as_bool().unwrap_or(false);
    let model = body["model"].as_str().unwrap_or_default().to_string();
    *up.last_body.lock().unwrap() = Some(body);
    if model == "fail-model" {
        return (
            http::StatusCode::TOO_MANY_REQUESTS,
            [("retry-after", "7")],
            Json(json!({"error": {"message": "slow down, key=sk-test-secret", "code": "rate_limit_exceeded"}})),
        )
            .into_response();
    }
    if !stream {
        return Json(json!({
            "id": "up-1", "model": model,
            "choices": [{"index": 0, "message": {"role": "assistant", "content": "pong"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 3, "completion_tokens": 1}
        }))
        .into_response();
    }
    let chunks = [
        json!({"id": "up-2", "model": model, "choices": [{"index": 0, "delta": {"role": "assistant", "reasoning_content": "Thinking"}}]}),
        json!({"choices": [{"index": 0, "delta": {"content": "Let me run it."}}]}),
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "id": "call_9", "type": "function", "function": {"name": "shell", "arguments": "{\"command\":"}}]}}]}),
        json!({"choices": [{"index": 0, "delta": {"tool_calls": [{"index": 0, "function": {"arguments": "[\"ls\"]}"}}]}}]}),
        json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]}),
        json!({"choices": [], "usage": {"prompt_tokens": 12, "completion_tokens": 8}}),
    ];
    let mut body = String::new();
    for c in chunks {
        body.push_str(&format!("data: {c}\n\n"));
    }
    body.push_str("data: [DONE]\n\n");
    ([("content-type", "text/event-stream")], body).into_response()
}

async fn start_upstream() -> (String, Arc<Upstream>) {
    let up = Arc::new(Upstream::default());
    let app = axum::Router::new().route("/v1/chat/completions", post(upstream_chat)).with_state(up.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/v1"), up)
}

async fn start_gateway(upstream_base: &str) -> String {
    static SET_KEY: std::sync::Once = std::sync::Once::new();
    // SAFETY: set exactly once, before any gateway in this test binary reads it.
    SET_KEY.call_once(|| unsafe { std::env::set_var("OWO_E2E_UPSTREAM_KEY", "sk-test-secret") });
    let config_text = format!(
        r#"
version = 1
[providers.mock]
adapter = "openai-chat"
base_url = "{upstream_base}"
api_key = "env:OWO_E2E_UPSTREAM_KEY"
allow_private_network = true

[[models]]
id = "mock-model"
provider = "mock"
upstream_model = "mock-upstream-1"
aliases = {{ codex = "gpt-mock" }}
reasoning_efforts = ["low", "medium", "high"]

[[models]]
id = "failing"
provider = "mock"
upstream_model = "fail-model"
"#
    );
    let (config, _) = Config::from_toml_str(&config_text, "e2e").unwrap();
    let (registry, _) = Registry::build(&config, PresetCatalog::builtin(), &["openai-chat"]).unwrap();
    let adapter = OpenAiChatAdapter::new(AdapterSettings::default()).unwrap();
    let router = Router::new(Arc::new(registry), CredentialStore::new(CredentialBackend::Env), vec![Arc::new(adapter)]);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = Arc::new(AppState {
        router: Arc::new(router),
        config: GatewayConfig {
            listen: addr,
            auth_token: None,
            max_body_bytes: 1 << 20,
            request_timeout: Duration::from_secs(30),
            max_concurrent_requests: 16,
            control_api: true,
        },
        started: Instant::now(),
        codex_template: None,
        codex_native_aliases: Vec::new(),
        shutdown: Arc::default(),
    });
    tokio::spawn(async move { axum::serve(listener, owo_gateway::app(state)).await.unwrap() });
    format!("http://{addr}")
}

fn sse_events(text: &str) -> Vec<(Option<String>, String)> {
    let mut dec = owo_sse::SseDecoder::new();
    let mut out: Vec<_> = dec.feed(text.as_bytes()).unwrap().into_iter().map(|e| (e.event, e.data)).collect();
    if let Some(e) = dec.finish().unwrap() {
        out.push((e.event, e.data));
    }
    out
}

#[tokio::test]
async fn codex_style_responses_stream_through_chat_provider() {
    let (upstream, up) = start_upstream().await;
    let gw = start_gateway(&upstream).await;
    let body = json!({
        "model": "gpt-mock",
        "instructions": "You are Codex.",
        "input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "list files"}]}],
        "tools": [{"type": "function", "name": "shell", "parameters": {"type": "object", "properties": {"command": {"type": "array"}}}}],
        "reasoning": {"effort": "high", "summary": "auto"},
        "stream": true,
        "store": false
    });
    let resp = reqwest::Client::new().post(format!("{gw}/c/codex/v1/responses")).json(&body).send().await.unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.headers()["content-type"], "text/event-stream");
    let events = sse_events(&resp.text().await.unwrap());

    let kinds: Vec<&str> = events.iter().filter_map(|(k, _)| k.as_deref()).collect();
    assert_eq!(kinds.first(), Some(&"response.created"));
    assert_eq!(kinds.last(), Some(&"response.completed"));
    assert!(kinds.contains(&"response.reasoning_summary_text.delta"));
    assert!(kinds.contains(&"response.function_call_arguments.done"));

    let completed: Value = serde_json::from_str(&events.last().unwrap().1).unwrap();
    let output = &completed["response"]["output"];
    assert_eq!(output[0]["type"], "reasoning");
    assert_eq!(output[1]["content"][0]["text"], "Let me run it.");
    assert_eq!(output[2]["call_id"], "call_9");
    assert_eq!(output[2]["arguments"], "{\"command\":[\"ls\"]}");
    assert_eq!(completed["response"]["usage"]["input_tokens"], 12);
    assert_eq!(completed["response"]["model"], "gpt-mock");

    // What the upstream saw: canonical → Chat encoding, credential as bearer.
    let sent = up.last_body.lock().unwrap().clone().unwrap();
    assert_eq!(sent["model"], "mock-upstream-1");
    assert_eq!(sent["messages"][0], json!({"role": "system", "content": "You are Codex."}));
    assert_eq!(sent["messages"][1], json!({"role": "user", "content": "list files"}));
    assert_eq!(sent["reasoning_effort"], "high");
    assert_eq!(sent["tools"][0]["function"]["name"], "shell");
    assert_eq!(up.last_auth.lock().unwrap().as_deref(), Some("Bearer sk-test-secret"));
}

#[tokio::test]
async fn chat_stream_and_non_stream() {
    let (upstream, _) = start_upstream().await;
    let gw = start_gateway(&upstream).await;
    let client = reqwest::Client::new();

    let resp = client
        .post(format!("{gw}/v1/chat/completions"))
        .json(&json!({"model": "mock-model", "messages": [{"role": "user", "content": "ping"}]}))
        .send()
        .await
        .unwrap();
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["choices"][0]["message"]["content"], "pong");
    assert_eq!(v["usage"]["prompt_tokens"], 3);

    let resp = client
        .post(format!("{gw}/v1/chat/completions"))
        .json(&json!({"model": "mock-model", "stream": true, "stream_options": {"include_usage": true},
                      "messages": [{"role": "user", "content": "ping"}]}))
        .send()
        .await
        .unwrap();
    let events = sse_events(&resp.text().await.unwrap());
    assert_eq!(events.last().unwrap().1, "[DONE]");
    let chunks: Vec<Value> = events[..events.len() - 1].iter().map(|(_, d)| serde_json::from_str(d).unwrap()).collect();
    assert!(chunks.iter().any(|c| c["choices"][0]["delta"]["reasoning_content"] == "Thinking"));
    assert!(chunks.iter().any(|c| c["choices"][0]["finish_reason"] == "tool_calls"));
    assert_eq!(chunks.last().unwrap()["usage"]["completion_tokens"], 8);
}

#[tokio::test]
async fn upstream_errors_are_sanitized_and_mapped() {
    let (upstream, _) = start_upstream().await;
    let gw = start_gateway(&upstream).await;
    let resp = reqwest::Client::new()
        .post(format!("{gw}/v1/responses"))
        .json(&json!({"model": "failing", "input": "x", "stream": true}))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 429);
    assert_eq!(resp.headers()["retry-after"], "7");
    let v: Value = resp.json().await.unwrap();
    assert_eq!(v["error"]["code"], "rate_limit_exceeded");
    let msg = v["error"]["message"].as_str().unwrap();
    assert!(!msg.contains("sk-test-secret"), "{msg}");
    assert!(msg.contains("provider `mock`"), "{msg}");
}
