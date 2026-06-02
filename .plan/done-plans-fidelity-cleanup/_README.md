# Done-Plan Fidelity Cleanup

> Complexity: S (overridden to 2 rounds) | Rounds: 2 | Generated: 2026-06-02
> Repo: /workspaces/codex-session Status: todo

## Problem Statement

A full adversarial review of every completed (`status: done`) plan in `.plan/`
(7 plan areas, 18 rounds) found that the **substance** of every plan is
implemented and correct, but uncovered a set of genuine gaps and drifts between
what the plans claimed and what the code actually does. None are functional
regressions; several "divergences" are cases where the implementation
deliberately improved on the plan. This cleanup closes the real gaps, reconciles
live documentation, durably records the intentional divergences, and verifies
the cross-repo work that cannot be confirmed from this repository.

Concretely, the review found:

- **Genuine code/test gaps:** a missing dedicated "no A→B→C→A cycling" failover
  test (the Plan 00 README advertised one, but only a 2-account rotation test
  exists); an unclamped `percent_left` parse path that could theoretically break
  the soft-knee scoring dominance guarantee; a missing `DoctorReport::all_checks()`
  helper (Plan 04 named it, but the code inlines `flat_map` twice); and a stale
  `LRU` code comment referencing a resolution source that was removed.
- **A latent footgun:** the `runtime::block_on` sync→async bridge only works
  under the multi-thread runtime; a future current-thread `#[tokio::test]` on the
  sync exec path would panic. Nothing currently guards or tests this contract.
- **Live-doc drift:** the CLI style guide §9b still lists `doctor`,
  `account refresh`, and `account add` spinners under "Future scope" though they
  shipped in Plan 02 round 03.
- **Undocumented intentional divergences:** the historical done-plan files
  still carry acceptance criteria and line numbers that the (better)
  implementation diverged from — e.g. `quota::get` stayed synchronous behind the
  new `runtime::block_on` bridge rather than becoming `async` as Plan 02 stated.
- **Cross-repo unknowns:** Plan 05 round 05 (dotfiles) and Plan 06 rounds 02–03
  (prex skills, dctl, conventions docs) land in `~/.claude/skills`,
  `~/.dotfiles`, and `~/DocsNNotes` — their "done" status cannot be verified from
  this repository.

## Strategy

Two rounds, each a self-contained `/prex` unit. **Round 01** makes all in-repo
fixes — code, tests, the live style-guide reconcile, the runtime-bridge guard,
and a short "Implementation Notes / Divergences" addendum appended to each
affected done-plan `_README.md`. **Round 02** performs a read-only audit of
the three external repos that hold the cross-repo work and records a findings
report. The rounds are independent (no ordering dependency) but are split because
auditing other repos is a distinct, non-code work unit that does not cohere with
in-repo edits (round-splitting rule 4: cohesion over file count).

## Execution Order

| Round | File                         | Topic                                    | Status | Completed |
| ----- | ---------------------------- | ---------------------------------------- | ------ | --------- |
| 01    | `in-repo-fidelity-fixes.md`  | In-repo code/test/doc fixes + addenda    | todo   | --        |
| 02    | `cross-repo-verification.md` | Read-only cross-repo verification report | todo   | --        |

## Execution Commands

```bash
# Execute a single round:
/prex -ar .plan/done-plans-fidelity-cleanup/in-repo-fidelity-fixes.md

# Execute rounds sequentially (run each after the previous completes):
/prex -ar .plan/done-plans-fidelity-cleanup/in-repo-fidelity-fixes.md
/prex -ar .plan/done-plans-fidelity-cleanup/cross-repo-verification.md

# Execute with full directory context (executor reads _QUEUE.yaml, runs first todo round):
/prex -ar @.plan/done-plans-fidelity-cleanup/
```

## Execution Discipline

**Rounds must be executed one at a time. This is a hard rule, not a suggestion.**

1. **One round per `/prex` session.** Each round executes in its own isolated
   `/prex` invocation. Never execute multiple rounds in a single session.
