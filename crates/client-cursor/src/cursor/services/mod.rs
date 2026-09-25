//! Exposes Cursor services outside the Agent loop.
//!
//! Account, profile, plan-usage, entitlement, and analytics calls are not served
//! here: they fall through to the upstream proxy and reach Cursor unchanged.

pub mod blob_sync;
pub mod commit_message;
pub mod context_sync;
pub mod knowledge;
pub mod model_catalog;
pub mod observability;
pub mod server_config;
pub mod tab;
pub mod usage;
