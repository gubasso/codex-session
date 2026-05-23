//! Filesystem write adapter.
//!
//! What this is: atomic byte writes plus destination validation for
//! wrapper-owned state files.
//! What this is not: higher-level auth/account policy.

use std::fs::{OpenOptions, Permissions};
use std::io::{Read as _, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};

use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum FsError {
    #[error("io error at {path}: {source}")]
    Io {
        path: Utf8PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("refusing to write through symlink at {path}")]
    SymlinkRefused { path: Utf8PathBuf },
    #[error("refusing to write through hardlinked file at {path}")]
    HardlinkRefused { path: Utf8PathBuf },
    #[error("bad ownership on {path}: uid {actual} != expected {expected}")]
    BadOwnership {
        path: Utf8PathBuf,
        actual: u32,
        expected: u32,
    },
}

pub(crate) fn atomic_write(path: &Utf8Path, contents: &[u8]) -> Result<(), FsError> {
    let Some(parent) = path.parent() else {
        return Err(FsError::Io {
            path: path.to_path_buf(),
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, "target has no parent"),
        });
    };

    let mut temp =
        tempfile::NamedTempFile::new_in(parent.as_std_path()).map_err(|source| FsError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    temp.write_all(contents).map_err(|source| FsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    temp.as_file_mut()
        .sync_all()
        .map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?;
    temp.as_file_mut()
        .set_permissions(Permissions::from_mode(0o600))
        .map_err(|source| FsError::Io {
            path: path.to_path_buf(),
            source,
        })?;

    match std::fs::symlink_metadata(path.as_std_path()) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() {
                return Err(FsError::SymlinkRefused {
                    path: path.to_path_buf(),
                });
            }
            if metadata.nlink() > 1 {
                return Err(FsError::HardlinkRefused {
                    path: path.to_path_buf(),
                });
            }
            let expected = current_uid();
            let actual = metadata.uid();
            if actual != expected {
                return Err(FsError::BadOwnership {
                    path: path.to_path_buf(),
                    actual,
                    expected,
                });
            }
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(FsError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    }

    temp.persist(path.as_std_path())
        .map_err(|err| FsError::Io {
            path: path.to_path_buf(),
            source: err.error,
        })?;
    Ok(())
}

/// Read a wrapper-owned state file with the same hardening as
/// `atomic_write` applies on the destination: refuse symlinks (via
/// `O_NOFOLLOW`), refuse hardlinks (`nlink > 1`), and refuse files not
/// owned by the current UID. Returns `Ok(None)` if the file does not
/// exist; the caller picks the empty-vs-missing semantics. Use this for
/// any state file (cooldown, last-account, etc.) where an attacker who
/// can plant a symlink in the parent dir could otherwise hand the wrapper
/// arbitrary bytes from outside the registry root.
pub(crate) fn secure_read(path: &Utf8Path) -> Result<Option<Vec<u8>>, FsError> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path.as_std_path())
    {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) if err.raw_os_error() == Some(libc::ELOOP) => {
            return Err(FsError::SymlinkRefused {
                path: path.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(FsError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let metadata = file.metadata().map_err(|source| FsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.nlink() > 1 {
        return Err(FsError::HardlinkRefused {
            path: path.to_path_buf(),
        });
    }
    let expected = current_uid();
    let actual = metadata.uid();
    if actual != expected {
        return Err(FsError::BadOwnership {
            path: path.to_path_buf(),
            actual,
            expected,
        });
    }
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(|source| FsError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(Some(bytes))
}

fn current_uid() -> u32 {
    rustix::process::getuid().as_raw()
}
