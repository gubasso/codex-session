# Quota Soft-Gate + Out-of-Quota / Resume Messaging

> Complexity: M | Rounds: 1 | Generated: 2026-06-01 | Repo: /workspaces/codex-session | Status: done

## Problem Statement

A `prex`/`review-loop` run repeatedly failed with exit 75 (`AutoExhausted`).
Investigation proved it was **not a bug**: every `~/.dotfiles` skill and
`~/DocsNNotes` reference invokes `codex-session` with auto account selection
correctly (no pinned account, no `CODEX_SESSION_ACCOUNT`), and the API flags are
right (`--json` for `exec` passthrough vs `--format json` for wrapper commands).

The real cause is a design choice in auto-selection: a **hard eligibility gate**
requires `five_hour_left > 50% AND weekly_left > 10%`; an account failing either
is _dropped before ranking_. At the time of the failure all three accounts were
at 46% / 41% / 0% five-hour-left, so all were dropped → no candidate → exit 75.
"Being #1 in the ranking" never mattered: the gate is applied before scoring.

The owner of the project decided the gate is wrong. This plan makes three
coordinated changes:

1. **Remove the hard gate.** Below-threshold accounts must never be _blocked_ —
   only **penalized** in scoring, so they're strongly deprioritized but still
   selectable. The pool is used until quota actually hits 0%; real depletion is
   driven by the API returning 429 → cooldown → rotate, not by a preemptive floor.
2. **Friendly "out of quota" report.** When every account genuinely cannot run,
   print a human- _and_ AI-readable message naming each account, what's blocking
   it, and when it comes back (reset ETA + earliest-available summary).
