//! The `openai-chat` adapter: one implementation for every provider that speaks
//! OpenAI Chat Completions (DeepSeek, OpenRouter, Groq, Ollama, vLLM, ...).

use std::collections::HashSet;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{ACCEPT, CONTENT_TYPE};
use serde_json::Value;
use owo_core::{ModelError, ModelEvent, ModelEventStream, ModelRequest};
use owo_protocol_openai_chat::upstream::{self, ChatStreamDecoder, EncodeOptions};
pub use owo_provider_http::AdapterSettings;
use owo_provider_http::{self as http, StreamDecoder};
use owo_registry::Model;
use owo_routing::{DiscoveredModel, ProviderAccess, ProviderAdapter};
use owo_sse::SseEvent;

pub const ADAPTER_KIND: &str = "openai-chat";

pub struct OpenAiChatAdapter {
    http: reqwest::Client,
    settings: AdapterSettings,
}

impl OpenAiChatAdapter {
    pub fn new(settings: AdapterSettings) -> Result<Self, reqwest::Error> {
        Ok(Self { http: http::client(&settings)?, settings })
    }
}

struct ChatSse(ChatStreamDecoder);

impl StreamDecoder for ChatSse {
    fn push(&mut self, event: &SseEvent) -> Vec<ModelEvent> {
        self.0.push_data(&event.data)
    }

    fn finish(&mut self) -> Vec<ModelEvent> {
        self.0.finish(false)
    }

    fn is_ended(&self) -> bool {
        self.0.is_ended()
    }
}

#[async_trait]
impl ProviderAdapter for OpenAiChatAdapter {
    fn kind(&self) -> &'static str {
        ADAPTER_KIND
    }

    async fn execute(
        &self,
        access: &ProviderAccess,
        model: &Model,
        request: ModelRequest,
    ) -> Result<ModelEventStream, ModelError> {
        let provider_id = access.provider.id.clone();
        let options = EncodeOptions { send_reasoning_effort: !model.reasoning_efforts.is_empty(), ..Default::default() };
        let encoded = upstream::encode_request(&request, &model.upstream_model, &options)
            .map_err(|e| e.with_provider(&provider_id))?;
        for note in &encoded.notes {
            if note.starts_with(upstream::WARN_NOTE_PREFIX) {
                tracing::warn!(request_id = %request.request_id, provider = %provider_id, "{note}");
            } else {
                tracing::debug!(request_id = %request.request_id, provider = %provider_id, "{note}");
            }
        }

        let url = http::endpoint(access, "chat/completions");
        let mut headers = http::headers(access)?;
        headers.insert(CONTENT_TYPE, "application/json".parse().expect("static header"));
        if request.stream {
            headers.insert(ACCEPT, "text/event-stream".parse().expect("static header"));
        }
        let mut builder = self.http.post(url).headers(headers).json(&encoded.body);
        if !request.stream {
            builder = builder.timeout(self.settings.response_timeout);
        }
        let resp = builder.send().await.map_err(|e| http::transport_error(&provider_id, e))?;
        if !resp.status().is_success() {
            return Err(http::status_error(access, resp).await);
        }

        if request.stream {
            return Ok(http::event_stream(
                resp,
                ChatSse(ChatStreamDecoder::new(model.upstream_model.clone(), encoded.custom_tools)),
                self.settings.stream_idle_timeout,
                access.clone(),
            ));
        }

        let body: Value = resp
            .json()
            .await
            .map_err(|_| ModelError::upstream_invalid(format!("provider `{provider_id}` returned invalid JSON")))?;
        let response = upstream::decode_response(&body, &model.upstream_model, &encoded.custom_tools)
            .map_err(|e| e.with_provider(&provider_id))?;
        Ok(Box::pin(futures::stream::iter(response.into_events())))
    }

    async fn discover_models(&self, access: &ProviderAccess) -> Result<Vec<DiscoveredModel>, ModelError> {
        let provider_id = &access.provider.id;
        let resp = self
            .http
            .get(http::endpoint(access, "models"))
            .headers(http::headers(access)?)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| http::transport_error(provider_id, e))?;
        if !resp.status().is_success() {
            return Err(http::status_error(access, resp).await);
        }
        let body: Value = resp
            .json()
            .await
            .map_err(|_| ModelError::upstream_invalid(format!("provider `{provider_id}` returned invalid JSON")))?;
        parse_model_list(&body).ok_or_else(|| {
            ModelError::upstream_invalid(format!("provider `{provider_id}` returned an unrecognized model list"))
        })
    }
}

/// Parses `{"data": [...]}` (OpenAI) or `{"models": [...]}` (some gateways).
fn parse_model_list(body: &Value) -> Option<Vec<DiscoveredModel>> {
    let rows = body.get("data").or_else(|| body.get("models")).and_then(Value::as_array)?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for row in rows {
        let Some(id) = row.get("id").and_then(Value::as_str).filter(|s| !s.is_empty()) else { continue };
        if !seen.insert(id.to_string()) {
            continue;
        }
        let context_window = ["context_length", "context_window", "max_context_length"]
            .iter()
            .find_map(|k| row.get(*k).and_then(Value::as_u64))
            .and_then(|v| u32::try_from(v).ok());
        out.push(DiscoveredModel {
            id: id.to_string(),
            display_name: row.get("name").and_then(Value::as_str).map(str::to_string),
            context_window,
            owned_by: row.get("owned_by").and_then(Value::as_str).map(str::to_string),
        });
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_model_lists() {
        let models = parse_model_list(&json!({
            "data": [
                {"id": "a", "owned_by": "x"},
                {"id": "b", "name": "Model B", "context_length": 128000},
                {"id": "a"},
                {"name": "no id"}
            ]
        }))
        .unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[1].context_window, Some(128000));
        assert_eq!(models[1].display_name.as_deref(), Some("Model B"));
        assert!(parse_model_list(&json!({"object": "list"})).is_none());
    }
}
