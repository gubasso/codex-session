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

    let first_arg = argv.first().and_then(|a| a.to_str()).unwrap_or("");
    match first_arg {
        "login" => {
            let opts = crate::services::account::gate::LoginOptions::from_argv(&argv[1..]);
            return crate::services::account::gate::run_login(ctx, &opts);
        }
        "logout" => return crate::services::account::gate::run_logout(ctx),
        _ => {}
    }

    let gated = crate::services::account::gate::ensure(ctx)?;

    if let Some(intent) = detect_resume(argv) {
        return run_resume(ctx, argv, &intent, &gated);
    }

    if ctx.global.dry_run {
        let prepared = prepare_invocation(ctx, argv, &gated, None)?;
        let dry_ctx = crate::domain::child_invocation::DryRunContext {
            account: gated.id.to_string(),
            account_source: crate::services::account::resolver::source_label(gated.source)
                .to_owned(),
        };
        ctx.ui.write_dry_run(
            &crate::domain::child_invocation::dry_run_report_with_context(
                &prepared.invocation,
                Some(&dry_ctx),
            ),
        )?;
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
    resolved: &crate::services::account::resolver::ResolvedAccount,
    session: &SignalSession,
    capture: bool,
    group_id_override: Option<&str>,
) -> Result<(i32, Vec<u8>, Vec<u8>), crate::error::AppError> {
    let json_mode = has_json_flag(argv);
    let effective_capture = capture || json_mode;
    let account = &resolved.id;
    tracing::info!(
        op = "pass-through.run-once",
        status = "start",
        argc = argv.len(),
        account = %account
    );
    let prepared = prepare_invocation(ctx, argv, resolved, group_id_override)?;

    let cache_config = cache_config_target(ctx);
    let session_config = prepared.session_dir.join("config.toml");
    let PreparedInvocation {
        invocation,
        session_dir,
        baseline_projects,
        group_id,
        cwd,
    } = prepared;

    let result = run_child(ctx, invocation, session, effective_capture);
    sync_group_auth_to_seed(ctx, account, &session_dir);
    persist_trust(&session_config, &cache_config, baseline_projects.as_ref());

    if json_mode
        && let Ok((_, stdout_buf, _)) = &result
        && let Some(thread_id) =
            crate::services::session::thread_index::extract_thread_id(stdout_buf)
    {
        let entry = crate::services::session::thread_index::ThreadEntry {
            thread_id,
            account: resolved.id.to_string(),
            group_id,
            cwd,
            created_at: crate::services::session::thread_index::utc_now_rfc3339(),
        };
        if let Err(err) =
            crate::services::session::thread_index::append(&ctx.config.paths.state_dir, &entry)
        {
            tracing::warn!(
                op = "thread_index.append",
                thread_id = %entry.thread_id,
                err = %err,
                "failed to append thread index entry"
            );
        }
    }
    result
}

struct PreparedInvocation {
    invocation: ChildInvocation,
    session_dir: camino::Utf8PathBuf,
    baseline_projects: Option<toml::Table>,
    group_id: String,
    cwd: camino::Utf8PathBuf,
}

