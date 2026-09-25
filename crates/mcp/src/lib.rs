//! MCP servers, defined once in OwO AI Gateway's config (`[mcp.<name>]`) and written into every app
//! they are enabled for, each in that app's own MCP config format.
//!
//! Writing goes through [`owo_client_apps::managed`]: one managed integration per app
//! (`state/mcp-<app>/`), one fragment per server, the file backed up before the first
//! change. Entries OwO AI Gateway did not write are never touched; an entry it wrote that was
//! edited since is a conflict. Resolved secrets are written only into the app's file; OwO AI Gateway's
//! state keeps a digest of each entry, not its values.

pub mod apps;
pub mod render;
pub mod sync;

pub use apps::{Env, McpApp, UNSUPPORTED};
pub use sync::{sync_app, Ctx, Outcome, SyncReport};

#[cfg(test)]
mod tests;
