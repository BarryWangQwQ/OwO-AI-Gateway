//! Routing: resolve a canonical request to a provider and hand it to that
//! provider's adapter. Inbound protocols never talk to providers directly.

mod adapter;
mod preflight;
mod record;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use owo_core::{ErrorKind, ModelError, ModelEventStream, ModelRequest};
use owo_credentials::CredentialStore;
use owo_registry::{Provider, Registry, Target};

pub use adapter::{DiscoveredModel, ProviderAccess, ProviderAdapter};
pub use preflight::preflight;
pub use record::{CallError, CallRecord, CallSink, CallStatus};

pub struct RoutedStream {
    pub target: Target,
    pub events: ModelEventStream,
}

pub struct Router {
    registry: Arc<Registry>,
    credentials: CredentialStore,
    adapters: HashMap<&'static str, Arc<dyn ProviderAdapter>>,
    sink: Option<Arc<dyn CallSink>>,
}

impl Router {
    pub fn new(registry: Arc<Registry>, credentials: CredentialStore, adapters: Vec<Arc<dyn ProviderAdapter>>) -> Self {
        let adapters = adapters.into_iter().map(|a| (a.kind(), a)).collect();
        Self { registry, credentials, adapters, sink: None }
    }

    /// Records every call made through [`Router::execute`] into `sink`.
    pub fn with_sink(mut self, sink: Arc<dyn CallSink>) -> Self {
        self.sink = Some(sink);
        self
    }

    pub fn registry(&self) -> &Arc<Registry> {
        &self.registry
    }

