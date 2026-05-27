# Thread Resume Support for codex-session

> Complexity: L | Rounds: 4 | Generated: 2026-05-26T00:00:00Z
> Repo: /workspaces/codex-session | Status: todo

## Problem Statement

codex-session wraps OpenAI's Codex CLI with multi-account support by setting
`CODEX_HOME` to a per-account session directory. Codex stores all state
(auth, config, sessions, threads) under `$CODEX_HOME`, so each account gets
fully isolated session storage. However, codex-session currently has no
support for Codex's thread resume feature (`codex exec resume <ID>`,
`codex exec resume --last`).

When a caller (e.g., the `prex` skill) tries to resume a thread and
`--account auto` selects a different account than the one that created the
thread, the resume fails with "thread not found" because the new account's
`CODEX_HOME` has no record of the original thread. The caller sees:

```text
Thread resume failed (thread not found across accounts).
Switching to fresh exec with --sandbox workspace-write.
```

To fix this, codex-session must:
1. Track which account created each thread (cross-account thread index).
2. Capture thread IDs from Codex's JSONL output after each `exec --json` run.
3. Intercept `exec resume` commands and route them to the correct account.
4. Maintain Codex-native CLI API compatibility — callers that know Codex's
    resume interface should work identically with codex-session.

## Strategy

The work is split into 4 rounds following a bottom-up dependency order:

1. **Thread index data layer** — new module for the JSONL-based cross-account
    thread index (read/write/query primitives).
2. **Thread capture wiring** — detect `--json` in child argv, tee stdout when
    present, parse JSONL for thread events, write entries to the index.
3. **Resume routing** — intercept `exec resume` in the pass-through path, look
    up thread→account mapping, route to the correct account's `CODEX_HOME`.
4. **Documentation & dotfiles** — update `docs/upstream-codex.md`,
    `docs/multi-account-architecture.md`, project README, and sync dotfiles.

Each round produces a compilable, testable increment.

## Execution Order

| Round | File                      | Topic             | Status | Completed |
| ----- | ------------------------- | ----------------- | ------ | --------- |
| 01    | `01-thread-index.md`      | Thread index      | todo   | --        |
| 02    | `02-capture-wiring.md`    | Capture wiring    | todo   | --        |
| 03    | `03-resume-routing.md`    | Resume routing    | todo   | --        |
| 04    | `04-docs-dotfiles.md`     | Docs & dotfiles   | todo   | --        |

## Execution Commands

```bash
# Execute a single round:
/prex .plan/01-todo/thread-resume-support/01-thread-index.md

# Execute rounds sequentially (run each after the previous completes):
/prex .plan/01-todo/thread-resume-support/01-thread-index.md
/prex .plan/01-todo/thread-resume-support/02-capture-wiring.md
/prex .plan/01-todo/thread-resume-support/03-resume-routing.md
/prex .plan/01-todo/thread-resume-support/04-docs-dotfiles.md

# Execute all rounds sequentially (auto-advance):
/prex -ar @.plan/01-todo/thread-resume-support/
```

## Decisions & Constraints

1. **Smart routing over pure passthrough.** codex-session intercepts
    `exec resume <ID>` and `exec resume --last`, looks up the thread→account
    mapping in the cross-account index, and forwards to Codex with the correct
    account's `CODEX_HOME`. Falls back to fresh exec on miss.

2. **Always capture thread IDs (when `--json` in argv).** After every
    `codex exec --json` run, parse JSONL output for thread start events and
    persist to the cross-account index. Non-JSON runs (TUI/interactive) produce
    no parseable thread events and are not tee'd (no overhead).

3. **JSONL append log for the index.** One JSON line per thread event appended
    to `<state_dir>/thread-index.jsonl`. Crash-safe (append-only), matches
    Codex's own rollout format, easy to scan for `--last`.

4. **Group-scoped `--last`.** `resume --last` defaults to the current group's
    most recent thread. `--all-groups` flag for global scope, matching Codex's
    `resume --all` pattern.

5. **Native Codex API compatibility.** `exec resume <SESSION_ID>` and
    `exec resume --last` use the same flag syntax as upstream Codex. Callers
    that know Codex's resume API work identically with codex-session.

6. **`CODEX_HOME` fully scopes sessions.** Confirmed: Codex stores sessions,
    `state_5.sqlite`, and `session_index.jsonl` all under `$CODEX_HOME/`.
    Per-account `CODEX_HOME` isolation (already implemented) means each
    account's threads are physically separate.

## Rejected Alternatives

- **Pure passthrough without thread index:** Would let Codex fail on wrong
  account and fall back to fresh exec. Loses thread continuity entirely —
  the core problem the user reported.

- **Per-account `resume-state.json` instead of global index:** Requires
  scanning all account directories for `--last`. Slower, no cross-account
  correlation, harder to query.

- **Opt-in capture via `--track-thread` flag:** Requires callers to opt in.
  `resume --last` wouldn't work unless callers remembered to pass the flag.

- **Always tee stdout (even non-JSON runs):** Adds pipe overhead to
  interactive/TUI sessions that produce no parseable thread events.

- **SQLite index:** Overkill for a CLI wrapper managing 3-5 accounts. JSONL is
  simpler, crash-safe, and sufficient.

## Risks & Edge Cases

- **Thread ID format change:** If upstream Codex changes from UUID v7 to
  another format, the index parser must adapt. Mitigated by storing thread IDs
  as opaque strings.

- **Index growth:** JSONL file grows without bound. Should consider a rotation
  or truncation strategy in a future follow-up (not in scope for this plan).

- **Race on concurrent exec:** Two concurrent `codex-session exec --json` runs
  appending to the same JSONL file could interleave lines. Mitigated by
  appending complete lines atomically (single `write()` call).

- **Stale index entries:** Thread's rollout file may be deleted by Codex or
  user. Resume will fail at the Codex level — codex-session falls back to
  fresh exec.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mv .plan/01-todo/thread-resume-support .plan/02-done/thread-resume-support
```
