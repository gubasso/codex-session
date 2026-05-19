use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};
use std::thread;

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
}

pub(crate) fn install(child_pid: Arc<AtomicI32>) -> Result<SignalGuard, std::io::Error> {
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
    Ok(SignalGuard { _handle: handle })
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
