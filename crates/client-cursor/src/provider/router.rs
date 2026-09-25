//! Routes Cursor model requests for OwO AI Gateway-provided models to the OwO AI Gateway backend.
use std::{sync::Arc, time::Duration};

use async_stream::try_stream;
use futures_util::StreamExt;
use tokio_util::sync::CancellationToken;

use crate::{
    model::{ModelInvocation, ModelLatency, NewLlmCall, ProviderType},
    store::Store,
    Error, Result,
};

use super::{normalize::NormalizedProvider, recorder::CancelOnDrop, CallRecorder, Provider, ProviderStream};

/// Recorded as the request URL of OwO AI Gateway-served calls.
pub const OWO_REQUEST_URL: &str = "owo://router";

pub struct ProviderRouter {
    store: Store,
    backend: Arc<dyn Provider>,
    request_timeout: Duration,
    stream_idle_timeout: Duration,
}

impl ProviderRouter {
    /// `backend` serves every model in the store (all of them are synced from OwO AI Gateway's registry).
    pub fn new(store: Store, backend: Arc<dyn Provider>, request_timeout: Duration, stream_idle_timeout: Duration) -> Self {
        Self { store, backend: Arc::new(NormalizedProvider::new(backend)), request_timeout, stream_idle_timeout }
    }
}

impl Provider for ProviderRouter {
    fn stream(&self, invocation: ModelInvocation, cancellation: CancellationToken) -> ProviderStream {
        let store = self.store.clone();
        let backend = self.backend.clone();
        let request_timeout = self.request_timeout;
        let stream_idle_timeout = self.stream_idle_timeout;
        Box::pin(try_stream! {
            let selected = invocation.request.model.model_id.clone();
            let model = store.model(&selected).await?.ok_or_else(|| Error::Provider(format!("unknown model: {selected}")))?;
            let mut routed = invocation.clone();
            model.configure(&mut routed.request.model);
            if routed.request.model.max_output_tokens.is_none() {
                routed.request.model.max_output_tokens = model.max_output_tokens();
            }
            routed.request.model.model_id = model.model_id.clone();
            let recorder = start_recorder(&store, &invocation, &model.model_hash, &model.display_name, &model.model_id).await?;
            let _cancel_on_drop: CancelOnDrop = recorder.cancel_on_drop();
            let mut stream = backend.stream(routed, cancellation.clone());

            let stream_started = std::time::Instant::now();
            let mut last_event_time = std::time::Instant::now();
            let mut event_count: u64 = 0;
            loop {
                let event = match next_provider_event(&mut stream, stream_idle_timeout).await {
                    Ok(Some(event)) => event,
                    Ok(None) => break,
                    Err(_) => {
                        let error = stream_idle_timeout_error(stream_idle_timeout);
                        tracing::warn!(
                            error = %error,
                            elapsed_ms = stream_started.elapsed().as_millis() as u64,
                            event_count,
                            "provider stream idle timeout"
                        );
                        Err(error)
                    }
                };
                let now = std::time::Instant::now();
                let gap_ms = now.duration_since(last_event_time).as_millis() as u64;
                event_count += 1;
                match event {
                    Ok(event) => {
                        if gap_ms > 5000 {
                            tracing::debug!(gap_ms, event = event_name(&event), event_count, "slow gap between provider events");
                        }
                        recorder.event(&event).await?;
                        last_event_time = now;
                        yield event;
                    }
                    Err(error) => {
                        let error = normalize_provider_stream_error(error, request_timeout);
                        tracing::debug!(error = %error, gap_ms, event_count, "provider stream error");
                        recorder.failed(&error).await?;
                        Err(error)?;
                    }
                }
            }
            finish_stream(&recorder, &cancellation).await?;
        })
    }
}

