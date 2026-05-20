use std::sync::Arc;
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::thread;

use signal_hook::flag;
use signal_hook::iterator::Signals;

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

pub(crate) fn install(child_pid: Arc<AtomicI32>) -> Result<SignalGuard, std::io::Error> {
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
        flag::register_usize(
            sig,
            Arc::clone(&observed_signal),
            usize::try_from(sig).unwrap_or(0),
        )?;
    }
    let mut signals = Signals::new([libc::SIGINT, libc::SIGTERM, libc::SIGHUP])?;
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
