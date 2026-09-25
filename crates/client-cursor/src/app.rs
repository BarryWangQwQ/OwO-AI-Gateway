//! Assembles the Cursor backend and starts it. Model calls are served by OwO AI Gateway's router.
use std::{future::IntoFuture, net::SocketAddr, sync::Arc, time::Duration};

use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::{
    api,
    config::Config,
    cursor::{
        prompting::{PromptAssets, PromptCompiler},
        transport::TransportRegistry,
    },
    local_app::CursorHarness,
    provider::{ProviderRouter, OwoProvider},
    search::WebCache,
    store::Store,
    Result,
};

pub struct App {
    config: Config,
    router: axum::Router,
    registry: TransportRegistry,
    harness: CursorHarness,
    store: Store,
}

impl App {
    pub async fn new(config: Config, owo: Arc<owo_routing::Router>) -> Result<Self> {
        let store = Store::connect(&config.database_url).await?;
        let compiler = PromptCompiler::new(PromptAssets::embedded()?);
        let clients = crate::network::NetworkClients::new(store.clone());
        let provider = Arc::new(ProviderRouter::new(
            store.clone(),
            Arc::new(OwoProvider::new(owo)),
            config.provider_request_timeout,
            config.provider_stream_idle_timeout,
        ));
        let registry = TransportRegistry::with_services(
            store.clone(),
            provider,
            compiler,
            WebCache::managed()?,
            crate::config::managed_data_dir()?.join("rules"),
        );
        let harness = CursorHarness::new(store.clone())?;
        let router = api::router(registry.clone(), clients)?;
        Ok(Self { router, registry, harness, store, config })
    }

    pub fn harness(&self) -> CursorHarness {
        self.harness.clone()
    }

    pub fn store(&self) -> Store {
        self.store.clone()
    }

    pub async fn bind(&self) -> Result<TcpListener> {
        Ok(TcpListener::bind(self.config.listen_addr).await?)
    }

    /// Serves until `shutdown` is cancelled, then tears the Cursor takeover down.
    pub async fn serve_on(self, listener: TcpListener, shutdown: CancellationToken) -> Result<()> {
        let address: SocketAddr = listener.local_addr()?;
        self.registry.web_cache().set_service_addr(address);
        self.harness.set_backend_addr(address);
        tracing::info!(%address, "Cursor backend listening");
        let registry = self.registry;
        let harness = self.harness;
        let graceful = shutdown.clone();
        let server = axum::serve(listener, self.router)
            .with_graceful_shutdown(async move { graceful.cancelled().await })
            .into_future();
        tokio::pin!(server);
        tokio::select! {
            result = &mut server => {
                if let Err(error) = harness.disable().await {
                    tracing::warn!(%error, "failed to disable Cursor takeover after server stop");
                }
                result?
            },
            () = shutdown.cancelled() => {
                if let Err(error) = harness.disable().await {
                    tracing::warn!(%error, "failed to disable Cursor takeover during shutdown");
                }
                registry.shutdown().await;
                if tokio::time::timeout(Duration::from_secs(10), &mut server).await.is_err() {
                    tracing::warn!("Cursor backend shutdown timed out; forcing close");
                }
            }
        }
        Ok(())
    }
}
