#![allow(clippy::missing_errors_doc, clippy::result_large_err)]

pub(crate) mod add;
pub(crate) mod cooldown;
pub(crate) mod current;
pub(crate) mod list;
pub(crate) mod quota;
pub(crate) mod refresh;
pub(crate) mod remove;
pub(crate) mod use_;

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
    pub(crate) name: String,
    pub(crate) path: camino::Utf8PathBuf,
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
    pub(crate) stale: bool,
    pub(crate) live: bool,
    pub(crate) error: Option<String>,
    pub(crate) five_hour: Option<AccountQuotaWindowView>,
    pub(crate) weekly: Option<AccountQuotaWindowView>,
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

pub(super) fn spawn_child(
    ctx: &crate::context::AppContext,
    args: impl IntoIterator<Item = &'static str>,
) -> Result<(), crate::error::AppError> {
    use crate::adapters::spawner::Spawner as _;
    use std::sync::atomic::AtomicI32;

    let binary = ctx.resolved_child().map_err(map_spawner_error)?.clone();
    let invocation = crate::domain::child_invocation::ChildInvocation {
        binary,
        args: args
            .into_iter()
            .map(std::ffi::OsString::from)
            .collect::<Vec<_>>(),
        env: crate::domain::child_invocation::ChildEnv::scrubbed_default(),
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

fn map_spawner_error(err: &crate::adapters::spawner::SpawnerError) -> crate::error::AppError {
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

pub(super) fn copy_native_auth_to_seed(
    ctx: &crate::context::AppContext,
    registry: &crate::services::account::registry::Registry,
    name: &crate::services::account::AccountId,
) -> Result<(), crate::error::AppError> {
    let native_auth = ctx.home_dir().join(".codex").join("auth.json");
    if !native_auth.exists() {
        return Err(crate::services::account::AccountError::NativeAuthMissing.into());
    }
    let bytes = crate::services::auth::secure_file_read(&native_auth)?;
    crate::adapters::fs::atomic_write(&registry.group_auth_seed_path(name), &bytes).map_err(
        |err| match err {
            crate::adapters::fs::FsError::Io { path, source } => {
                crate::services::account::AccountError::RegistryIo { path, source }
            }
            crate::adapters::fs::FsError::SymlinkRefused { path } => {
                crate::services::account::AccountError::RegistryIo {
                    path,
                    source: std::io::Error::other("symlink refused"),
                }
            }
            crate::adapters::fs::FsError::HardlinkRefused { path } => {
                crate::services::account::AccountError::RegistryIo {
                    path,
                    source: std::io::Error::other("hardlink refused"),
                }
            }
            crate::adapters::fs::FsError::BadOwnership { path, .. } => {
                crate::services::account::AccountError::RegistryIo {
                    path,
                    source: std::io::Error::other("bad ownership"),
                }
            }
        },
    )?;
    Ok(())
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
        AccountCommand::Use(args) => use_::run(ctx, &args),
        AccountCommand::Remove(args) => remove::run(ctx, &args),
        AccountCommand::Refresh(args) => refresh::run(ctx, &args),
        AccountCommand::Quota(args) => quota::run(ctx, args),
        AccountCommand::Cooldown(args) => cooldown::run(ctx, args),
    }
}

fn as_unix(ts: Option<std::time::SystemTime>) -> Option<u64> {
    ts.and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|value| value.as_secs())
}
