//! `GET /v1/models` in three shapes, chosen by the caller:
//! - Codex (`?client_version=`): Codex model catalog, same entries as `model_catalog_json`.
//! - Anthropic (`anthropic-version` header): Claude clients list models through this path.
//! - OpenAI list otherwise.

use std::sync::Arc;

use axum::extract::{Path, RawQuery, State};
use axum::response::{IntoResponse, Response};
use axum::Json;
use http::HeaderMap;
use serde_json::{json, Value};
use owo_client_codex::catalog;

use crate::openai::valid_client;
use crate::reply::openai_error;
use crate::AppState;

pub(crate) async fn list_default(State(state): State<Arc<AppState>>, RawQuery(query): RawQuery, headers: HeaderMap) -> Response {
    Json(list(&state, None, query.as_deref(), &headers)).into_response()
}

pub(crate) async fn list_client(
    State(state): State<Arc<AppState>>,
    Path(client): Path<String>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Response {
    match valid_client(client) {
        Ok(client) => Json(list(&state, Some(&client), query.as_deref(), &headers)).into_response(),
        Err(e) => openai_error(&e),
    }
}

fn wants_codex_catalog(query: Option<&str>) -> bool {
    query.is_some_and(|q| q.split('&').any(|pair| pair.split('=').next() == Some("client_version")))
}

fn list(state: &AppState, client: Option<&str>, query: Option<&str>, headers: &HeaderMap) -> Value {
    let registry = state.router.registry();
    if wants_codex_catalog(query) {
        let template = state.codex_template.clone().unwrap_or_else(catalog::builtin_template);
        let desktop = client == Some(owo_client_codex::DESKTOP_CLIENT_ID);
        let models = catalog::catalog_models(registry, client.unwrap_or(owo_client_codex::CLIENT_ID));
        return if !desktop || state.codex_native_aliases.is_empty() {
            catalog::build_catalog(&template, &models)
        } else {
            catalog::build_aliased_catalog(&template, &models, &state.codex_native_aliases)
        };
    }
    let models: Vec<_> = registry.available_models().collect();
    if headers.contains_key("anthropic-version") {
        let claude_code = client == Some(owo_client_claude_code::CLIENT_ID);
        let data: Vec<Value> = models
            .iter()
            .map(|m| {
                let id = m.exposed_id(client);
                json!({
                    "type": "model",
                    "id": if claude_code { owo_client_claude_code::ids::picker_id(id) } else { id.to_string() },
                    "display_name": m.display_name,
                    "created_at": "1970-01-01T00:00:00Z",
                })
            })
            .collect();
        let first = data.first().map(|d| d["id"].clone()).unwrap_or(Value::Null);
        let last = data.last().map(|d| d["id"].clone()).unwrap_or(Value::Null);
        return json!({ "data": data, "has_more": false, "first_id": first, "last_id": last });
    }
    let data: Vec<Value> = models
        .iter()
        .map(|m| {
            let mut v = json!({
                "id": m.exposed_id(client),
                "object": "model",
                "created": 0,
                "owned_by": m.provider,
                "display_name": m.display_name,
            });
            if let Some(ctx) = m.context_window {
                v["context_window"] = json!(ctx);
            }
            v
        })
        .collect();
    json!({ "object": "list", "data": data })
}
