//! Local Cursor backend: Cursor protocol, agent runtime, and the Cursor takeover
//! (local CA, proxy, settings). Model calls go through OwO AI Gateway's shared router.
pub mod api;
pub mod app;
pub mod config;
pub mod cursor;
pub mod error;
pub mod local_app;
pub mod model;
pub mod network;
pub mod provider;
pub mod run;
pub mod search;
pub mod store;
pub mod owo_sync;

pub use app::App;
pub use config::Config;
pub use error::{Error, Result};
