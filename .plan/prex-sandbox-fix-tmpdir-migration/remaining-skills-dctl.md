# Round 03: Remaining Skills + dctl Config

> Plan: prex-sandbox-fix-tmpdir-migration | Round: 03 of 03 | Complexity: L
> Generated: 2026-05-27T21:00:00Z | Repo: /workspaces/codex-session

## Context

All Claude Code skills use hardcoded `/tmp` for run directories (`mktemp -d /tmp/<skill>-XXXXXX`).
This is non-XDG-compliant, leaks across container boundaries via host-shared bind mount, and breaks
when Codex sandbox modes restrict `/tmp` access (the `workspace-write` sandbox excludes `/tmp` by
default).

The migration target is `~/.local/state/claude-session/skill-runs/` — nested under the existing
`claude-session` devcontainer mount, so no new mount is needed.

This round completes the migration for the 7 remaining skills (prex was handled in round 02),
updates cross-skill scanning paths, and removes the `/tmp` bind mount from the dctl devcontainer
config.

## Previous Rounds

Round 01 established the conventions:

- `codex-conventions.md` now documents the XDG base path pattern under "Skill Run Directories"
- Lock file convention: `${XDG_RUNTIME_DIR:-$HOME/.local/state/claude-session/skill-runs}`

Round 02 applied the changes to the two prex skills:

- `prex/SKILL.md` uses `$_SKILL_RUNS/prex-<timestamp>-<pid>` for run dirs
- `prex-resume/SKILL.md` lock dir uses the XDG path
- Both use `--dangerously-bypass-approvals-and-sandbox` uniformly

## Scope of This Round

**IN scope:**

- 7 skill files: run-dir migration from `/tmp` to XDG base path
- 2 skill files: cross-skill scanning path updates (merge-queue, spec-impl)
- 1 dctl config file: remove `/tmp` bind mount
- Lock file migration in skills that use `${XDG_RUNTIME_DIR:-/tmp}`

**OUT of scope:**

- prex/SKILL.md and prex-resume/SKILL.md (completed in round 02)
- Documentation files (completed in round 01)
- Rust source code changes
- Any changes to exec-queue skills (xq-*) — they already use `$XDG_STATE` properly

## Current State

### Key Files — Skills with `mktemp -d /tmp/...`

- `/home/gu/.claude/skills/plan-exec/SKILL.md`
  - Line 49: `RUN_DIR="$(mktemp -d /tmp/plan-exec-XXXXXX)"`
  - Line 60: `LOCK_DIR="${XDG_RUNTIME_DIR:-/tmp}"`

- `/home/gu/.claude/skills/spec-impl/SKILL.md`
  - Line 81: `RUN_DIR="$(mktemp -d /tmp/spec-impl-XXXXXX)"`
  - Lines 129, 131: `find /tmp -maxdepth 1 -type d -name 'prex-*'` (cross-skill scanning)

- `/home/gu/.claude/skills/review-loop/SKILL.md`
  - Line 92: `RUN_DIR="$(mktemp -d /tmp/review-loop-XXXXXX)"`

- `/home/gu/.claude/skills/ask/SKILL.md`
  - Line 23: Documentation references `mktemp -d /tmp/ask-codex-XXXXXX`
  - Line 104: `RUN_DIR="$(mktemp -d /tmp/ask-codex-XXXXXX)"`

- `/home/gu/.claude/skills/tsk-impl/SKILL.md`
  - Line 59: `STAGE_DIR="$(mktemp -d /tmp/tsk-impl-XXXXXX)"`

- `/home/gu/.claude/skills/tsk-new/SKILL.md`
  - Line 201: `STAGE_DIR="$(mktemp -d /tmp/tsk-new-XXXXXX)"`

- `/home/gu/.claude/skills/refactor-migration-plan/SKILL.md`
  - Line 114: `RUN_DIR="$(mktemp -d /tmp/refactor-migration-plan-XXXXXX)"`

