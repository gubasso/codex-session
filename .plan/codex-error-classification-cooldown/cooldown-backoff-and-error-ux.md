# Reset-Aware Cooldown, Transient Backoff & Unhandled-Error UX

> Plan: codex-error-classification-cooldown | Round: 3 of 3 | Complexity: L |
> Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

`codex-session` wraps the `codex` CLI. Its auto-rotation path
(`src/services/account/retry.rs::run_auto`) detects a `429`/auth failure in the
child's output and, for any `429`, writes a **flat 5-minute cooldown** and
**rotates to the next account**. This is wrong for the production failure being
fixed: heavy `gpt-5.5/high` planning via `codex-session exec --json` hits a
`429` immediately after the model's first tool call, on every account, at ~99%
reported quota. That is a **transient per-minute/burst limit**, not a usage-window
exhaustion — so rotating burns the whole pool into 5-minute cooldowns and waiting
does not help (the 5-hour window is not the limiter).

Round 1 built the reading + classification foundation: a `codex_events` module
that parses the `codex exec --json` JSONL stream (`token_count.rate_limits`
windows + `turn.failed` error), a tail-preserving capture so those trailing
events survive the 1 MiB cap, and a `classify(...)` in `failover.rs` returning a
`Classification { category, reset_after_seconds, snippet }` that distinguishes
`RateLimit(UsageLimitExhausted)` from `RateLimit(Transient)` and also surfaces
`ContextWindowExceeded`, `ServerError`, `AuthFailure`, and `Unclassified`.

This round makes the wrapper **act** on that classification: cooldown duration
from the server's real reset (300s fallback), a bounded **same-account backoff**
for transient limits (rotate/long-cooldown only on genuine exhaustion), the same
logic at the second (resume-blocked) call site, and a **general unhandled-error
handler** that surfaces a styled message + log pointer for anything not yet
modeled. It also records the verified upstream schema in `docs/upstream-codex.md`.

## Previous Rounds

Round 1 (`live-harness-and-gate-wiring`) produced the live `codex exec --json`
schema-verification test (`tests/codex_exec_json_live.rs`), its `cargo-nextest-live`
pre-commit hook (`pre-push` stage) + `just test-live` delegation, and the
`CLAUDE.md` "pre-commit is the Source of Truth" principle. The `#14728` verdict it
recorded informs how the classifier weights snapshots vs `turn.failed`.

Round 2 (`event-reader-and-classification`) is expected to have produced:

- `src/services/account/codex_events.rs` with `scan_events(stdout: &[u8]) ->
  EventSummary { last_rate_limits: Option<RateLimitSnapshot>, turn_error:
  Option<TurnError> }`, declared in `src/services/account/mod.rs`.
- A tail-/line-preserving capture in `src/ui/raw_passthrough.rs` so trailing
  JSONL events survive `failover::MAX_CAPTURE_BYTES`.
- In `src/services/account/failover.rs`: `enum RateLimitClass {
  UsageLimitExhausted, Transient }`, `enum Category { RateLimit(RateLimitClass),
  AuthFailure, ContextWindowExceeded, ServerError, Unclassified }`, `struct
  Classification { category: Category, reset_after_seconds: Option<u64>, snippet:
  String }`, and `fn classify(events, stdout, stderr) -> Option<Classification>`
  (structured-first, text fallback). The legacy `scan`/`pick_priority`/patterns
  remain in place.

Adapt to the actual names/signatures if Round 1 chose slightly different ones.

## Scope of This Round

IN scope:

- Reset-aware cooldown duration in `retry.rs::write_cooldown` + a `Cooldown`
  schema field recording the reset source (tolerant deserialize).
- Transient same-account backoff in `run_auto` (reuse the `force_same`
  mechanism); rotate + cooldown only on `UsageLimitExhausted`.
- Apply classification at the second call site
  `resume_blocked_from_live_rate_limit` in `src/commands/pass_through.rs`.
- General unhandled-error handler: for `ContextWindowExceeded`/`ServerError`/
  `Unclassified`, emit a styled stderr message (class + one-line hint) + a pointer
  to the rolling tracing log, and always `tracing::warn!/error!` the full snippet.
- Update `docs/upstream-codex.md` with the verified codex 0.135.0 JSONL event +
  rate-limit/error schema, with a `Last verified` date.
