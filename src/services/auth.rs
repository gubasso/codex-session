//! Bridge auth.json between native ~/.codex and per-session `CODEX_HOME`.
//!
//! What this is: copy-in/copy-back of upstream codex's auth.json under
//! flock and a `tokens.last_refresh` timestamp guard.
//! What this is not: token issuance, refresh, or any cryptography.

pub(crate) mod signal;

use std::fs::{File, OpenOptions, Permissions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug)]
pub(crate) struct AuthBridge {
    #[allow(dead_code)]
    native_dir: Utf8PathBuf,
    native_auth: Utf8PathBuf,
    lockfile: Utf8PathBuf,
    session_auth: Utf8PathBuf,
    seeded_refresh_ts: Option<SystemTime>,
}

impl AuthBridge {
    pub(crate) fn new(home: &Utf8Path, session_dir: &Utf8Path) -> Result<Self, AuthError> {
        let native_dir = home.join(".codex");
        ensure_owned_dir_0700(&native_dir)?;
        Ok(Self {
            native_auth: native_dir.join("auth.json"),
            lockfile: native_dir.join(".auth.json.lock"),
            session_auth: session_dir.join("auth.json"),
            native_dir,
            seeded_refresh_ts: None,
        })
    }

    pub(crate) fn seed_into_session(&mut self) -> Result<(), AuthError> {
        with_lock(&self.lockfile, || {
            if !self.native_auth.exists() {
                self.seeded_refresh_ts = None;
                return Ok(());
            }

            let bytes = secure_file_read(&self.native_auth)?;
            self.seeded_refresh_ts = last_refresh_from_json(&bytes);
            secure_file_write_atomic(&self.session_auth, &bytes)?;
            Ok(())
        })
    }

    pub(crate) fn persist_to_native(&self) -> Result<(), AuthError> {
        if !self.session_auth.exists() {
            tracing::debug!(op = "auth.persist", outcome = "no-session-file");
            return Ok(());
        }

        let bytes = secure_file_read(&self.session_auth)?;
        let session_ts = last_refresh_from_json(&bytes).unwrap_or(UNIX_EPOCH);

        with_lock(&self.lockfile, || {
            let native_ts = if self.native_auth.exists() {
                let native_bytes = secure_file_read(&self.native_auth)?;
                last_refresh_from_json(&native_bytes).unwrap_or(UNIX_EPOCH)
            } else {
                UNIX_EPOCH
            };

            if !self.native_auth.exists() || session_ts > native_ts {
                secure_file_write_atomic(&self.native_auth, &bytes)?;
                tracing::info!(
                    op = "auth.persist",
                    outcome = "wrote",
                    path = %self.native_auth,
                    seeded_refresh_ts = ?self.seeded_refresh_ts
                );
            } else {
                tracing::info!(
                    op = "auth.persist",
                    outcome = "skip-stale",
                    path = %self.native_auth,
                    seeded_refresh_ts = ?self.seeded_refresh_ts
                );
            }
            Ok(())
        })
    }

    #[allow(dead_code)]
    pub(crate) fn session_auth_path(&self) -> &Utf8Path {
        &self.session_auth
    }
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AuthError {
    #[error("auth: io error at {path}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("auth: refused symlink at {path}")]
    SymlinkRefused { path: Utf8PathBuf },
    #[error("auth: refused hardlinked file at {path} (nlink > 1)")]
    HardlinkRefused { path: Utf8PathBuf },
    #[error("auth: bad ownership at {path}")]
    BadOwnership { path: Utf8PathBuf },
    #[error("auth: failed to acquire lock at {path}")]
    LockFailed {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[allow(dead_code)]
    #[error("auth: malformed json at {path}")]
    JsonParse {
        path: Utf8PathBuf,
        #[source]
        source: serde_json::Error,
    },
}

impl AuthError {
    /// Stable machine-readable error kind.
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "auth-io",
            Self::SymlinkRefused { .. } => "auth-symlink-refused",
            Self::HardlinkRefused { .. } => "auth-hardlink-refused",
            Self::BadOwnership { .. } => "auth-bad-ownership",
            Self::LockFailed { .. } => "auth-lock-failed",
            Self::JsonParse { .. } => "auth-json-parse",
        }
    }

    /// Path the error was reported against.
    pub(crate) fn path(&self) -> &Utf8Path {
        match self {
            Self::Io { path, .. }
            | Self::SymlinkRefused { path }
            | Self::HardlinkRefused { path }
            | Self::BadOwnership { path }
            | Self::LockFailed { path, .. }
            | Self::JsonParse { path, .. } => path.as_path(),
        }
    }
}

fn ensure_owned_dir_0700(path: &Utf8Path) -> Result<(), AuthError> {
    if std::fs::symlink_metadata(path.as_std_path()).is_ok_and(|meta| meta.file_type().is_symlink())
    {
        return Err(AuthError::SymlinkRefused {
            path: path.to_path_buf(),
        });
    }

    std::fs::create_dir_all(path.as_std_path()).map_err(|source| AuthError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let metadata =
        std::fs::symlink_metadata(path.as_std_path()).map_err(|source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(AuthError::SymlinkRefused {
            path: path.to_path_buf(),
        });
    }
    if metadata.uid() != current_uid() {
        return Err(AuthError::BadOwnership {
            path: path.to_path_buf(),
        });
    }
    std::fs::set_permissions(path.as_std_path(), Permissions::from_mode(0o700)).map_err(
        |source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        },
    )?;
    Ok(())
}

