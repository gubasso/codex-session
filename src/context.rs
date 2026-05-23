//! Application context.
//!
//! What this is: immutable process-wide state shared by command handlers.
//! What this is not: business logic; commands and services consume this state.

#![allow(clippy::result_large_err)]

use std::sync::{Arc, OnceLock};

use camino::{Utf8Path, Utf8PathBuf};

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
        ctx: &crate::context::AppContext,
    ) -> Result<Arc<SessionContext>, crate::error::AppError> {
        if let Some(session) = self.cell.get() {
            return Ok(Arc::clone(session));
        }

        let resolved = crate::services::session::group_id::current(ctx)?;
        let resolved_account = crate::services::account::resolver::resolve(ctx)?;
        let root = crate::services::session::dir::resolve_session_root(
            ctx.config.paths.runtime_dir.as_deref(),
            &ctx.config.paths.state_dir,
        )?;
        let dir = crate::services::session::dir::session_dir(
            &root.path,
            &resolved_account.id,
            resolved.id.as_str(),
        )?;
        let session = Arc::new(SessionContext {
            account: resolved_account.id.clone(),
            account_source: resolved_account.source,
            dir,
            group_id: resolved.id,
            group_id_source: resolved.source,
            composition: None,
        });
        let _ = self.cell.set(Arc::clone(&session));
        Ok(session)
    }
}

/// Session-scoped state shared by profile composition commands.
pub(crate) struct SessionContext {
    #[allow(dead_code)]
    pub(crate) account: crate::services::account::AccountId,
    #[allow(dead_code)]
    pub(crate) account_source: crate::services::account::resolver::AccountResolutionSource,
    pub(crate) dir: Utf8PathBuf,
    pub(crate) group_id: crate::services::session::group_id::GroupId,
    #[allow(dead_code)]
    pub(crate) group_id_source: crate::services::session::group_id::GroupIdSource,
    #[allow(dead_code)]
    pub(crate) composition: Option<crate::services::profile::Composition>,
}

/// Shared application state.
pub(crate) struct AppContext {
    /// Immutable resolved configuration.
    pub(crate) config: Arc<crate::config::Config>,
    /// Resolved home directory.
    pub(crate) home_dir: Utf8PathBuf,
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
        home_dir: Utf8PathBuf,
    ) -> Self {
        Self {
            config,
            home_dir,
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

    /// Borrow the resolved home directory.
    pub(crate) fn home_dir(&self) -> &Utf8Path {
        &self.home_dir
    }

    /// Borrow the resolved child path, resolving on first access.
    pub(crate) fn resolved_child(&self) -> Result<&Utf8PathBuf, &SpawnerError> {
        self.resolved_child
            .get_or_resolve(self.spawner, &self.config.child)
    }

    /// Borrow the resolved session context, resolving on first access.
    pub(crate) fn session(&self) -> Result<Arc<SessionContext>, crate::error::AppError> {
        self.session.get_or_resolve(self)
    }
}