- Integration tests for the new behavior.

OUT of scope:

- The event reader / classifier internals (done in Round 1).
- Changing the WHAM `account health` HTTP path (`quota.rs`) — unrelated.
- Injecting `--json` into codex invocations the user did not request.

## Current State

### Key Files

- `src/services/account/retry.rs` — the auto path. The rate-limit arm and
  cooldown writer to modify:

  ```rust
  // inside run_auto's loop, after `failover::pick_priority(...)`:
  failover::MatchKind::RateLimit => {
      write_cooldown(&registry, &resolved.id, "429", &matched)?;
      ("429", OutcomeState::RateLimited429, format!("429 rate limit: {}", matched.snippet))
  }

  // the flat-300s writer:
  pub(crate) fn write_cooldown(
      registry: &Registry, account: &AccountId, reason: &str, matched: &failover::Match,
  ) -> Result<(), AppError> {
      let now_unix = now_unix();
      let cd = cooldown::Cooldown {
          reset_at_unix: now_unix + 300,   // <-- flat constant to replace
          reason: format!("{reason} detected: {:?}", matched.snippet),
          last_429_at_unix: now_unix,
          snippet_truncated: matched.snippet.chars().take(256).collect(),
      };
      let account_root = registry.account_dir(account);
      cooldown::write(&account_root, &cd).map_err(AccountError::from)?;
      tracing::info!(op = "cooldown.write", account = %account, reset_at_unix = cd.reset_at_unix, reason = %cd.reason);
      Ok(())
  }
  ```

  The **same-account retry mechanism** already exists (used by the 401-refresh
  path) and should be reused for transient backoff:

  ```rust
  // 401 path: refresh succeeded → retry same account
  force_same = Some(resolved.clone());
  continue;
  ```

  The loop is bounded by `cap` (eligible-account count or `max_retries+1`) and
  tracks a `tried: HashSet<AccountId>`; `force_same` bypasses re-resolution.

- `src/services/account/cooldown.rs` — the persisted record (extend tolerantly):

  ```rust
  pub(crate) struct Cooldown {
      pub reset_at_unix: u64,
      pub reason: String,
      pub last_429_at_unix: u64,
      pub snippet_truncated: String,
  }
  pub(crate) fn read(account_root: &Utf8Path) -> Result<Option<Cooldown>, CooldownError> { /* ... */ }
  pub(crate) fn write(account_root: &Utf8Path, cooldown: &Cooldown) -> Result<(), CooldownError> { /* atomic, 0o600 */ }
  pub(crate) const fn is_active(cd: &Cooldown, now_unix: u64) -> bool { cd.reset_at_unix > now_unix }
  ```

- `src/commands/pass_through.rs` — the second detection site (pinned/resume
  path), which must use the same classification:

  ```rust
  fn resume_blocked_from_live_rate_limit(ctx, registry, resolved, thread_id, stdout, stderr)
      -> Result<Option<AccountError>, AppError> {
      let Some(matched) = failover::pick_priority(failover::scan(stderr), failover::scan(stdout)) else {
          return Ok(None);
      };
      if matched.kind != failover::MatchKind::RateLimit { return Ok(None); }
      retry::write_cooldown(registry, &resolved.id, "429", &matched)?;
      // ... builds ResumeBlocked { thread_id, owner, others }
  }
  ```

- `src/error.rs` — top-level rendering. `print_and_exit` logs then renders:

  ```rust
  pub(crate) fn print_and_exit(err: &AppError, global: &crate::cli::GlobalArgs) -> ExitCode {
      log_error(err);
      if !global.silent {
          if let AppError::Usage(clap_err) = err { let _ = clap_err.print(); }
          else { let _ = render_error(err); }
      }
      ExitCode::from(err.exit_code())
  }
  ```

  `AppError::kind()` returns a stable machine string; `detail(err) ->
  ErrorDetail { what, why_line }` builds the rendered text.

- `src/ui/mod.rs` — `pub(crate) fn write_warning(&self, body: &str) ->
  std::io::Result<()>` (the warning renderer to reuse for the verbose message).

- `src/logging.rs` — rolling daily log; `pub(crate) fn
  log_dir_from_config(config: &Config) -> &Utf8Path` and
  `.filename_prefix("codex-session.log")`. Use these to build the "see log"
  pointer path.

