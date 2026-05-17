//! Crate-level error type and exit-code mapping.
#![allow(clippy::must_use_candidate)]

/// Application-wide error type.
#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// Missing `codex` on `PATH`.
    #[error("ERROR: codex binary not found in PATH")]
    CodexNotFound,

    /// Missing base config for forced merge.
    #[error("ERROR: base config not found at {0}")]
    BaseMissing(std::path::PathBuf),

    /// Unknown `self` subcommand.
    #[error("unknown self verb: {0}\nrun: codex-session self help")]
    UnknownSelfVerb(String),

    /// Process-layer failure after dispatch.
    #[error(transparent)]
    Process(#[from] crate::adapters::process::ProcessError),

    /// Filesystem adapter failure.
    #[error(transparent)]
    Fs(#[from] crate::adapters::fs::FsError),

    /// Merge service failure.
    #[error(transparent)]
    Merge(#[from] crate::services::merge::MergeError),

    /// Unexpected I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Opaque application-edge failure.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    /// Convert to the process exit code.
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::CodexNotFound | Self::BaseMissing(_) => 1,
            Self::UnknownSelfVerb(_) => 2,
            Self::Fs(err) | Self::Merge(crate::services::merge::MergeError::Fs(err)) => {
                fs_error_exit_code(err)
            }
            Self::Io(err) if err.kind() == std::io::ErrorKind::NotFound => 66,
            Self::Io(err) if err.kind() == std::io::ErrorKind::PermissionDenied => 77,
            Self::Process(_) | Self::Io(_) => 74,
            Self::Other(_) => 70,
        }
    }
}

fn fs_error_exit_code(err: &crate::adapters::fs::FsError) -> u8 {
    use crate::adapters::fs::FsError;
    match err {
        FsError::Read { source, .. } if source.kind() == std::io::ErrorKind::NotFound => 66,
        FsError::Read { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => 77,
        FsError::Stat { source, .. } if source.kind() == std::io::ErrorKind::NotFound => 66,
        FsError::Stat { source, .. } if source.kind() == std::io::ErrorKind::PermissionDenied => 77,
        FsError::Write { source, .. }
        | FsError::Mkdir { source, .. }
        | FsError::Touch { source, .. }
            if source.kind() == std::io::ErrorKind::PermissionDenied =>
        {
            77
        }
        FsError::Read { .. }
        | FsError::Write { .. }
        | FsError::Mkdir { .. }
        | FsError::Stat { .. }
        | FsError::Touch { .. } => 74,
    }
}

#[cfg(test)]
mod tests {
    use super::AppError;

    #[test]
    fn codex_not_found_is_one() {
        assert_eq!(AppError::CodexNotFound.exit_code(), 1);
    }

    #[test]
    fn base_missing_is_one() {
        assert_eq!(
            AppError::BaseMissing(std::path::PathBuf::from("/tmp/base")).exit_code(),
            1
        );
    }

    #[test]
    fn unknown_self_verb_is_two() {
        assert_eq!(
            AppError::UnknownSelfVerb(String::from("nope")).exit_code(),
            2
        );
    }

    #[test]
    fn process_is_seventy_four() {
        assert_eq!(
            AppError::Process(crate::adapters::process::ProcessError::Exec(
                std::io::Error::from(std::io::ErrorKind::Other),
            ))
            .exit_code(),
            74
        );
    }

    #[test]
    fn io_not_found_is_sixty_six() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::NotFound)).exit_code(),
            66
        );
    }

    #[test]
    fn io_permission_denied_is_seventy_seven() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::PermissionDenied)).exit_code(),
            77
        );
    }

    #[test]
    fn io_other_is_seventy_four() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::Other)).exit_code(),
            74
        );
    }

    #[test]
    fn fs_read_not_found_is_sixty_six() {
        assert_eq!(
            AppError::Fs(crate::adapters::fs::FsError::Read {
                path: std::path::PathBuf::from("/tmp/missing"),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
            .exit_code(),
            66
        );
    }

    #[test]
    fn fs_read_permission_denied_is_seventy_seven() {
        assert_eq!(
            AppError::Fs(crate::adapters::fs::FsError::Read {
                path: std::path::PathBuf::from("/tmp/secret"),
                source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
            })
            .exit_code(),
            77
        );
    }

    #[test]
    fn fs_write_is_seventy_four() {
        assert_eq!(
            AppError::Fs(crate::adapters::fs::FsError::Write {
                path: std::path::PathBuf::from("/tmp/out"),
                source: std::io::Error::from(std::io::ErrorKind::Other),
            })
            .exit_code(),
            74
        );
    }

    #[test]
    fn merge_fs_write_other_is_seventy_four() {
        assert_eq!(
            AppError::Merge(crate::services::merge::MergeError::Fs(
                crate::adapters::fs::FsError::Write {
                    path: std::path::PathBuf::from("/tmp/out"),
                    source: std::io::Error::from(std::io::ErrorKind::Other),
                },
            ))
            .exit_code(),
            74
        );
    }

    #[test]
    fn merge_fs_read_not_found_is_sixty_six() {
        assert_eq!(
            AppError::Merge(crate::services::merge::MergeError::Fs(
                crate::adapters::fs::FsError::Read {
                    path: std::path::PathBuf::from("/tmp/missing"),
                    source: std::io::Error::from(std::io::ErrorKind::NotFound),
                },
            ))
            .exit_code(),
            66
        );
    }

    #[test]
    fn merge_fs_write_permission_denied_is_seventy_seven() {
        assert_eq!(
            AppError::Merge(crate::services::merge::MergeError::Fs(
                crate::adapters::fs::FsError::Write {
                    path: std::path::PathBuf::from("/tmp/out"),
                    source: std::io::Error::from(std::io::ErrorKind::PermissionDenied),
                },
            ))
            .exit_code(),
            77
        );
    }

    #[test]
    fn other_is_seventy() {
        assert_eq!(AppError::Other(anyhow::anyhow!("boom")).exit_code(), 70);
    }
}
