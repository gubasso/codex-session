//! Process spawner adapter.
//!
//! What this is: the hexagonal port for fork/exec/wait, plus child
//! resolution and the version probe.
//! What this is not: the typed invocation model — that lives in
//! `crate::domain::child_invocation`.

use std::os::unix::fs::PermissionsExt as _;

use camino::{Utf8Path, Utf8PathBuf};

use crate::config::ChildConfig;
use crate::domain::child_invocation::ChildInvocation;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SpawnerError {
    #[error("wrapped child not found")]
    NotFound {
        tried: Utf8PathBuf,
        path_searched: Option<std::ffi::OsString>,
    },
    #[error("wrapped child is not executable: {path}")]
    NotExecutable { path: Utf8PathBuf },
    #[error("exec failed: {0}")]
    Exec(#[from] std::io::Error),
    #[error("child binary resolves to the wrapper itself ({path})")]
    Recursion { path: Utf8PathBuf },
    #[error("non-utf8 child path")]
    NonUtf8Path(#[from] camino::FromPathBufError),
}

pub(crate) trait Spawner {
    fn resolve_child(&self, cfg: &ChildConfig) -> Result<Utf8PathBuf, SpawnerError>;
    fn child_version_line(&self, child: &Utf8Path) -> Option<String>;
    /// Spawn the child, publish its PID via `pid_sink`, then wait.
    ///
    /// The `pid_sink` parameter looks like a leaky abstraction but is
    /// load-bearing: the signal-forwarding path installed by
    /// `services::auth::signal::install` needs the child's PID to forward
    /// signals to. Two designs were considered (see the reviewed plan,
    /// Phase 5):
    ///
    /// (a) thread an `&AtomicI32` through `spawn_and_wait` so the spawner
    ///     publishes the PID after `Command::spawn` returns. *Chosen.*
    /// (b) inline `Command::spawn` + `wait` in `pass_through::run`,
    ///     bypassing the trait for the one caller that needs signal
    ///     forwarding. Rejected because it breaks the hexagonal port
    ///     boundary for every other spawner-using path.
    ///
    /// Mock spawners must also accept the sink (and write a non-zero
    /// value when they "spawn" so the signal thread treats their fake
    /// child as live). The coupling is intentional.
    fn spawn_and_wait(
        &self,
        inv: ChildInvocation,
        pid_sink: &std::sync::atomic::AtomicI32,
    ) -> Result<std::process::ExitStatus, SpawnerError>;
    /// Replace the current process. Returns only on failure.
    #[allow(dead_code)]
    fn exec(&self, inv: ChildInvocation) -> SpawnerError;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StdSpawner;

impl Spawner for StdSpawner {
    fn resolve_child(&self, cfg: &ChildConfig) -> Result<Utf8PathBuf, SpawnerError> {
        // Phase-05 marker check, evaluated first so a user can fix it
        // without untangling the rest of the resolution chain.
        if std::env::var("CODEX_SESSION_REENTRY").as_deref() == Ok("1") {
            let candidate = cfg
                .bin
                .clone()
                .unwrap_or_else(|| Utf8PathBuf::from("codex"));
            return Err(SpawnerError::Recursion { path: candidate });
        }

        let candidate: Utf8PathBuf = if let Some(path) = &cfg.bin {
            path.clone()
        } else {
            Utf8PathBuf::try_from(which::which("codex").map_err(|_| SpawnerError::NotFound {
                tried: Utf8PathBuf::from("codex"),
                path_searched: std::env::var_os("PATH"),
            })?)?
        };

        let meta =
            std::fs::metadata(candidate.as_std_path()).map_err(|_| SpawnerError::NotFound {
                tried: candidate.clone(),
                path_searched: None,
            })?;
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            return Err(SpawnerError::NotExecutable { path: candidate });
        }

        // Phase-05 inode self-check. Best-effort: on canonicalize failure
        // we fall through to literal comparison.
        if let Ok(self_exe) = std::env::current_exe() {
            let self_canon = self_exe.canonicalize().unwrap_or(self_exe);
            let cand_canon = candidate
                .as_std_path()
                .canonicalize()
                .unwrap_or_else(|_| candidate.as_std_path().to_path_buf());
            if self_canon == cand_canon {
                return Err(SpawnerError::Recursion { path: candidate });
            }
        }

        // Preserve current behavior: canonicalize the override path so the
        // resolved path is absolute & symlink-free; fall back to the literal.
        Ok(candidate
            .as_std_path()
            .canonicalize()
            .ok()
            .and_then(|path| Utf8PathBuf::try_from(path).ok())
            .unwrap_or(candidate))
    }

    fn child_version_line(&self, child: &Utf8Path) -> Option<String> {
        // Defense-in-depth: keep the inode self-check here even though
        // `resolve_child` now does the same. `version` and `config status`
        // swallow `SpawnerError::Recursion` via `.ok()`, so if the resolver
        // ever returns Ok on a self-resolving path (e.g. canonicalize
        // races), the probe still won't fork-bomb.
        if let Ok(self_exe) = std::env::current_exe() {
            let self_canon = self_exe.canonicalize().unwrap_or(self_exe);
            let prog_canon = child
                .as_std_path()
                .canonicalize()
                .unwrap_or_else(|_| child.as_std_path().to_path_buf());
            if self_canon == prog_canon {
                tracing::warn!(
                    op = "self.version.child_probe",
                    status = "skipped",
                    reason = "child_resolves_to_self",
                    child.path = %child,
                    "skipping `<child> --version` probe to avoid self-recursion"
                );
                return None;
            }
        }
        let output = std::process::Command::new(child.as_std_path())
            .arg("--version")
            .output()
            .ok()?;
        let line = String::from_utf8(output.stdout).ok()?;
        line.lines()
            .find(|candidate| !candidate.trim().is_empty())
            .map(ToOwned::to_owned)
    }

    fn spawn_and_wait(
        &self,
        inv: ChildInvocation,
        pid_sink: &std::sync::atomic::AtomicI32,
    ) -> Result<std::process::ExitStatus, SpawnerError> {
        use std::sync::atomic::Ordering;

        let mut cmd = inv.into_command();
        let mut child = cmd.spawn().map_err(SpawnerError::Exec)?;
        pid_sink.store(
            i32::try_from(child.id()).unwrap_or(i32::MAX),
            Ordering::SeqCst,
        );
        let status = child.wait().map_err(SpawnerError::Exec)?;
        Ok(status)
    }

    fn exec(&self, inv: ChildInvocation) -> SpawnerError {
        use std::os::unix::process::CommandExt as _;
        let mut cmd = inv.into_command();
        SpawnerError::from(cmd.exec())
    }
}
