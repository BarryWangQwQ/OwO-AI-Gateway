//! Defines the provider interface. Model calls are served by OwO AI Gateway's shared router (`owo`).
mod event;
mod normalize;
mod recorder;
mod router;
pub mod owo;

use std::pin::Pin;

use futures_util::Stream;
use tokio_util::sync::CancellationToken;

use crate::{model::ModelInvocation, Result};

pub use event::*;
pub use recorder::CallRecorder;
pub use router::ProviderRouter;
pub use owo::OwoProvider;

pub type ProviderStream = Pin<Box<dyn Stream<Item = Result<ModelEvent>> + Send>>;

pub trait Provider: Send + Sync {
    fn stream(&self, invocation: ModelInvocation, cancellation: CancellationToken) -> ProviderStream;
}
