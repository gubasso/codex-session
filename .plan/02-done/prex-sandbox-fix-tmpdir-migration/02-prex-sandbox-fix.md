# Round 02: Prex Sandbox Fix + Run-Dir Migration

> Plan: prex-sandbox-fix-tmpdir-migration | Round: 02 of 03 | Complexity: L
> Generated: 2026-05-27T21:00:00Z | Repo: /workspaces/codex-session

## Context

The prex skill orchestrates `codex-session` across multiple stages: stage 1 runs Codex in read-only
mode to generate a plan, stage 3 resumes the same thread for write-mode implementation. This resume
fails with JSON-RPC -32600 ("no rollout found") because the sandbox mode changes between the two
calls. The OpenAI Codex backend rejects resume requests when sandbox parameters differ.

The fix: use `--dangerously-bypass-approvals-and-sandbox` uniformly for all Codex calls (container
environment), enforce read-only/write behavior via strengthened prompt injection, and migrate run
directories from `/tmp` to `~/.local/state/claude-session/skill-runs/`.

## Previous Rounds

Round 01 updated the documentation and conventions:

- Added F15 (sandbox mismatch constraint) to `docs/upstream-codex.md`
- Updated `codex-conventions.md` with unified sandbox approach, strengthened orientation blocks,
  `--full-auto` deprecation, run-dir base path convention, and resume constraint documentation

## Scope of This Round

**IN scope:**

- `~/.claude/skills/prex/SKILL.md` — sandbox fix + run-dir migration + resume fallback
- `~/.claude/skills/prex-resume/SKILL.md` — same changes

**OUT of scope:**

- Other skill files (round 03)
- dctl devcontainer config (round 03)
- Rust source code changes
- `docs/upstream-codex.md` or `codex-conventions.md` (round 01)

## Current State

### Key Files

- `/home/gu/.claude/skills/prex/SKILL.md` (906 lines) — Main prex orchestrator skill. Key sections:
  - Line 137: `RUN_DIR="$(mktemp -d /tmp/prex-XXXXXX)"` — run dir creation
  - Lines 169: `LOCK_DIR="${XDG_RUNTIME_DIR:-/tmp}"` — lock file dir
  - Lines 248-259: `SANDBOX_MODE` extraction and fallback notification
  - Lines 338-339: References read-only orientation from codex-conventions.md
  - Lines 351-368: Stage 1 Codex call (native/fallback branching)
  - Lines 581-589: Stage 3 write orientation reference
  - Lines 591-592: "Do not re-send the original task description" (unconditional)
  - Lines 600-618: Stage 3 Codex resume call (native/fallback branching)
  - Lines 787-826: Stage 5 scanning `/tmp` for `review-loop-*` directories

- `/home/gu/.claude/skills/prex-resume/SKILL.md` — Companion resume skill. Key sections:
  - Line 103: `LOCK_DIR="${XDG_RUNTIME_DIR:-/tmp}"` — lock file dir
  - Lines 172-177: `SANDBOX_MODE` extraction and persistence
  - Lines 208-209: "Do not re-send the original task description" (unconditional)
  - Lines 213-214: Stage 3 instructions referencing `--full-auto`/`--dangerously-bypass...`

### Existing Patterns

Both skills reference `$DOCS_NOTES_REPO/tech/tools/claude-code/codex-conventions.md` for:

- Behavioral orientation blocks (read-only and write)
- Codex command patterns
- Thread ID extraction
- Timeout requirements

The `SANDBOX_MODE` variable currently drives conditional branching between native (`--sandbox
read-only` / `--full-auto`) and fallback (`-c sandbox_permissions` /
`--dangerously-bypass-approvals-and-sandbox`) patterns.

## Implementation Steps

### Step 1: Migrate prex run directory from /tmp

In `/home/gu/.claude/skills/prex/SKILL.md`, replace the run directory creation (around line 137):

**Before:**