- Tests: `tests/account_failover_retry.rs`, `tests/account_cooldown_cli.rs`, and
  the `fake-429.sh` fixture family (a fixture script emitting a 429 pattern).
  Match this harness style (`TestEnv`, `seed_account`, `write_quota_cache`).

### Existing Patterns

- `Result<T, AppError>` everywhere; `thiserror` enums; no `unwrap()` in
  production; `tracing::{info,warn,error}!` with `op = "..."` structured fields.
- CLI output obeys `docs/design/cli-style-guide.md`: stderr owns
  warnings/errors/progress; `BOLD_RED` for error, `BOLD_CYAN` for hint; never
  restyle passthrough child output; wrapper JSON is `--format json`, never
  `--json`.
- Quality gates via `just` recipes, not raw cargo (repo `CLAUDE.md`).

## Implementation Steps

### Step 1: Reset-aware `write_cooldown`

Change `write_cooldown` to take a resolved reset (e.g. add a
`reset_after_seconds: Option<u64>` parameter, or pass the `Classification`).
Compute `reset_at_unix = now_unix + reset_after_seconds.unwrap_or(300)`, clamped
to a sane maximum (e.g. ≤ 6h) to defend against absurd server values. Keep the
existing call ergonomics minimal. Update all callers.

Extend `cooldown::Cooldown` with a tolerant field recording provenance, e.g.:

```rust
#[serde(default)]
pub reset_source: Option<String>,   // "server-reset" | "retry-after" | "fallback-300s"
```

`#[serde(default)]` ensures existing on-disk `cooldown.json` files still
deserialize. Set it in `write_cooldown` based on whether a server value was used.

### Step 2: Classify at the call site in `run_auto`

In `run_auto`, after running the child, call
`codex_events::scan_events(&stdout_buf)` and `failover::classify(&events,
&stdout_buf, &stderr_buf)`. Replace the `MatchKind`-based branch with the
`Classification.category` branch:

- `Category::AuthFailure` → unchanged behavior (first-use refresh via
  `try_refresh`, else cooldown + rotate).
- `Category::RateLimit(UsageLimitExhausted)` → `write_cooldown(.., reset =
  classification.reset_after_seconds)` and rotate (current rotate behavior,
  recording the precise outcome line + reset source).
- `Category::RateLimit(Transient)` → **bounded same-account backoff**: track a
  per-account transient-attempt counter (e.g. a small `HashMap<AccountId, u8>`
  alongside `tried`); while under the cap (e.g. ≤ 3), sleep the server-suggested
  short delay (`reset_after_seconds`, clamped to ≤ 60s; small default like 5–10s
  if `None`), emit `ctx.ui.write_warning("rate limited; backing off Ns on account
  '<id>'…")` and `tracing::info!(op = "retry.backoff", ...)`, set `force_same =
  Some(resolved.clone()); continue;`. Once the transient cap is exceeded, fall
  through to `write_cooldown` + rotate (defensive: a persistent "transient" is
  treated as exhaustion).
- `Category::ContextWindowExceeded | ServerError | Unclassified` → do NOT rotate
  or cooldown; hand to the general handler (Step 4) so the user sees a clear
  message and the run ends with codex's own output intact.

Bias ambiguous cases toward the safe path (treat as exhaustion/rotate) rather
than retrying a dead account.

### Step 3: Apply classification at the resume-blocked call site

Update `resume_blocked_from_live_rate_limit` in `src/commands/pass_through.rs` to
use `codex_events::scan_events` + `failover::classify` instead of raw
`scan`/`pick_priority`, and pass the resolved reset into `write_cooldown`. Keep
its `ResumeBlocked { thread_id, owner, others }` return shape. (The pinned/resume
path does not rotate, so a transient here still records a cooldown — but with the
real reset duration, not flat 300s.)

### Step 4: General unhandled-error handler (verbose msg + log pointer)

For classified-but-not-rotated categories (`ContextWindowExceeded`,
`ServerError`, `Unclassified`), surface a concise styled stderr line via the
existing renderer surface and append a pointer to the rolling tracing log so the
full detail is recoverable:

