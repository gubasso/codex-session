//! Pass-through command path.
//!
//! What this is: the handler for forwarded child invocations (including
//! bare `codex-session`, which forwards an empty child argv -> Codex TUI).
//! What this is not: clap parsing or child process execution primitives.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;

use crate::adapters::spawner::Spawner as _;
use crate::domain::child_invocation::{ChildEnv, ChildInvocation};

struct PersistGuard<'a> {
    bridge: &'a crate::services::auth::AuthBridge,
}

impl Drop for PersistGuard<'_> {
    fn drop(&mut self) {
        if let Err(err) = self.bridge.persist_to_native() {
            tracing::warn!(op = "auth.persist", status = "error", err = %err);
        }
    }
}

/// Run the non-`self` path.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<(), crate::error::AppError> {
    tracing::info!(op = "pass-through", status = "start", argc = argv.len());
    let resolved = ctx.resolved_child().map_err(|err| match err {
        crate::adapters::spawner::SpawnerError::NotFound {
            tried,
            path_searched,
        } => crate::error::AppError::ChildNotFound {
            tried: tried.clone().into_std_path_buf(),
            path_searched: path_searched.clone(),
        },
        crate::adapters::spawner::SpawnerError::NotExecutable { path } => {
            crate::error::AppError::ChildNotExecutable {
                path: path.clone().into_std_path_buf(),
            }
        }
        crate::adapters::spawner::SpawnerError::Recursion { path } => {
            crate::error::AppError::ChildRecursion { path: path.clone() }
        }
        crate::adapters::spawner::SpawnerError::Exec(io) => {
            crate::error::AppError::ChildExec(std::io::Error::new(io.kind(), io.to_string()))
        }
        crate::adapters::spawner::SpawnerError::NonUtf8Path(_) => {
            crate::error::AppError::Other(anyhow::anyhow!("non-utf8 child path"))
        }
    })?;

    let session = ctx.session()?;
    let terminal_id = session.terminal_id.clone();
    let session_dir = session.dir.clone();
    let cwd = current_cwd()?;
    let profile = ctx.config.profile.active.clone();

    let env = if let Some(profile_name) = profile.as_deref() {
        let composition = crate::services::profile::compose(
            profile_name,
            &crate::services::profile::ProfilePaths {
                profiles_dir: ctx.config.profile.profiles_dir.clone(),
                settings_dir: ctx.config.profile.settings_dir.clone(),
                cache_settings: cache_settings_path(ctx),
            },
        )?;
        crate::services::profile::write_session_artifacts(&composition, &session_dir)?;
        composition.env
    } else {
        crate::services::profile::write_stock_session_artifacts(&session_dir)?;
        std::collections::BTreeMap::new()
    };

    let meta = crate::services::session::meta::SessionMeta::new(
        profile.as_deref(),
        &terminal_id,
        cwd.as_ref(),
    );
    crate::services::session::meta::write(&session_dir, &meta)?;

    let mut child_env = ChildEnv::scrubbed_default();
    child_env
        .set
        .push(("CODEX_HOME".to_owned(), session_dir.clone().into()));
    for (key, value) in env {
        if key.starts_with("CODEX_SESSION_") {
            return Err(crate::config::ConfigError::EnvKeyInvalid {
                key,
                reason: "CODEX_SESSION_* keys are wrapper-private and may not be injected"
                    .to_owned(),
            }
            .into());
        }
        child_env.set.push((key, value.into()));
    }

    let inv = ChildInvocation {
        binary: resolved.clone(),
        args: argv.to_vec(),
        env: child_env,
    };

    if ctx.global.dry_run {
        ctx.ui.write_dry_run(&inv.dry_run_report())?;
        tracing::info!(op = "pass-through", status = "ok", outcome = "dry-run");
        return Ok(());
    }

    run_under_bridge(ctx, &session_dir, inv)
}

fn run_under_bridge(
    ctx: &crate::context::AppContext,
    session_dir: &camino::Utf8Path,
    inv: ChildInvocation,
) -> Result<(), crate::error::AppError> {
    let child_pid = Arc::new(AtomicI32::new(0));
    let sig_guard = crate::services::auth::signal::install(Arc::clone(&child_pid))
        .map_err(crate::error::AppError::Io)?;
    let home = ctx.home_dir().to_path_buf();
    let mut bridge = crate::services::auth::AuthBridge::new(&home, session_dir)?;
    bridge.seed_into_session()?;

    // `_persist` is declared before the scope, so it drops AFTER
    // `thread::scope` has joined the watcher. This means
    // `persist_to_native()` (run in the Drop impl) can never race with a
    // watcher write — there is no live watcher by the time persist runs.
    let _persist = PersistGuard { bridge: &bridge };
    let stop = std::sync::atomic::AtomicBool::new(false);
    let spawn_result = std::thread::scope(|s| {
        let _watcher = crate::services::auth::watcher::run_until(s, &bridge, &stop);
        let result = ctx.spawner.spawn_and_wait(inv, &child_pid);
        // Clear the child PID before stopping/joining the watcher so any
        // signal arriving during the join window cannot be forwarded to a
        // PID the kernel may have already recycled for an unrelated
        // process. The signal thread's `pid <= 0` branch re-raises with
        // default semantics.
        child_pid.store(0, std::sync::atomic::Ordering::SeqCst);
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        result
    });
    let status = spawn_result?;
    finalize_child_status(status, &sig_guard)
}

fn finalize_child_status(
    status: std::process::ExitStatus,
    sig_guard: &crate::services::auth::signal::SignalGuard,
) -> Result<(), crate::error::AppError> {
    match status.code() {
        // Child exited cleanly. If the wrapper itself observed a fatal
        // signal (e.g. user pressed Ctrl-C and the child trapped it),
        // surface it with conventional shell semantics (exit 128+sig).
        // The kernel never reported a signal on the child, so this is
        // the only chance we have to propagate the user's intent.
        Some(0) => sig_guard.observed_signal().map_or(Ok(()), |signal| {
            let synthetic =
                <std::process::ExitStatus as std::os::unix::process::ExitStatusExt>::from_raw(
                    signal,
                );
            Err(crate::error::AppError::ChildSignaled(synthetic))
        }),
        Some(code) => Err(crate::error::AppError::ChildExitNonZero(code)),
        // Child died of a signal. Prefer the kernel-reported status — it
        // names the exact signal the child took (which may differ from
        // the signal the wrapper observed, e.g. when the child SIGSEGV'd
        // independently of the user's Ctrl-C).
        None => Err(crate::error::AppError::ChildSignaled(status)),
    }
}

fn current_cwd() -> Result<Utf8PathBuf, crate::config::ConfigError> {
    Utf8PathBuf::try_from(std::env::current_dir().map_err(crate::config::ConfigError::CurrentDir)?)
        .map_err(crate::config::ConfigError::from)
}

fn cache_settings_path(ctx: &crate::context::AppContext) -> Option<Utf8PathBuf> {
    let path = ctx.config.paths.cache_dir.join("settings.toml");
    path.is_file().then_some(path)
}
