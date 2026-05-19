//! Application context.
//!
//! What this is: immutable process-wide state shared by command handlers.
//! What this is not: business logic; commands and services consume this state.

#![allow(clippy::result_large_err)]

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
    /// Construct an empty lazy child-path cache.
    pub(crate) const fn new() -> Self {
        Self {
            cell: OnceLock::new(),
        }
    }

    /// Resolve and cache the child path on first access.
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

/// Lazily resolved per-terminal session context.
pub(crate) struct LazySession {
    cell: OnceLock<Arc<SessionContext>>,
}

impl LazySession {
    pub(crate) const fn new() -> Self {
        Self {
            cell: OnceLock::new(),
        }
    }

    pub(crate) fn get_or_resolve(
        &self,
        cfg: &crate::config::Config,
    ) -> Result<Arc<SessionContext>, crate::config::ConfigError> {
        if let Some(session) = self.cell.get() {
            return Ok(Arc::clone(session));
        }

        let terminal_id = crate::services::session::terminal_id::current();
        let root = crate::services::session::dir::resolve_session_root(
            cfg.paths.runtime_dir.as_deref(),
            &cfg.paths.state_dir,
        )?;
        let dir = crate::services::session::dir::session_dir(&root.path, &terminal_id)?;
        let session = Arc::new(SessionContext {
            dir,
            terminal_id,
            composition: None,
        });
        let _ = self.cell.set(Arc::clone(&session));
        Ok(session)
    }
}

/// Session-scoped state shared by profile composition commands.
pub(crate) struct SessionContext {
    pub(crate) dir: Utf8PathBuf,
    pub(crate) terminal_id: String,
    #[allow(dead_code)]
    pub(crate) composition: Option<crate::services::profile::Composition>,
}

/// Shared application state.
pub(crate) struct AppContext {
    /// Immutable resolved configuration.
    pub(crate) config: Arc<crate::config::Config>,
    /// Process spawner adapter.
    pub(crate) spawner: StdSpawner,
    /// Human-facing output adapter.
    pub(crate) ui: crate::ui::Ui,
    /// Parsed global CLI flags.
    pub(crate) global: crate::cli::GlobalArgs,
    /// Lazily resolved child path.
    pub(crate) resolved_child: LazyChild,
    /// Lazily resolved session context.
    pub(crate) session: LazySession,
}

impl AppContext {
    /// Construct the application context.
    pub(crate) const fn new(
        config: Arc<crate::config::Config>,
        global: crate::cli::GlobalArgs,
    ) -> Self {
        Self {
            config,
            spawner: StdSpawner,
            ui: crate::ui::Ui::new(),
            global,
            resolved_child: LazyChild::new(),
            session: LazySession::new(),
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

    /// Borrow the resolved session context, resolving on first access.
    pub(crate) fn session(&self) -> Result<Arc<SessionContext>, crate::config::ConfigError> {
        self.session.get_or_resolve(&self.config)
    }
}
