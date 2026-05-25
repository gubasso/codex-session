//! One-shot auth import plus hardened file helpers.
//!
//! What this is: the minimal auth surface still needed after `AuthBridge`
//! retirement.
//! What this is not: a live sync loop, a signal-forwarding owner, or any
//! timestamp-based rollback protection.

use std::fs::{File, OpenOptions, Permissions};
use std::io::Read as _;
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};

use camino::{Utf8Path, Utf8PathBuf};

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
}

impl AuthError {
    pub(crate) const fn kind(&self) -> &'static str {
        match self {
            Self::Io { .. } => "auth-io",
            Self::SymlinkRefused { .. } => "auth-symlink-refused",
            Self::HardlinkRefused { .. } => "auth-hardlink-refused",
            Self::BadOwnership { .. } => "auth-bad-ownership",
        }
    }

    pub(crate) fn path(&self) -> &Utf8Path {
        match self {
            Self::Io { path, .. }
            | Self::SymlinkRefused { path }
            | Self::HardlinkRefused { path }
            | Self::BadOwnership { path } => path.as_path(),
        }
    }
}

impl From<crate::adapters::fs::FsError> for AuthError {
    fn from(err: crate::adapters::fs::FsError) -> Self {
        match err {
            crate::adapters::fs::FsError::Io { path, source } => Self::Io { path, source },
            crate::adapters::fs::FsError::SymlinkRefused { path } => Self::SymlinkRefused { path },
            crate::adapters::fs::FsError::HardlinkRefused { path } => {
                Self::HardlinkRefused { path }
            }
            crate::adapters::fs::FsError::BadOwnership { path, .. } => Self::BadOwnership { path },
        }
    }
}

pub(crate) fn ensure_owned_dir_0700(path: &Utf8Path) -> Result<(), AuthError> {
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

pub(crate) fn secure_file_read(path: &Utf8Path) -> Result<Vec<u8>, AuthError> {
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
