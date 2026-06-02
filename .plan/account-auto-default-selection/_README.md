# Account auto-selection by default, per-invocation pinning

> Complexity: L | Rounds: 3 | Generated: 2026-05-29 | Repo: /workspaces/codex-session | Status:
> done

## Problem Statement

`codex-session` wraps OpenAI's `codex` CLI and multiplexes several accounts, each with its own
`CODEX_HOME`. Today the wrapper only auto-selects an account when the user explicitly passes
`--account auto`; with no flag it falls through a stale chain — `registry.current()` (LRU pointer
written by `account use`) → `config.account.pinned` → a `NoneResolved` error. This is unlike stock
`codex`, where `codex exec` "just works" assuming you are logged in.

We want the wrapper to behave the same way: **running `codex-session exec …` with no account
argument auto-selects an enabled account, transparently rotates across accounts on rate-limit / auth
failures (with clear stderr warnings), and errors explanatorily when nothing is usable.** Explicit
per-invocation pinning stays available via `--account <name>`.

A second, equally important requirement: the quota-aware scoring **selector must fire ONLY on the
positive execution path** (an actual `exec`/`resume`/bare-verb run). Read-only and inspection
commands (`doctor`, `version`, `config status`, `config-recipe compose`, `account list`,
`account current`) must NEVER trigger a pick — today several of them call the resolver and would, in
the new model, fire a quota fetch + write `last-account` just to print status.

This is a pre-v1.0 cleanup: breaking changes are allowed and **no back-compat shims** are added. The
legacy fallbacks are deleted, not layered over.

## Strategy

Three rounds, forward-only dependencies, each leaving the tree compiling with tests green:

- **Round 01 — Core selection engine (atomic hard cut).** The resolver API changes shape (split
  "interpret inputs" from "pick an account"), which ripples through `gate`, `retry`, `pass_through`,
  the error types, and every display caller. These cannot be split without a shim, so they land
  together. This is the round that makes no-arg = auto, adds no-cycling failover-by-default, and
  guarantees auto fires only on the exec path.
- **Round 02 — Cleanup & removals.** Delete the now-orphaned `account use` command and the dead
  `config.account.pinned` / `CODEX_SESSION_ACCOUNT_PINNED` config surface; update `--account` help
  text and the root-help snapshot. Independent of round 01's compile.
- **Round 03 — Docs API sweep.** Update prose and examples across the repo, `~/DocsNNotes`, and
  `~/.dotfiles` to the new API: prioritize the no-arg form, document `auto` as the explicit alias /
  default, and remove "must pass `--account auto`" mandates and `account use` references.

## Execution Order

| Round | File                          | Topic                                  | Status | Completed  |
| ----- | ----------------------------- | -------------------------------------- | ------ | ---------- |
| 01    | `01-core-selection-engine.md` | Resolver split, run_auto, gate, errors | done   | 2026-05-30 |
| 02    | `02-cleanup-removals.md`      | Remove `account use` + config pinned   | done   | 2026-06-01 |
| 03    | `03-docs-api-sweep.md`        | Repo + DocsNNotes + dotfiles doc sweep | done   | 2026-06-01 |

## Execution Commands

