//! Application context.

use std::sync::{Arc, OnceLock};

use camino::Utf8PathBuf;

use crate::adapters::spawner::{Spawner as _, SpawnerError, StdSpawner};

/// Lazy resolver for the child binary path.
///
/// Wrapper-owned introspection verbs (`help`, `version`, `config show-local`,
/// `config status`) must keep working when `codex` is missing or
/// mis-configured. Eager resolution would regress that. The resolver runs
/// at most once per process; both Ok and Err are cached.
pub(crate) struct LazyChild {
    cell: OnceLock<Result<Utf8PathBuf, SpawnerError>>,
}

impl LazyChild {
    pub(crate) const fn new() -> Self {
        Self {
            cell: OnceLock::new(),
        }
    }

    pub(crate) fn get_or_resolve(
        &self,
        spawner: StdSpawner,
        cfg: &crate::config::ChildConfig,
    ) -> Result<&Utf8PathBuf, &SpawnerError> {
        self.cell
            .get_or_init(|| spawner.resolve_child(cfg))
            .as_ref()
    }
}

/// Shared application state.
pub(crate) struct AppContext {
    /// Immutable resolved configuration.
    pub(crate) config: Arc<crate::config::Config>,
    /// Filesystem adapter.
    pub(crate) fs: crate::adapters::fs::StdFs,
    /// Process spawner adapter.
    pub(crate) spawner: StdSpawner,
    /// Human-facing output adapter.
    pub(crate) ui: crate::ui::Ui,
    /// Parsed global CLI flags.
    pub(crate) global: crate::cli::GlobalArgs,
    /// Lazily resolved child path.
    pub(crate) resolved_child: LazyChild,
}

impl AppContext {
    /// Construct the application context.
    pub(crate) const fn new(
        config: Arc<crate::config::Config>,
        global: crate::cli::GlobalArgs,
    ) -> Self {
        Self {
            config,
            fs: crate::adapters::fs::StdFs,
            spawner: StdSpawner,
            ui: crate::ui::Ui::new(),
            global,
            resolved_child: LazyChild::new(),
        }
    }

    /// Convenience access to the resolved path set.
    pub(crate) fn paths(&self) -> &crate::config::PathsConfig {
        &self.config.paths
    }

    /// Borrow the resolved child path, resolving on first access.
    pub(crate) fn resolved_child(&self) -> Result<&Utf8PathBuf, &SpawnerError> {
        self.resolved_child
            .get_or_resolve(self.spawner, &self.config.child)
    }
}