fn event_name(event: &super::ModelEvent) -> &'static str {
    match event {
        super::ModelEvent::Start { .. } => "Start",
        super::ModelEvent::TextStart => "TextStart",
        super::ModelEvent::TextDelta(_) => "TextDelta",
        super::ModelEvent::TextEnd => "TextEnd",
        super::ModelEvent::ThinkingStart => "ThinkingStart",
        super::ModelEvent::ThinkingDelta(_) => "ThinkingDelta",
        super::ModelEvent::ThinkingEnd => "ThinkingEnd",
        super::ModelEvent::ToolCallStart { .. } => "ToolCallStart",
        super::ModelEvent::ToolCallArgumentsDelta { .. } => "ToolCallArgsDelta",
        super::ModelEvent::ToolCallEnd { .. } => "ToolCallEnd",
        super::ModelEvent::ProviderReplayState(_) => "ReplayState",
        super::ModelEvent::Usage(_) => "Usage",
        super::ModelEvent::Done(_) => "Done",
    }
}

async fn start_recorder(
    store: &Store,
    invocation: &ModelInvocation,
    model_hash: &str,
    display_name: &str,
    model_id: &str,
) -> Result<CallRecorder> {
    CallRecorder::start(
        store.clone(),
        NewLlmCall {
            call_id: invocation.call_id.clone(),
            run_id: invocation.run_id.clone(),
            conversation_id: invocation.conversation_id.clone(),
            provider_call_index: invocation.provider_call_index.min(i64::MAX as u64) as i64,
            model_hash: model_hash.into(),
            provider_type: ProviderType::Owo,
            provider_url: OWO_REQUEST_URL.into(),
            request_type: ProviderType::Owo,
            request_url: OWO_REQUEST_URL.into(),
            model_id: model_id.into(),
            display_name: display_name.into(),
            reasoning_effort: invocation.request.model.reasoning.effort.clone(),
            fast: invocation.request.model.latency == ModelLatency::Fast,
            message_count: invocation.request.history.len(),
            tool_count: invocation.request.prompt.tools.len(),
            detailed: false,
        },
    )
    .await
}

async fn finish_stream(recorder: &CallRecorder, cancellation: &CancellationToken) -> Result<()> {
    if recorder.is_finished() {
        return Ok(());
    }
    if cancellation.is_cancelled() {
        recorder.cancelled().await
    } else {
        let error = Error::Provider("provider stream ended without Done".into());
        recorder.failed(&error).await?;
        Err(error)
    }
}

async fn next_provider_event(
    stream: &mut ProviderStream,
    idle_timeout: Duration,
) -> std::result::Result<Option<Result<super::ModelEvent>>, tokio::time::error::Elapsed> {
    tokio::time::timeout(idle_timeout, stream.next()).await
}

fn stream_idle_timeout_error(idle_timeout: Duration) -> Error {
    Error::Provider(format!(
        "provider stream idle timeout: no events received for {} seconds ({} minutes)",
        idle_timeout.as_secs(),
        idle_timeout.as_secs() / 60
    ))
}

fn request_timeout_error(request_timeout: Duration) -> Error {
    Error::Provider(format!(
        "provider request timed out after {} seconds ({} minutes)",
        request_timeout.as_secs(),
        request_timeout.as_secs() / 60
    ))
}

fn normalize_provider_stream_error(error: Error, request_timeout: Duration) -> Error {
    match error {
        Error::Http(source) if source.is_timeout() => request_timeout_error(request_timeout),
        Error::Http(source) if source.is_body() => Error::Provider(format!(
            "provider stream transport failed while reading the response body: {}",
            root_error_message(&source)
        )),
        error => error,
    }
}

fn root_error_message(error: &(dyn std::error::Error + 'static)) -> String {
    let mut current = error;
    while let Some(source) = current.source() {
        current = source;
    }
    current.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pending_provider_event_hits_the_idle_timeout() {
        let mut stream: ProviderStream = Box::pin(futures_util::stream::pending());
        let result = next_provider_event(&mut stream, Duration::from_millis(1)).await;
        assert!(result.is_err());
    }

    #[test]
    fn timeout_errors_state_the_boundary_and_duration() {
        let Error::Provider(idle) = stream_idle_timeout_error(Duration::from_secs(30 * 60)) else {
            panic!("idle timeout must be a provider error");
        };
        assert_eq!(idle, "provider stream idle timeout: no events received for 1800 seconds (30 minutes)");
    }
}
