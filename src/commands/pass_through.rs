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

/// Pairs the shared child-PID slot with the once-installed signal-forwarding
/// guard. Created once per wrapper invocation by the entry point and reused
/// across every retry attempt — installing a fresh forwarder per attempt
/// would leak iterator threads whose stale `child_pid` Arcs (now `0`) would
/// take the no-child branch on a later signal and terminate the wrapper.
pub(crate) struct SignalSession {
    pub(crate) child_pid: Arc<AtomicI32>,
    pub(crate) guard: crate::adapters::spawner::SignalGuard,
}

impl SignalSession {
    pub(crate) fn install() -> Result<Self, crate::error::AppError> {
        let child_pid = Arc::new(AtomicI32::new(0));
        let guard = crate::adapters::spawner::install_signal_forwarding(Arc::clone(&child_pid))
            .map_err(crate::error::AppError::Io)?;
        Ok(Self { child_pid, guard })
    }
}

/// Run the non-`self` path.
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<i32, crate::error::AppError> {
    tracing::info!(op = "pass-through", status = "start", argc = argv.len());
    if ctx.global.dry_run {
        let account = crate::services::account::resolver::resolve(ctx)?.id;
        let prepared = prepare_invocation(ctx, argv, &account)?;
        ctx.ui
            .write_dry_run(&prepared.invocation.dry_run_report())?;
        tracing::info!(op = "pass-through", status = "ok", outcome = "dry-run");
        return Ok(0);
    }
    crate::services::account::retry::run_with_retry(ctx, argv)
}

/// Run the child for one attempt. Returns the exit code plus stdout and
/// stderr buffers.
///
/// When `capture` is `true`, the child's streams are tee'd to the
/// parent's real stdio **and** independently captured (each capped at
/// `failover::MAX_CAPTURE_BYTES`). The two streams are returned
/// separately so callers can run `failover::scan` on each without
/// manufacturing a synthetic interleaving — concatenating them risks
/// false-positive line boundaries (a partial line on stdout joined to a
/// fragment on stderr) and false negatives (real interleaving reordered
/// into a non-matching shape).
///
/// When `capture` is `false`, the child inherits the parent's stdio
/// directly (no pipe, no tee overhead) and the returned buffers are
/// empty.
pub(crate) fn run_once(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
    account: &crate::services::account::AccountId,
    session: &SignalSession,
    capture: bool,
) -> Result<(i32, Vec<u8>, Vec<u8>), crate::error::AppError> {
    tracing::info!(
        op = "pass-through.run-once",
        status = "start",
        argc = argv.len(),
        account = %account
    );
    let prepared = prepare_invocation(ctx, argv, account)?;

    let cache_settings = cache_settings_target(ctx);
    let session_config = prepared.session_dir.join("config.toml");
    let result = run_child(
        ctx,
        &prepared.session_dir,
        prepared.invocation,
        session,
        capture,
    );
    persist_trust(
        &session_config,
        &cache_settings,
        prepared.baseline_projects.as_ref(),
    );
    result
}

struct PreparedInvocation {
    invocation: ChildInvocation,
    session_dir: camino::Utf8PathBuf,
    baseline_projects: Option<toml::Table>,
}

fn prepare_invocation(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
    account: &crate::services::account::AccountId,
) -> Result<PreparedInvocation, crate::error::AppError> {
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

    let group = crate::services::session::group_id::current(ctx)?;
    let root = crate::services::session::dir::resolve_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    )?;
    let group_id = group.id.as_str().to_owned();
    let session_dir = crate::services::session::dir::session_dir(&root.path, account, &group_id)?;
    let cwd = current_cwd()?;
    let profile = ctx.config.profile.active.clone();
    materialize_account_auth_seed(ctx, account, &session_dir)?;

    let (env, baseline_projects) = if let Some(profile_name) = profile.as_deref() {
        let composition = crate::services::profile::compose(
            profile_name,
            &crate::services::profile::ProfilePaths {
                profiles_dir: ctx.config.profile.profiles_dir.clone(),
                settings_dir: ctx.config.profile.settings_dir.clone(),
                cache_settings: cache_settings_path(ctx),
            },
        )?;
        crate::services::profile::write_session_artifacts(&composition, &session_dir)?;
        (composition.env, composition.baseline_projects)
    } else {
        crate::services::profile::write_stock_session_artifacts(&session_dir)?;
        (std::collections::BTreeMap::new(), None)
    };

    let meta = crate::services::session::meta::SessionMeta::new(
        profile.as_deref(),
        &group_id,
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

    Ok(PreparedInvocation {
        invocation: inv,
        session_dir,
        baseline_projects,
    })
}

