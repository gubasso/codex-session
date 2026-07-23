//! Pass-through command path.
//!
//! What this is: the handler for forwarded child invocations (including
//! bare `codex-session`, which forwards an empty child argv -> Codex TUI).
//! What this is not: clap parsing or child process execution primitives.
#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

use camino::Utf8PathBuf;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicI32;

use crate::adapters::spawner::Spawner as _;
use crate::clock::now_unix;
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

    if let Some(intent) = detect_resume(argv) {
        return run_resume(ctx, argv, &intent);
    }

    let outcome = crate::services::account::gate::ensure(ctx)?;

    if ctx.global.dry_run {
        let resolved = match &outcome {
            crate::services::account::gate::GateOutcome::Resolved(resolved) => resolved.clone(),
            crate::services::account::gate::GateOutcome::AutoDeferred => {
                crate::services::account::resolver::resolve_for_exec(ctx, &HashSet::new())?
            }
        };
        let prepared = prepare_invocation(ctx, argv, &resolved, None)?;
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
        tracing::info!(op = "pass-through", status = "ok", outcome = "dry-run");
        return Ok(0);
    }
    match outcome {
        crate::services::account::gate::GateOutcome::Resolved(resolved) => {
            crate::services::account::retry::single_attempt(ctx, argv, &resolved)
        }
        crate::services::account::gate::GateOutcome::AutoDeferred => {
            if is_interactive_passthrough(argv) {
                crate::services::account::retry::run_auto_interactive(ctx, argv)
            } else {
                crate::services::account::retry::run_auto(ctx, argv)
            }
        }
    }
}

/// Run the child for one attempt. Returns the exit code plus stdout and
/// stderr buffers.
///
/// When `capture` is `true`, the child's streams are tee'd to the
/// parent's real stdio **and** independently captured (each capped at
/// `failover::MAX_CAPTURE_BYTES`). The two streams are returned
/// separately so callers can run `failover::classify_run` on each without
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
    ctx.ensure_child_version()?;
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
                profiles_dir: ctx.config.config_recipe.profiles_dir.clone(),
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

pub(crate) fn has_json_flag(argv: &[std::ffi::OsString]) -> bool {
    argv.iter().any(|arg| arg.to_str() == Some("--json"))
}

/// Shape-only: argv looks like an interactive TUI launch (not exec, not --json).
/// Pure and deterministic so it can be unit-tested without a real terminal.
fn argv_is_interactive_shape(argv: &[std::ffi::OsString]) -> bool {
    if has_json_flag(argv) {
        return false;
    }
    // Forwarded argv always starts with a non-flag verb token: clap's
    // `external_subcommand` capture consumes wrapper global flags first and
    // rejects unknown leading flags with EX_USAGE, so `--flag exec ...` can
    // never reach this predicate.
    let first = argv.first().and_then(|a| a.to_str()).unwrap_or("");
    // `codex exec ...` and `codex exec resume <id>` are non-interactive streaming runs.
    first != "exec"
}

