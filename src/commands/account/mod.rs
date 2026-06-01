#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) mod add;
pub(crate) mod cooldown;
pub(crate) mod current;
pub(crate) mod health;
pub(crate) mod list;
pub(crate) mod quota;
pub(crate) mod refresh;
pub(crate) mod remove;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountListView {
    pub(crate) active: Option<AccountCurrentView>,
    pub(crate) accounts: Vec<AccountListEntryView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountListEntryView {
    pub(crate) name: String,
    pub(crate) dir: camino::Utf8PathBuf,
    pub(crate) has_auth: bool,
    pub(crate) last_used_at_unix: Option<u64>,
    pub(crate) current: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountCurrentView {
    pub(crate) name: String,
    pub(crate) source: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountMutationView {
    pub(crate) verb: &'static str,
    pub(crate) name: String,
    pub(crate) path: camino::Utf8PathBuf,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountScoringView {
    pub(crate) base: f64,
    pub(crate) plan_bonus: f64,
    pub(crate) recency: f64,
    pub(crate) recency_label: String,
    pub(crate) avail_score: f64,
    pub(crate) five_hour_pct: Option<f64>,
    pub(crate) weekly_pct: Option<f64>,
    pub(crate) five_hour_weight: f64,
    pub(crate) weekly_pressure: f64,
    pub(crate) fh_pressure: f64,
    pub(crate) pressure_label: String,
    pub(crate) total: f64,
    pub(crate) eligible: bool,
    pub(crate) ineligible_reason: Option<String>,
    pub(crate) tie_five_hour: Option<f64>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaWindowView {
    pub(crate) percent_left: f64,
    pub(crate) reset_at_unix: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountQuotaEntryView {
    pub(crate) account: String,
    pub(crate) active: bool,
    pub(crate) mode: String,
    pub(crate) fetched_at_unix: u64,
    pub(crate) ttl_secs: u64,
    pub(crate) error: Option<String>,
    pub(crate) five_hour: Option<AccountQuotaWindowView>,
    pub(crate) weekly: Option<AccountQuotaWindowView>,
    pub(crate) score: Option<f64>,
    pub(crate) rank: Option<usize>,
    pub(crate) status_label: String,
    pub(crate) scoring: Option<AccountScoringView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountHealthEntryView {
    pub(crate) account: String,
    pub(crate) token: String,
    pub(crate) token_detail: String,
    pub(crate) plan: String,
    pub(crate) score: Option<f64>,
    pub(crate) rank: Option<usize>,
    pub(crate) status: String,
    pub(crate) active: bool,
    pub(crate) cooldown: bool,
    pub(crate) last_used: Option<u64>,
    pub(crate) fetched_at_unix: u64,
    pub(crate) score_label: String,
    pub(crate) scoring: Option<AccountScoringView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountHealthView {
    pub(crate) entries: Vec<AccountHealthEntryView>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct AccountCooldownView {
    pub(crate) entries: Vec<AccountCooldownEntryView>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct AccountCooldownEntryView {
    pub(crate) account: String,
    pub(crate) cooled_down: bool,
    pub(crate) reset_at_unix: Option<u64>,
    pub(crate) reset_in_seconds: Option<u64>,
    pub(crate) reason: Option<String>,
    pub(crate) last_429_at_unix: Option<u64>,
}

#[allow(dead_code)]
pub(crate) fn spawn_child(
    ctx: &crate::context::AppContext,
    args: impl IntoIterator<Item = &'static str>,
) -> Result<(), crate::error::AppError> {
    spawn_child_isolated(ctx, args, None)
}

pub(crate) fn spawn_child_isolated(
    ctx: &crate::context::AppContext,
    args: impl IntoIterator<Item = &'static str>,
    codex_home: Option<&camino::Utf8Path>,
) -> Result<(), crate::error::AppError> {
    use crate::adapters::spawner::Spawner as _;
    use std::sync::atomic::AtomicI32;

    let binary = ctx.resolved_child().map_err(map_spawner_error)?.clone();
    ctx.ensure_child_version()?;
    let mut env = crate::domain::child_invocation::ChildEnv::scrubbed_default();
    if let Some(home) = codex_home {
        env.set
            .push(("CODEX_HOME".to_owned(), home.as_os_str().to_owned()));
    }
    let invocation = crate::domain::child_invocation::ChildInvocation {
        binary,
        args: args
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        env,
    };
    let status = ctx
        .spawner
        .spawn_and_wait(invocation, &AtomicI32::new(0))
        .map_err(crate::error::AppError::from)?;
    if status.success() {
        Ok(())
    } else {
        Err(crate::error::AppError::ChildExitNonZero(
            status.code().unwrap_or(1),
        ))
    }
}

pub(crate) fn map_spawner_error(
    err: &crate::adapters::spawner::SpawnerError,
) -> crate::error::AppError {
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

/// Create a temporary directory for isolated auth operations.
///
/// Placed under `state_dir/auth-ops/`, NOT `/tmp` — codex refuses to
/// create helper binaries when `CODEX_HOME` is under a temporary
/// filesystem (see `heartbeat_probe` in `gate.rs`).  Returns a
/// `TempDir` handle; the caller keeps it alive as long as needed.
pub(crate) fn create_auth_ops_dir(
    ctx: &crate::context::AppContext,
) -> Result<tempfile::TempDir, crate::error::AppError> {
    let parent = ctx.config.paths.state_dir.join("auth-ops");
    std::fs::create_dir_all(parent.as_std_path()).map_err(|source| {
        crate::error::AppError::Other(anyhow::anyhow!(
            "failed to create auth-ops dir {parent}: {source}"
        ))
    })?;
    tempfile::tempdir_in(parent.as_std_path()).map_err(|source| {
        crate::error::AppError::Other(anyhow::anyhow!(
            "failed to create auth-ops tmpdir: {source}"
        ))
    })
}

/// Run `codex logout` + `codex login` inside an isolated `CODEX_HOME`.
///
/// Returns `(dir_handle, auth_json_path)`.  The caller must keep
/// `dir_handle` alive until `persist_auth_to_seed` has copied the
/// token — the temporary directory is cleaned up on drop.
pub(crate) fn run_isolated_login(
    ctx: &crate::context::AppContext,
) -> Result<(tempfile::TempDir, camino::Utf8PathBuf), crate::error::AppError> {
    // Surface a too-old codex as ChildVersionTooOld (exit 78) before we
    // spawn anything — otherwise the failure would be wrapped as a generic
    // LoginFailed (exit 75) with the retry hint, hiding the upgrade path.
    ctx.ensure_child_version()?;

    let dir = create_auth_ops_dir(ctx)?;
    let home = camino::Utf8PathBuf::try_from(dir.path().to_path_buf())
        .map_err(|err| crate::error::AppError::Other(anyhow::anyhow!("{err}")))?;

    // Precautionary logout — no-op in the empty temp dir (no auth.json
    // to find/revoke), which is exactly what we want.
    let _ = spawn_child_isolated(ctx, ["logout"], Some(&home)).inspect_err(|err| {
        tracing::warn!(
            op = "isolated_login",
            outcome = "logout-failed-non-fatal",
            error = %err
        );
    });

    if let Err(err) = spawn_child_isolated(ctx, ["login"], Some(&home)) {
        // A child-version mismatch is a hard, user-actionable error —
        // propagate it as exit 78 instead of folding into LoginFailed.
        if matches!(err, crate::error::AppError::ChildVersionTooOld { .. }) {
            return Err(err);
        }
        return Err(crate::services::account::AccountError::LoginFailed {
            detail: err.to_string(),
        }
        .into());
    }

    let auth_path = home.join("auth.json");
    Ok((dir, auth_path))
}

/// Copy an `auth.json` from `source` into the account seed.
pub(crate) fn persist_auth_to_seed(
    source: &camino::Utf8Path,
    registry: &crate::services::account::registry::Registry,
    name: &crate::services::account::AccountId,
) -> Result<(), crate::error::AppError> {
    if !source.as_std_path().exists() {
        return Err(crate::services::account::AccountError::NativeAuthMissing.into());
    }
    let bytes = crate::services::auth::secure_file_read(source)?;
    crate::adapters::fs::atomic_write(&registry.group_auth_seed_path(name), &bytes)
        .map_err(map_fs_error)?;
    // With CODEX_HOME isolation the source is a temp dir — cleanup
    // happens on TempDir drop. This deletion is defense-in-depth.
    match std::fs::remove_file(source.as_std_path()) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            tracing::warn!(
                op = "persist_auth_to_seed",
                path = %source,
                error = %err,
                "failed to delete source auth file"
            );
        }
    }
    Ok(())
}

/// Legacy wrapper — reads from `~/.codex/auth.json`.  Prefer
/// `persist_auth_to_seed` with an explicit source path.
#[allow(dead_code)]
pub(crate) fn move_native_auth_to_seed(
    ctx: &crate::context::AppContext,
    registry: &crate::services::account::registry::Registry,
    name: &crate::services::account::AccountId,
) -> Result<(), crate::error::AppError> {
    let native_auth = ctx.home_dir().join(".codex").join("auth.json");
    persist_auth_to_seed(&native_auth, registry, name)
}

fn map_fs_error(err: crate::adapters::fs::FsError) -> crate::error::AppError {
    match err {
        crate::adapters::fs::FsError::Io { path, source } => {
            crate::services::account::AccountError::RegistryIo { path, source }.into()
        }
        crate::adapters::fs::FsError::SymlinkRefused { path } => {
            crate::services::account::AccountError::RegistryIo {
                path,
                source: std::io::Error::other("symlink refused"),
            }
            .into()
        }
        crate::adapters::fs::FsError::HardlinkRefused { path } => {
            crate::services::account::AccountError::RegistryIo {
                path,
                source: std::io::Error::other("hardlink refused"),
            }
            .into()
        }
        crate::adapters::fs::FsError::BadOwnership { path, .. } => {
            crate::services::account::AccountError::RegistryIo {
                path,
                source: std::io::Error::other("bad ownership"),
            }
            .into()
        }
    }
}

pub(crate) fn dispatch(
    ctx: &crate::context::AppContext,
    args: crate::cli::account::AccountArgs,
) -> Result<(), crate::error::AppError> {
    use crate::cli::account::AccountCommand;

    match args.command {
        AccountCommand::Add(args) => add::run(ctx, &args),
        AccountCommand::List(args) => list::run(ctx, args),
        AccountCommand::Current(args) => current::run(ctx, args),
        AccountCommand::Remove(args) => remove::run(ctx, &args),
        AccountCommand::Refresh(args) => refresh::run(ctx, &args),
        AccountCommand::Quota(args) => quota::run(ctx, args),
        AccountCommand::Health(args) => health::run(ctx, args),
        AccountCommand::Cooldown(args) => cooldown::run(ctx, args),
    }
}

fn as_unix(ts: Option<std::time::SystemTime>) -> Option<u64> {
    ts.and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
}
