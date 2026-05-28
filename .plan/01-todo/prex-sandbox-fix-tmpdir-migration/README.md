# Fix Prex Resume Failure + Migrate Skill Run Dirs from /tmp to XDG

> Complexity: L | Rounds: 3 | Generated: 2026-05-27T21:00:00Z
> Repo: /workspaces/codex-session
> Status: todo

## Problem Statement

The prex skill's stage 3 `exec resume` fails with "no rollout found" (JSON-RPC -32600) from the
OpenAI Codex backend because the sandbox mode changes between stage 1 (`--sandbox read-only`) and
stage 3 (`--full-auto`). The backend rejects resume requests when sandbox parameters differ from the
original session.

Investigation confirmed: the thread IS indexed correctly, the local rollout file EXISTS, and
codex-session routes the resume to the correct account. The error is purely server-side parameter
validation. Additionally, `--full-auto` is deprecated since Codex v0.128.0.

A secondary issue: when resume fails and the orchestrator falls back to fresh exec, the fallback
prompt references `/tmp` paths that Codex can't read under `workspace-write` sandbox (which excludes
`/tmp` by default via `sandbox_workspace_write.exclude_slash_tmp`).

A broader cleanup: all 9 skill files use hardcoded `/tmp` for run directories, which is
non-XDG-compliant, leaks across container boundaries (host-shared bind mount), and breaks when
sandbox modes restrict `/tmp` access.

## Strategy

Three rounds, ordered by dependency:

1. **Documentation & conventions** — Document the sandbox mismatch finding in upstream-codex.md and
    update codex-conventions.md with new patterns (unified sandbox approach, strengthened orientation
    blocks, run-dir base path). This establishes the source of truth before skills reference it.

2. **Prex core fix** — Apply the sandbox fix and run-dir migration to prex/SKILL.md and
    prex-resume/SKILL.md (the two skills affected by the resume bug). Add resume fallback procedure.

3. **Remaining skills + dctl** — Migrate the 7 other skills from `/tmp` to the new XDG base dir,
    update cross-skill scanning in merge-queue and spec-impl, remove the `/tmp` bind mount from dctl
    devcontainer config.

## Execution Order

| Round | File | Topic | Status | Completed |
| ----- | ---- | ----- | ------ | --------- |
| 01 | `01-docs-and-conventions.md` | Documentation & conventions | done | 2026-05-27 |
| 02 | `02-prex-sandbox-fix.md` | Prex sandbox fix + run-dir migration | done | 2026-05-28 |
| 03 | `03-remaining-skills-dctl.md` | Remaining skills + dctl config | todo | -- |

## Execution Commands

```bash
# Execute a single round:
/prex -ar .plan/01-todo/prex-sandbox-fix-tmpdir-migration/01-docs-and-conventions.md

# Execute rounds sequentially (run each after the previous completes):
/prex -ar .plan/01-todo/prex-sandbox-fix-tmpdir-migration/01-docs-and-conventions.md
/prex -ar .plan/01-todo/prex-sandbox-fix-tmpdir-migration/02-prex-sandbox-fix.md
/prex -ar .plan/01-todo/prex-sandbox-fix-tmpdir-migration/03-remaining-skills-dctl.md

# Execute with full directory context:
/prex -ar @.plan/01-todo/prex-sandbox-fix-tmpdir-migration/
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

1. **Unified sandbox approach (Option A):** Use `--dangerously-bypass-approvals-and-sandbox` for ALL
    Codex calls (both read-only planning and write implementation). This eliminates sandbox mode
    mismatch on resume. Behavioral enforcement moves to strengthened prompt injection. Acceptable
    because this always runs in a container environment. (Established in conversation — user chose
    Option A explicitly.)

2. **Run-dir base path:** `${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs/`.
    Nested under the existing `~/.local/state/claude-session/` mount. No new devcontainer mount
    needed. (User chose this over the independent `claude-skill-runs/` option.)

3. **Remove /tmp mount entirely:** The dctl devcontainer base layer's `/tmp` bind mount is removed.
    Container gets its own ephemeral tmpfs. Forces all persistent state through XDG paths. (User
    chose full removal over read-only or keep-as-is.)

4. **Cross-skill scanning:** Skills that scan for sibling run directories (merge-queue scans for
    `prex-*`, spec-impl scans for `prex-*`, prex scans for `review-loop-*`) must update their
    scan paths from `/tmp` to the new base dir.

5. **Lock files:** Migrate from `${XDG_RUNTIME_DIR:-/tmp}` to
    `${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs}`.

## Rejected Alternatives

1. **Option B (skip resume entirely):** Always use fresh exec with inlined context. Eliminates the
    resume failure class but loses stage 1 conversation context and increases token cost. Rejected as
    primary approach but adopted as the **fallback** when resume fails.

2. **Option C (workspace-write for both stages):** Keeps sandbox but makes it uniform. Rejected
    because it still uses bubblewrap which doesn't work in containers (the primary environment).

3. **Independent `~/.local/state/claude-skill-runs/` dir:** Would need a new devcontainer mount.
    Rejected in favor of nesting under the existing `claude-session` mount.

4. **Keep /tmp mount as read-only:** Safety net for remaining readers. Rejected — clean break
    forces proper migration.

## Risks & Edge Cases

1. **Other tools using host /tmp:** After removing the `/tmp` bind mount, any tool that writes to
    `/tmp` expecting host persistence will break. Mitigated: only Claude/Codex skills used it for
    run dirs; other `/tmp` usage (keepassxc, nextcloud locks) is host-side only.

2. **Lock file location change:** Existing lock files in `/tmp` won't be found after migration.
    Acceptable: locks are ephemeral and cleared on session boundaries.

3. **Prompt injection as sole enforcement:** Without sandbox, Codex could theoretically write files
    during a "read-only" stage. Mitigated: container isolation provides external boundary;
    strengthened prompt injection is defense-in-depth.

## Completion

When all rounds are done:

```bash
# Update status in this file to "done"
# Fill in completion timestamps in the execution order table
mv .plan/01-todo/prex-sandbox-fix-tmpdir-migration .plan/02-done/prex-sandbox-fix-tmpdir-migration
```
