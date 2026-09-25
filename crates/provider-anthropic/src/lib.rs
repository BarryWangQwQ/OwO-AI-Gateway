//! The `anthropic` adapter: Anthropic's Messages API and Messages-compatible endpoints.
//! Requests are always streamed upstream; non-streaming clients get the folded result.

use std::time::Duration;

use async_trait::async_trait;
use reqwest::header::{HeaderValue, ACCEPT, CONTENT_TYPE};
use serde_json::Value;
use owo_core::{ModelError, ModelEvent, ModelEventStream, ModelRequest};
use owo_protocol_anthropic::decode::AnthropicStreamDecoder;
use owo_protocol_anthropic::encode::{self, EncodeOptions, WARN_NOTE_PREFIX};
pub use owo_provider_http::AdapterSettings;
use owo_provider_http::{self as http, StreamDecoder};
use owo_registry::Model;
use owo_routing::{DiscoveredModel, ProviderAccess, ProviderAdapter};
use owo_sse::SseEvent;
use url::Url;

pub const ADAPTER_KIND: &str = "anthropic";
const API_VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 32_000;

pub struct AnthropicAdapter {
    http: reqwest::Client,
    settings: AdapterSettings,
}

impl AnthropicAdapter {
    pub fn new(settings: AdapterSettings) -> Result<Self, reqwest::Error> {
        Ok(Self { http: http::client(&settings)?, settings })
    }
}

struct MessagesSse(AnthropicStreamDecoder);

impl StreamDecoder for MessagesSse {
    fn push(&mut self, event: &SseEvent) -> Vec<ModelEvent> {
        self.0.push(&event.data)
    }

    fn finish(&mut self) -> Vec<ModelEvent> {
        self.0.finish()
    }

    fn is_ended(&self) -> bool {
        self.0.is_ended()
    }
}

/// `https://api.anthropic.com` → `v1/<tail>`; a base already ending in `/v1` → `<tail>`.
fn resource(base: &Url, tail: &str) -> String {
    if base.path().trim_end_matches('/').ends_with("/v1") {
        tail.to_string()
    } else {
        format!("v1/{tail}")
    }
}

fn headers(access: &ProviderAccess) -> Result<reqwest::header::HeaderMap, ModelError> {
    let mut h = http::headers(access)?;
    if !h.contains_key("anthropic-version") {
        h.insert("anthropic-version", HeaderValue::from_static(API_VERSION));
    }
    Ok(h)
}

#[async_trait]
impl ProviderAdapter for AnthropicAdapter {
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
        let options = EncodeOptions {
            default_max_tokens: model.max_output_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            prompt_caching: true,
            thinking: !model.reasoning_efforts.is_empty(),
        };
        let encoded = encode::encode_request(&request, &model.upstream_model, &options)
            .map_err(|e| e.with_provider(&provider_id))?;
        for note in &encoded.notes {
            if note.starts_with(WARN_NOTE_PREFIX) {
                tracing::warn!(request_id = %request.request_id, provider = %provider_id, "{note}");
            } else {
                tracing::debug!(request_id = %request.request_id, provider = %provider_id, "{note}");
            }
        }

        let url = http::endpoint(access, &resource(&access.provider.base_url, "messages"));
        let mut h = headers(access)?;
        h.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        h.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
        let resp = self
            .http
            .post(url)
            .headers(h)
            .json(&encoded.body)
            .send()
            .await
            .map_err(|e| http::transport_error(&provider_id, e))?;
        if !resp.status().is_success() {
            return Err(http::status_error(access, resp).await);
        }
        let decoder = AnthropicStreamDecoder::new(model.upstream_model.clone(), encoded.custom_tools, encoded.tool_names);
        Ok(http::event_stream(resp, MessagesSse(decoder), self.settings.stream_idle_timeout, access.clone()))
    }

    async fn discover_models(&self, access: &ProviderAccess) -> Result<Vec<DiscoveredModel>, ModelError> {
        let provider_id = &access.provider.id;
        let mut url = http::endpoint(access, &resource(&access.provider.base_url, "models"));
        url.query_pairs_mut().append_pair("limit", "1000");
        let resp = self
            .http
            .get(url)
            .headers(headers(access)?)
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
        let rows = body.get("data").and_then(Value::as_array).ok_or_else(|| {
            ModelError::upstream_invalid(format!("provider `{provider_id}` returned an unrecognized model list"))
        })?;
        Ok(rows
            .iter()
            .filter_map(|r| {
                Some(DiscoveredModel {
                    id: r.get("id")?.as_str()?.to_string(),
                    display_name: r.get("display_name").and_then(Value::as_str).map(str::to_string),
                    context_window: None,
                    owned_by: Some("anthropic".into()),
                })
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn messages_path() {
        assert_eq!(resource(&Url::parse("https://api.anthropic.com").unwrap(), "messages"), "v1/messages");
        assert_eq!(resource(&Url::parse("https://relay.example.com/v1/").unwrap(), "messages"), "messages");
        assert_eq!(resource(&Url::parse("https://relay.example.com/claude").unwrap(), "messages"), "v1/messages");
    }
}
