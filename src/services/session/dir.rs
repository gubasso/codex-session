//! Secure session-root resolution.
//!
//! What this is: secure creation and validation of runtime/state-backed session roots.
//! What this is not: profile composition or child-env handling.

#![allow(clippy::result_large_err)]

use camino::{Utf8Path, Utf8PathBuf};
use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

#[derive(Debug, Clone)]
pub(crate) struct SessionRoot {
    pub(crate) path: Utf8PathBuf,
    pub(crate) source: SessionRootSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionRootSource {
    Runtime,
    State,
}

pub(crate) fn resolve_session_root(
    runtime_dir: Option<&Utf8Path>,
    state_dir: &Utf8Path,
) -> Result<SessionRoot, crate::config::ConfigError> {
    let runtime_candidate = runtime_dir.map(Utf8Path::to_path_buf);
    let state_candidate = state_dir.to_path_buf();

    if validate_root(&state_candidate).is_ok() {
        return Ok(SessionRoot {
            path: state_candidate,
            source: SessionRootSource::State,
        });
    }

    if let Some(candidate) = runtime_candidate.as_ref()
        && validate_root(candidate).is_ok()
    {
        return Ok(SessionRoot {
            path: candidate.clone(),
            source: SessionRootSource::Runtime,
        });
    }

    Err(crate::config::ConfigError::SessionDirUnresolvable {
        runtime: runtime_candidate,
        state: Some(state_candidate),
        reason: "neither XDG runtime nor XDG state produced a secure session root".to_owned(),
    })
}

pub(crate) fn session_dir(
    root: &Utf8Path,
    account: &str,
    group_id: &str,
) -> Result<Utf8PathBuf, crate::config::ConfigError> {
    let accounts = root.join("accounts");
    secure_dir(&accounts)?;
    let account = accounts.join(account);
    secure_dir(&account)?;
    let groups = account.join("groups");
    secure_dir(&groups)?;
    let group = groups.join(group_id);
    secure_dir(&group)?;
    Ok(group)
}

/// Result of a non-mutating session-root inspection.
#[derive(Debug, Clone)]
pub(crate) struct InspectedRoot {
    pub(crate) root: SessionRoot,
    /// True when the chosen root directory itself does not yet exist on
    /// disk. The execution path will create it on first compose /
    /// pass-through; `doctor` should surface this as a friendly "not yet
    /// initialized" warning rather than a hard failure.
    pub(crate) root_missing: bool,
    /// True when the root directory exists and is valid, but the
    /// `accounts/` subdirectory has not been created yet. (`true` is also
    /// implied whenever `root_missing` is `true`.)
    pub(crate) accounts_subdir_missing: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct InspectedSessionDir {
    pub(crate) path: Utf8PathBuf,
    #[allow(dead_code)]
    pub(crate) missing: bool,
}

/// Read-only inspection variant of [`resolve_session_root`].
///
/// Returns the first candidate that is or could legitimately be promoted
/// to a session root, without creating or chmoding anything. The
/// preference order matches [`resolve_session_root`]: state first, then
/// runtime.
///
/// A candidate is considered usable when either:
///   * the candidate itself exists, is a non-symlink directory, is owned
///     by the current uid (its `accounts/` subdirectory may or may not
///     exist), or
///   * the candidate does not exist yet but its parent directory exists
///     and is owned by the current uid — meaning the regular execution
///     path would create it. In this case `root_missing == true` and
///     `accounts_subdir_missing == true`.
///
/// Unlike [`resolve_session_root`], this function never calls
/// `create_dir_all` or `set_permissions`, so it is safe from validation
/// paths like `codex-session doctor`.
pub(crate) fn inspect_session_root(
    runtime_dir: Option<&Utf8Path>,
    state_dir: &Utf8Path,
) -> Result<InspectedRoot, crate::config::ConfigError> {
    let runtime_candidate = runtime_dir.map(Utf8Path::to_path_buf);
    let state_candidate = state_dir.to_path_buf();

    if let Some(status) = inspect_root(&state_candidate) {
        return Ok(InspectedRoot {
            root: SessionRoot {
                path: state_candidate,
                source: SessionRootSource::State,
            },
            root_missing: status.root_missing,
            accounts_subdir_missing: status.accounts_subdir_missing,
        });
    }

    if let Some(candidate) = runtime_candidate.as_ref()
        && let Some(status) = inspect_root(candidate)
    {
        return Ok(InspectedRoot {
            root: SessionRoot {
                path: candidate.clone(),
                source: SessionRootSource::Runtime,
            },
            root_missing: status.root_missing,
            accounts_subdir_missing: status.accounts_subdir_missing,
        });
    }

    Err(crate::config::ConfigError::SessionDirUnresolvable {
        runtime: runtime_candidate,
        state: Some(state_candidate),
        reason: "neither XDG runtime nor XDG state has a usable session root".to_owned(),
    })
}

pub(crate) fn inspect_session_dir(
    root: &Utf8Path,
    account: &str,
    group_id: &str,
) -> Result<InspectedSessionDir, crate::config::ConfigError> {
    let path = root
        .join("accounts")
        .join(account)
        .join("groups")
        .join(group_id);
    let missing = !path.as_std_path().exists();
    if missing {
        return Ok(InspectedSessionDir {
            path,
            missing: true,
        });
    }
    inspect_dir(&path)?;
    Ok(InspectedSessionDir {
        path,
        missing: false,
    })
}

#[derive(Debug, Clone, Copy)]
struct RootStatus {
    root_missing: bool,
    accounts_subdir_missing: bool,
}

/// Returns `Some(status)` when the candidate is usable (either already a
/// valid owned dir, or missing-but-its-parent-is-a-valid-owned-dir);
/// `None` when the candidate cannot be promoted to a session root at all.
fn inspect_root(path: &Utf8Path) -> Option<RootStatus> {
    match inspect_dir(path) {
        Ok(()) => {
            let accounts = path.join("accounts");
            let accounts_missing = match inspect_dir(&accounts) {
                Ok(()) => false,
                Err(_) if !accounts.as_std_path().exists() => true,
                Err(_) => return None,
            };
            Some(RootStatus {
                root_missing: false,
                accounts_subdir_missing: accounts_missing,
            })
        }
        Err(_) if !path.as_std_path().exists() => {
            // Path itself is missing — fall back to inspecting the parent.
            // If the parent is a valid owned directory, the execution
            // path will create the session root there on first use.
            let parent = path.parent()?;
            inspect_dir(parent).ok()?;
            Some(RootStatus {
                root_missing: true,
                accounts_subdir_missing: true,
            })
        }
        Err(_) => None,
    }
}

fn inspect_dir(path: &Utf8Path) -> Result<(), crate::config::ConfigError> {
    let metadata = std::fs::symlink_metadata(path.as_std_path())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(crate::config::ConfigError::SessionDirUnresolvable {
            runtime: None,
            state: Some(path.to_path_buf()),
            reason: format!("{path} is not a real directory"),
        });
    }
    if metadata.uid() != current_uid() {
        return Err(crate::config::ConfigError::SessionDirUnresolvable {
            runtime: None,
            state: Some(path.to_path_buf()),
            reason: format!("{path} is not owned by the current uid"),
        });
    }
    Ok(())
}

fn validate_root(path: &Utf8Path) -> Result<(), crate::config::ConfigError> {
    secure_dir(path)?;
    secure_dir(&path.join("accounts"))?;
    Ok(())
}

fn secure_dir(path: &Utf8Path) -> Result<(), crate::config::ConfigError> {
    if std::fs::symlink_metadata(path.as_std_path()).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(crate::config::ConfigError::SessionDirUnresolvable {
            runtime: None,
            state: Some(path.to_path_buf()),
            reason: format!("{path} is a symlink"),
        });
    }

    std::fs::create_dir_all(path.as_std_path())?;
    let metadata = std::fs::symlink_metadata(path.as_std_path())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(crate::config::ConfigError::SessionDirUnresolvable {
            runtime: None,
            state: Some(path.to_path_buf()),
            reason: format!("{path} is not a real directory"),
        });
    }

    if metadata.uid() != current_uid() {
        return Err(crate::config::ConfigError::SessionDirUnresolvable {
            runtime: None,
            state: Some(path.to_path_buf()),
            reason: format!("{path} is not owned by the current uid"),
        });
    }

    std::fs::set_permissions(path.as_std_path(), std::fs::Permissions::from_mode(0o700))?;
    Ok(())
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}
