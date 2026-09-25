//! Exposes provider-independent search capabilities.
mod cache;
mod catalog;
mod engine;
mod federation;
mod fetch;
#[cfg(feature = "semble")]
mod search_provider;

pub use cache::{WebCache, WebCacheEntry};
pub use engine::{HtmlEngine, JsonEngine, SearchEngine, SearchHit};
pub use federation::{SearchError, WebSearch};
pub use fetch::{FetchError, FetchedPage, WebFetch};
#[cfg(feature = "semble")]
pub(crate) use search_provider::execute as execute_semble;

/// Tools that exist only when the `semble` feature is compiled in.
pub(crate) const SEMBLE_TOOLS: [&str; 2] = ["SembleSearch", "SembleFindRelated"];

#[cfg(not(feature = "semble"))]
pub(crate) async fn execute_semble(
    tool_name: &str,
    _arguments: serde_json::Value,
    _store: Option<crate::store::Store>,
) -> std::result::Result<serde_json::Value, String> {
    Err(format!("{tool_name} is not included in this build"))
}
