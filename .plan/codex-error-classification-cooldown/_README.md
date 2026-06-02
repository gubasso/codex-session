# Codex Error Classification & Reset-Aware Cooldown

> Complexity: L | Rounds: 3 | Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Problem Statement

When `codex-session exec --json` drives heavy planning (gpt-5.5/high) inside
agent workflows (`prex`, `ask`), it hits a recurring **429 that fires
immediately after the model's first tool call, on every account, even when
`account health` reports ~99% remaining quota**. The wrapper rotates all
accounts into flat 5-minute cooldowns and exhausts the pool; waiting 20–60
minutes does not help because the 5-hour usage window is not the limiter being
hit.

Root cause, established by investigation this session:

1. Failure detection (`src/services/account/failover.rs`) is a **post-exit
   regex scan over captured stdout/stderr text**. It matches 6 coarse patterns
   (`\b429\b`, `rate.?limit`, …) and **cannot distinguish a true usage-window
   exhaustion from a transient per-minute/burst rate limit**. By the time it
   scans, codex has already run its own Retry-After-respecting retry loop and
   given up.
2. `retry.rs::write_cooldown` sets a **flat `now + 300`** cooldown for every
   detected 429, ignoring the backend's real reset window.
3. The auto path rotates accounts on **any** 429 — pointless when the limit is
   request/token-shaped (the observed "all accounts fail identically at 99%").

Ground truth from the installed **codex 0.135.0** binary (verified via `strings`
on `@openai/codex-linux-x64/.../bin/codex`, not inference): `codex exec --json`
emits JSONL events `thread.started`, `turn.started`, `turn.completed`,
`turn.failed`, `item.*`, and **`token_count`**. `token_count` carries
`rate_limits` = `RateLimitSnapshot { primary, secondary, plan_type,
rate_limit_reached_type }`, each window a `RateLimitWindow { used_percent,
window_minutes, resets_in_seconds | resets_at }`. Error discriminants exist
(`usage_limit_reached`, `usage_limit_exceeded`, `context_window_exceeded`);
codex parses a `retry_after` and a `try again in N (s|ms|seconds)` regex. So the
wrapper has a far better signal available than the regex it uses today — the
structured `--json` stream (which the failing flows already request) carries the
exact error class and the server's real reset time.

Intended outcome: read the real error class and reset time, back off transient
limits on the same account instead of burning the pool, set accurate cooldowns,
and never silently swallow an unmodeled error.

## Strategy

Replace blind post-exit text matching with a **two-tier classifier** (structured
JSONL first, improved text fallback for human-mode runs) feeding the existing
retry/cooldown machinery — reusing the current `run_auto` control flow and the
`force_same` same-account-retry mechanism rather than rewriting the spawn path.

Split bottom-up: a verification/gate foundation, then the reading foundation,
then the behavior change that consumes it:

