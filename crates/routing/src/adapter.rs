use std::sync::Arc;

use async_trait::async_trait;
use owo_core::{ModelError, ModelEventStream, ModelRequest};
use owo_credentials::Secret;
use owo_registry::{Model, Provider};

/// A provider plus its resolved credential, handed to an adapter per call.
#[derive(Debug, Clone)]
pub struct ProviderAccess {
    pub provider: Arc<Provider>,
    pub secret: Option<Secret>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModel {
    pub id: String,
    pub display_name: Option<String>,
    pub context_window: Option<u32>,
    pub owned_by: Option<String>,
}

/// One implementation per upstream wire protocol (`openai-chat`, `anthropic`, ...).
/// Providers are data; an adapter serves every provider that speaks its protocol.
#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    /// The adapter kind referenced by `providers.<id>.adapter`.
    fn kind(&self) -> &'static str;

    /// Sends the request upstream. Errors before the first event (auth, HTTP status)
    /// are returned as `Err`; later failures arrive as [`owo_core::ModelEvent::Error`].
    async fn execute(
        &self,
        access: &ProviderAccess,
        model: &Model,
        request: ModelRequest,
    ) -> Result<ModelEventStream, ModelError>;

    async fn discover_models(&self, access: &ProviderAccess) -> Result<Vec<DiscoveredModel>, ModelError>;
}