```bash
# Execute a single round:
/prex -ar .plan/01-todo/00-account-auto-default-selection/01-core-selection-engine.md

# Execute rounds sequentially (run each after the previous completes):
/prex -ar .plan/01-todo/00-account-auto-default-selection/01-core-selection-engine.md
/prex -ar .plan/01-todo/00-account-auto-default-selection/02-cleanup-removals.md
/prex -ar .plan/01-todo/00-account-auto-default-selection/03-docs-api-sweep.md

# Execute with full directory context (still runs ONE round — the next `todo`):
/prex -ar @.plan/01-todo/00-account-auto-default-selection/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed for
a single `/prex` session. Do not attempt to implement multiple rounds in one session.

When `/prex` is pointed at the directory or this README, it selects **one** round, not all: read the
**Execution Order** table, find the first round with status `todo`, execute ONLY that round, then
stop.

After completing a round:

1. Consult the **Execution Order** table above.
2. Find the next round with status `todo`.
3. Execute it in a **fresh** `/prex` session.
4. Repeat until all rounds show status `done`.

Why: fresh sessions prevent context contamination between rounds, keep token usage predictable, and
let the user review intermediate results before proceeding.

## Decisions & Constraints

- **Executor: prex (EF 1.5).** Rounds are sized for the prex pipeline (Codex plan → Claude review →
  Codex implement → Claude review-loop).
- **No-arg ⇒ auto.** Absence of `--account` / `CODEX_SESSION_ACCOUNT` resolves to the quota-aware
  selector. (Established with the user.)
- **`auto` keyword KEPT** as an explicit alias of the default (`--account auto`,
  `CODEX_SESSION_ACCOUNT=auto`). Existing scripts keep working; docs are reframed to prioritize the
  no-arg form. (Established with the user.)
- **`--account <name>` / `CODEX_SESSION_ACCOUNT=<name>` ⇒ pin** — no rotation; hard error if the
  account is missing or unauthenticated.
- **Failover ON by default for auto**, with **no cycling**: each account is tried at most once per
  invocation; rotation emits a concise stderr warning; exhaustion produces a multi-line
  per-account explanatory error. (Established with the user.)
- **Auto fires only on the positive exec path.** The scoring selector (`selector::pick`, with quota
  fetch + `set_current`) is invoked exclusively from `pass_through::run`. All display/inspection
  commands use a side-effect-free display resolver.
- **No new debug flag.** Concise rotation warnings go to stderr by default (respecting
  `--quiet`/`--silent`); detailed per-attempt diagnostics stay in `tracing`, surfaced via the
  existing `--log-stderr` / `-v`.
- **No back-compat.** Remove the LRU resolution fallback, `config.account.pinned`,
  `CODEX_SESSION_ACCOUNT_PINNED`, the `account use` command, and the dead `NoneResolved` error
  outright.
- **`account add` keeps seeding the first account as current**; the interactive gate prompt remains
  the manual re-selection path now that `account use` is gone.
- **`config-recipe compose` on a never-run auto pool errors** (`NoneSelected`, exit 64) rather than
  triggering a pick — inspection must not fire quota fetches.

## Rejected Alternatives

- **Keep auto opt-in (status quo).** Rejected: the whole point is stock-`codex`-like ergonomics.
- **Remove the `auto` keyword entirely.** Rejected: would break every existing `--account auto`
  invocation (parsed as an account literally named "auto" → NotFound). Keeping it as an alias is
  non-breaking and clearer.
- **Buffer output to enable failover.** Not needed: `run_once(capture=true)` already TEES to live
  stdio while capturing, so failover scanning does not regress streaming.
- **Transient `resolve()` shim to split round 01.** Rejected by the user: contradicts the
  "no leftover" goal; the core change stays atomic.
- **Cross-`CODEX_HOME` / cross-account resume support.** Out of scope and impossible upstream
  (documented in `docs/upstream-codex.md` F14–F16); resume always pins the thread's owning account.

## Risks & Edge Cases

- **Round 01 is large and touches shared infra.** Mitigated by prex's in-round review-loop and the
  precise, file-by-file steps in the round file. Every caller of the removed `resolve()` is
  enumerated so the tree compiles at round end.
- **Display side-effects.** If any display caller is missed, status commands would fire quota
  fetches / write `last-account`. Acceptance criteria require verifying (via `-vv`) that
  `doctor`/`list`/`version`/`config status`/`config-recipe compose` perform no pick.
- **Rotation cycling.** The `tried` set + bound derived from eligible-account count makes A→B→C→A
  structurally impossible; a dedicated test asserts no repeat.
- **Token-refresh loop.** The "refresh same account once" guard is keyed on first-use so a second
  401 cannot re-refresh.
- **Docs drift across three repos.** Round 03 reviews `~/.dotfiles` and `~/DocsNNotes` in full;
  since `auto` stays valid, those changes are non-breaking even if a reference is missed.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mv .plan/01-todo/00-account-auto-default-selection .plan/02-done/00-account-auto-default-selection
```

## Implementation Notes / Divergences (added 2026-06-02)

- `run_auto` behaviors are covered by integration tests (`tests/account_failover_*.rs`), not unit tests inside `retry.rs`.
- The usability helper is named `unusable_reason` (not the plan's `skip_reason`); same responsibility.
- The `NoneSelected` message wording differs slightly from the plan; semantics are equivalent and `account use` is correctly absent.
- The dedicated 3-account no-recycle test the README advertised was missing and is added by plan `done-plans-fidelity-cleanup` round 01.
- A stale `LRU` code comment in `src/ui/mod.rs` was corrected in the same fix plan.