### Key Files — Cross-Skill Scanning

- `/home/gu/.claude/skills/merge-queue/SKILL.md`
  - Line 1038: `ls -d /tmp/prex-*/ 2>/dev/null | sort > "$QUEUE_DIR/.pre-dirs-before"`
  - Line 1063: `ls -d /tmp/prex-*/ 2>/dev/null | sort > "$QUEUE_DIR/.pre-dirs-after"`
  - Line 1166: `ls -d /tmp/prex-*/ 2>/dev/null | sort > "$QUEUE_DIR/.pre-dirs-before-regression"`
  - Line 1182: `ls -d /tmp/prex-*/ 2>/dev/null | sort > "$QUEUE_DIR/.pre-dirs-after-regression"`

### Key Files — Lock Files

- `/home/gu/.claude/skills/xq-start/SKILL.md`
  - Line 66: `LOCK_FILE="${XDG_RUNTIME_DIR:-/tmp}/xq-active-$COORD_ID"`

- `/home/gu/.claude/skills/gc/SKILL.md`
  - Line 187: References `${XDG_RUNTIME_DIR:-/tmp}/commit-hook-...` in documentation/error messages

### Key Files — dctl Config

- `/home/gu/.dotfiles/dctl/.config/dctl/devcontainer/base/devcontainer.json`
  - Lines 176-180: `/tmp` bind mount:
    ```json
    {
      "source": "/tmp",
      "target": "/tmp",
      "type": "bind"
    }
    ```

### Existing Patterns

The XDG base path pattern established in round 01 (codex-conventions.md §Skill Run Directories):

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
mkdir -p "$_SKILL_RUNS"
RUN_DIR="$_SKILL_RUNS/<skill>-$(date -u +%Y%m%dT%H%M%S)-$$"
mkdir -p "$RUN_DIR"
```

## Implementation Steps

### Step 1: Migrate plan-exec run dir and lock

In `/home/gu/.claude/skills/plan-exec/SKILL.md`:

Replace `RUN_DIR="$(mktemp -d /tmp/plan-exec-XXXXXX)"` with:

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
mkdir -p "$_SKILL_RUNS"
RUN_DIR="$_SKILL_RUNS/plan-exec-$(date -u +%Y%m%dT%H%M%S)-$$"
mkdir -p "$RUN_DIR"
```

Replace `LOCK_DIR="${XDG_RUNTIME_DIR:-/tmp}"` with:

```bash
LOCK_DIR="${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs}"
```

### Step 2: Migrate spec-impl run dir + cross-skill scanning

In `/home/gu/.claude/skills/spec-impl/SKILL.md`:

Replace `RUN_DIR="$(mktemp -d /tmp/spec-impl-XXXXXX)"` with the standard pattern (substituting
`spec-impl` as the skill name).

