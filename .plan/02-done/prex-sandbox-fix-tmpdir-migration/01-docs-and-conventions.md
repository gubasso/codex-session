# Round 01: Documentation & Conventions

> Plan: prex-sandbox-fix-tmpdir-migration | Round: 01 of 03 | Complexity: L
> Generated: 2026-05-27T21:00:00Z | Repo: /workspaces/codex-session

## Context

The prex skill orchestrates `codex-session` across multiple stages: stage 1 runs Codex in read-only
mode to generate a plan, stage 3 resumes the same thread for write-mode implementation. This resume
fails with JSON-RPC -32600 ("no rollout found") because the sandbox mode changes between the two
calls (`--sandbox read-only` → `--full-auto`). The OpenAI Codex backend rejects resume requests when
sandbox parameters differ from the original session.

Additionally, `--full-auto` is deprecated since Codex v0.128.0 (replaced by
`--sandbox workspace-write`), and all skill run directories use hardcoded `/tmp` paths which are
blocked by the `workspace-write` sandbox and leak across container boundaries.

The fix: use `--dangerously-bypass-approvals-and-sandbox` uniformly for all Codex calls (container
environment), enforce read-only/write behavior via strengthened prompt injection, and migrate run
directories from `/tmp` to `~/.local/state/claude-session/skill-runs/`.

This round establishes the documentation and conventions that subsequent rounds reference.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

**IN scope:**
- Add F15 (sandbox mode mismatch constraint) to `docs/upstream-codex.md`
- Update `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` with:
  - Unified sandbox approach for resume-compatible workflows
  - `--full-auto` deprecation note
  - Strengthened behavioral orientation blocks
  - New run-dir base path convention
  - Resume constraint documentation
  - Updated safety rules flag matrix
  - Source citations for the GitHub issues found during research
- Bump `last-synced` in `~/DocsNNotes/tech/tools/claude-code/AGENTS.md`

**OUT of scope:**
- Modifying any skill files (rounds 02 and 03)
- Modifying dctl devcontainer config (round 03)
- Any changes to Rust source code

## Current State

### Key Files

- `/workspaces/codex-session/docs/upstream-codex.md` — Verified Codex CLI behavior reference. Last
  fact is F14 (Thread/session resume). Needs F15 about sandbox mismatch constraint. Currently 309
  lines.

- `/home/gu/DocsNNotes/tech/tools/claude-code/codex-conventions.md` — Source of truth for Codex CLI
  patterns used by all skills. Currently 338 lines. Contains:
  - "Environment Compatibility" section (lines 89-151) with native/fallback sandbox branching
  - "Non-Interactive Plan Call" (lines 153-171) using `--sandbox read-only`
  - "Non-Interactive Implementation Call" (lines 173-188) using `--full-auto`
  - "Session Resumption" (lines 190-217) with two patterns (write and read-only resume)
  - "Behavioral Orientation" (lines 219-239) with read-only and write blocks
  - "Safety Rules" (lines 287-306) with native/fallback flag matrices

- `/home/gu/DocsNNotes/tech/tools/claude-code/AGENTS.md` — Index digest with `last-synced` date
  and `source-files` list.

### Existing Patterns

The codex-conventions.md file uses H2 sections with code blocks showing exact command patterns.
Safety rules use bullet-list flag matrices. The upstream-codex.md uses `## F<N> — <title>` format
with Sources lists and Implementation notes.

## Implementation Steps

### Step 1: Add F15 to upstream-codex.md

Add a new section after F14 (line 275) in `/workspaces/codex-session/docs/upstream-codex.md`:

