//! Eager auth-bridge watcher.
//!
//! What this is: a polling thread that drives `AuthBridge::sync_once()` while
//! the wrapped child is alive, so concurrent `codex-session` terminals share
//! the freshest auth state in both directions.
//!
//! What this is not: an inotify subscriber, an async runtime task, or a
//! refresh-token coordinator inside the wrapped `codex` process.
//!
//! The flock contract, symlink/hardlink/ownership/mode checks, and atomic
//! native-file writes still come from `AuthBridge`. This module only polls.
//! The residual OAuth single-use-refresh race remains: if two terminals rotate
//! a refresh token within roughly one polling tick of each other, the wrapped
//! `codex` processes can still conflict before the bridge notices.

use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{Scope, ScopedJoinHandle};
use std::time::{Duration, Instant};

use super::{AuthBridge, SyncOutcome};

#[allow(dead_code)]
const TICK: Duration = Duration::from_millis(500);
#[allow(dead_code)]
const SHUTDOWN_SLICE: Duration = Duration::from_millis(50);

#[allow(dead_code)]
pub(crate) fn run_until<'a>(
    scope: &'a Scope<'a, '_>,
    bridge: &'a AuthBridge,
    stop: &'a AtomicBool,
) -> ScopedJoinHandle<'a, ()> {
    let span = tracing::info_span!("auth.watch");
    scope.spawn(move || {
        let _entered = span.enter();
        let start = Instant::now();
        let mut ticks = 0_u64;
        let mut writes_native = 0_u64;
        let mut writes_session = 0_u64;
        let mut errors = 0_u64;
        // Rate-limit identical errors: emit a WARN on the first occurrence
        // of a given `err.kind()`, suppress consecutive duplicates, but keep
        // counting them so the final summary reflects the true total. Any
        // success or a different err.kind() resets the suppression window.
        let mut last_err_kind: Option<&'static str> = None;
        let mut suppressed = 0_u64;

        while !stop.load(Ordering::Relaxed) {
            match bridge.sync_once() {
                Ok(SyncOutcome::WroteNative) => {
                    writes_native += 1;
                    reset_err_window(&mut last_err_kind, &mut suppressed);
                }
                Ok(SyncOutcome::WroteSession) => {
                    writes_session += 1;
                    reset_err_window(&mut last_err_kind, &mut suppressed);
                }
                Ok(SyncOutcome::Unchanged) => {
                    reset_err_window(&mut last_err_kind, &mut suppressed);
                }
                Err(err) => {
                    errors += 1;
                    let kind = err.kind();
                    if last_err_kind == Some(kind) {
                        suppressed += 1;
                    } else {
                        flush_suppressed(&mut suppressed);
                        last_err_kind = Some(kind);
                        tracing::warn!(
                            op = "auth.watch",
                            status = "error",
                            err.kind = kind,
                            err = %err,
                        );
                    }
                }
            }
            ticks += 1;

            let deadline = Instant::now() + TICK;
            while !stop.load(Ordering::Relaxed) && Instant::now() < deadline {
                std::thread::sleep(SHUTDOWN_SLICE);
            }
        }

        flush_suppressed(&mut suppressed);

        let dur_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
        tracing::info!(
            op = "auth.watch",
            status = "ok",
            ticks,
            writes_native,
            writes_session,
            errors,
            dur_ms,
        );
    })
}

#[allow(dead_code)]
fn reset_err_window(last_kind: &mut Option<&'static str>, suppressed: &mut u64) {
    if last_kind.is_some() {
        flush_suppressed(suppressed);
        *last_kind = None;
    }
}

#[allow(dead_code)]
fn flush_suppressed(suppressed: &mut u64) {
    if *suppressed > 0 {
        tracing::debug!(
            op = "auth.watch",
            status = "error-suppressed",
            count = *suppressed,
        );
        *suppressed = 0;
    }
}
