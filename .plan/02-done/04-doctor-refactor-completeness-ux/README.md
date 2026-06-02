# Doctor Refactor: Full Completeness & Beautiful UX

> Complexity: L (override) | Rounds: 2 | Generated: 2026-05-27 | Updated: 2026-06-02 |
> Executor: prex (EF 1.5) | Repo: /workspaces/codex-session | Status: done

## Hard Dependencies — SATISFIED

Both prerequisite plans are **done and merged** (in `.plan/02-done/`), so this plan is unblocked:

1. **CLI Design System & Colorful Output** — `.plan/02-done/01-cli-design-system-colorful-output/`.
   Provides the shared `styles` module, color constants, design spec, and `--format` conventions
   this plan's rendering relies on.

2. **Spinner UX + Parallel Async Operations** — `.plan/02-done/02-spinner-parallel-async-ux/`.
   Provides the tokio async runtime, `indicatif` spinner infrastructure (`src/ui/spinner.rs`), the
   `crate::runtime::block_on` sync→async bridge, and the parallel-fetch patterns this plan reuses.

These dependencies landed differently than originally assumed (2026-05-27). They already touched
`doctor.rs` and `ui/mod.rs` substantially — the spinner is already wired into `doctor::run()`, the
text output is already partly styled, and the child-version check already exists. Each round file's
**Current State** section reflects the actual codebase as of 2026-06-02.

## Problem Statement

The `codex-session doctor` subcommand validates config-recipe manifests, layers, composition,
session roots, XDG paths, child binary, child version, account health, and native auth. It is still
missing several diagnostics and its check list is flat (no grouping), which hurts both text
scannability and JSON consumption.

Genuinely-missing checks this plan adds:

- **`config-recipe.ping-profile`** — whether `[profiles.ping]` exists (required for token probes).
- **`online.token-probe`** — whether the active account's token actually validates against the API.
- **`online.quota-api`** — whether the quota API is reachable for the active account.
- **`session.trust-cache`** — health of the trust-sync cache file `<cache_dir>/configs.toml`.
- **`session.permissions`** — session directory ownership/mode beyond just the root.

> Note: child-version compatibility is **already** covered by the existing `codex.version` check
> (`check_codex_version_minimum`). This plan only _groups_ it — it does not add a duplicate.

This plan refactors doctor into a grouped, comprehensive diagnostic with:

- **Grouped check sections** — Environment, Accounts, Config Recipe, Session, Auth, Online.
- **The five new checks** above.
- **`--online` flag** — local checks run by default (fast, offline). Network checks (token probe +
  quota connectivity) run only under `--online`, and run **concurrently** via `tokio::join!`.
- **Beautiful text rendering** — section headers, ✓/⚠/✗ status symbols, human-readable timestamps
  (the colored summary banner and styled account list already exist).
- **Redesigned JSON schema** — `groups: Vec<CheckGroup>` replaces the flat `checks` array
  (pre-v1.0 breaking change, accepted).

## Strategy

The work splits into two rounds along a data→presentation seam:

1. **Round 01 — Check completeness + report model**: add the `--online` flag, the `CheckGroup`
   model, restructure `DoctorReport` into groups, implement the five new checks (online pair runs
   concurrently), assign every existing check to a group, update JSON serialization, and update
   tests to compile against the new shape. Text rendering stays flat in this round.

2. **Round 02 — Beautiful UX rendering + tests**: rewrite the text branch of `write_doctor` to
   iterate groups with section headers and ✓/⚠/✗ symbols, render human-readable timestamps, set
   online-aware spinner messages, and complete the integration-test updates. Round 02 is smaller
   than originally scoped because the dependency plans already delivered ~70% of the styling.

## Execution Order

| Round | File                              | Topic                             | Status | Completed  |
| ----- | --------------------------------- | --------------------------------- | ------ | ---------- |
| 01    | `01-checks-and-report-model.md`   | New checks + grouped report model | done   | 2026-06-02 |
| 02    | `02-beautiful-rendering-tests.md` | UX rendering + full test suite    | done   | 2026-06-02 |

## Execution Commands

```bash
# Execute a single round:
/prex -ar .plan/01-todo/04-doctor-refactor-completeness-ux/01-checks-and-report-model.md
/prex -ar .plan/01-todo/04-doctor-refactor-completeness-ux/02-beautiful-rendering-tests.md

# Execute with full directory context (still runs ONE round — the first `todo`):
/prex -ar @.plan/01-todo/04-doctor-refactor-completeness-ux/
```

## Execution Discipline

**One round per `/prex` session. This is a hard rule, not a suggestion.**

1. **One round per session.** Each round is a self-contained unit sized for a single `/prex`
   invocation. Never implement multiple rounds in one session.
2. **Directory/README invocation selects ONE round, not all.** When `/prex` receives the directory
   (`@.plan/01-todo/04-doctor-refactor-completeness-ux/`) or this README, it MUST read the Execution
   Order table, find the first round with status `todo`, execute ONLY that round, then stop.