- **Round 1 (verification + gate wiring):** an automated live test that resolves
  the `token_count.rate_limits`-in-exec-mode question (openai/codex #14728) and
  pins the event/error schema, wired into the gates **the project's way** — a
  dedicated `cargo-nextest-live` pre-commit hook (pre-push stage) with the
  `justfile` delegating to pre-commit, plus a `CLAUDE.md` "pre-commit is the
  Source of Truth" principle. Its recorded verdict feeds Round 2.
- **Round 2 (reading foundation):** the structured event reader, the
  tail-preserving capture fix (trailing events must survive the 1 MiB cap), and
  the classification layer in `failover.rs`. Produces the types/classifier
  nothing consumes yet. Unit-tested in isolation.
- **Round 3 (behavior + UX):** consume the classification — reset-aware cooldown
  duration, transient same-account backoff, the second (resume-blocked) call
  site, the general unhandled-error verbose+log handler, and the upstream-codex
  doc update. Integration-tested end to end.

## Rounds

Authoritative order/status live in `_QUEUE.yaml`; this list mirrors it.

1. `live-harness-and-gate-wiring.md` — live `codex exec --json` schema test
   (answers #14728), `cargo-nextest-live` pre-commit hook (pre-push, own group)
   - `just test-live` delegation, and the `CLAUDE.md` pre-commit-SoT principle.
2. `event-reader-and-classification.md` — new `codex_events.rs` JSONL reader,
   tail-preserving capture, and the `RateLimitClass`/`Classification` layer in
   `failover.rs` (+ unit tests).
3. `cooldown-backoff-and-error-ux.md` — reset-aware cooldown duration, transient
   same-account backoff in `run_auto`, the resume-blocked call site, the general
   unhandled-error verbose+log-pointer handler, and `docs/upstream-codex.md`
   (+ integration tests).

## Execution Commands

```bash
# Execute the next todo round (executor reads _QUEUE.yaml, runs the first `todo`
# round, then stops):
/prex -ar @.plan/codex-error-classification-cooldown/

# Or target a specific round file directly:
/prex -ar .plan/codex-error-classification-cooldown/live-harness-and-gate-wiring.md
/prex -ar .plan/codex-error-classification-cooldown/event-reader-and-classification.md
/prex -ar .plan/codex-error-classification-cooldown/cooldown-backoff-and-error-ux.md
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of
work designed for a single `/prex` session. Do not implement multiple rounds in
one session.

When `/prex` is pointed at this directory or this `_README.md`, it MUST:

1. Read this plan's `_QUEUE.yaml`.
2. Find the first round with status `todo`.
3. Execute ONLY that round, then stop.
4. End the session — a fresh `/prex` session is launched for any subsequent
   round.

**Why:** Fresh sessions prevent context contamination between rounds, keep token
usage predictable, and let the user review intermediate results (Round 1's
#14728 verdict; Round 2's classifier) before the behavior change in Round 3
touches the account-rotation engine.

## Decisions & Constraints

- **Executor: prex (EF 1.5).**
- **pre-commit is the Source of Truth for quality gates.** New test tiers are
  defined as pre-commit hooks first; the `justfile` gate recipes delegate to
  `pre-commit run …` rather than calling `cargo` directly. Round 1 codifies this
  as a named principle in `CLAUDE.md` (it was previously only an aside).
- **Live test runs on `pre-push` (user decision).** The `cargo-nextest-live`
  hook is wired to the `pre-push` git stage in its own grouping, so live schema
  verification runs on every `git push`. Trade-off accepted by the user: this
  requires real OAuth credentials + network on push, and a push environment
  without them (e.g. CI, or a contributor without accounts) will fail the
  pre-push gate. The recommended-but-not-chosen alternative was a `manual`-stage
  hook (define in SoT, run only via `just test-live`); revisit if push friction
  or CI breakage becomes a problem.
- **Scope:** Fix the 429 now; classify the _important_ other errors now
  (`usage_limit_reached`/`usage_limit_exceeded`, `context_window_exceeded`, auth
  401 — already handled, server 5xx / stream error); route everything else
  through a **general handler that preserves the message to log + stderr** so
  unmapped errors can be mapped later. (Settled with user this session.)
- **Cooldown duration = server's real reset** (`resets_in_seconds` /
  `retry_after` / parsed "try again in N"), **300s only as fallback**, clamped to
  a sane maximum. (Settled.)
- **Transient 429** (rate-limit hit but usage window NOT exhausted) → **bounded
  short same-account backoff & retry** before rotating; only a genuine
  `usage_limit_reached`/exhausted window rotates + long-cooldowns. (Settled.)
- **Unhandled errors** → styled stderr message (class + one-line hint) + pointer
  to the rolling tracing log; full snippet always logged. (Settled.)
- **Two-tier classifier:** structured JSONL parse preferred; existing text
  patterns kept as fallback for human-mode runs where codex is not invoked with
  `--json` (the wrapper must not inject `--json` it did not receive — that would
  break the transparent passthrough / byte-compat contract).
- **Complexity override:** formula gives M (1 round, raw 13 ÷ 1.5 = 8.67); split
  to L/2 rounds because the combined surface (new parsing module + capture-path
  refactor + classification + retry/cooldown behavior change + general error UX +
  two test layers + docs) exceeds a comfortable single 600s Codex implement
  session and has a clean foundation/behavior dependency seam.
- **Quality gates:** use project `just` recipes (`just lint`, `just test-unit`,
  `just test-integration`), not raw cargo — per repo `CLAUDE.md`.
- **CLI output:** obey `docs/design/cli-style-guide.md` (stderr ownership,
  `BOLD_RED` error / `BOLD_CYAN` hint, never restyle passthrough child output;
  wrapper JSON uses `--format json`, never `--json`).

## Rejected Alternatives

- **Keep flat 300s, only improve classification** — rejected: cooldowns would
  still not match the backend's real reset window; user chose server-reset.
- **Always rotate on any 429 (status quo) but log the class** — rejected: does
  not fix the pool-burn; the failure is request-shaped, so a same-account backoff
  is required.
- **Inject `--json` into every codex invocation so the wrapper can always parse
  structured events** — rejected: breaks the transparent passthrough and
  byte-compat contract; structured parsing is therefore best-effort, with text
  fallback for human-mode.
- **Make the wrapper consume codex's HTTP response headers directly** —
  rejected: the wrapper spawns codex as a child and never sees the HTTP layer;
  the JSONL event stream is the available structured signal.

## Risks & Edge Cases

- **Auto-pre-push live test needs creds + network.** Wiring `cargo-nextest-live`
  to `pre-push` means `git push` invokes a real, billable codex call; pushes from
  environments without OAuth creds/network/quota (CI, fresh clones) will fail the
  gate, and a flurry of pushes burns quota. Round 1 must update the now-stale
  `.pre-commit-config.yaml` / `.config/nextest.toml` comments that claim live
  "never runs in git hooks" so the config is self-consistent. Mitigation if it
  bites: switch the hook to `stages: [manual]` (one-line change) — the test and
  `just test-live` delegation stay identical.
- **`token_count.rate_limits` may be `null` in exec mode** (openai/codex issue
  #14728 reports exec-mode rate-limit fields unpopulated; the `x-codex-*` headers
  feed app-server mode). The binary contains the machinery but runtime population
  for ChatGPT-auth `exec --json` in 0.135.0 is unconfirmed. **Round 1 must
  capture one real `codex exec --json` stream (when not rate-limited) and inspect
  `token_count`.** Either way the design degrades gracefully: if `rate_limits` is
  null, classification still works from `turn.failed` (`usage_limit_reached`) and
  duration comes from `retry_after` / "try again in N".
- **Trailing-event truncation:** `token_count`/`turn.failed` arrive last; the
  1 MiB `MAX_CAPTURE_BYTES` cap can drop them on large planning output. Round 1
  must retain the tail (line-oriented ring buffer or incremental line-scan in the
  tee path), independent of the text-capture cap.
- **Blast radius:** Round 2 changes the account-rotation engine. A
  misclassification that treats a true usage-limit as transient would retry a
  dead account; clamp backoff attempts and durations, and bias ambiguous cases
  toward the safe (rotate) path. The prex review-loop is the mitigation; Round 1
  lands and is reviewed before Round 2.
- **Cooldown schema migration:** adding a field to `Cooldown` must deserialize
  tolerantly so existing on-disk `cooldown.json` files still read.

## Completion

When all rounds are done, set each round `done` in this plan's `_QUEUE.yaml` and
set this plan `done` in the top-level `.plan/_QUEUE.yaml`. Nothing moves on disk.
