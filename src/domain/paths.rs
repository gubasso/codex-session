//! Path resolution.
//!
//! Unlike the legacy bash wrapper running under `set -u`, this module does not
//! abort when `HOME` is unset. It instead resolves a coherent fallback path so
//! the binary can return a normal error rather than panicking.

/// Resolved runtime paths.
#[derive(Debug, Clone)]
pub(crate) struct CodexPaths {
    /// Base config path.
    pub(crate) base: std::path::PathBuf,
    /// Target config path.
    pub(crate) target: std::path::PathBuf,
    /// Cache directory path.
    pub(crate) cache_dir: std::path::PathBuf,
    /// Stamp file path.
    pub(crate) stamp: std::path::PathBuf,
}

impl CodexPaths {
    /// Build from the process environment.
    pub(crate) fn from_env() -> Self {
        // Treat empty env vars as absent so we never produce relative paths.
        // Bash uses `${XDG_CACHE_HOME:-$HOME/.cache}`, where the `:-` operator
        // falls back on either unset OR empty; mirror that here. An empty
        // `HOME` should likewise not yield a relative `.codex` path.
        let home = non_empty_var("HOME")
            .map_or_else(|| std::path::PathBuf::from("/"), std::path::PathBuf::from);
        let codex_dir = home.join(".codex");
        let cache_root = non_empty_var("XDG_CACHE_HOME")
            .map_or_else(|| home.join(".cache"), std::path::PathBuf::from);
        let cache_dir = cache_root.join("codex-session");
        Self {
            base: codex_dir.join("config.base.toml"),
            target: codex_dir.join("config.toml"),
            cache_dir: cache_dir.clone(),
            stamp: cache_dir.join("last-merge"),
        }
    }
}

/// Read an environment variable, treating empty values as absent.
fn non_empty_var(name: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}
