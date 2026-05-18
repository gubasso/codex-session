//! Application context.

/// Shared application state.
pub(crate) struct AppContext {
    /// Filesystem adapter.
    pub(crate) fs: crate::adapters::fs::StdFs,
    /// Process adapter.
    pub(crate) process: crate::adapters::process::StdProcess,
    /// Resolved codex paths.
    pub(crate) paths: crate::domain::paths::CodexPaths,
    /// Human-facing output adapter.
    pub(crate) ui: crate::ui::Ui,
}

impl AppContext {
    /// Construct the application context.
    pub(crate) const fn new(paths: crate::domain::paths::CodexPaths) -> Self {
        Self {
            fs: crate::adapters::fs::StdFs,
            process: crate::adapters::process::StdProcess,
            paths,
            ui: crate::ui::Ui::new(),
        }
    }
}