```bash
RUN_DIR="$(mktemp -d /tmp/prex-XXXXXX)"
```

**After:**

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
mkdir -p "$_SKILL_RUNS"
RUN_DIR="$_SKILL_RUNS/prex-$(date -u +%Y%m%dT%H%M%S)-$$"
mkdir -p "$RUN_DIR"
```

### Step 2: Migrate prex lock directory

In the same file, replace the lock directory (around line 169):

**Before:**

```bash
LOCK_DIR="${XDG_RUNTIME_DIR:-/tmp}"
```

**After:**

```bash
LOCK_DIR="${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs}"
```

### Step 3: Remove SANDBOX_MODE branching from prex

Remove the `SANDBOX_MODE` extraction and fallback notification block (lines 248-259). The
`SANDBOX_MODE` variable from preflight.json is no longer used to drive branching. The preflight
health check (account availability, codex-session on PATH) is still needed and stays.

Remove these lines:

```markdown
SANDBOX_MODE="$(jq -r '.codex_session.sandbox_mode' "$RUN_DIR/preflight.json")"
echo "SANDBOX_MODE=$SANDBOX_MODE"
```

And the `Persist SANDBOX_MODE...` paragraph and the `If SANDBOX_MODE=fallback, briefly inform the
user` block.

### Step 4: Update prex stage 1 Codex call

Replace the dual-branch stage 1 command (lines 351-368) with a single command:

**Before:**

````markdown
When `SANDBOX_MODE=native`:

```bash
codex-session --account auto exec --sandbox read-only --json \
  ...
```
````

When `SANDBOX_MODE=fallback`:

```bash
codex-session --account auto exec \
  -c 'sandbox_permissions=["disk-full-read-access"]' --json \
  ...
```

`````markdown
**After:**

````markdown
```bash
codex-session --account auto exec \
  --dangerously-bypass-approvals-and-sandbox --json \
  --output-last-message "$RUN_DIR/stage1-plan.txt" \
  "<planning prompt>" \
  < /dev/null > "$RUN_DIR/stage1-events.jsonl"
```
````
`````

`````markdown
Also update the stage 1 prompt construction note (lines 338-339) to emphasize that the read-only
orientation block is the **primary** enforcement mechanism, not just defense-in-depth.

### Step 5: Update prex stage 3 Codex call + resume fallback

Replace the dual-branch stage 3 command (lines 600-618) with a single command:

````markdown
```bash
codex-session --account auto exec resume "$PLAN_THREAD_ID" \
  --dangerously-bypass-approvals-and-sandbox --json \
  --output-last-message "$RUN_DIR/stage3-impl-report.txt" \
  "$(cat "$RUN_DIR/stage3-prompt.md")" \
  < /dev/null > "$RUN_DIR/stage3-events.jsonl"
echo "EXIT_CODE=$?"
```
````
`````

`````markdown
After the command, add a resume fallback procedure:

````markdown
### Resume Fallback

If the resume call fails (non-zero exit code, empty `stage3-events.jsonl`, or stderr containing
"no rollout found" or "thread not found"):

1. Build a self-contained implementation prompt that **inlines** all context directly in the prompt
   body:

- The write orientation block from codex-conventions.md
- The full content of `$RUN_DIR/stage2-reviewed-plan.md` (not a file path reference)
- The full content of `$RUN_DIR/request.md` (not a file path reference)
- Relevant repo constraints and conventions from CLAUDE.md
- The implementation instructions (implement phases in order, report files changed, etc.)

2. Run a fresh `exec` (not resume) with the inlined prompt:

