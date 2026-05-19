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

    if let Some(candidate) = runtime_candidate.as_ref() {
        if validate_root(candidate).is_ok() {
            return Ok(SessionRoot {
                path: candidate.clone(),
                source: SessionRootSource::Runtime,
            });
        }
    }

    if validate_root(&state_candidate).is_ok() {
        return Ok(SessionRoot {
            path: state_candidate,
            source: SessionRootSource::State,
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
    terminal_id: &str,
) -> Result<Utf8PathBuf, crate::config::ConfigError> {
    let sessions = root.join("sessions");
    secure_dir(&sessions)?;

    let session_dir = sessions.join(terminal_id);
    secure_dir(&session_dir)?;
    Ok(session_dir)
}

fn validate_root(path: &Utf8Path) -> Result<(), crate::config::ConfigError> {
    secure_dir(path)?;
    secure_dir(&path.join("sessions"))?;
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
    std::process::Command::new("id")
        .arg("-u")
        .output()
        .ok()
        .and_then(|output| {
            output
                .status
                .success()
                .then(|| String::from_utf8(output.stdout).ok())
                .flatten()
        })
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(u32::MAX)
}
