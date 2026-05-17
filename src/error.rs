//! Crate-level error type and exit-code mapping.
#![allow(clippy::must_use_candidate)]

/// Application-wide error type.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// Missing `codex` on `PATH`.
    #[error("ERROR: codex binary not found in PATH")]
    CodexNotFound,

    /// Missing base config for forced merge.
    #[error("ERROR: base config not found at {0}")]
    BaseMissing(std::path::PathBuf),

    /// Unknown `self` subcommand.
    #[error("unknown self verb: {0}\nrun: codex-session self help")]
    UnknownSelfVerb(String),

    /// Unexpected I/O failure.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Opaque application-edge failure.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    /// Convert to the process exit code.
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::CodexNotFound | Self::BaseMissing(_) => 1,
            Self::UnknownSelfVerb(_) => 2,
            Self::Io(_) => 74,
            Self::Other(_) => 70,
        }
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
    fn io_is_seventy_four() {
        assert_eq!(
            AppError::Io(std::io::Error::from(std::io::ErrorKind::Other)).exit_code(),
            74
        );
    }

    #[test]
    fn other_is_seventy() {
        assert_eq!(AppError::Other(anyhow::anyhow!("boom")).exit_code(), 70);
    }
}
