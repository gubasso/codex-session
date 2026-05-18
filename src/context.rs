//! Application context.

use std::sync::Arc;

/// Shared application state.
pub(crate) struct AppContext {
    /// Immutable resolved configuration.
    pub(crate) config: Arc<crate::config::Config>,
    /// Filesystem adapter.
    pub(crate) fs: crate::adapters::fs::StdFs,
    /// Process adapter.
    pub(crate) process: crate::adapters::process::StdProcess,
    /// Human-facing output adapter.
    pub(crate) ui: crate::ui::Ui,
}

impl AppContext {
    /// Construct the application context.
    pub(crate) const fn new(config: Arc<crate::config::Config>) -> Self {
        Self {
            config,
            fs: crate::adapters::fs::StdFs,
            process: crate::adapters::process::StdProcess,
            ui: crate::ui::Ui::new(),
        }
    }

    /// Convenience access to the resolved path set.
    pub(crate) fn paths(&self) -> &crate::config::PathsConfig {
        &self.config.paths
    }
}
