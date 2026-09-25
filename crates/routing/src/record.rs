//! One record per routed call: who asked for which model, what happened, and the tokens
//! it used. Prompts and responses are never part of a record.

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use futures::Stream;
use owo_config::Price;
use owo_core::{ModelError, ModelEvent, ModelEventStream, ModelRequest, StopReason, Usage};
use owo_registry::Target;

/// Where finished call records go. Must not block: it runs on the request path.
pub trait CallSink: Send + Sync {
    fn record(&self, call: CallRecord);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallStatus {
    Ok,
    Error,
    /// The client stopped reading before the response finished.
    Cancelled,
}

impl CallStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            CallStatus::Ok => "ok",
            CallStatus::Error => "error",
            CallStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    pub kind: String,
    pub message: String,
    pub upstream_status: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct CallRecord {
    pub started_at_ms: i64,
    pub request_id: String,
    /// Client integration id (`codex`, `claude_code`, `cursor`, ...), when known.
    pub client: Option<String>,
    /// The model id as the client asked for it, after client aliases.
    pub requested_model: String,
    pub model: Option<String>,
    pub provider: Option<String>,
    pub upstream_model: Option<String>,
    pub stream: bool,
    pub status: CallStatus,
    pub error: Option<CallError>,
    pub duration_ms: u64,
    pub first_token_ms: Option<u64>,
    pub usage: Option<Usage>,
    /// Estimated cost in US dollars, when the model has a price and the call reported usage.
    pub cost_usd: Option<f64>,
    pub stop_reason: Option<String>,
}

impl CallRecord {
    pub(crate) fn begin(request: &ModelRequest) -> Self {
        let started_at_ms =
            SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or_default();
        Self {
            started_at_ms,
            request_id: request.request_id.clone(),
            client: request.metadata.client.clone(),
            requested_model: request.model.as_str().to_string(),
            model: None,
            provider: None,
            upstream_model: None,
            stream: request.stream,
            status: CallStatus::Ok,
            error: None,
            duration_ms: 0,
            first_token_ms: None,
            usage: None,
            cost_usd: None,
            stop_reason: None,
        }
    }

    pub(crate) fn routed_to(mut self, target: &Target) -> Self {
        self.model = Some(target.model.id.clone());
        self.provider = Some(target.provider.id.clone());
        self.upstream_model = Some(target.model.upstream_model.clone());
        self
    }

    pub(crate) fn failed(mut self, error: &ModelError, started: Instant) -> Self {
        self.status = CallStatus::Error;
        self.error = Some(call_error(error));
        self.duration_ms = elapsed_ms(started);
        self
    }
}

fn call_error(error: &ModelError) -> CallError {
    CallError { kind: error.kind.as_str().to_string(), message: sanitize(&error.message), upstream_status: error.upstream_status }
}

fn elapsed_ms(started: Instant) -> u64 {
    started.elapsed().as_millis().min(u64::MAX as u128) as u64
}

const MAX_ERROR_CHARS: usize = 500;

/// Drops URL query strings (a provider with `auth = "query"` carries its key there) and
/// caps the length.
fn sanitize(message: &str) -> String {
    let mut out = String::with_capacity(message.len().min(MAX_ERROR_CHARS));
    let mut rest = message;
    while let Some(start) = [rest.find("http://"), rest.find("https://")].into_iter().flatten().min() {
        let (before, url) = rest.split_at(start);
        out.push_str(before);
        let end = url.find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | ')' | '>')).unwrap_or(url.len());
        let (url, after) = url.split_at(end);
        out.push_str(url.split(['?', '#']).next().unwrap_or(url));
        rest = after;
    }
    out.push_str(rest);
    if out.chars().count() > MAX_ERROR_CHARS {
        out = out.chars().take(MAX_ERROR_CHARS).collect::<String>() + "…";
    }
    out
}

fn stop_reason(reason: &StopReason) -> String {
    match reason {
        StopReason::EndTurn => "end_turn".into(),
        StopReason::MaxTokens => "max_tokens".into(),
        StopReason::ToolUse => "tool_use".into(),
        StopReason::StopSequence => "stop_sequence".into(),
        StopReason::ContentFilter => "content_filter".into(),
        StopReason::Refusal => "refusal".into(),
        StopReason::Other(other) => other.clone(),
    }
}

/// Passes events through unchanged and records the call once it ends: at `ResponseEnd`,
/// when the stream runs out, or when the consumer drops it early.
pub(crate) struct Tracked {
    inner: ModelEventStream,
    record: Option<CallRecord>,
    sink: Arc<dyn CallSink>,
    started: Instant,
    price: Option<Price>,
    error: Option<ModelError>,
}

impl Tracked {
    pub(crate) fn wrap(inner: ModelEventStream, record: CallRecord, target: &Target, sink: Arc<dyn CallSink>, started: Instant) -> ModelEventStream {
        Box::pin(Self { inner, record: Some(record), sink, started, price: target.model.price, error: None })
    }

    fn observe(&mut self, event: &ModelEvent) {
        let Some(record) = self.record.as_mut() else { return };
        match event {
            ModelEvent::TextDelta { .. } | ModelEvent::ReasoningDelta { .. } | ModelEvent::ToolCallStart { .. } => {
                record.first_token_ms.get_or_insert_with(|| elapsed_ms(self.started));
            }
            ModelEvent::Usage(usage) => match &mut record.usage {
                Some(total) => total.merge(usage),
                None => record.usage = Some(*usage),
            },
            ModelEvent::MessageEnd { stop_reason: reason } => record.stop_reason = Some(stop_reason(reason)),
            ModelEvent::Error(error) => self.error = Some(error.clone()),
            ModelEvent::ResponseEnd => self.finish(false),
            _ => {}
        }
    }

    fn finish(&mut self, dropped: bool) {
        let Some(mut record) = self.record.take() else { return };
        record.duration_ms = elapsed_ms(self.started);
        record.cost_usd = match (&self.price, &record.usage) {
            (Some(price), Some(u)) => Some(price.cost(
                u.input_tokens,
                u.cached_input_tokens.unwrap_or(0),
                u.cache_creation_input_tokens.unwrap_or(0),
                u.output_tokens,
            )),
            _ => None,
        };
        record.status = match &self.error {
            Some(error) => {
                record.error = Some(call_error(error));
                CallStatus::Error
            }
            None if dropped && record.stop_reason.is_none() => CallStatus::Cancelled,
            None => CallStatus::Ok,
        };
        self.sink.record(record);
    }
}

impl Stream for Tracked {
    type Item = ModelEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<ModelEvent>> {
        let polled = self.inner.as_mut().poll_next(cx);
        match &polled {
            Poll::Ready(Some(event)) => self.observe(event),
            Poll::Ready(None) => self.finish(false),
            Poll::Pending => {}
        }
        polled
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        self.finish(true);
    }
}

#[cfg(test)]
mod tests {
    use super::sanitize;

    #[test]
    fn sanitize_drops_query_strings() {
        let m = sanitize("request to https://api.example.com/v1/chat?key=SECRET&x=1 failed (see http://a.b/c#frag)");
        assert_eq!(m, "request to https://api.example.com/v1/chat failed (see http://a.b/c)");
    }

    #[test]
    fn sanitize_caps_length() {
        assert_eq!(sanitize(&"x".repeat(900)).chars().count(), 501);
    }
}
