# Doctor Refactor: Full Completeness & Beautiful UX

> Complexity: L (override from M-9; size of doctor.rs rewrite + rendering replacement warrants 2
> rounds) | Rounds: 2 | Generated: 2026-05-27 | Repo: /workspaces/codex-session | Status: todo

## Hard Dependencies

This plan **must not execute** until both of these plans are fully completed (`Status: done`):

1. **CLI Design System & Colorful Output**
   (`.plan/01-todo/cli-design-system-colorful-output/`) — provides the shared styles module,
   color constants, design spec, and `--format` conventions that this plan's rendering relies on.

2. **Spinner UX + Parallel Async Operations**
   (`.plan/01-todo/spinner-parallel-async-ux/`) — provides the tokio async runtime, `indicatif`
   spinner infrastructure, and the `src/ui/spinner.rs` module that this plan uses for online
   check progress feedback.

Before starting Round 01, verify both plan directories have been moved to `.plan/02-done/`.

## Problem Statement

The `codex-session doctor` subcommand is functionally incomplete and visually plain. It validates
config-recipe manifests, layers, composition, session roots, XDG paths, child binary, account
health, and native auth — but it misses several important checks: whether a `[profiles.ping]`
configuration exists (required for token validation probes), whether tokens are actually valid
against the API, cache settings health, session directory permissions beyond the root, and child
binary version compatibility.

The text output is an unstructured dump of key=value pairs followed by a flat check table with no
colors, no grouping, no visual hierarchy, and no status symbols. The JSON output works but its
schema should be redesigned to match a new grouped structure while the project is still pre-v1.0.

This plan refactors doctor into a comprehensive diagnostic tool with:

- **Grouped check sections** — Environment, Accounts, Config Recipe, Session, Auth, Online
  (network) — each with headers and visual separation.
- **All identified checks** — 6+ new checks covering ping config, token probe, quota connectivity,
  trust-sync cache, session permissions, child version compatibility.
- **`--online` flag** — Non-network checks run by default (fast, offline). Network checks
  (token probe, quota connectivity) are behind `--online` for explicit opt-in.
- **Beautiful text rendering** — Design system colors, status symbols (✓/⚠/✗), aligned tables
  within sections, colored summary banner, human-readable timestamps.
- **Redesigned JSON schema** — Grouped checks, new fields, pre-v1.0 breaking change accepted.

## Strategy

The work splits into two rounds following a layer boundary (data before presentation):

1. **Round 01 — Check completeness + report model**: Add all new check functions, the `--online`
   CLI flag, restructure `DoctorReport` into grouped sections with a new `CheckGroup` model, and
   update the JSON serialization. Existing tests updated minimally to compile. New checks get their
   own tests.

2. **Round 02 — Beautiful UX rendering + test completeness**: Rewrite `write_doctor` in
   `src/ui/mod.rs` to use the design system styles, grouped section headers, status symbols,
   colored summary banner, styled account table, and spinner integration for online checks. Full
   test suite updates for new output format.

## Execution Order

| Round | File                              | Topic                             | Status | Completed |
| ----- | --------------------------------- | --------------------------------- | ------ | --------- |
| 01    | `01-checks-and-report-model.md`   | New checks + grouped report model | todo   | --        |
| 02    | `02-beautiful-rendering-tests.md` | UX rendering + full test suite    | todo   | --        |

## Execution Commands

```bash
# Execute a single round:
/prex -ar .plan/01-todo/doctor-refactor-completeness-ux/01-checks-and-report-model.md
/prex -ar .plan/01-todo/doctor-refactor-completeness-ux/02-beautiful-rendering-tests.md

# Execute with full directory context:
/prex -ar @.plan/01-todo/doctor-refactor-completeness-ux/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed for
a single `/prex` session. Do not attempt to implement multiple rounds in one session.

After completing a round:

1. Consult the **Execution Order** table above.
2. Find the next round with status `todo`.
3. Execute it in a **fresh** `/prex` session.
4. Repeat until all rounds show status `done`.

## Decisions & Constraints

1. **Pre-v1.0 breaking changes are acceptable.** The JSON schema is redesigned freely. No
   backward-compatibility shims.

2. **Non-network checks are the default; `--online` enables network checks.** Doctor must be fast
   and usable offline. Token probe and quota connectivity checks only run when `--online` is
   passed. This keeps the default experience snappy (~50ms) while providing deep diagnostics on
   demand.

3. **Checks are grouped into logical sections.** The `DoctorReport` struct gains a
   `Vec<CheckGroup>` where each `CheckGroup` has a name, description, and its own
   `Vec<CheckResult>`. Groups: Environment, Accounts, Config Recipe, Session, Auth, Online.

4. **The Online group is omitted entirely when `--online` is not passed.** The JSON output does
   not include an empty Online group — it is simply absent. Text output does not show the section
   header.

5. **Existing check IDs are preserved.** The `CheckResult.name` field values (e.g.,
   `config-recipe.active`, `session.root`) remain the same for grep-ability and test stability.
   New checks follow the same dotted naming convention.

6. **Spinner integration for online checks.** When `--online` is passed in a TTY with text format,
   the token probe and quota connectivity checks display spinners (via the spinner module from the
   spinner plan). In JSON mode or piped output, spinners are suppressed.

7. **Design system styling is applied uniformly.** All text rendering uses the shared styles module
   (renamed from `quota_styles` by the design system plan), `should_color()`, and
   `style_open()`/`style_close()`.

8. **Use `--format json` (global flag), never `--json`.** Consistent with all other commands.

## Rejected Alternatives

- **Making all checks run by default (including network):** Rejected because doctor should be fast
  and offline-friendly. A 15-second token probe timeout would ruin the default UX.

- **Adding a `--quick` / `--full` flag pair instead of `--online`:** Rejected because the
  distinction is specifically about network vs. no-network, not about thoroughness. All local
  checks always run; the flag only controls whether to make API calls.

- **Keeping the flat check list:** Rejected because grouped sections make the output scannable and
  allow users to quickly find the category they care about.

- **Merging this work into the CLI Design System or Spinner plans:** Rejected because those plans
  already have well-scoped rounds, and adding doctor completeness work would cause scope creep and
  oversized rounds.

## Risks & Edge Cases

- **Snapshot test churn.** The rendering rewrite changes all text output. Tests need full updates
  in Round 02. Accepted — the old output format has no external consumers.

- **Spinner dependency.** If the spinner plan changes its API before this plan executes, the online
  check rendering in Round 02 needs to adapt. Mitigated by the hard dependency requirement.

- **Token probe failure modes.** The probe can fail for many reasons (no ping profile, timeout,
  model unavailable, rate limited). Each failure mode needs a distinct, helpful check detail
  message. Handled in Round 01.

- **Async runtime requirement for online checks.** The spinner plan migrates to tokio. Online
  checks (probe_token, quota fetch) use async. If for some reason the async migration is incomplete,
  online checks fall back to blocking calls.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mkdir -p .plan/02-done && mv .plan/01-todo/doctor-refactor-completeness-ux .plan/02-done/doctor-refactor-completeness-ux
```
