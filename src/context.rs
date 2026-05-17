//! Application context.
#![allow(clippy::missing_errors_doc)]

/// Shared application state.
pub struct AppContext {
    /// Filesystem adapter.
    pub fs: crate::adapters::fs::StdFs,
    /// Process adapter.
    pub process: crate::adapters::process::StdProcess,
    /// Resolved codex paths.
    pub paths: crate::domain::paths::CodexPaths,
    /// Human-facing output adapter.
    pub ui: crate::ui::Ui,
}

impl AppContext {
    /// Construct the application context.
    #[allow(clippy::unnecessary_wraps)]
    pub fn new() -> Result<Self, crate::error::AppError> {
        Ok(Self {
            fs: crate::adapters::fs::StdFs,
            process: crate::adapters::process::StdProcess,
            paths: crate::domain::paths::CodexPaths::from_env(),
            ui: crate::ui::Ui::new(),
        })
    }
}