/// True when this launch is an interactive TUI attached to a real terminal, so the
/// child must inherit stdio (codex checks isatty on stdout/stdin and refuses a pipe).
///
/// Deliberately stricter than `gate::ensure`'s stdin-only interactive check:
/// the stdio capture decision requires an stdout terminal too (codex's
/// `isatty(stdout)` startup check), whereas prompt gating only needs stdin.
fn is_interactive_passthrough(argv: &[std::ffi::OsString]) -> bool {
    argv_is_interactive_shape(argv)
        && crate::ui::terminal::stdout_is_terminal()
        && crate::ui::terminal::stdin_is_terminal()
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

struct ResumeResolution {
    resolved: crate::services::account::resolver::ResolvedAccount,
    thread_id: String,
    group_id: String,
    recovered_from_scan: bool,
    /// When the owner was recovered via rollout scan (index miss), the entry to
    /// backfill into the thread index. Persisted only on the real execution
    /// path — never under `--dry-run`, which must not mutate on-disk state.
    recovered_entry: Option<crate::services::session::thread_index::ThreadEntry>,
}

#[allow(clippy::too_many_lines)]
fn resolve_resume_account(
    ctx: &crate::context::AppContext,
    intent: &ResumeIntent,
) -> Result<ResumeResolution, crate::error::AppError> {
    use crate::services::account::resolver::{AccountResolutionSource, ResolvedAccount};

    let state_dir = &ctx.config.paths.state_dir;

    match intent {
        ResumeIntent::ById(id) => {
            if let Some(entry) = crate::services::session::thread_index::lookup(state_dir, id)? {
                let account_id: crate::services::account::AccountId =
                    entry.account.parse().map_err(|reason| {
                        anyhow::anyhow!(
                            "thread index entry for `{}` contains malformed account `{}`: {}",
                            entry.thread_id,
                            entry.account,
                            reason
                        )
                    })?;
                tracing::info!(
                    op = "resume.resolve",
                    thread_id = %entry.thread_id,
                    account = %account_id,
                    group_id = %entry.group_id,
                    source = "thread-index",
                );
                return Ok(ResumeResolution {
                    resolved: ResolvedAccount {
                        id: account_id,
                        source: AccountResolutionSource::ThreadIndex,
                    },
                    thread_id: entry.thread_id,
                    group_id: entry.group_id,
                    recovered_from_scan: false,
                    recovered_entry: None,
                });
            }

            let registry = crate::services::account::registry::Registry::from_config(&ctx.config);
            let accounts = registry.list()?;
            let account_ids: Vec<_> = accounts.into_iter().map(|entry| entry.id).collect();
            let inspected = crate::services::session::dir::inspect_session_root(
                ctx.config.paths.runtime_dir.as_deref(),
                &ctx.config.paths.state_dir,
            )?;
            if !inspected.root_missing
                && !inspected.accounts_subdir_missing
                && let Some(owner) = crate::services::session::rollout_scan::find_owner(
                    &inspected.root.path,
                    &account_ids,
                    id,
                )
            {
                tracing::warn!(
                    op = "resume.resolve",
                    thread_id = %id,
                    account = %owner.account,
                    group_id = %owner.group_id,
                    session_dir = %owner.session_dir,
                    rollout_path = %owner.rollout_path,
                    "recovered owner from rollout scan"
                );
                // Defer the user-facing warning and the index backfill to the
                // real execution path in `run_resume`: under `--dry-run` we must
                // not emit a "pinned resume" notice or mutate `thread-index.jsonl`.
                let recovered_entry = crate::services::session::thread_index::ThreadEntry {
                    thread_id: id.clone(),
                    account: owner.account.to_string(),
                    group_id: owner.group_id.clone(),
                    cwd: current_cwd()?,
                    created_at: crate::services::session::thread_index::utc_now_rfc3339(),
                };
                return Ok(ResumeResolution {
                    resolved: ResolvedAccount {
                        id: owner.account,
                        source: AccountResolutionSource::RolloutScan,
                    },
                    thread_id: id.clone(),
                    group_id: owner.group_id,
                    recovered_from_scan: true,
                    recovered_entry: Some(recovered_entry),
                });
            }

            let recent = crate::services::session::thread_index::recent_entries(state_dir, 5)?
                .into_iter()
                .map(|entry| crate::services::account::error::ThreadCandidate {
                    thread_id: entry.thread_id,
                    account: entry.account,
                    group_id: entry.group_id,
                    created_at: entry.created_at,
                })
                .collect();
            Err(crate::services::account::AccountError::ResumeOwnerMissing {
                thread_id: id.clone(),
                recent,
            }
            .into())
        }
        ResumeIntent::Last { all_groups } => {
            let entry = if *all_groups {
                crate::services::session::thread_index::last_any(state_dir)?
            } else {
                let group = crate::services::session::group_id::current(ctx)?;
                crate::services::session::thread_index::last_for_group(
                    state_dir,
                    group.id.as_str(),
                )?
            };
            let Some(entry) = entry else {
                return Err(crate::services::account::AccountError::ResumeIndexEmpty {
                    scope: if *all_groups {
                        crate::services::account::error::ResumeIndexScope::AllGroups
                    } else {
                        crate::services::account::error::ResumeIndexScope::CurrentGroup
                    },
                }
                .into());
            };
            let account_id: crate::services::account::AccountId =
                entry.account.parse().map_err(|reason| {
                    anyhow::anyhow!(
                        "thread index entry for `{}` contains malformed account `{}`: {}",
                        entry.thread_id,
                        entry.account,
                        reason
                    )
                })?;
            Ok(ResumeResolution {
                resolved: crate::services::account::resolver::ResolvedAccount {
                    id: account_id,
                    source:
                        crate::services::account::resolver::AccountResolutionSource::ThreadIndex,
                },
                thread_id: entry.thread_id,
                group_id: entry.group_id,
                recovered_from_scan: false,
                recovered_entry: None,
            })
        }
    }
}

fn run_resume(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
    intent: &ResumeIntent,
) -> Result<i32, crate::error::AppError> {
    tracing::info!(op = "resume", status = "start", ?intent);

    let ResumeResolution {
        resolved,
        thread_id,
        group_id: original_group_id,
        recovered_from_scan,
        recovered_entry,
    } = resolve_resume_account(ctx, intent)?;

    // Resume is owner-bound (F16): an explicit `--account`/env pin cannot move
    // a thread to another account, so a disagreeing pin is overridden. Say so
    // instead of silently dropping the flag.
    if let crate::services::account::resolver::AccountIntent::Pinned { id: pinned, .. } =
        crate::services::account::resolver::intent(ctx)?
        && pinned != resolved.id
    {
        ctx.ui.write_warning(&format!(
            "warning: --account '{pinned}' ignored; thread {thread_id} owned by {} \
            (resume always pins to owner)",
            resolved.id
        ))?;
        tracing::warn!(
            op = "resume",
            pinned = %pinned,
            owner = %resolved.id,
            "explicit account pin overridden by thread owner"
        );
    }

    let effective_argv = match intent {
        ResumeIntent::Last { .. } => rewrite_last_to_id(argv, &thread_id),
        ResumeIntent::ById(_) => argv.to_vec(),
    };
    let gid_override = Some(original_group_id.as_str());

    let registry = crate::services::account::registry::Registry::from_config(&ctx.config);

    // The resume path bypasses `gate::ensure`, so mirror its auth assessment
    // here (see `gate::assess`): first confirm the owner account still exists
    // (stale index / rollout data can resolve to a removed account → `NotFound`
    // with a real remediation), then check the auth seed. A bare seed
    // `.exists()` check alone would misreport a deleted account as `AuthMissing`
    // and hint `account refresh <name>` for an account that no longer exists.
    registry.expect_account_dir(&resolved.id)?;
    let seed = registry.group_auth_seed_path(&resolved.id);
    if !seed.as_std_path().exists() {
        return Err(
            crate::services::account::AccountError::AuthMissing { name: resolved.id }.into(),
        );
    }

    if let Some(owner) = resume_preflight_block(ctx, &registry, &resolved)? {
        return Err(crate::services::account::AccountError::ResumeBlocked {
            thread_id,
            owner,
            others: resume_other_lines(ctx, &registry, &resolved.id)?,
        }
        .into());
    }

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
        tracing::info!(
            op = "resume",
            status = "ok",
            outcome = if recovered_from_scan {
                "dry-run-scan-recovery"
            } else {
                "dry-run"
            }
        );
        return Ok(0);
    }

    // Real execution only (never under `--dry-run`): announce the rollout-scan
    // recovery and backfill the thread index so the next resume is a clean hit.
    if let Some(entry) = recovered_entry {
        announce_and_backfill_recovery(ctx, &thread_id, &resolved.id, &entry)?;
    }

    let session = SignalSession::install()?;
    let interactive = is_interactive_passthrough(argv);
    let (exit_code, stdout, stderr) = run_once(
        ctx,
        &effective_argv,
        &resolved,
        &session,
        !interactive,
        gid_override,
    )?;
    if !interactive
        && let Some(err) = resume_blocked_from_live_rate_limit(
            ctx,
            &registry,
            &resolved,
            &thread_id,
            &original_group_id,
            exit_code,
            has_json_flag(&effective_argv),
            &stdout,
            &stderr,
        )?
    {
        return Err(err.into());
    }
    Ok(exit_code)
}