Replace all `find /tmp -maxdepth 1 -type d -name 'prex-*'` with:

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
find "$_SKILL_RUNS" -maxdepth 1 -type d -name 'prex-*'
```

Also update any documentation text that references `/tmp/prex-*` paths.

### Step 3: Migrate review-loop run dir

In `/home/gu/.claude/skills/review-loop/SKILL.md`:

Replace `RUN_DIR="$(mktemp -d /tmp/review-loop-XXXXXX)"` with the standard pattern (substituting
`review-loop` as the skill name).

### Step 4: Migrate ask run dir

In `/home/gu/.claude/skills/ask/SKILL.md`:

Replace `RUN_DIR="$(mktemp -d /tmp/ask-codex-XXXXXX)"` with the standard pattern (substituting
`ask-codex` as the skill name).

Also update the documentation text (line 23) that references the `/tmp/ask-codex-XXXXXX` pattern.

### Step 5: Migrate tsk-impl and tsk-new stage dirs

In `/home/gu/.claude/skills/tsk-impl/SKILL.md`:

Replace `STAGE_DIR="$(mktemp -d /tmp/tsk-impl-XXXXXX)"` with the standard pattern (substituting
`tsk-impl` as the skill name, using `STAGE_DIR` as the variable name).

In `/home/gu/.claude/skills/tsk-new/SKILL.md`:

Replace `STAGE_DIR="$(mktemp -d /tmp/tsk-new-XXXXXX)"` with the standard pattern (substituting
`tsk-new` as the skill name, using `STAGE_DIR` as the variable name).

### Step 6: Migrate refactor-migration-plan run dir

In `/home/gu/.claude/skills/refactor-migration-plan/SKILL.md`:

Replace `RUN_DIR="$(mktemp -d /tmp/refactor-migration-plan-XXXXXX)"` with the standard pattern
(substituting `refactor-migration-plan` as the skill name).

### Step 7: Update merge-queue cross-skill scanning

In `/home/gu/.claude/skills/merge-queue/SKILL.md`, replace all four instances of
`ls -d /tmp/prex-*/` with:

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
ls -d "$_SKILL_RUNS"/prex-*/ 2>/dev/null | sort
```

The four locations are around lines 1038, 1063, 1166, and 1182.

### Step 8: Update xq-start and gc lock file paths

In `/home/gu/.claude/skills/xq-start/SKILL.md` (line 66), replace
`${XDG_RUNTIME_DIR:-/tmp}` with
`${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs}`.

In `/home/gu/.claude/skills/gc/SKILL.md` (line 187), update the documentation/error message
reference from `${XDG_RUNTIME_DIR:-/tmp}` to the new path.

### Step 9: Remove /tmp bind mount from dctl config

In `/home/gu/.dotfiles/dctl/.config/dctl/devcontainer/base/devcontainer.json`, remove lines 176-180:

```json
{
  "source": "/tmp",
  "target": "/tmp",
  "type": "bind"
}
```

Also remove the trailing comma from the preceding mount entry (the glab-cli mount ending at
line 175) if it would leave a dangling comma.

### Step 10: Verify no remaining /tmp references

Run a verification sweep:

```bash
grep -rn "mktemp.*tmp\|/tmp/" ~/.claude/skills/ | grep -v "XDG_RUNTIME_DIR"
```

Any remaining `/tmp` references (excluding `XDG_RUNTIME_DIR` fallbacks) must be investigated and
updated.

### Final Step: Update plan index

Update the plan's `README.md` (in the same directory as this round file) to record completion:

1. In the `## Execution Order` table, find the row for round 03.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).
4. In the README.md header blockquote, change `Status: todo` to `Status: done`.
5. Move the plan directory to done:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/prex-sandbox-fix-tmpdir-migration .plan/02-done/prex-sandbox-fix-tmpdir-migration
```

## Acceptance Criteria

- [ ] All 7 skill files use `$_SKILL_RUNS/<skill>-<timestamp>-<pid>` for run directories
- [ ] No skill file contains `mktemp -d /tmp/` patterns
- [ ] merge-queue scans `$_SKILL_RUNS/prex-*/` instead of `/tmp/prex-*/`
- [ ] spec-impl scans `$_SKILL_RUNS` instead of `/tmp` for prex directories
- [ ] xq-start and gc lock paths use XDG fallback
- [ ] dctl `base/devcontainer.json` no longer has a `/tmp` bind mount
- [ ] `grep -rn "mktemp.*tmp\|/tmp/" ~/.claude/skills/ | grep -v XDG_RUNTIME_DIR` returns empty
- [ ] Plan `README.md` execution order table shows round 03 as `done` with today's date
- [ ] Plan `README.md` header status is `done`
- [ ] Plan directory moved from `.plan/01-todo/prex-sandbox-fix-tmpdir-migration` to
      `.plan/02-done/prex-sandbox-fix-tmpdir-migration`

## Next Round

This is the final round.
