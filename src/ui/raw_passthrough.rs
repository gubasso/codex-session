use std::io::{Read, Write as _};
use std::{collections::VecDeque, mem};

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
/// per chunk; the capture half retains the most recent complete lines up to
/// `crate::services::account::failover::MAX_CAPTURE_BYTES` while continuing to
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
    let mut tail_capture = LineTailCapture::from_existing(
        crate::services::account::failover::MAX_CAPTURE_BYTES,
        capture,
    );
    loop {
        let n = match src.read(&mut buf) {
            Ok(n) => n,
            Err(err) => {
                tail_capture.finish(capture);
                return Err(err);
            }
        };
        if n == 0 {
            tail_capture.finish(capture);
            return Ok(());
        }
        let chunk = &buf[..n];
        tail_capture.push_chunk(chunk);
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

struct LineTailCapture {
    cap: usize,
    lines: VecDeque<Vec<u8>>,
    retained_line_bytes: usize,
    pending: Vec<u8>,
}

impl LineTailCapture {
    fn from_existing(cap: usize, capture: &[u8]) -> Self {
        let mut state = Self {
            cap,
            lines: VecDeque::new(),
            retained_line_bytes: 0,
            pending: Vec::new(),
        };
        state.push_chunk(capture);
        state
    }

    fn push_chunk(&mut self, chunk: &[u8]) {
        for byte in chunk {
            self.pending.push(*byte);
            if *byte == b'\n' {
                let line = mem::take(&mut self.pending);
                self.retained_line_bytes += line.len();
                self.lines.push_back(line);
                self.trim_complete_lines();
            } else {
                self.trim_pending();
            }
        }
    }

    /// Bound the in-progress (unterminated) line so a child that emits a single
    /// very long line — or never terminates one — cannot grow `pending` without
    /// limit. The old head-biased capture hard-capped the buffer at `cap`; this
    /// preserves that bounded-memory contract by retaining only the most recent
    /// `cap` bytes of an over-cap pending line. A leading partial line is skipped
    /// by `scan_events` anyway, so trimming the head loses no usable JSONL.
    ///
    /// Trimming is deferred until `pending` reaches `2 * cap` and then drains
    /// back down to `cap`, so the amortized cost stays O(1) per byte rather than
    /// memmoving on every append past the cap.
    fn trim_pending(&mut self) {
        let slack_limit = self.cap.saturating_mul(2);
        if self.pending.len() > slack_limit {
            let overflow = self.pending.len() - self.cap;
            self.pending.drain(..overflow);
        }
    }

    fn finish(mut self, capture: &mut Vec<u8>) {
        capture.clear();
        // The trailing record is the whole point of this buffer, and JSONL
        // producers do not guarantee a newline on the final line. So the newest
        // `pending` bytes take precedence: evict oldest complete lines until the
        // (already cap-bounded) `pending` fits, rather than dropping the final
        // unterminated event because older complete lines filled the cap.
        let pending_len = self.pending.len();
        if pending_len <= self.cap {
            while self.retained_line_bytes + pending_len > self.cap {
                let Some(removed) = self.lines.pop_front() else {
                    self.retained_line_bytes = 0;
                    break;
                };
                self.retained_line_bytes = self.retained_line_bytes.saturating_sub(removed.len());
            }
        }
        let keep_pending = pending_len <= self.cap.saturating_sub(self.retained_line_bytes);
        let target_len = self.retained_line_bytes + if keep_pending { pending_len } else { 0 };
        capture.reserve(target_len);
        for line in self.lines.drain(..) {
            capture.extend_from_slice(&line);
        }
        if keep_pending {
            capture.extend_from_slice(&self.pending);
        }
    }

    fn trim_complete_lines(&mut self) {
        while self.retained_line_bytes > self.cap {
            let Some(removed) = self.lines.pop_front() else {
                self.retained_line_bytes = 0;
                return;
            };
            self.retained_line_bytes = self.retained_line_bytes.saturating_sub(removed.len());
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::LineTailCapture;
    use crate::services::account::{codex_events, failover::MAX_CAPTURE_BYTES};

    fn finish_capture(chunks: &[&[u8]]) -> Vec<u8> {
        let mut capture = Vec::new();
        let mut tail = LineTailCapture::from_existing(MAX_CAPTURE_BYTES, &capture);
        for chunk in chunks {
            tail.push_chunk(chunk);
        }
        tail.finish(&mut capture);
        capture
    }

    #[test]
    fn retains_trailing_jsonl_events_under_cap() {
        let filler_line = format!("{}\n", "a".repeat(16 * 1024 - 1));
        let repeat = (MAX_CAPTURE_BYTES / filler_line.len()) + 8;
        let filler = filler_line.repeat(repeat);
        let token_line = format!(
            "{}\n",
            serde_json::json!({
                "type": "token_count",
                "rate_limits": {
                    "primary": {"used_percent": 42.0, "resets_in_seconds": 17},
                    "rate_limit_reached_type": "primary",
                },
            })
        );
        let failed_line = format!(
            "{}\n",
            serde_json::json!({
                "type": "turn.failed",
                "message": "usage_limit_reached",
                "error": {"http_status_code": 429},
            })
        );

        let capture = finish_capture(&[
            filler.as_bytes(),
            token_line.as_bytes(),
            failed_line.as_bytes(),
        ]);

        assert!(capture.len() <= MAX_CAPTURE_BYTES);
        let summary = codex_events::scan_events(&capture);
        assert_eq!(
            summary
                .last_rate_limits
                .and_then(|snapshot| snapshot.primary)
                .and_then(|window| window.resets_in_seconds),
            Some(17)
        );
        assert_eq!(
            summary
                .turn_error
                .and_then(|turn_error| turn_error.error_code),
            Some("usage_limit_reached".to_owned())
        );
    }

    #[test]
    fn split_final_line_across_chunks_still_surfaces_event() {
        let token_event = serde_json::json!({
            "type": "token_count",
            "rate_limits": {"primary": {"used_percent": 12.0}},
        })
        .to_string();
        let failed_event = serde_json::json!({
            "type": "turn.failed",
            "message": "context_window_exceeded",
            "error": {"code": "context_window_exceeded"},
        })
        .to_string();
        let stream = format!("{token_event}\n{failed_event}\n");
        // Split mid-way through the final record to simulate a read() boundary.
        let split_at = token_event.len() + 1 + (failed_event.len() / 2);
        let bytes = stream.as_bytes();
        let first = &bytes[..split_at];
        let second = &bytes[split_at..];

        let capture = finish_capture(&[first, second]);
        let summary = codex_events::scan_events(&capture);

        assert_eq!(
            summary
                .last_rate_limits
                .and_then(|snapshot| snapshot.primary)
                .and_then(|window| window.used_percent),
            Some(12.0)
        );
        assert_eq!(
            summary
                .turn_error
                .and_then(|turn_error| turn_error.error_code),
            Some("context_window_exceeded".to_owned())
        );
    }

    #[test]
    fn unterminated_line_stays_bounded_in_memory() {
        // A child that emits a very long line with no trailing newline must not
        // grow `pending` without bound (regression guard for the old head-biased
        // cap). The retained tail must stay within the deferred-trim slack.
        let huge = vec![b'x'; MAX_CAPTURE_BYTES * 5];
        let mut capture = Vec::new();
        let mut tail = LineTailCapture::from_existing(MAX_CAPTURE_BYTES, &capture);
        // Feed in chunks so trim is exercised incrementally, never terminating.
        for chunk in huge.chunks(8 * 1024) {
            tail.push_chunk(chunk);
            assert!(
                tail.pending.len() <= MAX_CAPTURE_BYTES * 2,
                "pending grew past the 2x cap slack bound"
            );
        }
        tail.finish(&mut capture);
        // An over-cap unterminated line does not fit and is dropped at finish.
        assert!(capture.is_empty());
    }

    #[test]
    fn trailing_unterminated_event_survives_when_complete_lines_fill_cap() {
        // The final JSONL record arrives WITHOUT a trailing newline, after enough
        // complete lines to fill the cap. The newest (trailing) event must win:
        // older complete lines are evicted so the final record survives.
        let filler_line = format!("{}\n", "c".repeat(16 * 1024 - 1));
        let repeat = (MAX_CAPTURE_BYTES / filler_line.len()) + 8;
        let filler = filler_line.repeat(repeat);
        // No trailing newline: the final record arrives unterminated.
        let final_event = serde_json::json!({
            "type": "turn.failed",
            "message": "usage_limit_reached",
            "error": {"http_status_code": 429},
        })
        .to_string();

        let capture = finish_capture(&[filler.as_bytes(), final_event.as_bytes()]);

        assert!(capture.len() <= MAX_CAPTURE_BYTES);
        let summary = codex_events::scan_events(&capture);
        assert_eq!(
            summary
                .turn_error
                .and_then(|turn_error| turn_error.error_code),
            Some("usage_limit_reached".to_owned())
        );
    }

    #[test]
    fn malformed_leading_fragment_is_skipped_but_trailing_events_survive() {
        let token_line =
            "{\"type\":\"token_count\",\"rate_limits\":{\"primary\":{\"used_percent\":55.0}}}\n";
        let failed_line = "{\"type\":\"turn.failed\",\"message\":\"usage_limit_exceeded\"}\n";
        let filler_line = format!("{}\n", "b".repeat(8 * 1024 - 1));
        let repeat = (MAX_CAPTURE_BYTES / filler_line.len()) + 4;
        let capture = finish_capture(&[
            filler_line.repeat(repeat).as_bytes(),
            token_line.as_bytes(),
            failed_line.as_bytes(),
            b"{\"type\":\"turn.failed\"",
        ]);

        let summary = codex_events::scan_events(&capture);
        assert_eq!(
            summary
                .last_rate_limits
                .and_then(|snapshot| snapshot.primary)
                .and_then(|window| window.used_percent),
            Some(55.0)
        );
        assert_eq!(
            summary
                .turn_error
                .and_then(|turn_error| turn_error.error_code),
            Some("usage_limit_exceeded".to_owned())
        );
    }
}