    pub fn adapter_kinds(&self) -> Vec<&'static str> {
        let mut kinds: Vec<_> = self.adapters.keys().copied().collect();
        kinds.sort_unstable();
        kinds
    }

    /// Resolves the provider's credential. Error messages name the reference, never the value.
    pub fn access(&self, provider: &Arc<Provider>) -> Result<ProviderAccess, ModelError> {
        let secret = self.credentials.resolve(&provider.api_key).map_err(|e| {
            ModelError::new(
                ErrorKind::AuthenticationFailed,
                format!("credential for provider `{}` is unavailable: {e}", provider.id),
            )
            .with_provider(&provider.id)
        })?;
        Ok(ProviderAccess { provider: provider.clone(), secret })
    }

    fn adapter_for(&self, provider: &Provider) -> Result<&Arc<dyn ProviderAdapter>, ModelError> {
        self.adapters.get(provider.adapter.as_str()).ok_or_else(|| {
            ModelError::new(
                ErrorKind::ProviderUnavailable,
                format!("adapter `{}` for provider `{}` is not available", provider.adapter, provider.id),
            )
        })
    }

    pub async fn execute(&self, request: ModelRequest) -> Result<RoutedStream, ModelError> {
        let Some(sink) = &self.sink else {
            return self.route(request).await;
        };
        let started = Instant::now();
        let record = CallRecord::begin(&request);
        let target = match self.resolve(&request) {
            Ok(target) => target,
            Err(error) => {
                sink.record(record.failed(&error, started));
                return Err(error);
            }
        };
        let record = record.routed_to(&target);
        match self.dispatch(target, request).await {
            Ok(RoutedStream { target, events }) => {
                Ok(RoutedStream { events: record::Tracked::wrap(events, record, &target, sink.clone(), started), target })
            }
            Err(error) => {
                sink.record(record.failed(&error, started));
                Err(error)
            }
        }
    }

    fn resolve(&self, request: &ModelRequest) -> Result<Target, ModelError> {
        self.registry.resolve(request.model.as_str(), request.metadata.client.as_deref())
    }

    async fn route(&self, request: ModelRequest) -> Result<RoutedStream, ModelError> {
        let target = self.resolve(&request)?;
        self.dispatch(target, request).await
    }

    async fn dispatch(&self, target: Target, mut request: ModelRequest) -> Result<RoutedStream, ModelError> {
        preflight(&mut request, &target.model)?;
        let adapter = self.adapter_for(&target.provider)?;
        let access = self.access(&target.provider)?;
        tracing::debug!(
            request_id = %request.request_id,
            model = %target.model.id,
            provider = %target.provider.id,
            "routing request"
        );
        let events = adapter.execute(&access, &target.model, request).await?;
        Ok(RoutedStream { target, events })
    }

    pub async fn discover(&self, provider_id: &str) -> Result<Vec<DiscoveredModel>, ModelError> {
        let provider = self.registry.provider(provider_id).ok_or_else(|| {
            ModelError::new(ErrorKind::ConfigurationError, format!("provider `{provider_id}` is not configured"))
        })?;
        let adapter = self.adapter_for(provider)?;
        let access = self.access(provider)?;
        adapter.discover_models(&access).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::StreamExt;
    use owo_config::Config;
    use owo_core::{Message, ModelEvent, ModelResponse, StopReason};
    use owo_credentials::CredentialBackend;
    use owo_registry::{Model, PresetCatalog};

    struct Echo;

    #[async_trait::async_trait]
    impl ProviderAdapter for Echo {
        fn kind(&self) -> &'static str {
            "openai-chat"
        }

        async fn execute(
            &self,
            access: &ProviderAccess,
            model: &Model,
            request: ModelRequest,
        ) -> Result<ModelEventStream, ModelError> {
            if request.metadata.user.as_deref() == Some("fail") {
                let mut error = ModelError::new(ErrorKind::RateLimited, "slow down: https://x.test/v1?key=SECRET");
                error.upstream_status = Some(429);
                return Ok(Box::pin(futures::stream::iter([
                    ModelEvent::ResponseStart { id: "x".into(), model: "qwen".into() },
                    ModelEvent::Error(error),
                ])));
            }
            let text = format!(
                "{}|{}|{:?}",
                access.provider.id,
                model.upstream_model,
                request.reasoning.and_then(|r| r.effort)
            );
            let resp = ModelResponse {
                id: "x".into(),
                model: model.upstream_model.clone(),
                content: vec![owo_core::ContentBlock::text(text)],
                stop_reason: StopReason::EndTurn,
                usage: Some(owo_core::Usage { input_tokens: 10, output_tokens: 3, ..Default::default() }),
            };
            Ok(Box::pin(futures::stream::iter(resp.into_events())))
        }

        async fn discover_models(&self, _: &ProviderAccess) -> Result<Vec<DiscoveredModel>, ModelError> {
            Ok(vec![])
        }
    }

    fn router() -> Router {
        let (config, _) = Config::from_toml_str(
            r#"
version = 1
[providers.local]
preset = "ollama"
[providers.keyed]
preset = "deepseek"
api_key = "env:OWO_TEST_UNSET_KEY_FOR_ROUTING"
[[models]]
id = "m"
provider = "local"
upstream_model = "qwen"
reasoning_efforts = ["low", "high"]
default_reasoning_effort = "low"
price = { input = 2, output = 10 }
[[models]]
id = "k"
provider = "keyed"
upstream_model = "deepseek-chat"
"#,
            "t",
        )
        .unwrap();
        let (registry, _) = Registry::build(&config, PresetCatalog::builtin(), &["openai-chat"]).unwrap();
        Router::new(Arc::new(registry), CredentialStore::new(CredentialBackend::Env), vec![Arc::new(Echo)])
    }

    async fn text_of(stream: ModelEventStream) -> String {
        stream
            .filter_map(|e| async move {
                match e {
                    ModelEvent::TextDelta { text } => Some(text),
                    _ => None,
                }
            })
            .collect::<Vec<_>>()
            .await
            .concat()
    }

    #[tokio::test]
    async fn routes_with_default_reasoning_effort() {
        let r = router();
        let mut req = ModelRequest::new("1", "m");
        req.messages.push(Message::user_text("hi"));
        let routed = r.execute(req).await.unwrap();
        assert_eq!(text_of(routed.events).await, "local|qwen|Some(\"low\")");
    }

    #[tokio::test]
    async fn missing_credential_is_authentication_failure() {
        let r = router();
        let err = r.execute(ModelRequest::new("1", "k")).await.err().unwrap();
        assert_eq!(err.kind, ErrorKind::AuthenticationFailed);
        assert!(err.message.contains("OWO_TEST_UNSET_KEY_FOR_ROUTING"));
    }

    #[tokio::test]
    async fn unknown_model() {
        let err = router().execute(ModelRequest::new("1", "nope")).await.err().unwrap();
        assert_eq!(err.kind, ErrorKind::ModelNotFound);
    }

    #[derive(Default)]
    struct Collect(std::sync::Mutex<Vec<CallRecord>>);

    impl CallSink for Collect {
        fn record(&self, call: CallRecord) {
            self.0.lock().unwrap().push(call);
        }
    }

    fn recording_router() -> (Router, Arc<Collect>) {
        let sink = Arc::new(Collect::default());
        (router().with_sink(sink.clone()), sink)
    }

    fn request(model: &str, client: &str) -> ModelRequest {
        let mut req = ModelRequest::new("r1", model);
        req.metadata.client = Some(client.into());
        req.messages.push(Message::user_text("hi"));
        req
    }

    #[tokio::test]
    async fn records_a_finished_call_with_its_usage() {
        let (r, sink) = recording_router();
        let routed = r.execute(request("m", "codex")).await.unwrap();
        text_of(routed.events).await;
        let calls = sink.0.lock().unwrap();
        let call = &calls[0];
        assert_eq!(calls.len(), 1);
        assert_eq!(call.status, CallStatus::Ok);
        assert_eq!((call.client.as_deref(), call.model.as_deref(), call.provider.as_deref()), (Some("codex"), Some("m"), Some("local")));
        assert_eq!(call.upstream_model.as_deref(), Some("qwen"));
        assert_eq!(call.usage.map(|u| (u.input_tokens, u.output_tokens)), Some((10, 3)));
        assert!((call.cost_usd.unwrap() - (10.0 * 2.0 + 3.0 * 10.0) / 1e6).abs() < 1e-12);
        assert_eq!(call.stop_reason.as_deref(), Some("end_turn"));
        assert!(call.first_token_ms.is_some());
    }

    #[tokio::test]
    async fn records_calls_that_fail_before_or_during_the_stream() {
        let (r, sink) = recording_router();
        assert!(r.execute(request("nope", "claude_code")).await.is_err());
        let mut failing = request("m", "claude_code");
        failing.metadata.user = Some("fail".into());
        text_of(r.execute(failing).await.unwrap().events).await;

        let calls = sink.0.lock().unwrap();
        assert_eq!(calls[0].status, CallStatus::Error);
        assert_eq!(calls[0].model, None);
        assert_eq!(calls[0].error.as_ref().unwrap().kind, "model_not_found");
        let stream_error = calls[1].error.as_ref().unwrap();
        assert_eq!((calls[1].status, stream_error.upstream_status), (CallStatus::Error, Some(429)));
        assert!(!stream_error.message.contains("SECRET"), "{}", stream_error.message);
    }

    #[tokio::test]
    async fn a_call_failing_after_resolution_keeps_its_model() {
        let (r, sink) = recording_router();
        assert!(r.execute(request("k", "codex")).await.is_err());
        let calls = sink.0.lock().unwrap();
        assert_eq!((calls[0].model.as_deref(), calls[0].provider.as_deref()), (Some("k"), Some("keyed")));
        assert_eq!(calls[0].error.as_ref().unwrap().kind, "authentication_failed");
    }

    #[tokio::test]
    async fn a_stream_dropped_early_is_cancelled() {
        let (r, sink) = recording_router();
        let mut events = r.execute(request("m", "cursor")).await.unwrap().events;
        events.next().await;
        drop(events);
        assert_eq!(sink.0.lock().unwrap()[0].status, CallStatus::Cancelled);
    }
}