```bash
codex-session --account auto exec \
  --dangerously-bypass-approvals-and-sandbox --json \
  --output-last-message "$RUN_DIR/stage3-impl-report.txt" \
  "$(cat "$RUN_DIR/stage3-prompt-full.md")" \
  < /dev/null > "$RUN_DIR/stage3-events.jsonl"
```
````
`````

**Critical:** The fallback prompt must NEVER reference `/tmp` or `$RUN_DIR` file paths as
instructions for Codex to read. Codex cannot access paths outside the workspace under most sandbox
modes. Inline all content directly in the prompt body.

````markdown
Also update line 591-592: change "Do not re-send the original task description or repo
constraints/conventions. Those remain available in the resumed session context from stage 1." to:

"When resume succeeds, do not re-send the original task description or repo
constraints/conventions — those remain in the resumed session context from stage 1. When falling
back to fresh exec, inline all context (see Resume Fallback above)."

### Step 6: Update prex stage 5 /tmp scanning

Replace the stage 5 review-loop directory scanning (around lines 787-826) to use the new base dir:

**Before:**

```bash
find /tmp -maxdepth 1 -type d -name 'review-loop-*' -printf '%p\n' 2>/dev/null | sort > "$RUN_DIR/stage5-pre-rl.snap"
```
````

**After:**

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
find "$_SKILL_RUNS" -maxdepth 1 -type d -name 'review-loop-*' -printf '%p\n' 2>/dev/null | sort > "$RUN_DIR/stage5-pre-rl.snap"
```

Apply the same change to the post-delegation snapshot and the error messages that reference `/tmp`.

### Step 7: Apply same changes to prex-resume/SKILL.md

In `/home/gu/.claude/skills/prex-resume/SKILL.md`:

1. **Lock dir** (line 103): Replace `${XDG_RUNTIME_DIR:-/tmp}` with
   `${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs}`

2. **Remove SANDBOX_MODE** (lines 172-177): Remove the extraction and persistence instructions.

3. **Stage 3 instructions** (lines 213-214): Replace "Use the write-capable variant (`--full-auto`
   / `--dangerously-bypass-approvals-and-sandbox`) based on `SANDBOX_MODE`" with: "Use
   `--dangerously-bypass-approvals-and-sandbox`."

4. **Resume fallback** (after line 217): Add the same resume fallback procedure as prex. This is
   especially critical for prex-resume since it runs from exec-queue (automated, no user
   interaction).

5. **Context re-send** (lines 208-209): Make conditional on resume success, same as step 5 above.

### Final Step: Update plan index

Update the plan's `README.md` (in the same directory as this round file) to record completion:

1. In the `## Execution Order` table, find the row for round 02.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `prex/SKILL.md` uses `~/.local/state/claude-session/skill-runs/prex-...` for run directory
- [ ] `prex/SKILL.md` has no `SANDBOX_MODE` branching
- [ ] `prex/SKILL.md` stage 1 uses `--dangerously-bypass-approvals-and-sandbox` (single command)
- [ ] `prex/SKILL.md` stage 3 uses `--dangerously-bypass-approvals-and-sandbox` (single command)
- [ ] `prex/SKILL.md` has resume fallback procedure with inlined context
- [ ] `prex/SKILL.md` stage 5 scans `$_SKILL_RUNS` not `/tmp` for review-loop dirs
- [ ] `prex-resume/SKILL.md` lock dir uses XDG path
- [ ] `prex-resume/SKILL.md` has no `SANDBOX_MODE` branching
- [ ] `prex-resume/SKILL.md` stage 3 uses `--dangerously-bypass-approvals-and-sandbox`
- [ ] `prex-resume/SKILL.md` has resume fallback procedure
- [ ] No references to `--full-auto` remain in either file
- [ ] No references to `--sandbox read-only` remain in either file
- [ ] No hardcoded `/tmp` paths remain in either file (except in `$XDG_RUNTIME_DIR` fallbacks)
- [ ] Plan `README.md` execution order table shows round 02 as `done` with today's date

## Next Round

Round 03 migrates the remaining 7 skills from `/tmp` to the XDG base dir, updates cross-skill
scanning in merge-queue and spec-impl, and removes the `/tmp` bind mount from the dctl devcontainer
config.