/// On the real (non-dry-run) execution path, emit the rollout-scan recovery
/// warning and backfill the thread index so the next resume is a clean index
/// hit. A failed append is non-fatal (logged, not surfaced).
fn announce_and_backfill_recovery(
    ctx: &crate::context::AppContext,
    thread_id: &str,
    owner: &crate::services::account::AccountId,
    entry: &crate::services::session::thread_index::ThreadEntry,
) -> Result<(), crate::error::AppError> {
    ctx.ui.write_warning(&format!(
        "warning: thread index had no entry for thread {thread_id}; \
        recovered owner '{owner}' from rollout store; pinned resume to it"
    ))?;
    if let Err(err) =
        crate::services::session::thread_index::append(&ctx.config.paths.state_dir, entry)
    {
        tracing::warn!(
            op = "thread_index.append",
            thread_id = %entry.thread_id,
            err = %err,
            "failed to append recovered thread index entry"
        );
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn resume_blocked_from_live_rate_limit(
    ctx: &crate::context::AppContext,
    registry: &crate::services::account::registry::Registry,
    resolved: &crate::services::account::resolver::ResolvedAccount,
    thread_id: &str,
    group_id: &str,
    exit_code: i32,
    json_mode: bool,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Option<crate::services::account::AccountError>, crate::error::AppError> {
    let events = crate::services::account::codex_events::scan_events(stdout);
    let Some(classification) = crate::services::account::failover::classify_run(
        &events, exit_code, json_mode, stdout, stderr,
    ) else {
        return Ok(None);
    };
    // The resume path is account-bound and cannot rotate, so only a rate limit
    // or credit exhaustion blocks the resume (-> `ResumeBlocked`). The unhandled categories that
    // `run_auto` surfaces via `codex_unhandled_error` get the same styled
    // stderr + log-pointer UX here too, instead of letting codex's raw output
    // stand silently. Auth failures keep the existing pass-through behavior.
    match &classification.category {
        crate::services::account::failover::Category::RateLimit(_) => {}
        crate::services::account::failover::Category::CreditExhausted => {
            // Resume is owner-bound, so credit exhaustion blocks it just like
            // a rate limit — but with credit-specific labels, and a
            // reset-aware cooldown derived from the owner's own usage windows
            // (the error carries no reset; see `retry::credit_cooldown` and
            // docs/upstream-codex.md §F9).
            let (reset, source) =
                crate::services::account::retry::credit_cooldown(ctx, &resolved.id);
            crate::services::account::retry::write_cooldown(
                registry,
                &resolved.id,
                "credits",
                &classification.snippet,
                reset,
                source,
            )?;
            let owner = crate::services::account::retry::account_outcome_line(
                ctx,
                registry,
                &resolved.id,
                crate::services::account::error::OutcomeState::CreditExhausted,
                format!("out of credits: {}", classification.snippet),
            );
            return Ok(Some(
                crate::services::account::AccountError::ResumeBlocked {
                    thread_id: thread_id.to_owned(),
                    owner,
                    others: resume_other_lines(ctx, registry, &resolved.id)?,
                },
            ));
        }
        crate::services::account::failover::Category::NoRolloutFound => {
            let inspected = crate::services::session::dir::inspect_session_root(
                ctx.config.paths.runtime_dir.as_deref(),
                &ctx.config.paths.state_dir,
            )?;
            let has_local_rollout = !inspected.root_missing
                && !inspected.accounts_subdir_missing
                && crate::services::session::rollout_scan::owner_has_thread(
                    &inspected.root.path,
                    &resolved.id,
                    group_id,
                    thread_id,
                );
            return Ok(Some(
                crate::services::account::AccountError::ResumeNoRollout {
                    thread_id: thread_id.to_owned(),
                    owner: resolved.id.clone(),
                    reason: if has_local_rollout {
                        crate::services::account::error::ResumeNoRolloutReason::SandboxMismatch
                    } else {
                        crate::services::account::error::ResumeNoRolloutReason::RolloutMissing
                    },
                    snippet: classification.snippet.clone(),
                },
            ));
        }
        crate::services::account::failover::Category::ContextWindowExceeded
        | crate::services::account::failover::Category::ServerError
        | crate::services::account::failover::Category::Unclassified => {
            return Err(crate::services::account::retry::codex_unhandled_error(
                ctx,
                &classification,
                exit_code,
            ));
        }
        crate::services::account::failover::Category::AuthFailure => return Ok(None),
    }

    crate::services::account::retry::write_cooldown(
        registry,
        &resolved.id,
        "429",
        &classification.snippet,
        classification.reset_after_seconds,
        classification.reset_source,
    )?;
    let owner = crate::services::account::retry::account_outcome_line(
        ctx,
        registry,
        &resolved.id,
        crate::services::account::error::OutcomeState::RateLimited429,
        format!("429 rate limit: {}", classification.snippet),
    );
    Ok(Some(
        crate::services::account::AccountError::ResumeBlocked {
            thread_id: thread_id.to_owned(),
            owner,
            others: resume_other_lines(ctx, registry, &resolved.id)?,
        },
    ))
}

fn resume_preflight_block(
    ctx: &crate::context::AppContext,
    registry: &crate::services::account::registry::Registry,
    resolved: &crate::services::account::resolver::ResolvedAccount,
) -> Result<Option<crate::services::account::error::AccountOutcomeLine>, crate::error::AppError> {
    let account_root = registry.account_dir(&resolved.id);
    if let Some(cd) = crate::services::account::cooldown::read(&account_root)
        .map_err(crate::services::account::AccountError::from)?
        && crate::services::account::cooldown::is_active(&cd, now_unix())
    {
        return Ok(Some(crate::services::account::retry::account_outcome_line(
            ctx,
            registry,
            &resolved.id,
            crate::services::account::error::OutcomeState::Cooldown,
            format!("cooldown active: {}", cd.reason),
        )));
    }

    let ttl = std::time::Duration::from_secs(ctx.config.account.quota_ttl_secs);
    let Ok(crate::services::account::quota::QuotaResult::Ok(quota)) =
        crate::services::account::quota::get(ctx, &resolved.id, ttl)
    else {
        return Ok(None);
    };
    if let Some(five_hour) = quota
        .five_hour
        .as_ref()
        .filter(|window| window.percent_left <= 0.0)
    {
        return Ok(Some(crate::services::account::retry::account_outcome_line(
            ctx,
            registry,
            &resolved.id,
            crate::services::account::error::OutcomeState::FiveHourExhausted,
            format!(
                "five-hour quota exhausted ({:.1}% left)",
                five_hour.percent_left
            ),
        )));
    }
    if let Some(weekly) = quota
        .weekly
        .as_ref()
        .filter(|window| window.percent_left <= 0.0)
    {
        return Ok(Some(crate::services::account::retry::account_outcome_line(
            ctx,
            registry,
            &resolved.id,
            crate::services::account::error::OutcomeState::WeeklyExhausted,
            format!("weekly quota exhausted ({:.1}% left)", weekly.percent_left),
        )));
    }

    Ok(None)
}

fn resume_other_lines(
    ctx: &crate::context::AppContext,
    registry: &crate::services::account::registry::Registry,
    owner: &crate::services::account::AccountId,
) -> Result<Vec<crate::services::account::error::AccountOutcomeLine>, crate::error::AppError> {
    let mut lines = Vec::new();
    for entry in registry.list()? {
        if &entry.id == owner {
            continue;
        }
        let (state, outcome) =
            crate::services::account::retry::alternate_state(ctx, registry, &entry);
        lines.push(crate::services::account::retry::account_outcome_line(
            ctx, registry, &entry.id, state, outcome,
        ));
    }
    Ok(lines)
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum ResumeIntent {
    ById(String),
    Last { all_groups: bool },
}

pub(crate) fn detect_resume(argv: &[std::ffi::OsString]) -> Option<ResumeIntent> {
    let strs: Vec<Option<&str>> = argv.iter().map(|a| a.to_str()).collect();

    // Locate the `resume` subcommand and the tokens that follow it.
    //
    // Two accepted argv shapes:
    //   * `resume <...>` — bare resume (first token).
    //   * `exec [exec-opts...] resume <...>` — resume nested under `exec`,
    //     with optional `exec`-level options (e.g. `--profile implementation`)
    //     between `exec` and `resume`. The `prex` stage-3 invocation uses this
    //     shape, so it must reach the owner-pinning resume router rather than
    //     falling through to normal pass-through.
    //
    // Tokens between `exec` and `resume` are only allowed to be option flags
    // (or their values); a bare positional there means this is not a resume we
    // own (e.g. `exec status resume`).
    let rest: &[Option<&str>] = match strs.first().copied().flatten() {
        Some("resume") => &strs[1..],
        Some("exec") => {
            let resume_idx = strs[1..].iter().position(|t| *t == Some("resume"))? + 1;
            if !exec_opts_only(&strs[1..resume_idx]) {
                return None;
            }
            &strs[resume_idx + 1..]
        }
        _ => return None,
    };

    match rest {
        // resume --last --all-groups [...] | resume --all [...]
        [Some("--last"), Some("--all-groups"), ..] | [Some("--all"), ..] => {
            Some(ResumeIntent::Last { all_groups: true })
        }
        // resume --last [...]
        [Some("--last"), ..] => Some(ResumeIntent::Last { all_groups: false }),
        // resume <SESSION_ID> [...]
        [Some(id), ..] if !id.starts_with('-') => Some(ResumeIntent::ById((*id).to_owned())),
        _ => None,
    }
}

/// Returns `true` if every token between `exec` and `resume` is an option
/// flag or an option value (i.e. no bare positional). A leading non-flag
/// token (other than a flag's value) means the argv is not an `exec … resume`
/// we should intercept.
fn exec_opts_only(between: &[Option<&str>]) -> bool {
    let mut prev_was_flag_expecting_value = false;
    for tok in between {
        match tok {
            Some(t) if t.starts_with('-') => {
                // `--flag=value` carries its own value; a bare `--flag` may
                // consume the next token as its value.
                prev_was_flag_expecting_value = !t.contains('=');
            }
            // A value immediately following a flag is allowed.
            _ if prev_was_flag_expecting_value => {
                prev_was_flag_expecting_value = false;
            }
            // A bare positional that is not a flag value: not our resume.
            _ => return false,
        }
    }
    true
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

    fn av(items: &[&str]) -> Vec<OsString> {
        items.iter().map(|s| OsString::from(*s)).collect()
    }

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
    fn argv_is_interactive_shape_classifies() {
        // Interactive (TUI) shapes:
        assert!(argv_is_interactive_shape(&av(&[]))); // bare TUI
        assert!(argv_is_interactive_shape(&av(&["resume"]))); // interactive picker
        assert!(argv_is_interactive_shape(&av(&["resume", "ID"]))); // interactive resume
        assert!(argv_is_interactive_shape(&av(&["some prompt"]))); // prompt-only TUI
        // Non-interactive shapes:
        assert!(!argv_is_interactive_shape(&av(&["exec", "do"]))); // exec
        assert!(!argv_is_interactive_shape(&av(&["exec", "resume", "ID"]))); // exec resume
        assert!(!argv_is_interactive_shape(&av(&["--json"]))); // json forces capture
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
    fn detect_resume_exec_profile_resume_by_id() {
        // The prex stage-3 shape: `exec --profile implementation resume <id>`.
        let argv = vec![
            OsString::from("exec"),
            OsString::from("--profile"),
            OsString::from("implementation"),
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
    fn detect_resume_exec_profile_eq_resume_last() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("--profile=implementation"),
            OsString::from("resume"),
            OsString::from("--last"),
        ];
        assert_eq!(
            detect_resume(&argv),
            Some(ResumeIntent::Last { all_groups: false })
        );
    }

    #[test]
    fn detect_resume_exec_profile_resume_all_groups() {
        let argv = vec![
            OsString::from("exec"),
            OsString::from("--profile"),
            OsString::from("implementation"),
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
    fn detect_resume_exec_positional_then_resume_returns_none() {
        // A bare positional between `exec` and `resume` is not our resume.
        let argv = vec![
            OsString::from("exec"),
            OsString::from("status"),
            OsString::from("resume"),
            OsString::from("abc123"),
        ];
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