- Reuse `ctx.ui.write_warning(...)` (or the error renderer in `src/error.rs` if
  it flows through `AppError`), naming the error class + a one-line hint, e.g.:
  `codex error (context-window-exceeded): the prompt exceeds the model context.
  hint: reduce input or compact the session. full detail: <log_dir>/codex-session.log-YYYY-MM-DD`.
- Build the log path from `logging::log_dir_from_config` + the
  `codex-session.log` prefix.
- Always `tracing::warn!(op = "codex.error.unhandled", class = ..., snippet =
  ...)` (or `error!`) with the full captured snippet, so unmapped errors remain
  fully recoverable from logs for future mapping into the taxonomy.
- Respect `--silent` (the existing `global.silent` gate) and the style guide
  (`BOLD_RED`/`BOLD_CYAN`, stderr only, no restyling of codex's own output).

### Step 5: Update `docs/upstream-codex.md`

Add a section recording the verified codex 0.135.0 facts (currently absent there):
the `codex exec --json` JSONL event types (`thread.started`, `turn.started`,
`turn.completed`, `turn.failed`, `item.*`, `token_count`), the
`RateLimitSnapshot`/`RateLimitWindow` shape (`used_percent`, `window_minutes`,
`resets_in_seconds`/`resets_at`, `plan_type`, `rate_limit_reached_type`), and the
error discriminants (`usage_limit_reached`, `usage_limit_exceeded`,
`context_window_exceeded`) + `retry_after`/"try again in N" parsing. Note the
open question re: exec-mode `rate_limits` population (openai/codex #14728) and
what Round 1's live capture observed. Set/refresh the `Last verified` date to
2026-06-02 and cite the binary as the source.

### Step 6: Integration tests

Extend the `fake-429.sh` fixture family with fixtures that emit (a) a
usage-limit `turn.failed` JSONL and (b) a transient 429 + a `token_count`
snapshot with headroom (and a `retry_after`/"try again in N"). Assert, matching
`tests/account_failover_retry.rs` style:

- Usage-limit fixture → account rotates + cooldown `reset_at_unix` reflects the
  server reset (not `now+300`); `reset_source` recorded.
- Transient fixture → SAME account is retried (backoff) up to the cap before any
  rotation; a following success exits 0 without rotating.
- An unmodeled-error fixture → no rotation/cooldown; verbose stderr message +
  log pointer emitted; full snippet present in the log.

Run the gates (per repo `CLAUDE.md`):

```bash
just lint
just test-unit
just test-integration
```

### Final Step: Update the queue

Record completion in the queue — status lives in YAML; nothing moves on disk:

1. In this plan's `_QUEUE.yaml`, set this round's
   (`item: cooldown-backoff-and-error-ux`) `status` to `done`.
2. All rounds are now done, so in the top-level `.plan/_QUEUE.yaml` set this plan's
   (`item: codex-error-classification-cooldown`) `status` to `done`. Leave the
   plan directory in place.

## Acceptance Criteria

- [ ] `write_cooldown` sets `reset_at_unix` from the classified server reset
      (`resets_in_seconds`/`retry_after`/"try again in N"), clamped, with a 300s
      fallback only when no signal exists; `Cooldown.reset_source` records which,
      and old `cooldown.json` files still deserialize.
- [ ] `run_auto` retries the SAME account with bounded backoff on
      `RateLimit(Transient)` and only rotates + long-cooldowns on
      `RateLimit(UsageLimitExhausted)`; ambiguous cases bias to rotate.
- [ ] `resume_blocked_from_live_rate_limit` uses the same classifier and
      reset-aware cooldown.
- [ ] `ContextWindowExceeded`/`ServerError`/`Unclassified` produce a styled
      stderr message + tracing-log pointer (honoring `--silent` and the style
      guide), with the full snippet logged; no spurious rotation/cooldown.
- [ ] `docs/upstream-codex.md` records the verified 0.135.0 event/rate-limit/error
      schema with a refreshed `Last verified` date.
- [ ] Integration tests cover usage-limit-rotate, transient-backoff-same-account,
      and unmodeled-error-verbose paths.
- [ ] `just lint`, `just test-unit`, `just test-integration` pass.
- [ ] This plan's `_QUEUE.yaml` shows round `cooldown-backoff-and-error-ux` as
      `done`.
- [ ] The top-level `.plan/_QUEUE.yaml` shows this plan as `done`.

## Next Round

This is the final round.