```markdown
## F15 — Sandbox mode mismatch on resume

`exec resume` fails with JSON-RPC -32600 ("no rollout found") when the
sandbox mode of the resumed call differs from the sandbox mode of the
original session.  The OpenAI backend validates that session parameters
match on resume and rejects requests with incompatible sandbox changes.

Observed failure chain:

1. Stage 1 creates a thread with `--sandbox read-only`.
2. Stage 3 attempts `exec resume <thread-id>` with `--full-auto`
  (or `--sandbox workspace-write`).
3. Backend returns -32600 "no rollout found" despite the thread
  existing in the local index and the local rollout file being present.

The local `codex-session` wrapper correctly resolves the thread ID and
routes to the right account/group via `thread-index.jsonl`.  The failure
is purely server-side parameter validation.

Workaround: use `--dangerously-bypass-approvals-and-sandbox` uniformly
across all stages that share a thread.  This flag bypasses bubblewrap
entirely and sends no sandbox parameters to the backend, so there is no
mismatch to validate.

- **Sources:** Empirical testing (2026-05-27),
  [issue #3947 — "Agent cannot edit files using sandbox when resuming"](https://github.com/openai/codex/issues/3947),
  [issue #5322 — "Sandbox flags not honored on resume"](https://github.com/openai/codex/issues/5322),
  [issue #16994 — "No rollout materializes"](https://github.com/openai/codex/issues/16994),
  [issue #18676 — "Resume session: stream disconnected"](https://github.com/openai/codex/issues/18676),
  [issue #19661 — "exec resume fails with encrypted_content"](https://github.com/openai/codex/issues/19661),
  [issue #23875 — "Desktop drops approvals_reviewer after resume"](https://github.com/openai/codex/issues/23875).
- **Implementation note:** `codex-session` does not intercept or translate
  sandbox flags — they pass through to the codex binary unchanged.  The
  constraint is upstream in the OpenAI Codex backend.
```

Bump the "Last verified" date at the top of the file to `2026-05-27`.

### Step 2: Update codex-conventions.md — Resume Constraint

In `/home/gu/DocsNNotes/tech/tools/claude-code/codex-conventions.md`, add a "Resume Constraint"
subsection inside the "Session Resumption" section (after line 217). Content:

```markdown
### Resume Constraint

`exec resume` fails with JSON-RPC -32600 ("no rollout found") when the sandbox mode changes between
the original session and the resumed call. The Codex backend validates session parameter consistency
and rejects mismatches. See `docs/upstream-codex.md` §F15.

For workflows that resume threads across sandbox mode transitions (e.g., read-only planning → write
implementation), use `--dangerously-bypass-approvals-and-sandbox` uniformly across all stages. This
bypasses bubblewrap entirely and sends no sandbox parameters to the backend.
```

### Step 3: Update codex-conventions.md — Deprecate --full-auto

Add a note after the "Non-Interactive Implementation Call" section heading (around line 173):

```markdown
> **Note:** `--full-auto` is deprecated since Codex v0.128.0. The replacement is
> `--sandbox workspace-write`. For resume-compatible workflows, use
> `--dangerously-bypass-approvals-and-sandbox` instead (see Resume Constraint above).
```

Update the command example from `--full-auto` to `--sandbox workspace-write` for the non-resume
case. Keep the `--dangerously-bypass-approvals-and-sandbox` example for the fallback case.

### Step 4: Update codex-conventions.md — Unified Sandbox for Resume Workflows

Add a new subsection "Unified Sandbox for Resume Workflows" after the "Fallback Patterns" subsection
(around line 151). Content:

```markdown
### Unified Sandbox for Resume Workflows

Workflows that use `exec resume` across stages with different access needs (e.g., read-only planning
then write implementation) must use the same sandbox flags on every call. The simplest approach: use
`--dangerously-bypass-approvals-and-sandbox` for all stages and enforce read-only/write behavior via
prompt injection.

Planning call (resume-compatible):

```bash
codex-session --account auto exec \
  --dangerously-bypass-approvals-and-sandbox --json \
  --output-last-message "$RUN_DIR/stage1-plan.txt" \
  "<planning prompt with read-only orientation>" \
  < /dev/null > "$RUN_DIR/stage1-events.jsonl"
```

Implementation call (resuming planning session):

```bash
codex-session --account auto exec resume "$THREAD_ID" \
  --dangerously-bypass-approvals-and-sandbox --json \
  --output-last-message "$RUN_DIR/stage3-impl-report.txt" \
  "<implementation prompt with write orientation>" \
  < /dev/null > "$RUN_DIR/stage3-events.jsonl"