2. **Directory or README invocation selects ONE round, not all.** When `/prex`
   receives the plan directory (`/prex -ar @.plan/done-plans-fidelity-cleanup/`)
   or this README, it MUST read this plan's `_QUEUE.yaml`, find the first round
   with status `todo`, execute ONLY that round, then stop. It does NOT proceed to
   the next round in the same session.
3. **Sequential sessions.** After completing a round (marking it `done` in this
   plan's `_QUEUE.yaml`), the session ends. The user launches a new `/prex`
   session for the next round.
4. **Why:** Fresh sessions prevent context contamination between rounds, keep
   token usage predictable, and let the user review intermediate results before
   proceeding.

## Decisions & Constraints

- **Executor: prex (EF 1.5).** Rounds are sized for the prex pipeline
  (Codex plan → Claude review → Codex implement → Claude review-loop).
- **Keep the better implementation — never regress an improvement.** Where the
  code intentionally diverged from a plan for the better (notably the synchronous
  `quota::get` + `runtime::block_on` bridge that keeps the exec hot path sync),
  the fix is to **document** the divergence, not to force the code back to the
  plan's literal text.
- **Scope = real gaps + live-doc reconciliation.** Additive plan-parity work that
  was explicitly judged out of scope is NOT included: the decorative
  `── Title ──` doctor group header, the "no resolved account to probe" online
  warn branch, and splitting the three merged doctor tests into separately-named
  tests. The current behavior for those is accepted as-is.
- **Historical done-plan files get a divergence addendum, not a rewrite.**
  Each affected plan README gets a short, clearly-labeled
  "Implementation Notes / Divergences (added 2026-06-02)" section appended. The
  original acceptance criteria and round text are left intact as a historical
  record.
- **Cross-repo work is verified read-only, not modified.** Round 02 cannot edit
  `~/.claude/skills`, `~/.dotfiles`, or `~/DocsNNotes` from this repo; it records
  findings and lists any follow-up needed in those repos.
- **Quality gates per CLAUDE.md.** Use `just` recipes, never raw cargo. Round 01
  ends with `just lint` and `just test`; the final round ends with `just check`.

## Rejected Alternatives

- **Force code↔plan parity (regress improvements).** Rejected: reverting the
  `runtime::block_on` bridge to make `quota::get` async, or removing the
  online-group-omit behavior, would regress deliberate, better engineering to
  satisfy stale plan prose.
- **Rewrite the done-plan acceptance criteria to match reality.** Rejected: it
  erases the historical record of what was planned. A dated addendum preserves
  both the original intent and the as-built truth.
- **Maximal plan-parity (implement decorative header, online else-branch, split
  named tests).** Rejected as out of scope: these are additive cosmetics, not
  gaps; the design-guide SoT was already updated to match the shipped layout.
- **Exclude cross-repo verification entirely.** Rejected: the user wants the
  cross-repo "done" claims confirmed; a read-only audit round provides that
  without overreaching into other repos.

## Risks & Edge Cases

- **`percent_left` clamp could mask a real upstream parse bug.** Clamping to
  `[0, 100]` is defensive, but a value far outside the range signals a malformed
  API response. Accepted: clamp silently (the value is still bounded and
  selection stays correct); a future round may add a debug log if out-of-range
  values are observed.
- **`build_report` reorder for `all_checks()`.** Routing `summarize` and
  `populate_next_steps` through a `DoctorReport::all_checks()` method requires
  computing them after the report struct exists. The round specifies a safe
  reorder (construct report with default summary/next_steps, then fill). Risk:
  low; covered by existing doctor snapshot/JSON tests.
- **Runtime guard test must use `flavor = "multi_thread"`.** A current-thread
  `#[tokio::test]` exercising the bridge would itself panic — the guard test must
  be multi-thread, matching the production `#[tokio::main]` default.
- **Cross-repo paths may not exist in the executor's environment.** Round 02 must
  degrade gracefully (report "not present / not accessible") rather than fail if
  `~/.claude/skills`, `~/.dotfiles`, or `~/DocsNNotes` are absent.

## Completion

When all rounds are done, set each round `done` in this plan's `_QUEUE.yaml` and
set this plan `done` in the top-level `.plan/_QUEUE.yaml`. Nothing moves on disk.
