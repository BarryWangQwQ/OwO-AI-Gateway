//! Response helpers.

use std::convert::Infallible;

use axum::body::Body;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures::Stream;
use http::{header, HeaderValue, StatusCode};
use owo_core::{ErrorKind, ModelError};
use owo_protocol_openai_chat::common::error_body;

/// An OpenAI-shaped error response with the canonical status mapping.
pub(crate) fn openai_error(err: &ModelError) -> Response {
    let status = StatusCode::from_u16(err.http_status()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut resp = (status, Json(error_body(err))).into_response();
    if let Some(secs) = err.retry_after_secs {
        if let Ok(v) = HeaderValue::from_str(&secs.to_string()) {
            resp.headers_mut().insert(header::RETRY_AFTER, v);
        }
    }
    resp
}

pub(crate) fn sse(stream: impl Stream<Item = Bytes> + Send + 'static) -> Response {
    use futures::StreamExt;
    let body = Body::from_stream(stream.map(Ok::<_, Infallible>));
    let mut resp = Response::new(body);
    let h = resp.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_static("text/event-stream"));
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    h.insert("x-accel-buffering", HeaderValue::from_static("no"));
    resp
}

pub(crate) async fn not_found() -> Response {
    let mut resp = openai_error(&ModelError::new(ErrorKind::InvalidRequest, "no such endpoint"));
    *resp.status_mut() = StatusCode::NOT_FOUND;
    resp
}
