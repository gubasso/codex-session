# Account quota: show 100% left for barely-used windows

> Complexity: S | Rounds: 1 | Generated: 2026-05-30 Repo: /workspaces/codex-session Status:
> todo

## Problem Statement

`codex-session account quota` renders a window's remaining quota as e.g. `99.0% left` even when the
window has just reset and is effectively unused. Users expect a freshly-reset window to read
`100% left`.

Root cause (confirmed against the live API): the ChatGPT backend endpoint
`GET https://chatgpt.com/backend-api/wham/usage` reports usage as a coarse **integer**
`used_percent` per window, and that is the _only_ usage signal — there are no token counts and no
`percent_left` field. A near-empty window comes back as `used_percent: 1`, which the wrapper
faithfully turns into `percent_left = 99.0` and prints as `99.0%`. The arithmetic is correct; the
problem is purely how that coarse value is presented.

Captured live response shape:

```json
"rate_limit": {
  "allowed": true, "limit_reached": false,
  "primary_window":   { "used_percent": 13, "limit_window_seconds": 18000,  "reset_after_seconds": 17646, "reset_at": 1780122185 },
  "secondary_window": { "used_percent": 35, "limit_window_seconds": 604800, "reset_after_seconds": 75517, "reset_at": 1780180057 }
}
```

## Strategy

One focused round. Keep parsing faithful (`percent_left = 100 - used_percent` stays unchanged) and
fix this strictly at the **display** layer: introduce a tiny pure helper in `src/ui/mod.rs` that
maps the faithful `percent_left` to a whole-number value shown to the user, forcing the near-empty
top bucket to read `100`. Apply that one value to both the printed number and the progress bar so
they always agree.

## Execution Order

| Round | File                        | Topic                             | Status | Completed |
| ----- | --------------------------- | --------------------------------- | ------ | --------- |
| 01    | `display-percent-helper.md` | Display helper + renderer + tests | todo   | --        |

## Execution Commands

```bash
# Execute the single round:
/prex -ar .plan/account-quota-percent-display/display-percent-helper.md

# Or with full directory context (executor reads _QUEUE.yaml, runs first todo round):
/prex -ar @.plan/account-quota-percent-display/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed for
a single `/prex` session. Do not implement multiple rounds in one session.

When `/prex` is pointed at this directory or this README, it MUST:

1. Read the **Execution Order** table above.
2. Find the first round with status `todo`.
3. Execute ONLY that single round, then stop.
4. End the session — a fresh `/prex` session is launched for any subsequent round.

This plan has a single round, so one `/prex` session completes it.

## Decisions & Constraints

- **Executor: prex (EF 1.5).**
- **Mapping policy (literal + top special-case):** keep `percent_left = 100 - used_percent`
  faithful; display **whole numbers**; force only the near-empty top bucket
  (`used_percent <= 1`, i.e. `percent_left >= 99`) to read `100%`. Mid-range values are unchanged.
  Examples: `used_percent 0 -> 100%`, `1 -> 100%`, `13 -> 87%`, `35 -> 65%`, `100 -> 0%`.
  (Chosen by the user over an "optimistic `101 - used`" mapping that would shift every window up
  by one.)
- **Faithful parse stays:** `src/services/account/quota.rs::parse_window` is NOT changed. The cached
  value and JSON output remain the raw `percent_left`. Only the human-facing text view changes.
- **CLI Design System:** `docs/design/cli-style-guide.md` is the source of truth for any
  `Ui::write_*` renderer change (per `CLAUDE.md`). Color semantics and stdout/stderr ownership must
  be preserved — this change only alters the numeric formatting, not colors or layout.
- **Quality gates:** use `just` recipes, not raw cargo (per `CLAUDE.md`): `just test-unit`,
  `just lint`.

## Rejected Alternatives

- **Compute from token counts / a fractional `used_percent`.** Impossible — the API response carries
  no finer-grained data than the integer `used_percent`.
- **Optimistic remaining (`min(100, 101 - used_percent)`).** Would make fresh windows read 100% but
  also shift every mid-range window up by one point (e.g. `13% used -> 88%`). User chose the literal
  mapping instead.
- **Plain whole-number rounding with no special case (`{:.0}%`).** Does not solve the bug:
  `100 - 1 = 99` still rounds to `99%`.

## Risks & Edge Cases

- **Bar vs. number consistency:** `quota_bar` and the printed number must use the SAME displayed
  value. Feed both from the helper. (Note: `quota_bar(99.0, 20)` already fills 20/20, so the visual
  bar does not regress; the fix is about the digits.)
- **Verbose mode (`--verbose`):** `write_quota_entry_verbose` prints `five_hour_pct: {:.1}` of the
  raw `percent_left`. Leave it faithful (it is the diagnostic view) — do NOT apply the display
  helper there.
- **JSON mode (`--format json`):** unaffected; it serializes the raw `percent_left`. Do not route
  JSON through the display helper.
- **Clamping:** the helper must clamp to `0..=100` to stay robust against float drift / out-of-range
  inputs.

## Completion

When the round is done, set the round `done` in this plan's `_QUEUE.yaml` and
set this plan `done` in the top-level `.plan/_QUEUE.yaml`. Nothing moves on disk.
