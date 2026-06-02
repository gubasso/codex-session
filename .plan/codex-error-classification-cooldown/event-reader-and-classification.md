# Structured Event Reader & Error Classification

> Plan: codex-error-classification-cooldown | Round: 2 of 3 | Complexity: L |
> Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

`codex-session` wraps the upstream `codex` CLI and, in its auto-rotation path,
detects rate-limit/auth failures by **regex-scanning the child's captured
stdout/stderr after it exits** (`src/services/account/failover.rs`). This coarse
text match cannot tell a genuine usage-window exhaustion (`429` because the
5-hour/weekly quota is spent — rotate accounts) apart from a transient
per-minute/burst rate limit (`429` that a short backoff on the _same_ account
would clear). The observed production failure — heavy `gpt-5.5/high` planning via
`codex-session exec --json` getting a `429` immediately after the model's first
tool call, on every account, at ~99% reported quota — is the transient shape
being misread as exhaustion, so the wrapper burns the whole account pool into
flat 5-minute cooldowns.

Ground truth from the installed **codex 0.135.0** binary (verified via `strings`
on `@openai/codex-linux-x64/vendor/x86_64-unknown-linux-musl/bin/codex`):
`codex exec --json` emits JSONL events including `turn.failed` and `token_count`.
`token_count` carries `rate_limits` = `RateLimitSnapshot { primary, secondary,
plan_type, rate_limit_reached_type }`; each window is `RateLimitWindow {
used_percent, window_minutes, resets_in_seconds | resets_at }`. Error
discriminants include `usage_limit_reached`, `usage_limit_exceeded`,
`context_window_exceeded`. This structured stream — already requested by the
failing `prex`/`ask` flows — is a far better signal than regex over text.

This round builds the **reading + classification foundation**: a new module that
parses the needed JSONL subset, a fix so the trailing events survive the capture
cap, and a richer classifier in `failover.rs`. Nothing consumes the new classes
yet — Round 2 wires behavior to them.

## Previous Rounds

Round 1 (`live-harness-and-gate-wiring`) is expected to have produced:

- `tests/codex_exec_json_live.rs` — a live integration test
  (`live_exec_json_event_schema`) that makes one real `codex exec --json` call,
  parses the JSONL stream, and reports `RATE_LIMITS_IN_EXEC_MODE` (populated /
  null / absent) plus `RUN_RATE_LIMITED`.
- A `cargo-nextest-live` pre-commit hook (own group, `pre-push` stage) and a
  `justfile` `test-live` recipe that delegates to it; corrected live-tier doc
  comments in `.pre-commit-config.yaml` / `.config/nextest.toml`.
