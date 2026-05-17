//! Application context.
#![allow(clippy::missing_errors_doc)]

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
    #[allow(clippy::unnecessary_wraps)]
    pub(crate) fn new() -> Result<Self, crate::error::AppError> {
        Ok(Self {
            fs: crate::adapters::fs::StdFs,
            process: crate::adapters::process::StdProcess,
            paths: crate::domain::paths::CodexPaths::from_env(),
            ui: crate::ui::Ui::new(),
        })
    }
}
