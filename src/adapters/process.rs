//! Process adapter.

use std::os::unix::fs::PermissionsExt as _;

/// Process-layer failures.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ProcessError {
    /// Wrapped child could not be found.
    #[error("wrapped child not found")]
    NotFound {
        /// The path or program name we attempted to resolve.
        tried: std::path::PathBuf,
        /// The PATH value consulted, if PATH lookup was used.
        path_searched: Option<std::ffi::OsString>,
    },

    /// Wrapped child exists but is not executable.
    #[error("wrapped child is not executable")]
    NotExecutable {
        /// Path that failed the executable check.
        path: std::path::PathBuf,
    },

    /// `exec()` failed after a binary was found.
    #[error("exec failed: {0}")]
    Exec(#[from] std::io::Error),
}

/// Process operations used by the application.
pub(crate) trait Process {
    /// Resolve the real `codex` binary on `PATH`.
    fn resolve_codex(
        &self,
        child_override: Option<&camino::Utf8Path>,
    ) -> Result<std::path::PathBuf, ProcessError>;

    /// Best-effort first line of `<program> --version`.
    fn child_version_line(&self, program: &std::path::Path) -> Option<String>;

    /// Replace the current process with `program`.
    fn exec_replace(&self, program: &std::path::Path, args: &[std::ffi::OsString]) -> ProcessError;
}

/// Real process adapter.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StdProcess;

impl Process for StdProcess {
    fn resolve_codex(
        &self,
        child_override: Option<&camino::Utf8Path>,
    ) -> Result<std::path::PathBuf, ProcessError> {
        if let Some(path) = child_override {
            let path = path.as_std_path().to_path_buf();
            let metadata = std::fs::metadata(&path).map_err(|_| ProcessError::NotFound {
                tried: path.clone(),
                path_searched: None,
            })?;
            // Reject anything that is not a regular file (directories with
            // search bits set, sockets, fifos, etc.) so callers get the
            // documented exit 126 instead of a downstream `exec()` failure.
            if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
                return Err(ProcessError::NotExecutable { path });
            }
            return path.canonicalize().or(Ok(path));
        }

        which::which("codex").map_err(|_| ProcessError::NotFound {
            tried: std::path::PathBuf::from("codex"),
            path_searched: std::env::var_os("PATH"),
        })
    }

    fn exec_replace(&self, program: &std::path::Path, args: &[std::ffi::OsString]) -> ProcessError {
        use std::os::unix::process::CommandExt as _;

        ProcessError::Exec(std::process::Command::new(program).args(args).exec())
    }

    fn child_version_line(&self, program: &std::path::Path) -> Option<String> {
        // Defense against a misconfigured CODEX_SESSION_CHILD_BIN that points
        // at the wrapper itself (e.g. `which codex-session`). Spawning the
        // wrapper here would re-enter pass-through, re-resolve itself, and
        // exec-replace in a loop while the parent blocks on `output()`.
        if let Ok(self_exe) = std::env::current_exe() {
            let self_canon = self_exe.canonicalize().unwrap_or(self_exe);
            let prog_canon = program
                .canonicalize()
                .unwrap_or_else(|_| program.to_path_buf());
            if self_canon == prog_canon {
                tracing::warn!(
                    op = "self.version.child_probe",
                    status = "skipped",
                    reason = "child_resolves_to_self",
                    child.path = %program.display(),
                    "skipping `<child> --version` probe to avoid self-recursion"
                );
                return None;
            }
        }
        let output = std::process::Command::new(program)
            .arg("--version")
            .output()
            .ok()?;
        let line = String::from_utf8(output.stdout).ok()?;
        line.lines()
            .find(|candidate| !candidate.trim().is_empty())
            .map(ToOwned::to_owned)
    }
}