```

This pattern applies to: `prex`, `prex-resume`, and any future skill that resumes threads across
access mode boundaries.
```

### Step 5: Update codex-conventions.md — Strengthened Orientation Blocks

Replace the "Behavioral Orientation" section (lines 219-239) with strengthened blocks that serve as
primary enforcement (not just defense-in-depth):

Read-only orientation:

```text
=== STRICT READ-ONLY MODE ===
You are operating in READ-ONLY mode. This is a hard constraint.
PROHIBITED actions — any of these is a critical violation:
- Creating, modifying, or deleting any file
- Writing to any path on disk
- Running git commands (commit, add, push, reset, checkout, etc.)
- Executing any command that mutates system state
PERMITTED actions:
- Reading files, analyzing code, producing text output
- Running read-only shell commands (cat, grep, find, ls, etc.)
Produce your plan as text output only.
===
```

Write orientation:

```text
=== WRITE MODE ACTIVE ===
The prior READ-ONLY restriction no longer applies. You now have WRITE access.
PERMITTED actions:
- Creating, modifying, and deleting files within the workspace
- Running build/lint/test commands
STILL PROHIBITED:
- Running any git commands (commit, add, push, reset, checkout, etc.)
- Writing outside the workspace directory
Implement the plan below exactly. Report all files changed and any deviations.
===
```

Keep the existing contract note about injecting orientation on every call.

### Step 6: Update codex-conventions.md — Run-Dir Base Path

Add a new section "Skill Run Directories" after "Timeout Requirement" (line 283). Content:

```markdown
## Skill Run Directories

Skills that create temporary run directories must use the XDG-compliant base path:

```bash
_SKILL_RUNS="${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs"
mkdir -p "$_SKILL_RUNS"
RUN_DIR="$_SKILL_RUNS/<skill>-$(date -u +%Y%m%dT%H%M%S)-$$"
mkdir -p "$RUN_DIR"
```

This produces paths like `~/.local/state/claude-session/skill-runs/prex-20260527T200809-12345/`.

Do NOT use `/tmp` for run directories. `/tmp` is excluded by Codex's `workspace-write` sandbox mode
and may not be shared between host and container.

Lock files follow the same pattern:

```bash
LOCK_DIR="${XDG_RUNTIME_DIR:-${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs}"
```
```

### Step 7: Update codex-conventions.md — Safety Rules

In the "Safety Rules" section (lines 287-306), update the flag matrices:

- Replace `--full-auto` with `--sandbox workspace-write` in the native matrix
- Add a note: "For resume-compatible workflows, replace both matrices with a single rule:
  `--dangerously-bypass-approvals-and-sandbox` for all calls (see Unified Sandbox for Resume
  Workflows above)."
- Add new rule: "Do NOT use `/tmp` for skill run directories. Use the XDG base path from
  §Skill Run Directories."

### Step 8: Update AGENTS.md

In `/home/gu/DocsNNotes/tech/tools/claude-code/AGENTS.md`, bump `last-synced` to `2026-05-27`.

### Final Step: Update plan index

Update the plan's `README.md` (in the same directory as this round file) to record completion:

1. In the `## Execution Order` table, find the row for round 01.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `/workspaces/codex-session/docs/upstream-codex.md` contains F15 documenting sandbox mode
      mismatch on resume, with sources and implementation note
- [ ] `~/DocsNNotes/tech/tools/claude-code/codex-conventions.md` contains:
  - "Resume Constraint" subsection under "Session Resumption"
  - `--full-auto` deprecation note
  - "Unified Sandbox for Resume Workflows" subsection with command examples
  - Strengthened read-only and write orientation blocks
  - "Skill Run Directories" section with XDG base path pattern
  - Updated safety rules flag matrix
- [ ] `~/DocsNNotes/tech/tools/claude-code/AGENTS.md` has `last-synced: 2026-05-27`
- [ ] No skill files or dctl config were modified
- [ ] Plan `README.md` execution order table shows round 01 as `done` with today's date

## Next Round

Round 02 applies the sandbox fix and run-dir migration to the two prex skills (`prex/SKILL.md` and
`prex-resume/SKILL.md`), referencing the conventions established in this round.
