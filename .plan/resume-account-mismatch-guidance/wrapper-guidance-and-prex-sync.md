# Resume Account-Mismatch: Steer the Right Account & Command

> Plan: resume-account-mismatch-guidance | Round: 1 of 1 | Complexity: M | Generated: 2026-06-02 |
> Repo: /workspaces/codex-session

## Context

`codex-session exec resume <ID>` resolves which account owns a thread via an on-disk
thread-index (`thread-index.jsonl`). When the index has **no** entry for `<ID>`, the wrapper
**silently falls back to normal account auto-selection** and forwards `resume <ID>` to whatever
account auto-selection picks. Because codex rollouts are fully scoped per-account
(`accounts/<acct>/groups/<g>/sessions/YYYY/MM/DD/rollout-*.jsonl`), the picked account almost never
owns that rollout, so codex fails deep with an opaque JSON-RPC error:

```text
[codex-session] auto-selection enabled (2 candidate(s)).
Error: thread/resume: thread/resume failed: no rollout found for thread id 019e…-… (code -32600)
```

This burned a real `prex` run: stage-1 planning created a thread under account **P**; stage-3
resume missed the index, auto-selected a different account, and got `-32600`. `docs/upstream-codex.md`
already documents this hazard (F16: "the wrapper must keep the resume pinned to the index entry's
account — if the resume re-resolves the account … you get the -32600 'no rollout found' failure").

This round makes the wrapper **diagnose and steer** instead of leaking a raw `-32600`:

1. On an index miss for `resume <ID>`, **discover the true owner** by scanning each account's
   rollout store; if found, **auto-pin to it and resume**, emitting a loud `warning:` that names the
   recovered owner.
2. If found in **no** account, emit a rich, classified error (typo vs. lost index) that lists recent
   known threads as candidates and gives the fresh-`exec` command.
3. Translate a `-32600 "no rollout found"` that codex returns **even after a correct pin** (sandbox
   mismatch per F15, or a deleted rollout) into a friendly, classified message.
4. Update the `prex` skill (`~/.dotfiles`) to pin the planning account across stages and to react to
   the new, clearer wrapper signals instead of blindly inlining a 41 KB fresh-exec prompt.

This plan runs **after** `codex-error-classification-cooldown`, which lands the general
unhandled-error verbose+log handler and the structured-event/text classification scaffolding this
round reuses for step 3.

## Previous Rounds

This is the first and only round. It assumes `codex-error-classification-cooldown` is already `done`:
its `failover.rs` classification layer, the general "unhandled error → styled stderr + tracing log"
handler, and any `codex_events.rs` JSONL reader exist and can be reused. If a helper this round
expects is absent, implement the minimal local equivalent rather than blocking.

## Scope of This Round

IN scope:

- New rollout-owner discovery (scan registered accounts' session stores for a thread id).
- Auto-pin recovery + `warning:` on index miss with a discoverable owner.
- New `AccountError` variant(s) + `ErrorDetail`/hint rendering for the "no owner anywhere" case,
  including a recent-threads candidate list.
- Post-exec `-32600 / "no rollout found"` classification on the resume path (sandbox-mismatch vs.
  deleted-rollout), reusing the cooldown plan's general handler where present.
- `docs/upstream-codex.md` F14/F16 update + `Last verified` bump.
- Unit + integration tests; a stderr snapshot for the new messages.
- `prex` SKILL.md (`~/.dotfiles`): pin planning account into the stage-3 resume; rewrite the Resume
  Fallback decision table to match the new wrapper behavior.

OUT of scope:

- Changing the cooldown/failover rotation engine itself (owned by `codex-error-classification-cooldown`).
- Cross-account _resume_ (impossible in stock codex per F16 — we pin to the owner, never migrate).
- Reworking `--last`/`--all` resolution beyond improving its miss message.

## Current State

### Key Files

- `/workspaces/codex-session/src/commands/pass_through.rs` — the resume path. `run_resume` (the
  `else` branch is the bug) silently falls back to auto-selection on an index miss:

  ```rust
  } else {
      tracing::info!(
          op = "resume", status = "fallback",
          "no thread index hit; falling back to normal resolution"
      );
      let sanitized = strip_wrapper_resume_flags(argv);
      let fallback_argv = sanitized.as_deref().unwrap_or(argv);
      // … dry-run …
      fallback.map_or_else(
          || crate::services::account::retry::run_auto(ctx, fallback_argv),
          |resolved| crate::services::account::retry::single_attempt(ctx, fallback_argv, resolved),
      )
  }
  ```

  `resolve_resume_account` returns `None` for `ResumeIntent::ById` when
  `thread_index::lookup` yields `Ok(None)` (the `entry?` at the end). `ResumeIntent` distinguishes
  `ById(String)` from `Last { all_groups }`. `run_once` already appends to the index post-exec when
  `--json` is set and a `thread.started` event is found. The existing `ThreadIndex` resolution
  source is `AccountResolutionSource::ThreadIndex`.

- `/workspaces/codex-session/src/services/session/thread_index.rs` — `lookup`, `last_any`,
  `last_for_group`, `read_entries`, `ThreadEntry { thread_id, account, group_id, cwd, created_at }`.
  `read_entries` is the basis for the "recent threads" candidate list.

- `/workspaces/codex-session/src/services/session/dir.rs` — `session_dir(root, account, group_id)`
  builds `accounts/<acct>/groups/<group_id>/`. The per-account CODEX_HOME under which codex writes
  `sessions/YYYY/MM/DD/rollout-*.jsonl`. Owner-discovery scans these.

- `/workspaces/codex-session/src/services/account/error.rs` — `AccountError` enum +
  `AccountOutcomeLine` + `OutcomeState`. Add the new variant(s) here; follow the existing
  `ResumeBlocked { thread_id, owner, others }` shape and `kind()` mapping.

- `/workspaces/codex-session/src/error.rs` — error rendering. `render` prints
  `codex-session: <what>` (`BOLD_RED`), `where:`/`why:` (`BOLD`), `hint:` (`BOLD_CYAN`).
  `account_error_detail` → `ErrorDetail { what, why_line }`; `error_hint` builds the hint;
  `resume_blocked_detail` + `account_report_hint` are the closest template. `exit_code` maps
  `ResumeBlocked`/`AutoExhausted` to 75 — give the new variants a deliberate code.

- `/workspaces/codex-session/src/services/account/registry.rs` — `list()` enumerates accounts,
  `account_dir(name)` gives an account root. Drives the per-account scan loop.

- `/workspaces/codex-session/src/ui/mod.rs` — `write_warning(body)` renders a `BOLD_YELLOW`
  `warning:` line (use it for the auto-pin notice; warnings go to stderr per the style guide).
  `human_duration_until(unix)` and the sibling `"{} ago"` past-duration formatter (near line 1674)
  format the candidate list's relative ages.

- `/workspaces/codex-session/docs/upstream-codex.md` — F14 (resume + index fallback), F15
  (`-32600` on sandbox mismatch), F16 (cross-account impossible; pin to owner). Update to describe
  the new discover-and-pin + classify behavior; bump `Last verified`.

- `/home/gu/.dotfiles/claude/.claude/skills/prex/SKILL.md` — the orchestrator. Stage-3 resume has
  **no** account pin:

  ```bash
  codex-session exec --profile implementation resume "$PLAN_THREAD_ID" \
    --dangerously-bypass-approvals-and-sandbox --json \
    --output-last-message "$RUN_DIR/stage3-impl-report.txt" \
    "$(cat "$RUN_DIR/stage3-prompt.md")" \
    < /dev/null > "$RUN_DIR/stage3-events.jsonl"
  ```

  The **Resume Fallback** section treats `"no rollout found"` / `"thread not found"` as an automatic
  trigger to inline a fresh-exec — the behavior that wasted a run.

### Existing Patterns

- Errors are `thiserror` variants on `AccountError`, rendered via `account_error_detail` + `error_hint`;
  never `println!`/`eprintln!` directly — use `ctx.ui.*` and the `error.rs` renderer
  (`docs/design/cli-style-guide.md`: stderr ownership, `BOLD_RED` error / `BOLD_CYAN` hint /
  `BOLD_YELLOW` warning).
- Quality gates run via `just`, not raw cargo: `just lint`, `just test-unit`, `just test-integration`.
- Resolution sources are tagged via `AccountResolutionSource`; pinning the recovered owner should use
  `ThreadIndex` (or a new `RolloutScan`) source so the dry-run/source label is honest.
- Wrapper machine-readable output uses `--format json`, never `--json` (which is codex's flag).

## Implementation Steps

### Step 1: Owner-discovery service

Add a function (e.g. `src/services/session/rollout_scan.rs`, module-registered in
`src/services/session/mod.rs`) that, given the session root and a thread id, returns
`Option<(AccountId, group_id)>` for the account whose store contains that thread's rollout. Iterate
`registry.list()` accounts; under each `accounts/<acct>/groups/*/sessions/`, match the thread id —
codex names rollouts `rollout-<date>-<session-uuid>.jsonl` and the thread id is that uuid, so prefer
a **filename** match (cheap) and fall back to a bounded content scan only if needed. Return the first
match. Keep it read-only and resilient (skip unreadable dirs, never panic). Unit-test against a
temp tree with a planted rollout under one of two accounts.

### Step 2: New error variant(s) + rendering

In `src/services/account/error.rs` add variants, e.g.:

- `ResumeOwnerMissing { thread_id, recent: Vec<AccountOutcomeLine | ThreadCandidate> }` — no account
  owns the rollout. Carry a small list of recent known threads (id, account, age) for candidates.
- (Optional) `ResumeSandboxMismatch { thread_id, owner }` for the post-exec `-32600` sandbox case, or
  fold that into the cooldown plan's general handler if it already models it.

Add `kind()` strings and an `exit_code()` mapping (use a distinct code; `ResumeOwnerMissing` is a
not-found condition — 75 is consistent with the other resume/eligibility errors, choose deliberately).
In `src/error.rs`, add `account_error_detail` arms and `error_hint` arms producing the agreed copy:

```text
codex-session: no rollout for thread 019e…
  why:  not found in the thread index or any account's rollout store
  hint: verify the id, or start a fresh thread: codex-session exec …
  recent threads: 01a2… (work, 4m ago), 0c91… (personal, 2h ago)
```

Mirror `resume_blocked_detail`/`append_account_outcome_block` formatting.

### Step 3: Rewire `run_resume` — discover, auto-pin + warn, else steer

Replace the silent `else`-fallback for `ResumeIntent::ById`:

1. Call Step 1's discovery.
2. **Owner found** → build a `ResolvedAccount { id: owner, source: ThreadIndex|RolloutScan }`, emit a
   `ctx.ui.write_warning` naming the recovered owner and the index gap, log a `tracing::warn!`, then
   run the resume pinned to that owner (reuse the existing pinned-resume path — `run_once` with the
   discovered `group_id` override, same as the index-hit branch). Backfill the index entry so the next
   resume is a clean hit.
3. **Owner not found** → return `AccountError::ResumeOwnerMissing { thread_id, recent }` built from
   `thread_index::read_entries` (most-recent N). Do **not** auto-select and forward.

Keep `ResumeIntent::Last { .. }` behavior, but when its index lookup misses, return a clear
"no recorded threads to resume" message rather than auto-forwarding a `--last` that codex can't honor.
Preserve `--dry-run` parity (the dry-run branch must report the recovered/pinned account, like the
index-hit path does today).

### Step 4: Post-exec `-32600` classification on the resume path

After a (now correctly pinned) resume returns, scan the child's captured stdout/stderr — the resume
path already captures both (`run_once(..., capture: true, ...)` and
`resume_blocked_from_live_rate_limit`). If the output carries `-32600` / `no rollout found` /
`thread not found`, classify:

- **Sandbox mismatch (F15):** hint to re-run resume with the **same** `--sandbox` flags as the
  original run.
- **Deleted/absent rollout:** hint that the rollout is gone; start fresh.

Reuse the `codex-error-classification-cooldown` general handler (structured-event first, text
fallback) so the message is styled and the full snippet is logged. If that handler isn't present,
implement a minimal text scan local to the resume path.

### Step 5: Update `docs/upstream-codex.md`

Update F14 (the index-miss path now discovers + pins, no silent auto-select), F15 (the wrapper now
classifies the post-exec `-32600` sandbox case), and F16 (discover-and-pin is how the wrapper avoids
cross-account resume). Bump `Last verified` to 2026-06-02 and keep the `src/commands/pass_through.rs`
line references current.

### Step 6: Tests

- Unit: owner discovery (planted rollout under one of two accounts; miss returns `None`);
  `ResumeOwnerMissing` rendering snapshot; recent-threads formatting.
- Integration (`tests/session_resume_routing.rs` + a new test): `resume <ID>` with an empty index but
  a rollout on disk → auto-pin + warning + success; `resume <ID>` with nothing anywhere → friendly
  `ResumeOwnerMissing` (assert it does **not** auto-select). Use the existing routing-test harness.
- A stderr snapshot covering the auto-pin warning and the no-owner error (color off).
- Run `just lint`, `just test-unit`, `just test-integration` and fix to green.

### Step 7: Update the `prex` skill in `~/.dotfiles`

> Cross-repo step — see Risks. This edits `/home/gu/.dotfiles/claude/.claude/skills/prex/SKILL.md`,
> outside this repo. Run this step with the working tree / sandbox scoped so `~/.dotfiles` is
> writable (e.g. invoke the implementation with cwd `~/.dotfiles`, or apply this single edit by
> hand). Do not `git commit` in either repo.

1. **Pin the planning account across stages.** In stage 1, capture the resolved planning account
   (from the session meta written under the planning `CODEX_HOME`, or via
   `codex-session account current --format json`) alongside `PLAN_THREAD_ID`. In stage 3, pass
   `--account "$PLAN_ACCOUNT"` to the resume so it never depends on auto-selection:

   ```bash
   codex-session --account "$PLAN_ACCOUNT" exec --profile implementation resume "$PLAN_THREAD_ID" \
     --dangerously-bypass-approvals-and-sandbox --json \
     --output-last-message "$RUN_DIR/stage3-impl-report.txt" \
     "$(cat "$RUN_DIR/stage3-prompt.md")" \
     < /dev/null > "$RUN_DIR/stage3-events.jsonl"
   ```

2. **Rewrite the Resume Fallback decision table** (SKILL.md ~511–555) to match the new wrapper signals:
   - `ResumeBlocked` (exit 75, `account: resume blocked`) → unchanged: wait for the owner's reset.
   - Auto-pin `warning:` (recovered owner) → resume **succeeded**; do **not** fall back.
   - `ResumeOwnerMissing` (no rollout in index or any account) → the thread is genuinely unknown;
     the fresh-`exec` fallback is the correct response (this is the only true fresh-exec trigger).
   - `-32600` classified as **sandbox mismatch** → re-run resume with the same `--sandbox`/bypass
     flags, **not** a fresh exec.
     Keep the existing prompt-file / no-run-dir-paths / 600s-timeout rules.

### Final Step: Update the queue

1. In this plan's `_QUEUE.yaml`, set round `wrapper-guidance-and-prex-sync` `status` to `done`.
2. All rounds are now done, so in the top-level `.plan/_QUEUE.yaml` set this plan
   (`item: resume-account-mismatch-guidance`) `status` to `done`. Leave the plan directory in place.

## Acceptance Criteria

- [ ] `resume <ID>` with an empty thread-index but a rollout present under some account **auto-pins to
      that owner, prints a `BOLD_YELLOW` `warning:` naming it, and resumes successfully** — no
      `auto-selection` and no `-32600`.
- [ ] `resume <ID>` with the thread present in **no** account returns `ResumeOwnerMissing`: a
      `BOLD_RED` error with a `why:`, a `BOLD_CYAN` fresh-`exec` hint, and a recent-threads candidate
      list — and does **not** auto-select/forward.
- [ ] A `-32600` returned after a correctly-pinned resume is classified (sandbox-mismatch vs.
      deleted-rollout) into a styled, logged message — no raw JSON-RPC code leaks as the only output.
- [ ] Owner discovery is read-only, bounded, and unit-tested for hit and miss.
- [ ] `docs/upstream-codex.md` F14/F16 (and F15) reflect the new behavior; `Last verified` = 2026-06-02.
- [ ] `prex` SKILL.md pins `--account "$PLAN_ACCOUNT"` in stage 3 and its Resume Fallback table
      distinguishes auto-pin-success / `ResumeOwnerMissing` / sandbox-mismatch / `ResumeBlocked`.
- [ ] `just lint`, `just test-unit`, `just test-integration` pass.
- [ ] This plan's `_QUEUE.yaml` shows round `wrapper-guidance-and-prex-sync` as `done`.
- [ ] The top-level `.plan/_QUEUE.yaml` shows this plan as `done`.

## Next Round

This is the final round.
