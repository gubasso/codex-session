//! Process adapter.

/// Process operations used by the application.
pub trait Process {
    /// Resolve the real `codex` binary on `PATH`.
    fn resolve_codex(&self) -> Option<std::path::PathBuf>;

    /// Replace the current process with `program`.
    fn exec_replace(
        &self,
        program: &std::path::Path,
        args: &[std::ffi::OsString],
    ) -> std::io::Error;
}

/// Real process adapter.
#[derive(Debug, Clone, Copy)]
pub struct StdProcess;

impl Process for StdProcess {
    fn resolve_codex(&self) -> Option<std::path::PathBuf> {
        which::which("codex").ok()
    }

    fn exec_replace(
        &self,
        program: &std::path::Path,
        args: &[std::ffi::OsString],
    ) -> std::io::Error {
        use std::os::unix::process::CommandExt as _;

        std::process::Command::new(program).args(args).exec()
    }
}