fn persist_trust(
    session_config: &camino::Utf8Path,
    cache_settings: &camino::Utf8Path,
    baseline: Option<&toml::Table>,
) {
    match crate::services::trust_sync::persist_projects(session_config, cache_settings, baseline) {
        Ok(crate::services::trust_sync::TrustSyncOutcome::Unchanged) => {
            tracing::debug!(op = "trust.persist", outcome = "unchanged");
        }
        Ok(crate::services::trust_sync::TrustSyncOutcome::Wrote { added, changed }) => {
            tracing::info!(
                op = "trust.persist",
                outcome = "wrote",
                added,
                changed,
                path = %cache_settings
            );
        }
        Err(err) => {
            tracing::warn!(
                op = "trust.persist",
                status = "error",
                err.kind = err.kind(),
                err = %err,
                path = %err.path(),
            );
        }
    }
}

fn run_child(
    ctx: &crate::context::AppContext,
    session_dir: &camino::Utf8Path,
    inv: ChildInvocation,
    session: &SignalSession,
    capture: bool,
) -> Result<(i32, Vec<u8>, Vec<u8>), crate::error::AppError> {
    crate::services::auth::import_if_missing(session_dir, ctx.home_dir())?;

    let (status, stdout, stderr) = if capture {
        let spawn_result = ctx.spawner.spawn_and_wait_output(inv, &session.child_pid);
        session
            .child_pid
            .store(0, std::sync::atomic::Ordering::SeqCst);
        let output = spawn_result?;
        (output.status, output.stdout, output.stderr)
    } else {
        let spawn_result = ctx.spawner.spawn_and_wait(inv, &session.child_pid);
        session
            .child_pid
            .store(0, std::sync::atomic::Ordering::SeqCst);
        let status = spawn_result?;
        (status, Vec::new(), Vec::new())
    };

    let exit_code = finalize_child_status(status, &session.guard)?;
    Ok((exit_code, stdout, stderr))
}

fn materialize_account_auth_seed(
    ctx: &crate::context::AppContext,
    account: &crate::services::account::AccountId,
    session_dir: &camino::Utf8Path,
) -> Result<(), crate::error::AppError> {
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let seed = registry.group_auth_seed_path(account);
    let group_auth = session_dir.join("auth.json");
    if group_auth.as_std_path().exists() || !seed.as_std_path().exists() {
        return Ok(());
    }
    let bytes = crate::services::auth::secure_file_read(&seed)?;
    crate::adapters::fs::atomic_write(&group_auth, &bytes)
        .map_err(crate::services::auth::AuthError::from)?;
    Ok(())
}

fn finalize_child_status(
    status: std::process::ExitStatus,
    sig_guard: &crate::adapters::spawner::SignalGuard,
) -> Result<i32, crate::error::AppError> {
    match status.code() {
        // Child exited cleanly. If the wrapper itself observed a fatal
        // signal (e.g. user pressed Ctrl-C and the child trapped it),
        // surface it with conventional shell semantics (exit 128+sig).
        // The kernel never reported a signal on the child, so this is
        // the only chance we have to propagate the user's intent.
        Some(0) => sig_guard.observed_signal().map_or(Ok(0), |signal| {
            let synthetic =
                <std::process::ExitStatus as std::os::unix::process::ExitStatusExt>::from_raw(
                    signal,
                );
            Err(crate::error::AppError::ChildSignaled(synthetic))
        }),
        Some(code) => Ok(code),
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
    let path = cache_settings_target(ctx);
    path.is_file().then_some(path)
}

/// Canonical cache-settings target path — used both as the source of the
/// machine-local layer during `compose()` (gated on existence by
/// `cache_settings_path`) and as the destination for trust-sync writes.
/// Always `<cache_dir>/settings.toml`.
fn cache_settings_target(ctx: &crate::context::AppContext) -> Utf8PathBuf {
    ctx.config.paths.cache_dir.join("settings.toml")
}