3. **Sequential sessions.** After a round is marked `done`, the session ends. Launch a new `/prex`
   session for the next round.
4. **Why:** fresh sessions prevent context contamination, keep token usage predictable, and let the
   user review intermediate results before proceeding.

## Decisions & Constraints

1. **Executor: prex (EF 1.5).** Rounds are sized assuming the prex pipeline (Codex plan → Claude
   review → Codex implement → Claude review-loop) absorbs in-round risk.

2. **Pre-v1.0 breaking changes are acceptable.** The JSON schema moves from flat `checks` to grouped
   `groups` with no backward-compatibility shim (per `CLAUDE.md` Breaking Changes Policy).

3. **Local checks are the default; `--online` enables network checks.** Doctor must be fast and
   usable offline. Token probe and quota connectivity run only under `--online`.

4. **Online checks run concurrently.** The token probe and quota connectivity check both target the
   active account, so a single `crate::runtime::block_on(tokio::join!(...))` runs them in parallel —
   overlapping the ~15s probe timeout with the ~10s quota timeout instead of summing them. Prior art:
   `src/commands/account/health.rs:181`. Accept the rare seed-write race documented at
   `health.rs:170-178` (doctor is a read-only diagnostic that never persists a rotated token).

5. **Checks are grouped into logical sections.** `DoctorReport` gains `groups: Vec<CheckGroup>`,
   each `CheckGroup` carrying a `name` and its own `Vec<CheckResult>`. Groups: Environment,
   Accounts, Config Recipe, Session, Auth, Online.

6. **The Online group is omitted entirely without `--online`.** No empty group in JSON; no section
   header in text.

7. **Existing check IDs are preserved.** `CheckResult.name` values (e.g. `config-recipe.active`,
   `session.root`, `codex.version`) are unchanged for grep-ability and test stability. New checks
   follow the same dotted convention.

8. **Spinner integration already exists — reuse it.** `doctor::run()` already builds a `SpinnerGroup`
   with a single `"Running checks..."` spinner, and `build_report` updates it via `set_progress`.
   Round 02 only adds online-aware messages (`"Probing token + quota (parallel)…"`); it does NOT
   restructure `run()` or introduce a per-check spinner system.

9. **Styling uses the existing `styles` module.** `src/ui/mod.rs` already has `mod styles` with the
   needed constants and the `style_open`/`style_close`/`styled_text`/`styled_padded` helpers. No
   module rename is required (the `quota_styles` rename already happened upstream).

10. **`run()` stays sync.** Async checks bridge through `crate::runtime::block_on` (`src/runtime.rs`),
    which works under the `#[tokio::main]` multi-thread runtime via `block_in_place`. Do not make
    `run`/`build_report` async.

11. **Use `--format json` (global flag), never `--json`.** Consistent with all other commands.

## Rejected Alternatives

- **All checks run by default (including network):** rejected — a 15-second probe timeout would
  ruin the default UX. Network work is gated behind `--online`.

- **`--quick` / `--full` flag pair instead of `--online`:** rejected — the distinction is network
  vs. no-network, not thoroughness. All local checks always run.

- **Running the two online checks sequentially:** rejected — they target the same account and have
  no ordering dependency, so `tokio::join!` halves wall-clock latency.

- **Keeping the flat check list / flat JSON:** rejected — grouped sections make output scannable and
  the JSON easier to consume. The user explicitly chose the full restructure.

- **Adding a new `check_child_version_compat`:** rejected — `codex.version` already covers it. A
  second check would duplicate logic and break the preserve-IDs rule.

## Risks & Edge Cases

- **Snapshot/assertion churn.** The grouped JSON shape changes `doctor_json_shape` and friends
  (~27 tests across `tests/cmd_doctor.rs` + `tests/account_doctor.rs`). Round 01 keeps text flat to
  limit churn to JSON; Round 02 updates text assertions. Accepted — no external consumers.

- **Concurrent seed-write race.** Running probe + quota refresh concurrently can re-orphan a refresh
  token in the narrow no-group case (`health.rs:170-178`). Accepted for doctor; note it in a code
  comment.

- **Token probe failure modes.** The probe fails for many reasons (no ping profile, timeout, model
  unavailable, rate limited). Each maps to a distinct, helpful check detail. Handled in Round 01.

- **Group coverage.** Every current check ID (including `session.account`, `accounts.registry`,
  `legacy-*`, per-account `cooldown.read`) must land in exactly one group — easy to miss. Round 01's
  acceptance criteria require full coverage.

## Completion

When all rounds are done:

```bash
# Update Status to "done" in this file's header and the Execution Order table,
# fill completion timestamps, then move the directory:
mkdir -p .plan/02-done && mv .plan/01-todo/04-doctor-refactor-completeness-ux .plan/02-done/04-doctor-refactor-completeness-ux
```