fn secure_file_read(path: &Utf8Path) -> Result<Vec<u8>, AuthError> {
    let mut file = open_nofollow_read(path)?;
    let metadata = file.metadata().map_err(|source| AuthError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.nlink() != 1 {
        return Err(AuthError::HardlinkRefused {
            path: path.to_path_buf(),
        });
    }
    if metadata.uid() != current_uid() || metadata.permissions().mode() & 0o077 != 0 {
        return Err(AuthError::BadOwnership {
            path: path.to_path_buf(),
        });
    }

    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|source| AuthError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(bytes)
}

fn secure_file_write_atomic(target: &Utf8Path, contents: &[u8]) -> Result<(), AuthError> {
    let Some(parent) = target.parent() else {
        return Err(AuthError::Io {
            path: target.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no parent"),
        });
    };

    let mut temp =
        tempfile::NamedTempFile::new_in(parent.as_std_path()).map_err(|source| AuthError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    temp.write_all(contents).map_err(|source| AuthError::Io {
        path: target.to_path_buf(),
        source,
    })?;
    temp.as_file_mut()
        .sync_all()
        .map_err(|source| AuthError::Io {
            path: target.to_path_buf(),
            source,
        })?;
    temp.as_file_mut()
        .set_permissions(Permissions::from_mode(0o600))
        .map_err(|source| AuthError::Io {
            path: target.to_path_buf(),
            source,
        })?;

    // Use symlink_metadata (which does NOT follow symlinks) rather than
    // `target.exists()`. The latter returns false for dangling symlinks, so
    // a dangling symlink at `target` would skip validation and silently be
    // renamed over by `persist`. Only treat NotFound as "no destination".
    match std::fs::symlink_metadata(target.as_std_path()) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(AuthError::SymlinkRefused {
                    path: target.to_path_buf(),
                });
            }
            if metadata.nlink() > 1 {
                return Err(AuthError::HardlinkRefused {
                    path: target.to_path_buf(),
                });
            }
            if metadata.uid() != current_uid() {
                return Err(AuthError::BadOwnership {
                    path: target.to_path_buf(),
                });
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(AuthError::Io {
                path: target.to_path_buf(),
                source,
            });
        }
    }

    // `persist` is an atomic rename into place. A concurrent attacker would
    // need to swap the destination after the validation above and before the
    // rename; that residual race is accepted for this wrapper threat model.
    temp.persist(target.as_std_path())
        .map_err(|err| AuthError::Io {
            path: target.to_path_buf(),
            source: err.error,
        })?;
    Ok(())
}

fn with_lock<R>(
    lockfile: &Utf8Path,
    f: impl FnOnce() -> Result<R, AuthError>,
) -> Result<R, AuthError> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .custom_flags(libc::O_CLOEXEC)
        .open(lockfile.as_std_path())
        .map_err(|source| AuthError::LockFailed {
            path: lockfile.to_path_buf(),
            source,
        })?;

    rustix::fs::flock(&file, rustix::fs::FlockOperation::LockExclusive).map_err(|errno| {
        AuthError::LockFailed {
            path: lockfile.to_path_buf(),
            source: std::io::Error::from_raw_os_error(errno.raw_os_error()),
        }
    })?;

    let result = f();
    let _ = rustix::fs::flock(&file, rustix::fs::FlockOperation::Unlock);
    result
}

// Missing/malformed `tokens.last_refresh` returns `None`, which callers treat
// as `UNIX_EPOCH`. This is deliberate: the rollback-protection contract is
// "the newer copy wins, ties skip." Treating a parse failure as epoch lets a
// well-formed session file replace a corrupt native file, instead of
// quarantining the user behind a stale token they can't refresh. The
// `AuthError::JsonParse` variant is reserved for a future strict-mode
// validator that wants to surface parse failures explicitly.
fn last_refresh_from_json(bytes: &[u8]) -> Option<SystemTime> {
    let value = serde_json::from_slice::<serde_json::Value>(bytes).ok()?;
    let ts = value.get("tokens")?.get("last_refresh")?.as_str()?;
    let parsed =
        time::OffsetDateTime::parse(ts, &time::format_description::well_known::Rfc3339).ok()?;
    let unix = parsed.unix_timestamp();
    if unix <= 0 {
        return Some(UNIX_EPOCH);
    }
    Some(UNIX_EPOCH + Duration::from_secs(u64::try_from(unix).ok()?))
}

fn open_nofollow_read(path: &Utf8Path) -> Result<File, AuthError> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.as_std_path())
        .map_err(|source| {
            if source.raw_os_error() == Some(libc::ELOOP) {
                AuthError::SymlinkRefused {
                    path: path.to_path_buf(),
                }
            } else {
                AuthError::Io {
                    path: path.to_path_buf(),
                    source,
                }
            }
        })
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}
