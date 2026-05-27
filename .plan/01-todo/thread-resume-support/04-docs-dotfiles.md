# Round 04: Documentation & Dotfiles

> Plan: thread-resume-support | Round: 04 of 04 | Complexity: L
> Generated: 2026-05-26T00:00:00Z | Repo: /workspaces/codex-session

## Context

codex-session wraps OpenAI's Codex CLI with multi-account support. Rounds
01-03 added thread resume support: a cross-account JSONL thread index,
automatic thread ID capture from `--json` exec runs, and smart resume
routing that intercepts `exec resume` commands and routes them to the
correct account's `CODEX_HOME`.

This round updates all documentation to reflect the new feature, adds the
resume usage pattern to the README, records upstream Codex resume behavior
in the reference docs, and syncs relevant dotfiles.

## Previous Rounds

Round 01 created:
- `src/services/session/thread_index.rs` — `ThreadEntry`, `append()`,
  `lookup()`, `last_for_group()`, `last_any()`.
- Thread index at `<state_dir>/thread-index.jsonl`.

Round 02 wired:
- `--json` detection, stdout tee for JSON mode, thread event parsing,
  automatic index writes after each exec.

Round 03 added:
- Resume argv detection (`detect_resume()`).
- Thread→account lookup from the index.
- `AccountResolutionSource::ThreadIndex` variant.
- `run_resume()` handler with argv rewriting and graceful fallback.
- `--all-groups` flag for global resume scope.

## Scope of This Round

**IN scope:**
- Update `docs/upstream-codex.md` — add F14 section documenting Codex's
  thread/session resume behavior and storage semantics.
- Update `docs/multi-account-architecture.md` — add a section on how
  codex-session solves cross-account thread resume.
- Update `README.md` — add resume usage examples to the Usage section and
  `thread-index.jsonl` to the Filesystem layout.
- Update `CLAUDE.md` if needed — mention resume-related just recipes or
  conventions.
- Sync dotfiles in `~/.dotfiles/` — update any codex-session or
  coding-agent-skills references that document codex-session's feature set.

**OUT of scope:**
- Code changes (all implementation is done in rounds 01-03).
- Index rotation/pruning strategy (future follow-up).
- New test files.

## Current State

### Key Files

- `/workspaces/codex-session/docs/upstream-codex.md` — canonical reference
  for upstream Codex behavior. Currently documents F1-F13. The last section
  is F13 (Token refresh endpoint). A new F14 section should follow.

  The file ends with a `## Sources (full list)` section containing links.
  New sources should be added there.

- `/workspaces/codex-session/docs/multi-account-architecture.md` — documents
  how upstream and community projects solve multi-account auth. Has 5
  sections: Problem statement, Upstream feature request, Community projects,
  Comparative table, What codex-session adopted. The comparative table and
  adoption section need updating.

- `/workspaces/codex-session/README.md` — project README. The Usage section
  shows common patterns. The Filesystem layout section shows the directory
  tree. Both need resume additions.

- `/workspaces/codex-session/CLAUDE.md` — agent guide. Currently focused on
  quality gates (`just` recipes). No changes expected unless a new recipe is
  added for thread index operations.

### Dotfiles locations

- `~/.dotfiles/claude/` — Claude Code configuration and skills. Contains
  `README.md`. If it documents codex-session capabilities, update it.
- `~/.dotfiles/codex-session/` — codex-session dotfiles (may contain stow
  manifests, config templates). Update if it documents features.
- `~/.dotfiles/coding-agent-skills/` — shared skill infrastructure. Update
  if any skill references codex-session resume (e.g., `prex` skill).

### Existing Patterns

- **docs/upstream-codex.md format:** Each fact is an `## F<N>` section with:
  - Description paragraph.
  - `- **Sources:**` bullet with linked PR/issue/doc references.
  - Optional `- **Implementation note:**` bullet with codex-session specifics.

- **docs/multi-account-architecture.md format:** Sections with comparison
  tables, bullet-point feature descriptions, and rationale paragraphs.

- **README.md format:** Code blocks for usage examples, `text` fenced blocks
  for directory trees, table for exit codes.

## Implementation Steps

### Step 1: Add F14 to `docs/upstream-codex.md`

Insert a new section before `## codex-session-specific notes`:

