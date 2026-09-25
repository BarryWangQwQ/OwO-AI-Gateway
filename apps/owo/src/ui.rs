//! Layout helpers for human-readable output.

use std::path::Path;

/// A row label, padded so the values line up.
pub fn label(name: &str) -> String {
    format!("{name:<9}")
}

/// `path` with the home directory shown as `~`.
pub fn tilde(path: &Path) -> String {
    match dirs::home_dir().and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}