3. **Resume-aware quota message.** A `resume <thread-id>` is account-bound (the
   rollout only exists in the owning account's `CODEX_HOME`). When that owner is
   quota-limited, print a clear message: when the owner returns, which other
   accounts have quota (with the caveat they can't continue this thread), or — if
   none are ready — every account's reset time. Mirror the guidance into the
   `~/.dotfiles` prex/review-loop skills and `~/DocsNNotes` codex-conventions.

## Strategy

Single round. Complexity scored M (axes files:3 cross-cut:3 deps:2 novelty:2
risk:3 → raw 13 ÷ EF 1.5 (prex) → 8.67 → M). Although the work touches core
account-selection infrastructure, the three parts share helpers (quota/cooldown
inspection, `human_duration_until`, the enriched outcome struct) and reviewing
them as one cohesive change avoids splitting atomic edits (the `eligible` field's
meaning, the report struct, and its consumers all move together). prex's in-round
review-loop de-risks the size; the fewer-larger-rounds preference applies.

## Execution Order

| Round | File                                  | Topic                                                       | Status | Completed  |
| ----- | ------------------------------------- | ----------------------------------------------------------- | ------ | ---------- |
| 01    | `01-soft-gate-and-quota-messaging.md` | Soft-penalty selection + exhaustion/resume messaging + docs | done   | 2026-06-02 |

## Execution Commands

```bash
# Execute the single round:
/prex -ar .plan/01-todo/03-quota-soft-gate-and-messaging/01-soft-gate-and-quota-messaging.md

# Or with full directory context:
/prex -ar @.plan/01-todo/03-quota-soft-gate-and-messaging/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of
work designed for a single `/prex` session. Do not implement multiple rounds in
one session. (This plan has one round, but the rule still governs how `/prex`
consumes the directory.)

When `/prex` receives the plan **directory** or this **README**, it MUST:

1. Read the **Execution Order** table above.
2. Find the first round with status `todo`.
3. Execute **only that single round**, then stop — it does NOT proceed to a next
   round in the same session.
4. Mark the round `done` in the table, then end the session. A new `/prex`
   session is launched for any subsequent round.

**Why:** fresh sessions prevent context contamination between rounds, keep token
usage predictable, and let the user review intermediate results before continuing.

## Decisions & Constraints

- **Executor: prex (EF 1.5)** — round sizing assumes the four-pass prex pipeline
  (Codex plan → Claude review → Codex implement → Claude review-loop).
- **Soft penalty, not a block.** Remove the only hard-gate point
  (`score_candidate` returning `None` on `!eligible`) and add a dominant constant
  `BELOW_KNEE_PENALTY = 1000.0` so any above-knee account outranks any below-knee
  account, while below-knee accounts still order among themselves by remaining
  quota. Rationale: 1000 exceeds the entire reachable score spread (~200), giving
  a strict two-tier order without a hard floor.
- **Keep config key names.** `five_hour_threshold` / `weekly_floor` keep their
  names but shift meaning from hard-floor → penalty-knee. Docs updated; no rename
  (avoids touching the config struct, env keys, and every doc reference).
- **`eligible` repurposed.** The `ScoreBreakdown.eligible` bool stops gating
  selection; it now means "above penalty knee" (informational, surfaced in
  `account quota`/`health`).
- **`NoEligible` / `AutoExhausted` semantics tighten.** `NoEligible` now means no
  _usable_ account (no auth / all cooldown / token expired). `AutoExhausted`
  fires only after accounts are tried and fail (401/429) — true exhaustion.
- **Reports are human + AI readable.** Enriched stderr block with stable,
  parseable per-account lines (`• <id>  <state> … back in <dur> (<time>)`) PLUS
  structured `tracing` fields (`earliest_available_at_unix`, per-account state)
  on the existing machine channel.
- **CLI style guide is authoritative.** All user-facing wording/color/stderr
  changes consult `docs/design/cli-style-guide.md` before editing.
- **External docs are cross-repo.** The `~/.dotfiles` and `~/DocsNNotes` edits are
  a step in this round but are NOT covered by this repo's tests or review-loop;
  the round flags them explicitly.

## Rejected Alternatives

- **Lower the threshold via config (keep the hard gate).** Rejected: still a hard
  floor, just at a different number; an account at "floor − ε" is still blocked.
  The owner wants _no_ block.
- **Graduated/per-knee penalty stacking** (below both knees → 2× penalty).
  Rejected for now: the continuous `avail_score` already separates below-both from
  below-one within the penalized tier; a single dominant constant is simpler and
  deterministic.
- **Rename config keys to `*_penalty_knee`.** Rejected: wider blast radius (config
  struct + env parsing + all docs) for a cosmetic gain; the existing names remain
  adequate.
- **Rotate resume to another account when the owner is quota-limited.** Impossible:
  the thread's rollout exists only in the owner's `CODEX_HOME`; B/C cannot
  continue it. Hence the resume message guides wait-or-fresh-exec instead.

## Risks & Edge Cases

- **Core-infra blast radius.** Account selection drives every `exec`. Mitigation:
  the change is a single scoring tweak plus removing one `return None`; the
  existing `selector.rs` unit tests + integration tests (`account_auto_selector.rs`)
  are updated to lock the new contract, including the tiering invariant.
- **Quota readings can be stale** (30s TTL). An account read as ">0%" may 429 on
  use — already handled by the 429 → cooldown → rotate path; the resume pre-flight
  also attempts (with `capture=true`) rather than relying solely on the cached read.
- **Reset-time formatting** reuses `human_duration_until` (`ui/mod.rs`), promoted
  to `pub(crate)`; ensure no duplicate/divergent formatter is introduced.
- **Cause-aware hints.** Suggesting `cooldown clear --all` for a quota-window block
  is misleading (it won't help). The hint must branch on cooldown vs window.

## Completion

When the round is done:

```bash
# Update status in this file to "done"; fill the completion timestamp.
mkdir -p .plan/02-done && mv .plan/01-todo/03-quota-soft-gate-and-messaging .plan/02-done/03-quota-soft-gate-and-messaging
```
