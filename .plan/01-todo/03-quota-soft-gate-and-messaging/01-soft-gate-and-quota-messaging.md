# Round 01: Soft-Penalty Selection + Out-of-Quota / Resume Messaging

> Plan: quota-soft-gate-and-messaging | Round: 01 of 01 | Complexity: M
> Generated: 2026-06-01 | Repo: /workspaces/codex-session

## Context

`codex-session` is a wrapper that dispatches `codex` invocations across multiple
accounts with quota-aware auto-selection. Auto-selection currently applies a
**hard eligibility gate**: an account is dropped from consideration unless
`five_hour_left > 50% AND weekly_left > 10%`. When all accounts fall below the
five-hour threshold (e.g. 46% / 41% / 0% left), every candidate is dropped, no
account is selected, and the run fails with exit 75 (`AutoExhausted`). This
blocks work even though the accounts still have usable quota and healthy weekly
budgets.

The project owner's decision: **remove the hard gate.** Below-threshold accounts
must never be _blocked_ — only **penalized** in scoring so they are strongly
deprioritized but still selectable. The pool should be used until quota actually
reaches 0%; genuine depletion is then driven by the API returning HTTP 429 →
the wrapper writing a cooldown → rotating to the next account, not by a
preemptive floor.

Two messaging gaps compound the problem: (1) when every account truly cannot run,
the error is a thin bullet list with a misleading "clear cooldowns" hint and no
reset ETA; (2) a `resume <thread-id>` is bound to the account that owns the
thread (the rollout only exists in that account's `CODEX_HOME`), so when that
owner is quota-limited the child just 429s with no guidance. Both messages must
be clear to a human _and_ parseable by an AI agent, and must say when quota
returns.

This round delivers all three changes plus documentation, in the repo and in the
external `~/.dotfiles` / `~/DocsNNotes` orientation files.

## Previous Rounds

This is the first round — no prior rounds. (Note: a separate plan,
`.plan/02-done/02-spinner-parallel-async-ux/`, migrates the wrapper to a tokio
async runtime; this round assumes that async runtime is present, e.g.
`account quota`/`health`/`exec` resolve through async paths and `src/runtime.rs`
exists. Adapt to whatever the codebase actually shows — the changes here are
runtime-agnostic.)

## Scope of This Round

**In scope:**

- Remove the hard quota gate in account scoring; replace with a dominant soft
  penalty so below-knee accounts are last-resort selectable.
- Tighten `NoEligible` / `AutoExhausted` semantics to mean _no usable account_ /
  _all tried-and-failed_.
- Enrich the out-of-quota (`AutoExhausted`, and the all-cooldown `NoEligible`)
  report to be human + AI readable with per-account reset ETAs.
- Add a resume-specific quota-blocked message (`ResumeBlocked`) and a pre-flight
  check in the resume path.
- Update `docs/account-auto-selector.md`; update the external `~/.dotfiles`
  prex/review-loop skills and `~/DocsNNotes` codex-conventions orientation.
- Update and extend unit + integration tests for all of the above.

**Out of scope:**

- Renaming config keys (`five_hour_threshold` / `weekly_floor` keep their names).
- Per-knee penalty stacking (single constant penalty is used).
- Spinner / async-runtime work (separate plan).
- Any change to the 429 detection (`failover::scan`) or cooldown duration policy
  beyond reading existing values for reporting.

## Current State

### Key Files

- `src/services/account/selector.rs` — auto-selection scoring and the hard gate.

  The single hard-block point, in `score_candidate`:

  ```rust
  let breakdown = score_for_display(&candidate.quota_state, &params);
  if !breakdown.eligible {
      return None;
  }

  Some(ScoredCandidate {
      id: candidate.id.clone(),
      total: breakdown.total,
      tie_five_hour: breakdown.tie_five_hour,
  })
  ```

  The eligibility + scoring math, in `score_for_display` (the `QuotaState::Known`
  arm):

  ```rust
  let eligible = quota.five_hour.percent_left > params.five_hour_threshold
      && quota.weekly.percent_left > params.weekly_floor;
  let fht = params.five_hour_threshold;
  let wf = params.weekly_floor;
  let reason = (!eligible).then_some(format!("five_hour>{fht} and weekly>{wf} required"));
  let weekly_weight = 1.0 - params.five_hour_weight;
  let avail_score = params.five_hour_weight.mul_add(
      quota.five_hour.percent_left,
      weekly_weight * quota.weekly.percent_left,
  ) - 50.0;
  let weekly_pressure = if quota.weekly.percent_left < 20.0 { -30.0 } else { 0.0 };
  let fh_pressure = if quota.five_hour.percent_left < 15.0 { -25.0 } else { 0.0 };
  ```

  And `total`:

  ```rust
  let total = base + plan_bonus_f + recency + avail_score + weekly_pressure + fh_pressure;
  ```

  `pick_from_candidates` drops `None`-scored candidates and errors `NoEligible`
  when none remain:

  ```rust
  let Some(scored) = score_candidate(candidate, now, five_hour_threshold, weekly_floor, five_hour_weight) else {
      tracing::debug!(account = %candidate.id, reason = "threshold");
      continue;
  };
  // ...
  best.map_or_else(
      || { tracing::warn!(op = "account.select_no_eligible"); Err(AccountError::NoEligible) },
      Ok,
  )
  ```

  `pick()` gathers candidates with `usability_skip_reason` (no quota check —
  below-floor accounts are already in the pool). The public `skip_reason`
  (`include_quota_threshold = true`) and `quota_threshold_reason` helpers exist
  only for the AutoExhausted report:

  ```rust
  fn quota_threshold_reason(ctx: &AppContext, account: &AccountId) -> Option<String> {
      // ...
      let below_five_hour = quota.five_hour.percent_left <= ctx.config.account.five_hour_threshold;
      let below_weekly = quota.weekly.percent_left <= ctx.config.account.weekly_floor;
      (below_five_hour || below_weekly).then(|| {
          format!("below quota threshold (5h {:.1}% / weekly {:.1}%)",
              quota.five_hour.percent_left, quota.weekly.percent_left)
      })
  }
  ```

- `src/services/account/quota.rs` — quota model. `Window { percent_left: f64,
  reset_at_unix: u64 }`; `percent_left` is quota **remaining** (not used).
  `quota::get(ctx, &id, ttl)` returns `QuotaResult::Ok(Quota)` /
  `ApiKeyMode`. The `Quota` struct has `.five_hour` and `.weekly` `Window`s.

- `src/services/account/retry.rs` — `run_auto` rotation loop and `build_report`.
  The cap counts usable accounts only (no quota):

  ```rust
  let eligible = accounts.iter().filter(|e| selector::is_usable(ctx, &registry, e)).count();
  ```

  `build_report` currently labels not-tried accounts with the quota-inclusive
  `skip_reason`:

  ```rust
  let outcome = ran.remove(&entry.id).unwrap_or_else(|| {
      selector::skip_reason(ctx, registry, entry).unwrap_or_else(|| {
          if tried.contains(&entry.id) { "attempted but no terminal outcome recorded".to_owned() }
          else { "not attempted (rotation cap reached)".to_owned() }
      })
  });
  ```

  `write_cooldown` writes a 300s cooldown with `reset_at_unix = now + 300` on a
  detected 429/401.

- `src/services/account/error.rs` — `AccountOutcomeLine { id: AccountId,
  outcome: String }`, and the error enum:

  ```rust
  #[error("no eligible account")]
  NoEligible,
  #[error("auto-selection exhausted; no account could complete the request")]
  AutoExhausted { report: Vec<AccountOutcomeLine> },
  ```

- `src/error.rs` — error rendering and exit-code mapping. Exit 75 group:

  ```rust
  AccountError::NoEligible
  | AccountError::AutoExhausted { .. }
  | AccountError::LoginFailed { .. }
  | AccountError::AuthMissing { .. } => 75,
  ```

  The `AutoExhausted` render arm builds a flat bullet list:

  ```rust
  AccountError::AutoExhausted { report } => {
      let mut why_line = "no account could complete the request".to_owned();
      for line in report {
          let _ = write!(why_line, "\n  • {}   {}", line.id, line.outcome);
      }
      ErrorDetail { what: "account: auto-selection exhausted".to_owned(), why_line }
  }
  ```

  `NoEligible` render: `what: "account: no eligible account"`, `why_line: "all
  accounts were filtered out"`. The hint for AutoExhausted currently suggests
  `account health` / `cooldown clear --all`. `log_error` emits a `tracing::error!`
  record with `op = "command.error"`.

- `src/commands/pass_through.rs` — `run_resume` (`fn run_resume(ctx, argv,
  intent, fallback)`) resolves the owning account via `resolve_resume_account`
  (thread-index lookup → `ResolvedAccount { id, source: ThreadIndex }`) and
  dispatches it directly:

  ```rust
  let session = SignalSession::install()?;
  let (exit_code, _stdout, _stderr) = run_once(ctx, &effective_argv, &resolved, &session, false, gid_override)?;
  Ok(exit_code)
  ```

  `run_once(ctx, argv, resolved, session, capture, group_id_override)` runs one
  child; `capture=true` tees+captures stdout/stderr for `failover::scan`.

- `src/ui/mod.rs` — `human_duration_until(reset_at_unix: u64) -> String`
  (private fn ~line 1562) formats "resets in 45m 20s"; the `account quota`
  renderer prints `"{:.1}% left   resets in {}"`. `account quota`/`health`
  surface `eligible` + `ineligible_reason` (~lines 1109-1111).

- `src/commands/account/{quota.rs,health.rs,mod.rs}` — carry `eligible` /
  `ineligible_reason` from `ScoreBreakdown` into the rendered views.

- `docs/account-auto-selector.md` — documents the thresholds as hard floors
  (table rows `weekly_floor 10.0`, `five_hour_threshold 50.0`, "Minimum … to be
  eligible") and the selection/failover behavior.

- `tests/account_auto_selector.rs` — integration tests. `quota_cache(fh, wk)`
  helper writes a cache; `auto_exec_returns_tempfail_when_all_are_below_threshold`
  asserts exit 75 when all below threshold.

### Existing Patterns

- Quota %s are always "percent **left**" (remaining). Preserve this everywhere.
- User-facing CLI output (stderr error blocks, `account` renderers, runtime
  narration) is governed by `docs/design/cli-style-guide.md` — consult it before
  changing wording, color, or stdout/stderr ownership.
- Quality gates run via `just` recipes, not raw cargo: `just test-unit`,
  `just test-integration`, `just test`, `just lint`, `just check`.
- Errors carry a structured `tracing` record (`log_error`) as the machine
  channel; `--format json` is wrapper-owned, `--json` is codex passthrough.

## Implementation Steps

### Step 1: Remove the hard gate, add the soft penalty (`selector.rs`)

In `score_for_display`, define a module-level constant and fold a dominant
penalty into `total` when below either knee:

```rust
const BELOW_KNEE_PENALTY: f64 = 1000.0; // dominates the full score spread (~200)
```

Compute `let knee_penalty = if eligible { 0.0 } else { -BELOW_KNEE_PENALTY };`
and add it to `total`:

```rust
let total = base + plan_bonus_f + recency + avail_score
    + weekly_pressure + fh_pressure + knee_penalty;
```

Keep `avail_score` and the existing `weekly_pressure`/`fh_pressure` knees for
intra-tier gradient. Reword the `ineligible_reason` string from
`"five_hour>{fht} and weekly>{wf} required"` to a non-blocking phrasing, e.g.
`"below penalty knee (5h≤{fht}% or weekly≤{wf}%): deprioritized"`. Retain the
`eligible` bool but treat it as informational ("above penalty knee").

In `score_candidate`, **remove** the `if !breakdown.eligible { return None; }`
block so it always returns a `ScoredCandidate`. Simplify its return type from
`Option<ScoredCandidate>` to `ScoredCandidate`, and in `pick_from_candidates`
drop the `let Some(scored) = … else { continue; }` plus the
`reason = "threshold"` debug line — call `score_candidate(...)` directly.
`pick_from_candidates` still returns `NoEligible` when the candidate slice is
empty (no usable accounts).

Result: any above-knee account outranks any below-knee account (1000 ≫ spread);
within each tier, ranking is by `avail_score`. No account is blocked for quota.

### Step 2: Tighten report sourcing + drop dead helpers (`retry.rs`, `selector.rs`)

In `retry.rs::build_report`, the `unwrap_or_else` fallback must no longer treat
below-quota as a skip reason (below-knee accounts are now tried). Switch the call
from the quota-inclusive `selector::skip_reason` to a usability-only reason
(reuse the existing `usability_skip_reason` logic — expose a `pub(crate)`
usability-only accessor if needed). After this, the public `skip_reason` and
`quota_threshold_reason` helpers in `selector.rs` are unused — delete them (and
any now-unused imports). Confirm no other caller exists (`grep -rn "skip_reason\b"`).

`run_auto`'s control flow is unchanged: `NoEligible` from `resolve_for_exec`
still breaks the loop, and `AutoExhausted` is built after the rotation. The
semantics now naturally mean: selection always returns a usable candidate, so
exit 75 only happens after real 401/429 failures or when no usable account
exists.

### Step 3: Enrich the outcome model + build_report ETAs (`error.rs`, `retry.rs`)

Extend `AccountOutcomeLine` (`src/services/account/error.rs`) with structured
fields alongside `outcome`:

```rust
pub(crate) struct AccountOutcomeLine {
    pub(crate) id: AccountId,
    pub(crate) outcome: String,
    pub(crate) state: OutcomeState,        // new enum
    pub(crate) five_hour_left: Option<f64>,
    pub(crate) weekly_left: Option<f64>,
    pub(crate) available_at_unix: Option<u64>,
}
```

Add an `OutcomeState` enum (e.g. `FiveHourExhausted`, `WeeklyExhausted`,
`RateLimited429`, `Cooldown`, `BelowKnee`, `NoAuth`, `TokenExpired`,
`NotAttempted`). In `build_report`, for each account read `quota::get` and
`cooldown::read` to populate the fields and compute `available_at_unix` (min of
the binding window reset and any cooldown reset). Track the **earliest**
`available_at_unix` across all accounts for the summary line. Keep `outcome` as
the human one-liner, derived from the structured fields.

### Step 4: Human + AI-readable AutoExhausted / NoEligible render (`error.rs`, `ui/mod.rs`)

Promote `human_duration_until` in `src/ui/mod.rs` to `pub(crate)` and reuse it.
Rewrite the `AutoExhausted` render arm to a structured, parseable block:

```text
codex-session: account: out of quota — no account can run right now
  why:  all 3 accounts are out of quota or rate-limited
    • cwnt   5-hour exhausted (0% left, weekly 84%)   back in 24m  (14:48)
    • isma   rate-limited (429), cooling down          back in 12m  (14:36)
    • mari   5-hour 0% left (weekly 84%)               back in 24m  (14:48)
  hint: earliest account frees up in ~12m (isma, 14:36) — weekly quota is
        healthy; this is the 5-hour window. Re-run after 14:36.
```

Each bullet has a stable shape: `• <id>  <state-phrase>  back in <dur> (<clock>)`.
Make the hint **cause-aware**: only suggest `cooldown clear --all` when cooldowns
are the block; for window exhaustion, give the reset ETA instead. Apply the same
ETA-bearing treatment to the `NoEligible` render (all-cooldown / token-expired
pool) rather than "all accounts were filtered out". Consult
`docs/design/cli-style-guide.md` for label/color/stderr conventions.

In `log_error` (`error.rs`), for `AutoExhausted` emit the structured per-account
report plus `earliest_available_at_unix` as `tracing` fields so the JSON log
record is fully machine-parseable.

### Step 5: Resume-aware quota-blocked message (`error.rs`, `pass_through.rs`)

Add `AccountError::ResumeBlocked` carrying: `thread_id: String`, owner `id` +
`owner_available_at_unix: Option<u64>`, owner window %s, and a `Vec` of the
_other_ accounts as `{ id, available_now: bool, available_at_unix: Option<u64>,
five_hour_left: Option<f64>, weekly_left: Option<f64> }`. Map it to exit code 75
in `src/error.rs` (same group as `AutoExhausted`).

In `run_resume` (`pass_through.rs`), after resolving the owner: pre-flight via
`cooldown::read`/`is_active` and `quota::get` — if the owner is clearly blocked
(active cooldown or a window at/near 0%), build and return `ResumeBlocked` before
spawning a doomed child. Otherwise attempt as today, but call `run_once` with
`capture=true` so a live 429 (`failover::scan` on the captured streams) is
converted into the same `ResumeBlocked` instead of a bare child exit.

Render `ResumeBlocked` (new arm in `error.rs::detail` + hint), reusing
`human_duration_until`:

```text
codex-session: resume blocked — thread belongs to a quota-limited account
  where: thread 01J… → account `cwnt`
  why:   `cwnt` is out of 5-hour quota (0% left); back in 24m (14:48)
  hint:  resume is bound to `cwnt` — accounts `isma`, `mari` have quota now but
         cannot continue THIS thread. Wait until 14:48 and re-run the resume, or
         start a fresh `exec` on an available account (loses thread continuity).
```

If no other account is ready, drop the "have quota now" line and list every
account's reset ETA, soonest highlighted. Emit structured fields via `log_error`
for the AI path. Cause-aware hint (cooldown vs window), like Step 4.

### Step 6: In-repo docs (`docs/account-auto-selector.md`)

Rewrite the "ineligible / hard floor" language as "penalty knee / soft
deprioritization": the thresholds no longer block; they apply a dominant scoring
penalty so below-knee accounts are last-resort. Document that the pool is used to
0% and that true exhaustion is 429-driven (cooldown → rotate → `AutoExhausted`),
and that `resume` is account-bound (`ResumeBlocked` when the owner is quota-limited).

### Step 7: External orientation docs (cross-repo — flagged)

> These edits are in separate git repos and are NOT covered by this repo's tests
> or review-loop. Apply them directly by absolute path.

- `~/.dotfiles/claude/.claude/skills/prex/SKILL.md` — the prex skill resumes a
  plan thread (`exec resume "$PLAN_THREAD_ID"`) and has a fresh-exec fallback.
  Document that exit 75 with a `ResumeBlocked` message means the owning account
  is quota-limited; recovery is to wait for the reset shown in the message OR
  take the stage-3 fresh-exec fallback on an available account (new thread, no
  continuity). Keep auto selection for fresh execs. Add the same note to
  `~/.dotfiles/claude/.claude/skills/review-loop/SKILL.md` if it resumes.
- `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` — add: (a) the now
  _soft_ quota thresholds (50% 5-hour / 10% weekly are penalty knees, not floors;
  accounts are used to 0%); (b) `percent_left` semantics ("% left", not used);
  (c) what `AutoExhausted` / `ResumeBlocked` mean and how to read their reset
  ETAs; (d) a "resume is account-bound" subsection (cannot rotate to other
  accounts; wait-for-reset or fresh exec).

### Step 8: Tests (`tests/account_auto_selector.rs`, `selector.rs` unit tests)

- **Rewrite** `auto_exec_returns_tempfail_when_all_are_below_threshold`: with all
  accounts below the five-hour knee, assert the **highest-quota** account IS
  selected and the exec runs (exit 0 via the fake codex), not exit 75. Rename to
  reflect the new contract (e.g. `auto_exec_selects_best_below_knee_account`).
- **selector.rs unit tests**: update the cases that asserted below-threshold →
  dropped / `NoEligible` / `assert!(score.eligible)` to the soft model. Add:
  (a) a below-knee account is still returned by `pick_from_candidates`;
  (b) any above-knee account outranks any below-knee account regardless of
  `avail_score` (tiering invariant); (c) below-knee accounts order among
  themselves by remaining quota; (d) `NoEligible` fires only with an empty usable
  pool.
- Keep/add an `AutoExhausted` test for the real path (all usable accounts emit
  429 / are in cooldown → exhausted) and assert the message contains per-account
  reset ETAs + earliest-available summary, and that the hint does NOT suggest
  `cooldown clear` for a pure quota-window block (but DOES for cooldowns).
- Add a resume test: owner seeded in active cooldown / 0% quota → `ResumeBlocked`
  naming the owner + reset ETA, listing other accounts' availability, and (when
  none ready) listing all reset times.

### Step 9: Verify

Run the gates and the manual scenarios:

```bash
just lint
just test
# Manual:
just run -- account quota            # below-knee accounts show as deprioritized, not "ineligible"
just run -- exec "echo probe"        # selects highest-quota account; no exit 75
```

Confirm exit 75 now requires genuine failure (all usable accounts in cooldown or
the fake codex emitting 429 → `AutoExhausted` with the friendly report), and that
a resume to a quota-limited owner prints `ResumeBlocked`.

### Final Step: Update plan index

Update the plan's `README.md` (in the same directory as this round file) to
record completion:

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

Because this is the final (only) round, also:

1. In the README.md header blockquote, change `Status: todo` to `Status: done`.
2. Move the plan directory to done:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/03-quota-soft-gate-and-messaging .plan/02-done/03-quota-soft-gate-and-messaging
```

## Acceptance Criteria

- [ ] `score_candidate` no longer returns `None` for `!eligible`; the hard-gate
      `return None` is removed and the fn returns `ScoredCandidate` directly.
- [ ] `BELOW_KNEE_PENALTY` is applied so any above-knee account outranks any
      below-knee account, while below-knee accounts remain selectable and ordered
      by remaining quota (covered by the tiering unit test).
- [ ] With all accounts below the five-hour knee, `exec` selects the
      highest-quota account and runs (no exit 75); the rewritten integration test
      asserts this.
- [ ] `NoEligible` only occurs with an empty usable pool; `AutoExhausted` only
      after real 401/429 failures.
- [ ] Dead `skip_reason` / `quota_threshold_reason` helpers removed; `build_report`
      uses usability-only reasons.
- [ ] `AutoExhausted` (and all-cooldown `NoEligible`) render a human+AI-readable
      block with per-account state, reset ETA, and an earliest-available summary;
      the hint is cause-aware (no misleading `cooldown clear` for window blocks).
- [ ] `log_error` emits structured per-account fields + `earliest_available_at_unix`
      for the AI/machine path.
- [ ] `resume <thread-id>` to a quota-limited owner yields `ResumeBlocked` (exit
      75) naming the owner + reset ETA, listing other accounts' availability, or
      all reset times when none are ready.
- [ ] `docs/account-auto-selector.md` rewritten for the soft-knee model;
      `~/.dotfiles` prex/review-loop skills and `~/DocsNNotes` codex-conventions
      updated with the soft-threshold + account-bound-resume guidance.
- [ ] `just lint` and `just test` pass.
- [ ] Plan `README.md` execution order table shows round 01 as `done` with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from `.plan/01-todo/03-quota-soft-gate-and-messaging` to `.plan/02-done/03-quota-soft-gate-and-messaging`.

## Next Round

This is the final round.
