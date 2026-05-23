use std::io::{Read, Write as _};

#[derive(Clone, Copy)]
pub(crate) enum RawStream {
    Stdout,
    Stderr,
}

#[allow(dead_code)]
pub(crate) fn write_raw(stream: RawStream, bytes: &[u8]) -> std::io::Result<()> {
    match stream {
        RawStream::Stdout => {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(bytes)?;
            stdout.flush()
        }
        RawStream::Stderr => {
            let mut stderr = std::io::stderr().lock();
            stderr.write_all(bytes)?;
            stderr.flush()
        }
    }
}

/// Copy bytes from `src` to the parent's real stdout/stderr (per `stream`)
/// AND into `capture`, until EOF. The live half is unbounded and flushed
/// per chunk; the capture half stops appending past
/// `crate::services::account::failover::MAX_CAPTURE_BYTES` but continues to
/// forward.
///
/// **Liveness invariant.** Any I/O failure on the live sink (`BrokenPipe`,
/// `EIO`, `ENOSPC`, ...) demotes the live half to "closed" but keeps draining
/// the source into `capture`. This is load-bearing for `spawn_and_wait_output`:
/// if this function returned early, the reader thread would stop draining
/// the child's pipe, and a still-writing child would block once the kernel
/// pipe buffer filled — while the parent is blocked in `child.wait()`. The
/// only way to fail is a `src.read()` error (the child's pipe itself), which
/// is fatal regardless.
pub(crate) fn tee_to_stdio<R: Read>(
    stream: RawStream,
    src: &mut R,
    capture: &mut Vec<u8>,
) -> std::io::Result<()> {
    let mut buf = [0u8; 8 * 1024];
    let mut live_open = true;
    loop {
        let n = src.read(&mut buf)?;
        if n == 0 {
            return Ok(());
        }
        let chunk = &buf[..n];
        let cap = crate::services::account::failover::MAX_CAPTURE_BYTES;
        if capture.len() < cap {
            let take = (cap - capture.len()).min(chunk.len());
            capture.extend_from_slice(&chunk[..take]);
        }
        if live_open {
            let res = match stream {
                RawStream::Stdout => {
                    let mut sink = std::io::stdout().lock();
                    sink.write_all(chunk).and_then(|()| sink.flush())
                }
                RawStream::Stderr => {
                    let mut sink = std::io::stderr().lock();
                    sink.write_all(chunk).and_then(|()| sink.flush())
                }
            };
            if let Err(err) = res {
                // Demote any live-sink failure to "stop forwarding, keep
                // capturing" so we continue draining the child's pipe. See the
                // liveness invariant above. BrokenPipe is the normal `head -n`
                // case and is silent; other kinds log a one-shot warning.
                if err.kind() != std::io::ErrorKind::BrokenPipe {
                    tracing::warn!(
                        op = "stdio.tee.live_sink_failed",
                        kind = ?err.kind(),
                        error = %err,
                        "live stdio sink failed; continuing to drain child pipe into capture buffer"
                    );
                }
                live_open = false;
            }
        }
    }
}