- A `CLAUDE.md` "pre-commit is the Source of Truth" principle.
- A recorded verdict on whether `token_count.rate_limits` is populated in exec
  mode (openai/codex #14728), which this round uses to weight structured-snapshot
  classification vs `turn.failed` + `retry_after`.

## Scope of This Round

IN scope:

- New module `src/services/account/codex_events.rs`: serde structs for the JSONL
  subset (`token_count.rate_limits` windows + `turn.failed` error view) and a
  `scan_events(stdout: &[u8]) -> EventSummary` that walks complete JSONL lines and
  returns the **last** rate-limit snapshot and any terminal error.
- Tail-preserving capture so trailing `token_count`/`turn.failed` lines survive
  the 1 MiB `MAX_CAPTURE_BYTES` cap (line-oriented retention).
- A classification layer in `src/services/account/failover.rs`: a
  `RateLimitClass { UsageLimitExhausted, Transient }` distinction and a
  `Classification` result that combines structured signal (preferred) with the
  existing text patterns (fallback), and carries any server-provided reset
  duration (`resets_in_seconds` / `retry_after` / parsed "try again in N").
- Add an `Unclassified`/other-error category that still carries the matched
  snippet (so Round 2's general handler has something to surface).
- Unit tests for the new module and the classifier.
- Capture one real `codex exec --json` event stream (when not rate-limited) to
  confirm whether `token_count.rate_limits` is populated in exec mode (see Risks).

OUT of scope (Round 2):

- Changing cooldown duration, rotation, or adding same-account backoff.
- The general unhandled-error verbose+log-pointer UX.
- The second call site `resume_blocked_from_live_rate_limit`.
- `docs/upstream-codex.md` update.

## Current State

### Key Files

- `src/services/account/failover.rs` — passive post-exit detection. Its own doc
  comment states: _"What this is: a post-wait observer over captured
  stdout+stderr bytes. What this is not: a streaming proxy or a mid-flight
  rotation hook."_ Key items:

  ```rust
  pub(crate) enum MatchKind { RateLimit, AuthFailure }

  const RATE_LIMIT_PATTERNS_RAW: [&str; 6] = [
      r"(?i)rate.?limit",
      r"(?i)quota.?exceeded",
      r"\b429\b",
      r"(?i)too.?many.?requests",
      r"(?i)exceeded.*rate",
      r"(?i)slow.?down",
  ];

  // 1 MiB cap on the per-stream capture buffer that scan reads:
  pub(crate) const MAX_CAPTURE_BYTES: usize = 1 << 20;

  pub(crate) struct Match {
      pub kind: MatchKind,
      pub line_no: usize,
      pub snippet: String,        // truncated to 256 chars
      pub pattern_index: usize,
  }

  pub(crate) fn scan(buf: &[u8]) -> Option<Match> { /* line-split regex, rate first then auth */ }
  pub(crate) fn pick_priority(stderr: Option<Match>, stdout: Option<Match>) -> Option<Match> { /* RateLimit wins, else stderr */ }
  ```

  Inline `#[cfg(test)]` tests live at the bottom (e.g. `pick_priority_*`); match
  their style for new tests.

- `src/services/session/thread_index.rs` — the existing JSONL-line-walk pattern
  to mirror (do not invent a new parsing idiom):

  ```rust
  pub(crate) fn extract_thread_id(stdout: &[u8]) -> Option<String> {
      for line in stdout.split(|b| *b == b'\n') {
          if line.is_empty() { continue; }
          let Ok(value) = serde_json::from_slice::<serde_json::Value>(line) else { continue; };
          if value.get("type").and_then(|v| v.as_str()) == Some("thread.started")
              && let Some(tid) = value.get("thread_id").and_then(|v| v.as_str())
          { return Some(tid.to_owned()); }
      }
      None
  }
  ```

- `src/ui/raw_passthrough.rs` — the tee path that live-forwards child output and
  fills the bounded capture buffer. It references `failover::MAX_CAPTURE_BYTES`
  (`let cap = crate::services::account::failover::MAX_CAPTURE_BYTES;`) and reads
  in 8 KiB chunks, stopping appends once the cap is reached (head-biased — this
  is what drops trailing events).

- `src/services/account/quota.rs` — already models the same window shape from the
  WHAM HTTP endpoint (`used_percent`, `reset_at`, `five_hour`/`weekly`). Align
  field names with this where practical.

### Existing Patterns

- Module visibility is `pub(crate)`; errors are `Result<T, AppError>` /
  `thiserror` enums; **no `unwrap()` in production code** (allowed only in
  tests). Use `#[serde(default)]` / `Option` and tolerate unknown fields.
- New module must be declared in `src/services/account/mod.rs` alongside
  `failover`, `cooldown`, `retry`, etc.
- JSONL lines may be partial at the buffer boundary — skip lines that fail to
  parse (mirror `extract_thread_id`).

## Implementation Steps

### Step 1: Run the live schema-verification test to confirm the schema

Round 1 created the live test `tests/codex_exec_json_live.rs` (test
`live_exec_json_event_schema`), wired into nextest's `binary(/live/)` filter, the
`cargo-nextest-live` pre-commit hook, and the `just test-live` recipe. It makes
one real `codex exec --json` call with a harmless no-op prompt, parses the JSONL
stream, and reports a verdict. Run it before coding the structs:

```bash
just test-live
```

Read the reported lines in the output (the `live` profile prints them
immediately):

```text
[codex-exec-json] event types observed: {...}
[codex-exec-json] RATE_LIMITS_IN_EXEC_MODE: populated | null | absent-from-token_count | no-token_count-event
[codex-exec-json] RUN_RATE_LIMITED: true | false
```

`RATE_LIMITS_IN_EXEC_MODE` answers the open question (openai/codex #14728): if
`populated`, classification can use the snapshot windows; if `null`/`absent`,
classification must lean on `turn.failed` + `retry_after`. The test does NOT
hard-fail when the call is itself rate-limited (`RUN_RATE_LIMITED: true`) — that
still confirms the error-event shape. Record the observed verdict in the commit
message / notes. If credentials or quota are unavailable, fall back to the
documented 0.135.0 schema below and note the assumption.

This round may also extend that live test (e.g. assert the concrete `turn.failed`
nesting once observed) so the schema stays pinned against upstream drift.

### Step 2: New module `src/services/account/codex_events.rs`

Define the minimal serde subset (tolerate unknown fields; everything optional):

```rust
//! Best-effort reader for the subset of `codex exec --json` JSONL events the
//! wrapper needs to classify failures. Mirrors the line-walk in
//! `crate::services::session::thread_index::extract_thread_id`.

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct RateLimitWindow {
    #[serde(default)] pub used_percent: Option<f64>,
    #[serde(default)] pub window_minutes: Option<u64>,
    #[serde(default)] pub resets_in_seconds: Option<u64>,
    #[serde(default)] pub resets_at: Option<i64>,
}

#[derive(Debug, Clone, serde::Deserialize, Default)]
pub(crate) struct RateLimitSnapshot {
    #[serde(default)] pub primary: Option<RateLimitWindow>,
    #[serde(default)] pub secondary: Option<RateLimitWindow>,
    #[serde(default)] pub plan_type: Option<String>,
    #[serde(default)] pub rate_limit_reached_type: Option<String>,
}

/// Terminal error view extracted from a `turn.failed` (or top-level `error`) event.
#[derive(Debug, Clone, Default)]
pub(crate) struct TurnError {
    pub message: String,
    pub error_code: Option<String>,   // usage_limit_reached | usage_limit_exceeded | context_window_exceeded | ...
    pub retry_after_seconds: Option<u64>,
    pub http_status: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct EventSummary {
    pub last_rate_limits: Option<RateLimitSnapshot>,
    pub turn_error: Option<TurnError>,
}

pub(crate) fn scan_events(stdout: &[u8]) -> EventSummary { /* see below */ }
```

`scan_events` walks `stdout.split(|b| *b == b'\n')`, `serde_json::from_slice`
into `serde_json::Value`, skips parse failures, and:

- on `type == "token_count"`: read `rate_limits` into `RateLimitSnapshot`,
  keeping the **last** one seen.
- on `type == "turn.failed"` (and top-level `type == "error"`): extract the
  human `message`, any nested error-code/classification string (search the event
  object for `usage_limit_reached`/`usage_limit_exceeded`/`context_window_exceeded`
  in the known field, falling back to substring on the message), `retry_after`
  (numeric seconds), and any `http_status_code`.

Because the exact nesting under `turn.failed`/`token_count` was confirmed in Step
1, decode the concrete fields you observed; keep the `Value`-probing fallback so
an unknown shape still yields a best-effort `TurnError { message, .. }`.

Declare the module in `src/services/account/mod.rs`.

### Step 3: Tail-preserving capture in `src/ui/raw_passthrough.rs`

The trailing `token_count`/`turn.failed` events are exactly what we need, and the
current head-biased 1 MiB cap drops them on large output. Change the capture so
it retains the **tail** of complete lines rather than the first 1 MiB. Preferred:
a line-oriented ring buffer that keeps the most recent complete JSONL lines up to
`MAX_CAPTURE_BYTES` (drop oldest whole lines when over cap). Keep live forwarding
to the parent's real stdio unbounded and unchanged. Do not alter the
`ChildOutput { status, stdout, stderr }` return shape consumed by
`spawner.rs`/`retry.rs`.

If a full ring buffer is too invasive for one session, the acceptable minimum is:
when the cap is reached, retain the last `MAX_CAPTURE_BYTES` bytes (tail) instead
of the first — but line-oriented retention is preferred so `scan_events` never
sees a truncated leading line.

### Step 4: Classification layer in `src/services/account/failover.rs`

Add, without removing the existing text patterns/`scan`/`pick_priority` (Round 2
and the text fallback still use them):

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RateLimitClass { UsageLimitExhausted, Transient }

#[derive(Debug, Clone)]
pub(crate) enum Category {
    RateLimit(RateLimitClass),
    AuthFailure,
    ContextWindowExceeded,
    ServerError,          // 5xx / stream error
    Unclassified,
}

#[derive(Debug, Clone)]
pub(crate) struct Classification {
    pub category: Category,
    pub reset_after_seconds: Option<u64>,  // server-provided when known
    pub snippet: String,                    // human-facing detail / matched line
}

/// Structured-first classifier. `events` comes from
/// `codex_events::scan_events(stdout)`; `stderr`/`stdout` feed the text fallback.
pub(crate) fn classify(
    events: &crate::services::account::codex_events::EventSummary,
    stdout: &[u8],
    stderr: &[u8],
) -> Option<Classification> { /* see rules */ }
```

Classification rules (precedence):

1. **Structured first.** If `events.turn_error` has an `error_code` of
   `usage_limit_reached`/`usage_limit_exceeded` → `RateLimit(UsageLimitExhausted)`
   with `reset_after_seconds` from `retry_after_seconds`, else from the matching
   window's `resets_in_seconds`. If `error_code == context_window_exceeded` →
   `ContextWindowExceeded`. If the error is a 429/rate-limit _without_ a
   usage-limit code (or a `token_count` shows the hit window still has headroom,
   i.e. `used_percent < ~99`) → `RateLimit(Transient)` with
   `reset_after_seconds` from `retry_after` / parsed "try again in N". A 5xx /
   "stream error" message → `ServerError`.
2. **Text fallback** (human-mode, no JSONL events found): reuse existing patterns
   via `scan`/`pick_priority`. Add usage-limit discrimination — patterns like
   `(?i)hit your usage limit` and `(?i)usage.?limit.?(reached|exceeded)` →
   `UsageLimitExhausted`; other rate-limit matches → `Transient`. Parse duration
   from `(?i)try again in\s*(\d+(?:\.\d+)?)\s*(s|ms|seconds?)`. Auth patterns →
   `AuthFailure`.
3. No match → `None` (callers treat absence as "child succeeded / nothing to do").
   A matched-but-unknown error → `Unclassified` carrying the snippet.

Keep `reset_after_seconds = None` when no signal exists (Round 2 applies the 300s
fallback). This module must NOT sleep, write cooldowns, or rotate — it only
classifies.

### Step 5: Unit tests

Add `#[cfg(test)]` tests matching the existing inline style in `failover.rs` plus
tests in the new module:

- `scan_events`: real captured JSONL fixture → correct last snapshot + `TurnError`;
  ignores unknown fields; picks the _last_ `token_count`; survives a truncated
  trailing line.
- `classify`: usage-limit JSONL → `UsageLimitExhausted` (+ reset from
  `resets_in_seconds`/`retry_after`); transient 429 JSONL / headroom snapshot →
  `Transient`; `context_window_exceeded` → `ContextWindowExceeded`; text-fallback
  usage-limit phrasing → `UsageLimitExhausted`; "try again in 12s" → 12.
- Tail capture: a `>1 MiB` buffer with the `token_count`/`turn.failed` lines at
  the end still yields them from `scan_events` after capture (test at the
  `raw_passthrough` level or via a helper that exercises the retention logic).

Run the project gates (per repo `CLAUDE.md`, not raw cargo):

```bash
just lint
just test-unit
```

### Final Step: Update the queue

Record completion in the queue — status lives in YAML; nothing moves on disk:

1. In this plan's `_QUEUE.yaml`, set this round's
   (`item: event-reader-and-classification`) `status` to `done`.

## Acceptance Criteria

- [ ] `src/services/account/codex_events.rs` exists, is declared in
      `src/services/account/mod.rs`, and `scan_events` returns the last
      `token_count.rate_limits` snapshot + any `turn.failed`/`error` view from a
      JSONL buffer, skipping unparseable lines.
- [ ] Trailing `token_count`/`turn.failed` lines survive a `>MAX_CAPTURE_BYTES`
      capture (tail-/line-preserving), verified by a test.
- [ ] `failover.rs` exposes `RateLimitClass`, `Category`, `Classification`, and a
      `classify(...)` that prefers structured signal and falls back to text,
      distinguishing `UsageLimitExhausted` from `Transient` and carrying any
      server reset duration. Existing `scan`/`pick_priority`/patterns remain.
- [ ] The classifier performs no I/O, no sleeping, no cooldown writes, no
      rotation (pure classification).
- [ ] The classifier's structured-vs-fallback weighting reflects Round 1's
      recorded `RATE_LIMITS_IN_EXEC_MODE` verdict (re-run `just test-live` to
      confirm if the verdict was pending).
- [ ] `just lint` and `just test-unit` pass.
- [ ] This plan's `_QUEUE.yaml` shows round `event-reader-and-classification` as
      `done`.

## Next Round

Round 3 (`cooldown-backoff-and-error-ux`) consumes this classifier: it sets the
cooldown reset from `Classification.reset_after_seconds` (300s fallback), adds a
bounded same-account backoff for `Transient` (reusing `run_auto`'s `force_same`
mechanism) while keeping rotation for `UsageLimitExhausted`, applies the same
logic at the resume-blocked call site, adds the general verbose+log-pointer
handler for `Unclassified`/`ContextWindowExceeded`/`ServerError`, and updates
`docs/upstream-codex.md` with the verified event/error schema.
