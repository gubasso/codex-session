# Interactive codex TUI passthrough: inherit stdio

> Complexity: S | Rounds: 1 | Generated: 2026-06-02
> Repo: /workspaces/codex-session Status:
> todo

## Problem Statement

Launching the interactive codex TUI through the wrapper fails:

```text
[codex-session] auto-selection enabled (2 candidate(s)).
Error: stdout is not a terminal
```

`"stdout is not a terminal"` is emitted by **upstream codex**, not the wrapper. The codex
TUI checks `isatty(stdout)` on startup and refuses to run when its stdout is a pipe.

Root cause: with no pinned account and multiple eligible accounts, `gate::ensure` returns
`AutoDeferred`, so `pass_through::run` routes to `retry::run_auto`, which always runs the
child with `capture=true` so it can scan stdout/stderr for `401`/`429` failover patterns.
`capture=true` makes the spawner **pipe** the child's stdout/stderr instead of inheriting
the terminal — breaking the TUI's `isatty` check. The identical bug exists in `run_resume`
for interactive `resume <id>` (it also forces `capture=true`).

Reactive, output-scan failover cannot help an interactive session anyway: once codex owns
the terminal there is no transparent way to rotate accounts and replay the user's session,
and the act of piping to observe output is what breaks the TUI. The correct defense for
interactive launches is the **pre-flight** account selection `resolver::resolve_for_exec`
already performs (skips cooled-down/quota-exhausted accounts, scores by quota). Pinned
accounts already prove the inherited-stdio path works (`Resolved` → `single_attempt` →
`run_once(capture=false)` → `spawn_and_wait`).

## Strategy

Single round. Introduce one shared predicate, `is_interactive_passthrough(argv)`, that
classifies an invocation as an interactive TUI launch attached to a real terminal, and route
those launches through an inherited-stdio path (no capture, no reactive failover). Apply it at
both seams — the `AutoDeferred` arm (`run_auto`) and `run_resume`'s by-id spawn. Everything
else (`exec`, `--json`, piped/redirected stdout) keeps capture + failover unchanged. The change
touches only child **stdio**; argv forwarding, env, and emitted `$CODEX_HOME` config are
untouched, so codex-CLI compatibility is preserved.

## Execution Order

| Round | File                               | Topic                                   | Status | Completed |
| ----- | ---------------------------------- | --------------------------------------- | ------ | --------- |
| 01    | `interactive-passthrough-stdio.md` | predicate + auto/resume inherited stdio | todo   | --        |

## Execution Commands

```bash
# Execute the single round:
/prex -ar .plan/interactive-tui-passthrough-stdio/interactive-passthrough-stdio.md

# Or with full directory context (executor reads _QUEUE.yaml, runs first todo round):
/prex -ar @.plan/interactive-tui-passthrough-stdio/
```

## Execution Discipline

**Rounds must be executed one at a time.** Each round is a self-contained unit of work designed
for a single `/prex` session. Do not implement multiple rounds in one session. (This plan has a
single round; it still executes in its own isolated `/prex` invocation.)

When the executor is handed the directory or this README, it must read the **Execution Order**
table, pick the first round with status `todo`, execute ONLY that round, then stop.

After completing a round:

1. Consult the **Execution Order** table above.
2. Find the next round with status `todo`.
3. Execute it in a **fresh** `/prex` session.
4. Repeat until all rounds show status `done`.

## Decisions & Constraints

- **Executor: prex (EF 1.5).** Sizing assumes the prex four-pass review-loop.
- **Scope: auto + resume (all interactive TUI passthrough).** Established in conversation —
  `run_resume`'s by-id path has the identical bug and is fixed in the same round using the same
  predicate.
- **Predicate definition.** `is_interactive_passthrough(argv)` is true when: not `--json`, AND
  `argv[0] != "exec"`, AND `std::io::stdout().is_terminal()`, AND `std::io::stdin().is_terminal()`.
  Split into a pure shape check (`argv_is_interactive_shape`) for unit testing plus a thin wrapper
  consulting the real terminals.
- **`exec` is non-interactive.** `codex exec` and `codex exec resume <id>` keep capture + failover
  (first arg `exec` → predicate false). Only interactive `resume <id>` / bare `resume` picker flip
  to inherited stdio.
- **Preserve auto bookkeeping.** The new `run_auto_interactive` records `set_current` best-effort,
  mirroring `run_auto`'s success path (selector recency penalty).
- **Keep `resume_preflight_block`.** `run_resume` still refuses cooled-down/exhausted owners before
  launch; only the reactive post-run `resume_blocked_from_live_rate_limit` scan is skipped when
  inheriting stdio.
- **Compatibility.** No change to argv forwarding, env, or emitted config — byte-for-byte
  compatible with upstream codex (per CLAUDE.md composer contract).
- **Docs are source of truth.** `docs/design/cli-style-guide.md` governs stdout/stderr ownership
  and must be updated; `docs/upstream-codex.md` records the verified codex-TUI `isatty` requirement.

## Rejected Alternatives

- **`exec`-replace (execvp) for the TUI.** `spawner.rs` has a dead `exec()` method, but switching
  to it would not compose with the auto rotation loop and is unnecessary — inherited-stdio spawn
  already preserves the terminal. Rejected as out of scope.
- **Keep capturing but detect tty and re-attach a pty.** Far more complex; reactive failover is
  useless for a TUI regardless. Rejected.
- **Gate on stdout-tty alone.** Insufficient to distinguish a TUI from `codex exec` running in a
  terminal; the predicate also excludes `exec`/`--json`. Rejected in favor of the combined check.

## Risks & Edge Cases

- **Predicate misfire on non-interactive paths** would silently disable failover for scripted runs.
  Mitigated: `--json` and `exec` are excluded, and `is_terminal()` is false under any pipe/redirect
  — exactly the scripted scenarios. Existing integration tests run under a non-tty harness, so they
  exercise the unchanged capture path and guard against regression.
- **Loss of mid-session 401/429 rotation for interactive launches** is intentional and was never
  meaningful for a TUI; pre-flight selection is the defense. Accepted.
- **`run_auto_interactive` resolution failure**: `resolve_for_exec` can only fail with NoEligible,
  which `ReadyAuto` already precludes; propagate the error if it occurs. Accepted.

## Completion

When all rounds are done, set the round `done` in this plan's `_QUEUE.yaml` and
set this plan `done` in the top-level `.plan/_QUEUE.yaml`. Nothing moves on disk.
