# `account quota` — aggregate (5h + weekly) totals panel

> Complexity: S | Rounds: 1 | Generated: 2026-05-28
> Repo: /workspaces/codex-session
> Status: todo

## Problem Statement

`codex-session account quota` renders one chart per account — a 5-hour bar
and a weekly bar with reset countdowns. When operating a multi-account
failover pool, the operator question "how much capacity does my pool have
_in total_ right now?" requires eyeballing the per-account list and
averaging mentally.

This plan adds an aggregate **TOTAL** panel that surfaces pool-wide
remaining capacity at a glance, using the same visual vocabulary as the
per-account charts (20-cell bars, `percent_style()` thresholds). A small
label cleanup lands in the same change: `Five-hour` → `5-hour` everywhere
the label is rendered (per-account and aggregate). The JSON output shape
becomes `{ entries, aggregate }` — a breaking change permitted by the
pre-v1.0 policy in `CLAUDE.md`.

## Strategy

S grade, one round. The work is mechanically cohesive: new view structs +
one pure aggregation helper + extended rendering function + JSON wrapper +
tests + style-guide doc update. All changes live in the `account quota`
slice of the codebase and have a linear dependency order (types → service
helper → command wiring → renderer → tests/docs), so a single `/prex`
session under `prex`'s in-round review-loop is the right shape.

## Execution Order

| Round | File                        | Topic                  | Status | Completed |
| ----- | --------------------------- | ---------------------- | ------ | --------- |
| 01    | `aggregate-totals-panel.md` | aggregate totals panel | todo   | --        |

## Execution Commands

```bash
# Execute the single round:
/prex -ar .plan/account-quota-aggregate-totals/aggregate-totals-panel.md

# Or with full directory context (executor reads _QUEUE.yaml, runs first todo round):
/prex -ar @.plan/account-quota-aggregate-totals/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained
unit of work designed for a single `/prex` session. Even when `/prex` is
invoked against the plan directory or `README.md`, it MUST:

1. Read the **Execution Order** table above.
2. Identify the **first** round whose status is `todo`.
3. Execute **only that single round**, then stop.
4. End the session. A new `/prex` session is required for the next round.

**Why:** Fresh sessions prevent context contamination between rounds, keep
token usage predictable, and let the user review intermediate results
before proceeding. This plan only has one round, but the discipline still
applies — do not chain follow-up work into the same session.

## Decisions & Constraints

- **Executor: `prex` (EF 1.5).** Complexity sized assuming the prex
  in-round review-loop catches mid-round errors. Re-sizing for a different
  executor would re-grade this plan.
- **Aggregation = mean of `percent_left`** across OAuth accounts. Skip
  accounts where `mode != "oauth"` (api-key / error). Per-window means are
  independent: if one account's 5h fetch errored but its weekly succeeded,
  it still counts toward the weekly mean.
- **Show TOTAL only when ≥ 2 OAuth accounts contributed.** A single
  account's "average" is just itself — noise, not signal.
- **Layout = footer summary.** Per-account list first, then a blank line,
  then the TOTAL panel. Header (DIM) reads `TOTAL (avg across N accounts)`.
- **No reset time on aggregate rows.** Resets are inherently per-account;
  averaging them is misleading. Right-side of each TOTAL row shows only the
  percentage (no `left`, no countdown).
- **Label rename: `Five-hour` → `5-hour`** at every text render site
  (per-account `write_quota_entry_text` line ~836 and
  `write_quota_entry_verbose` label key at line ~765, plus the aggregate
  row). JSON kebab field name `five-hour` stays unchanged — only the human
  text label changes.
- **JSON shape change (breaking, pre-v1.0):** today emits a bare array.
  Becomes `{ entries: [...], aggregate: {...} | null }`. Single-account
  invocations also emit the wrapper with `aggregate: null` for shape
  uniformity.
- **`AccountQuotaWindowView` is NOT migrated.** A separate
  `AccountQuotaAggregateWindowView { percent_left: f64 }` (no reset field)
  is introduced for the aggregate row. The existing per-account window
  struct keeps `reset_at_unix: u64` and its current JSON serialization.
- **`docs/design/cli-style-guide.md` §14 must be updated** in this round —
  it is the source of truth for CLI rendering per `CLAUDE.md`.

## Rejected Alternatives

- **Capacity-out-of-N** ("3.6 / 5 accounts of 5h capacity left") —
  rejected; user picked mean-percentage framing as more familiar.
- **Header summary (TOTAL panel at top)** — rejected; user picked footer
  placement so per-account detail leads.
- **Opt-in `--summary` flag** — rejected; the aggregate is unconditional
  whenever ≥ 2 OAuth accounts are listed.
- **Earliest / latest reset on TOTAL row** — rejected; resets are
  per-account and any aggregate framing is misleading.
- **Bare-array JSON + synthetic `__aggregate__` entry** — rejected; ugly
  sentinel and forces every consumer to filter. Object wrapper is cleaner.
- **Migrate `AccountQuotaWindowView.reset_at_unix` to `Option<u64>`** —
  rejected; would breaking-change the per-account JSON shape too. Separate
  aggregate struct is surgical.

## Risks & Edge Cases

- **JSON consumers downstream of `account quota --format json`.** The bare
  array → object wrapper is a breaking change. Permitted by pre-v1.0
  policy (`CLAUDE.md` "Breaking Changes Policy"). No compat shim. Surface
  this in the commit message / changelog when the round merges.
- **Single-account text mode** must not render TOTAL (only one
  contributor). Verified in tests.
- **All-`api-key` / all-error pool** must render `aggregate: null` and no
  TOTAL panel.
- **Pool where one account's 5h errored but weekly succeeded** — per-window
  means use independent contributor counts. Test this explicitly.
- **Color thresholds (`percent_style`) apply uniformly** to TOTAL bars
  too — no special-casing.

## Completion

When the single round is done, set the round `done` in this plan's
`_QUEUE.yaml` and set this plan `done` in the top-level `.plan/_QUEUE.yaml`.
Nothing moves on disk.
