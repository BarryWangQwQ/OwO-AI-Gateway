use std::collections::VecDeque;
use std::pin::Pin;
use std::time::Duration;

use bytes::Bytes;
use futures::{Stream, StreamExt};
use owo_core::{ErrorKind, ModelError, ModelEvent, ModelEventStream};
use owo_routing::ProviderAccess;
use owo_sse::{SseDecoder, SseEvent};

use crate::errors::{redact, transport_error};

/// A protocol's incremental SSE → canonical event decoder.
pub trait StreamDecoder: Send + 'static {
    fn push(&mut self, event: &SseEvent) -> Vec<ModelEvent>;
    /// Called once when the body ends; must close or fail the response.
    fn finish(&mut self) -> Vec<ModelEvent>;
    /// True once the response is complete (or failed); remaining bytes are not read.
    fn is_ended(&self) -> bool;
}

struct State<D> {
    body: Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    sse: SseDecoder,
    decoder: D,
    pending: VecDeque<ModelEvent>,
    done: bool,
    idle: Duration,
    access: ProviderAccess,
}

impl<D: StreamDecoder> State<D> {
    fn fail(&mut self, err: ModelError) {
        self.pending.push_back(ModelEvent::Error(err.with_provider(&self.access.provider.id)));
        self.done = true;
    }

    fn absorb(&mut self, events: Vec<ModelEvent>) {
        for e in events {
            let e = match e {
                ModelEvent::Error(err) => {
                    let message = redact(&err.message, &self.access);
                    ModelEvent::Error(ModelError { message, ..err }.with_provider(&self.access.provider.id))
                }
                other => other,
            };
            self.pending.push_back(e);
        }
        if self.decoder.is_ended() {
            self.done = true;
        }
    }
}

/// Converts an upstream SSE body into canonical events, incrementally.
/// Dropping the returned stream drops the HTTP body, which cancels the upstream call.
pub fn event_stream<D: StreamDecoder>(
    resp: reqwest::Response,
    decoder: D,
    idle: Duration,
    access: ProviderAccess,
) -> ModelEventStream {
    let state = State {
        body: Box::pin(resp.bytes_stream()),
        sse: SseDecoder::new(),
        decoder,
        pending: VecDeque::new(),
        done: false,
        idle,
        access,
    };
    Box::pin(futures::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(ev) = st.pending.pop_front() {
                return Some((ev, st));
            }
            if st.done {
                return None;
            }
            match tokio::time::timeout(st.idle, st.body.next()).await {
                Ok(Some(Ok(chunk))) => match st.sse.feed(&chunk) {
                    Ok(events) => {
                        for ev in events {
                            let decoded = st.decoder.push(&ev);
                            st.absorb(decoded);
                            if st.done {
                                break;
                            }
                        }
                    }
                    Err(e) => st.fail(ModelError::upstream_invalid(format!("malformed upstream stream: {e}"))),
                },
                Ok(Some(Err(e))) => {
                    let err = transport_error(&st.access.provider.id, e);
                    st.fail(ModelError::new(ErrorKind::ProviderUnavailable, format!("stream interrupted: {}", err.message)));
                }
                Ok(None) => {
                    if let Ok(Some(ev)) = st.sse.finish() {
                        let decoded = st.decoder.push(&ev);
                        st.absorb(decoded);
                    }
                    if !st.decoder.is_ended() {
                        let tail = st.decoder.finish();
                        st.absorb(tail);
                    }
                    st.done = true;
                }
                Err(_) => st.fail(ModelError::new(
                    ErrorKind::Timeout,
                    format!("no data from provider for {}s", st.idle.as_secs()),
                )),
            }
        }
    }))
}
