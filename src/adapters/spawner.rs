//! Process spawner adapter.
//!
//! What this is: the hexagonal port for fork/exec/wait, plus child
//! resolution and the version probe.
//! What this is not: the typed invocation model — that lives in
//! `crate::domain::child_invocation`.

use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::thread;

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
    /// [`install_signal_forwarding`] needs the child's PID to forward signals
    /// to. Two designs were considered (see the reviewed plan, Phase 5):
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
    fn spawn_and_wait_output(
        &self,
        inv: ChildInvocation,
        pid_sink: &std::sync::atomic::AtomicI32,
    ) -> Result<ChildOutput, SpawnerError>;
    /// Replace the current process. Returns only on failure.
    #[allow(dead_code)]
    fn exec(&self, inv: ChildInvocation) -> SpawnerError;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StdSpawner;

pub(crate) struct ChildOutput {
    pub(crate) status: std::process::ExitStatus,
    pub(crate) stdout: Vec<u8>,
    pub(crate) stderr: Vec<u8>,
}

// `SignalGuard` keeps the dispatch thread alive for the wrapper's lifetime
// but deliberately does NOT unregister the installed handlers on drop. The
// `Signals::forever()` iterator only exits when the underlying pipe FD is
// closed, which would require a parallel close-handle path through
// `signal_hook` registration objects. Since the wrapper is a short-lived
// process — it exits as soon as the child does — the OS reclaims both the
// dispatch thread and the registered handlers at exit. Adding proper
// teardown would complicate the module without any observable user benefit.
// If a long-running embedder ever links this module, revisit.
pub(crate) struct SignalGuard {
    _handle: thread::JoinHandle<()>,
    observed_signal: Arc<AtomicUsize>,
}

pub(crate) fn install_signal_forwarding(
    child_pid: Arc<AtomicI32>,
) -> Result<SignalGuard, std::io::Error> {
    // Two registration paths for the same fatal signals are deliberate:
    //
    //   1. `flag::register_usize` below installs a tiny async-signal-safe
    //      OS handler that writes the signal number into a slot. This
    //      runs synchronously inside the OS signal handler — before the
    //      kernel returns to userspace — so by the time `waitpid` returns
    //      from the child, the slot is guaranteed to be populated. If we
    //      recorded the signal from the iterator thread instead, the
    //      child could die from the forwarded signal and `waitpid` could
    //      return before the iterator scheduled, leaving the slot empty.
    //
    //   2. `Signals::new(..)` + the spawned thread below drives the
    //      "forward to child PID" logic. It cannot run in the OS handler
    //      because `kill(2)` is async-signal-safe but our pid bookkeeping
    //      uses `AtomicI32::load` plus a branch — fine in user-mode but
    //      mixing it with OS-handler code is footgun-prone, so we keep
    //      the dispatch in its own thread.
    //
    // `register_usize` is last-wins: if SIGINT then SIGTERM both arrive,
    // the slot ends up holding SIGTERM. Users sending two fatal signals
    // back-to-back is an edge case, and either signal correctly terminates
    // the wrapper with `128 + sig` semantics, so last-wins is acceptable.
    // A first-wins variant would require an `unsafe` custom handler,
    // which this crate forbids workspace-wide.
    let observed_signal = Arc::new(AtomicUsize::new(0));
    for sig in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
        signal_hook::flag::register_usize(
            sig,
            Arc::clone(&observed_signal),
            usize::try_from(sig).unwrap_or(0),
        )?;
    }
    let mut signals =
        signal_hook::iterator::Signals::new([libc::SIGINT, libc::SIGTERM, libc::SIGHUP])?;
    let handle = thread::spawn(move || {
        for sig in signals.forever() {
            let pid = child_pid.load(Ordering::SeqCst);
            if pid > 0 {
                let _ = forward_signal(pid, sig);
            } else {
                // No child yet (pre-spawn / mid-seed window). Don't drop the
                // signal silently — restore default behavior and re-raise so
                // the wrapper terminates with conventional Unix semantics.
                let _ = signal_hook::low_level::emulate_default_handler(sig);
            }
        }
    });
    Ok(SignalGuard {
        _handle: handle,
        observed_signal,
    })
}