```markdown
## F14 — Thread/session resume

Codex supports resuming previous sessions via:
- `codex resume` — interactive picker for recent sessions in current cwd.
- `codex resume --all` — include sessions from any directory.
- `codex resume --last` — skip picker, jump to most recent.
- `codex resume <SESSION_ID>` — resume a specific session by UUID.
- `codex exec resume <SESSION_ID>` — non-interactive variant.
- `codex exec resume --last` — non-interactive, most recent.

Session storage: `$CODEX_HOME/sessions/YYYY/MM/DD/rollout-<timestamp>-<uuid>.jsonl[.zst]`.
Thread metadata: `$CODEX_HOME/state_5.sqlite` (threads table with `rollout_path` column).
Session index: `$CODEX_HOME/session_index.jsonl`.

All three are fully scoped by `CODEX_HOME` — different `CODEX_HOME` values
produce completely isolated session namespaces. There is no cross-`CODEX_HOME`
session discovery.

Thread IDs are UUID v7, auto-generated on first user message.

Resume protocol: the app-server's `thread/resume` endpoint looks up the
thread_id in `state_5.sqlite`, loads the corresponding rollout JSONL file,
and replays events to reconstruct state. Returns "thread not found" if the
database entry or rollout file is missing.

- **Sources:** [Codex CLI features — session resume](https://developers.openai.com/codex/cli/features),
  [Codex CLI reference — resume](https://developers.openai.com/codex/cli/reference),
  [App Server README](https://github.com/openai/codex/blob/main/codex-rs/app-server/README.md),
  [Issue #19661 — resume fails with encrypted_content](https://github.com/openai/codex/issues/19661),
  [Issue #15538 — ephemeral resume](https://github.com/openai/codex/issues/15538),
  [Issue #21196 — missing rollout files](https://github.com/openai/codex/issues/21196).
- **Implementation note:** `codex-session` maintains a cross-account thread
  index at `<state_dir>/thread-index.jsonl`. When `exec resume <ID>` is
  invoked, the wrapper looks up the thread→account mapping and forwards to
  Codex with the correct account's `CODEX_HOME`. For `--last`, the wrapper
  resolves the group-scoped most recent thread from the index and rewrites
  the argv to use the concrete thread_id.
```

Also add the new source URLs to the `## Sources (full list)` section at the
bottom of the file.

Update the `Last verified` date to the current date.

### Step 2: Update `docs/multi-account-architecture.md`

Add a new section `## 6. Thread resume across accounts` after the existing
section 5:

Document:
- The problem: per-`CODEX_HOME` session isolation means threads are
  invisible across accounts.
- The solution: cross-account JSONL thread index at
  `<state_dir>/thread-index.jsonl`.
- How it works: automatic capture from `--json` output → index → smart
  routing on resume.
- How community projects handle it (or don't): codex-lb doesn't address it,
  CAAM isolates fully (no cross-account resume), codex-multi-auth uses
  shadow directories.

Update the comparative table in section 4 to add a "Thread resume" row:

```markdown
| Thread resume    | N/A          | No (isolated) | Shadow sync      | **Cross-account index** |
```

### Step 3: Update `README.md`

Add resume examples to the Usage section, after the existing usage code block:

```markdown
### Thread resume

codex-session tracks thread IDs from `--json` exec runs and routes resume
requests to the correct account automatically:

```bash
codex-session exec resume <SESSION_ID>        # resume by ID (routes to correct account)
codex-session exec resume --last              # resume last thread in this terminal
codex-session exec resume --last --all-groups # resume last thread across all terminals
codex-session resume --last                   # interactive resume (last session)
```
```

Add `thread-index.jsonl` to the Filesystem layout section:

```text
$XDG_STATE_HOME/codex-session/
  state/last-account                    LRU pointer (plain text)
  thread-index.jsonl                    cross-account thread index (JSONL)
  accounts/<account>/
    ...
```

### Step 4: Check and update dotfiles

Read the following dotfile locations and update if they document
codex-session features or capabilities:

1. `~/.dotfiles/claude/README.md` — if it lists codex-session features,
    add thread resume.
2. `~/.dotfiles/codex-session/` — if it has feature docs or config templates,
    update accordingly.
3. `~/.dotfiles/coding-agent-skills/` — if any skill (particularly `prex`)
    references codex-session's capabilities, update to note resume support.

If the dotfile directories don't exist or don't contain relevant
documentation, skip this step and note it in the commit message.

### Step 5: Verify documentation consistency

Read through the updated docs and verify:
- All file paths mentioned match the actual implementation.
- The `thread-index.jsonl` format description matches the `ThreadEntry`
  struct from round 01.
- The CLI flag names (`--last`, `--all-groups`) match what round 03
  implemented.
- No references to "the conversation" or "as discussed".

## Acceptance Criteria

- [ ] `docs/upstream-codex.md` has an F14 section documenting thread resume.
- [ ] `docs/upstream-codex.md` sources list includes new resume-related URLs.
- [ ] `docs/upstream-codex.md` `Last verified` date is updated.
- [ ] `docs/multi-account-architecture.md` has section 6 on thread resume.
- [ ] `docs/multi-account-architecture.md` comparative table includes
      thread resume row.
- [ ] `README.md` Usage section includes resume examples.
- [ ] `README.md` Filesystem layout includes `thread-index.jsonl`.
- [ ] Dotfiles checked and updated if relevant documentation exists.
- [ ] All documentation is internally consistent with the implementation.
- [ ] `just lint` passes (no code changes, but markdown may be linted).

## Next Round

This is the final round.
