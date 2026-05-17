//! Process adapter.

/// Process-layer failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ProcessError {
    /// Missing `codex` on `PATH`.
    #[error("codex binary not found in PATH")]
    CodexNotFound,

    /// `exec()` failed after a binary was found.
    #[error("exec failed: {0}")]
    Exec(#[from] std::io::Error),
}

/// Process operations used by the application.
pub(crate) trait Process {
    /// Resolve the real `codex` binary on `PATH`.
    fn resolve_codex(&self) -> Result<std::path::PathBuf, ProcessError>;

    /// Replace the current process with `program`.
    fn exec_replace(&self, program: &std::path::Path, args: &[std::ffi::OsString]) -> ProcessError;
}

/// Real process adapter.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StdProcess;

impl Process for StdProcess {
    fn resolve_codex(&self) -> Result<std::path::PathBuf, ProcessError> {
        which::which("codex").map_err(|_| ProcessError::CodexNotFound)
    }

    fn exec_replace(&self, program: &std::path::Path, args: &[std::ffi::OsString]) -> ProcessError {
        use std::os::unix::process::CommandExt as _;

        ProcessError::Exec(std::process::Command::new(program).args(args).exec())
    }
}