fn prepare_invocation(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
    resolved: &crate::services::account::resolver::ResolvedAccount,
    group_id_override: Option<&str>,
) -> Result<PreparedInvocation, crate::error::AppError> {
    let child_binary = ctx.resolved_child().map_err(map_child_err)?;

    let group_id = if let Some(override_gid) = group_id_override {
        match override_gid.parse::<crate::services::session::group_id::GroupId>() {
            Ok(gid) => gid.as_str().to_owned(),
            Err(reason) => {
                tracing::warn!(
                    op = "prepare.group_id_override",
                    override_gid,
                    reason = %reason,
                    "invalid group_id in thread index; falling back to current"
                );
                let group = crate::services::session::group_id::current(ctx)?;
                group.id.as_str().to_owned()
            }
        }
    } else {
        let group = crate::services::session::group_id::current(ctx)?;
        group.id.as_str().to_owned()
    };
    let root = crate::services::session::dir::resolve_session_root(
        ctx.config.paths.runtime_dir.as_deref(),
        &ctx.config.paths.state_dir,
    )?;
    let account = &resolved.id;
    let session_dir = crate::services::session::dir::session_dir(&root.path, account, &group_id)?;
    let cwd = current_cwd()?;
    let config_recipe = ctx.config.config_recipe.active.clone();
    materialize_account_auth_seed(ctx, account, &session_dir)?;

    let (env, baseline_projects) = if let Some(recipe_name) = config_recipe.as_deref() {
        let composition = crate::services::config_recipe::compose(
            recipe_name,
            &crate::services::config_recipe::ConfigRecipePaths {
                recipes_dir: ctx.config.config_recipe.recipes_dir.clone(),
                configs_dir: ctx.config.config_recipe.configs_dir.clone(),
                cache_config: cache_config_path(ctx),
            },
        )?;
        crate::services::config_recipe::write_session_artifacts(&composition, &session_dir)?;
        (composition.env, composition.baseline_projects)
    } else {
        crate::services::config_recipe::write_stock_session_artifacts(&session_dir)?;
        (std::collections::BTreeMap::new(), None)
    };

    let meta = crate::services::session::meta::SessionMeta::new(
        config_recipe.as_deref(),
        &group_id,
        cwd.as_ref(),
        account.as_str(),
        crate::services::account::resolver::source_label(resolved.source),
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
        binary: child_binary.clone(),
        args: argv.to_vec(),
        env: child_env,
    };

    Ok(PreparedInvocation {
        invocation: inv,
        session_dir,
        baseline_projects,
        group_id,
        cwd,
    })
}