impl SignalGuard {
    pub(crate) fn observed_signal(&self) -> Option<i32> {
        let signal = self.observed_signal.load(Ordering::SeqCst);
        if signal == 0 {
            return None;
        }
        i32::try_from(signal).ok()
    }
}

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
        let status = child.wait().map_err(|e| {
            pid_sink.store(0, Ordering::SeqCst);
            SpawnerError::Exec(e)
        })?;
        pid_sink.store(0, Ordering::SeqCst);
        Ok(status)
    }

    fn exec(&self, inv: ChildInvocation) -> SpawnerError {
        use std::os::unix::process::CommandExt as _;
        let mut cmd = inv.into_command();
        SpawnerError::from(cmd.exec())
    }

    fn spawn_and_wait_output(
        &self,
        inv: ChildInvocation,
        pid_sink: &std::sync::atomic::AtomicI32,
    ) -> Result<ChildOutput, SpawnerError> {
        use std::process::Stdio;
        use std::sync::atomic::Ordering;
        use std::thread;

        let mut cmd = inv.into_command();
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(SpawnerError::Exec)?;
        pid_sink.store(
            i32::try_from(child.id()).unwrap_or(i32::MAX),
            Ordering::SeqCst,
        );

        let mut child_stdout = child
            .stdout
            .take()
            .ok_or_else(|| SpawnerError::Exec(std::io::Error::other("stdout pipe missing")))?;
        let mut child_stderr = child
            .stderr
            .take()
            .ok_or_else(|| SpawnerError::Exec(std::io::Error::other("stderr pipe missing")))?;

        let out_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut cap = Vec::new();
            crate::ui::raw_passthrough::tee_to_stdio(
                crate::ui::raw_passthrough::RawStream::Stdout,
                &mut child_stdout,
                &mut cap,
            )?;
            Ok(cap)
        });
        let err_handle = thread::spawn(move || -> std::io::Result<Vec<u8>> {
            let mut cap = Vec::new();
            crate::ui::raw_passthrough::tee_to_stdio(
                crate::ui::raw_passthrough::RawStream::Stderr,
                &mut child_stderr,
                &mut cap,
            )?;
            Ok(cap)
        });

        // Wait BEFORE joining: the kernel keeps the pipe readable after the
        // writer dies, so the threads see EOF and return cleanly.
        let status = child.wait().map_err(SpawnerError::Exec)?;
        // Clear the PID immediately on reap. The tee-thread join below can
        // still take a moment to drain the kernel pipe buffer; without this
        // store, a signal that arrives in that window would be forwarded to
        // a now-recycled PID. The caller also clears it post-return for
        // belt-and-suspenders.
        pid_sink.store(0, Ordering::SeqCst);
        let stdout = out_handle
            .join()
            .map_err(|_| SpawnerError::Exec(std::io::Error::other("stdout tee panicked")))?
            .map_err(SpawnerError::Exec)?;
        let stderr = err_handle
            .join()
            .map_err(|_| SpawnerError::Exec(std::io::Error::other("stderr tee panicked")))?
            .map_err(SpawnerError::Exec)?;

        Ok(ChildOutput {
            status,
            stdout,
            stderr,
        })
    }
}

fn forward_signal(pid: i32, sig: i32) -> std::io::Result<()> {
    let Some(pid) = rustix::process::Pid::from_raw(pid) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "invalid pid",
        ));
    };
    let signal = match sig {
        libc::SIGINT => rustix::process::Signal::INT,
        libc::SIGTERM => rustix::process::Signal::TERM,
        libc::SIGHUP => rustix::process::Signal::HUP,
        _ => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsupported signal",
            ));
        }
    };
    rustix::process::kill_process(pid, signal)
        .map_err(|errno| std::io::Error::from_raw_os_error(errno.raw_os_error()))
}