fn persist_trust(
    session_config: &camino::Utf8Path,
    cache_config: &camino::Utf8Path,
    baseline: Option<&toml::Table>,
) {
    match crate::services::trust_sync::persist_projects(session_config, cache_config, baseline) {
        Ok(crate::services::trust_sync::TrustSyncOutcome::Unchanged) => {
            tracing::debug!(op = "trust.persist", outcome = "unchanged");
        }
        Ok(crate::services::trust_sync::TrustSyncOutcome::Wrote { added, changed }) => {
            tracing::info!(
                op = "trust.persist",
                outcome = "wrote",
                added,
                changed,
                path = %cache_config
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
    inv: ChildInvocation,
    session: &SignalSession,
    capture: bool,
) -> Result<(i32, Vec<u8>, Vec<u8>), crate::error::AppError> {
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

/// Propagate token refreshes from the session group copy back to the account seed.
/// See `docs/auth-gate-spec.md` §6 for rationale.
fn sync_group_auth_to_seed(
    ctx: &crate::context::AppContext,
    account: &crate::services::account::AccountId,
    session_dir: &camino::Utf8Path,
) {
    let group_auth = session_dir.join("auth.json");
    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
    let seed = registry.group_auth_seed_path(account);

    let Ok(group_bytes) = std::fs::read(group_auth.as_std_path()) else {
        return;
    };
    let seed_bytes = std::fs::read(seed.as_std_path()).unwrap_or_default();
    if group_bytes == seed_bytes {
        return;
    }
    tracing::info!(
        op = "auth.sync",
        account = %account,
        "group auth differs from seed; syncing refreshed token back"
    );
    if let Err(err) = crate::adapters::fs::atomic_write(&seed, &group_bytes) {
        tracing::warn!(
            op = "auth.sync",
            account = %account,
            error = %err,
            "failed to sync refreshed auth back to seed"
        );
    }
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

fn has_json_flag(argv: &[std::ffi::OsString]) -> bool {
    argv.iter().any(|arg| arg.to_str() == Some("--json"))
}

/// Strip codex-session-only resume flags (`--all-groups`) from argv before
/// forwarding to codex on the fallback path. `--all` is a real codex flag
/// and must be preserved. Returns `None` if no stripping was needed
/// (original argv is clean).
fn strip_wrapper_resume_flags(argv: &[std::ffi::OsString]) -> Option<Vec<std::ffi::OsString>> {
    let has_wrapper_flags = argv
        .iter()
        .any(|a| matches!(a.to_str(), Some("--all-groups")));
    if !has_wrapper_flags {
        return None;
    }
    Some(
        argv.iter()
            .filter(|a| !matches!(a.to_str(), Some("--all-groups")))
            .cloned()
            .collect(),
    )
}

fn rewrite_last_to_id(argv: &[std::ffi::OsString], thread_id: &str) -> Vec<std::ffi::OsString> {
    let mut result = Vec::with_capacity(argv.len());
    let mut replaced = false;
    for arg in argv {
        match arg.to_str() {
            Some("--last" | "--all") if !replaced => {
                result.push(std::ffi::OsString::from(thread_id));
                replaced = true;
            }
            Some("--all-groups") => {
                // Strip: wrapper-only flag, not forwarded to codex
            }
            _ => result.push(arg.clone()),
        }
    }
    result
}

fn resolve_resume_account(
    ctx: &crate::context::AppContext,
    intent: &ResumeIntent,
) -> Option<(
    crate::services::account::resolver::ResolvedAccount,
    String,
    String,
)> {
    let state_dir = &ctx.config.paths.state_dir;

    let entry = match intent {
        ResumeIntent::ById(id) => {
            match crate::services::session::thread_index::lookup(state_dir, id) {
                Ok(opt) => opt,
                Err(err) => {
                    tracing::warn!(
                        op = "resume.resolve", thread_id = %id,
                        err = %err, "thread index lookup failed",
                    );
                    return None;
                }
            }
        }
        ResumeIntent::Last { all_groups } => {
            if *all_groups {
                match crate::services::session::thread_index::last_any(state_dir) {
                    Ok(opt) => opt,
                    Err(err) => {
                        tracing::warn!(
                            op = "resume.resolve",
                            err = %err, "thread index last_any failed",
                        );
                        return None;
                    }
                }
            } else {
                let group = match crate::services::session::group_id::current(ctx) {
                    Ok(g) => g,
                    Err(err) => {
                        tracing::warn!(
                            op = "resume.resolve",
                            err = %err, "group_id resolution failed",
                        );
                        return None;
                    }
                };
                match crate::services::session::thread_index::last_for_group(
                    state_dir,
                    group.id.as_str(),
                ) {
                    Ok(opt) => opt,
                    Err(err) => {
                        tracing::warn!(
                            op = "resume.resolve",
                            group_id = %group.id.as_str(),
                            err = %err,
                            "thread index last_for_group failed",
                        );
                        return None;
                    }
                }
            }
        }
    };

    let entry = entry?;
    let account_id: crate::services::account::AccountId = match entry.account.parse() {
        Ok(id) => id,
        Err(reason) => {
            tracing::warn!(
                op = "resume.resolve",
                thread_id = %entry.thread_id,
                account = %entry.account,
                reason = %reason,
                "malformed account in thread index; falling back",
            );
            return None;
        }
    };

    tracing::info!(
        op = "resume.resolve",
        thread_id = %entry.thread_id,
        account = %account_id,
        group_id = %entry.group_id,
        "thread index hit",
    );

    Some((
        crate::services::account::resolver::ResolvedAccount {
            id: account_id,
            source: crate::services::account::resolver::AccountResolutionSource::ThreadIndex,
        },
        entry.thread_id,
        entry.group_id,
    ))
}

fn run_resume(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
    intent: &ResumeIntent,
    gated: &crate::services::account::resolver::ResolvedAccount,
) -> Result<i32, crate::error::AppError> {
    tracing::info!(op = "resume", status = "start", ?intent);

    if let Some((resolved, thread_id, original_group_id)) = resolve_resume_account(ctx, intent) {
        let effective_argv = match intent {
            ResumeIntent::Last { .. } => rewrite_last_to_id(argv, &thread_id),
            ResumeIntent::ById(_) => argv.to_vec(),
        };

        let gid_override = Some(original_group_id.as_str());

        if ctx.global.dry_run {
            let prepared = prepare_invocation(ctx, &effective_argv, &resolved, gid_override)?;
            let dry_ctx = crate::domain::child_invocation::DryRunContext {
                account: resolved.id.to_string(),
                account_source: crate::services::account::resolver::source_label(resolved.source)
                    .to_owned(),
            };
            ctx.ui.write_dry_run(
                &crate::domain::child_invocation::dry_run_report_with_context(
                    &prepared.invocation,
                    Some(&dry_ctx),
                ),
            )?;
            tracing::info!(op = "resume", status = "ok", outcome = "dry-run");
            return Ok(0);
        }

        let session = SignalSession::install()?;
        let (exit_code, _stdout, _stderr) = run_once(
            ctx,
            &effective_argv,
            &resolved,
            &session,
            false,
            gid_override,
        )?;
        Ok(exit_code)
    } else {
        tracing::info!(
            op = "resume",
            status = "fallback",
            "no thread index hit; falling back to normal resolution"
        );
        let sanitized = strip_wrapper_resume_flags(argv);
        let fallback_argv = sanitized.as_deref().unwrap_or(argv);
        if ctx.global.dry_run {
            let prepared = prepare_invocation(ctx, fallback_argv, gated, None)?;
            let dry_ctx = crate::domain::child_invocation::DryRunContext {
                account: gated.id.to_string(),
                account_source: crate::services::account::resolver::source_label(gated.source)
                    .to_owned(),
            };
            ctx.ui.write_dry_run(
                &crate::domain::child_invocation::dry_run_report_with_context(
                    &prepared.invocation,
                    Some(&dry_ctx),
                ),
            )?;
            tracing::info!(op = "resume", status = "ok", outcome = "dry-run-fallback");
            return Ok(0);
        }
        crate::services::account::retry::run_with_retry(ctx, fallback_argv)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ResumeIntent {
    ById(String),
    Last { all_groups: bool },
}

pub(crate) fn detect_resume(argv: &[std::ffi::OsString]) -> Option<ResumeIntent> {
    let strs: Vec<Option<&str>> = argv.iter().map(|a| a.to_str()).collect();
    match strs.as_slice() {
        // exec resume <SESSION_ID> [...]
        [Some("exec"), Some("resume"), Some(id), ..] | [Some("resume"), Some(id), ..]
            if !id.starts_with('-') =>
        {
            Some(ResumeIntent::ById((*id).to_owned()))
        }
        // exec resume --last --all-groups [...]
        [
            Some("exec"),
            Some("resume"),
            Some("--last"),
            Some("--all-groups"),
            ..,
        ]
        | [Some("exec"), Some("resume"), Some("--all"), ..]
        | [Some("resume"), Some("--all"), ..] => Some(ResumeIntent::Last { all_groups: true }),
        // exec resume --last [...]
        [Some("exec"), Some("resume"), Some("--last"), ..] => {
            Some(ResumeIntent::Last { all_groups: false })
        }
        // resume --last [...]
        [Some("resume"), Some("--last"), ..] => Some(ResumeIntent::Last { all_groups: false }),
        _ => None,
    }
}

fn map_child_err(err: &crate::adapters::spawner::SpawnerError) -> crate::error::AppError {
    match err {
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
    }
}

fn cache_config_path(ctx: &crate::context::AppContext) -> Option<Utf8PathBuf> {
    let path = cache_config_target(ctx);
    path.is_file().then_some(path)
}

/// Canonical cache config target path — used both as the source of the
/// machine-local layer during `compose()` (gated on existence by
/// `cache_config_path`) and as the destination for trust-sync writes.
/// Always `<cache_dir>/configs.toml`.
fn cache_config_target(ctx: &crate::context::AppContext) -> Utf8PathBuf {
    ctx.config.paths.cache_dir.join("configs.toml")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn has_json_flag_present() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("--json"),
            OsString::from("hello"),
        ];
        assert!(has_json_flag(&argv));
    }

    #[test]
    fn has_json_flag_absent() {
        let argv = vec![OsString::from("exec"), OsString::from("hello")];
        assert!(!has_json_flag(&argv));
    }

    #[test]
    fn has_json_flag_empty() {
        assert!(!has_json_flag(&[]));
    }

    #[test]
    fn has_json_flag_not_exact_match() {
        let argv = vec![OsString::from("--json=true")];
        assert!(!has_json_flag(&argv));
    }

    #[test]
    fn detect_resume_exec_resume_by_id() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("abc123"),
        ];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::ById("abc123".into()))
        );
    }

    #[test]
    fn detect_resume_exec_resume_last() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("--last"),
        ];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::Last { all_groups: false })
        );
    }

    #[test]
    fn detect_resume_exec_resume_last_all_groups() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("--last"),
            OsString::from("--all-groups"),
        ];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::Last { all_groups: true })
        );
    }

    #[test]
    fn detect_resume_bare_resume_by_id() {
        let argv = vec![OsString::from("resume"), OsString::from("abc123")];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::ById("abc123".into()))
        );
    }

    #[test]
    fn detect_resume_bare_resume_last() {
        let argv = vec![OsString::from("resume"), OsString::from("--last")];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::Last { all_groups: false })
        );
    }

    #[test]
    fn detect_resume_bare_resume_all() {
        let argv = vec![OsString::from("resume"), OsString::from("--all")];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::Last { all_groups: true })
        );
    }

    #[test]
    fn detect_resume_exec_resume_all() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("--all"),
        ];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::Last { all_groups: true })
        );
    }

    #[test]
    fn detect_resume_non_resume_returns_none() {
        let argv = vec![OsString::from("exec"), OsString::from("status")];
        assert_eq!(detect_resume(&argv), None);
    }

    #[test]
    fn detect_resume_exec_resume_by_id_with_trailing_flags() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("abc123"),
            OsString::from("--json"),
        ];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::ById("abc123".into()))
        );
    }

    #[test]
    fn rewrite_last_to_id_replaces_last() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("--last"),
        ];
        let rewritten = rewrite_last_to_id(&argv, "tid-1");
        assert_eq!(
            rewritten,
            vec![
                OsString::from("exec"),
                OsString::from("resume"),
                OsString::from("tid-1")
            ]
        );
    }

    #[test]
    fn rewrite_last_to_id_strips_all_groups() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("--last"),
            OsString::from("--all-groups"),
            OsString::from("--json"),
        ];
        let rewritten = rewrite_last_to_id(&argv, "tid-1");
        assert_eq!(
            rewritten,
            vec![
                OsString::from("exec"),
                OsString::from("resume"),
                OsString::from("tid-1"),
                OsString::from("--json")
            ]
        );
    }

    #[test]
    fn rewrite_last_to_id_preserves_non_last_argv() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("resume"),
            OsString::from("abc123"),
            OsString::from("--json"),
        ];
        let rewritten = rewrite_last_to_id(&argv, "tid-1");
        assert_eq!(rewritten, argv);
    }

    #[test]
    fn rewrite_last_to_id_strips_bare_all() {
        let argv = vec![OsString::from("resume"), OsString::from("--all")];
        let rewritten = rewrite_last_to_id(&argv, "tid-1");
        assert_eq!(
            rewritten,
            vec![OsString::from("resume"), OsString::from("tid-1")]
        );
    }
}
