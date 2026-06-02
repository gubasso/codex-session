# Round 02: Cross-Repo Verification Report

> Plan: done-plans-fidelity-cleanup | Round: 02 of 02 | Complexity: S
> Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

An adversarial review of the completed (`status: done`) plans in `.plan/` confirmed
the in-repo work but could not verify two plans whose work lands in **other
repositories**: Plan 05 round 05 (`configs-rename-split-profiles` dotfiles
propagation) and Plan 06 rounds 02–03 (`prex-sandbox-fix-tmpdir-migration` —
prex skills, the `codex-conventions.md`/`AGENTS.md` docs, and the dctl
devcontainer). Those plans are marked "done", but their "done" status reflects
work in `~/.claude/skills`, `~/.dotfiles`, and `~/DocsNNotes`, which the review
(scoped to this repo) could not confirm.

This round performs a **read-only audit** of those external repos against the
claims in the corresponding plan files and records a findings report inside this
plan directory. It cannot and must not modify the external repos; where a fix is
needed there, it is recorded as a follow-up for a separate, repo-specific effort.

## Previous Rounds

Round 01 made all in-repo fixes: clamped `percent_left`, removed a stale `LRU`
comment, added `DoctorReport::all_checks()`, added a 3-account no-recycle
failover test and a multi-thread `runtime::block_on` guard test, reconciled CLI
style guide §9b, and appended a dated "Implementation Notes / Divergences"
addendum to each of the 7 affected done-plan `_README.md` files. Round 02 has no
dependency on those changes.

## Scope of This Round

- **IN scope:** read-only verification of the cross-repo claims in
  `.plan/configs-rename-split-profiles/` (round `dotfiles-propagation-and-cross-repo-sync`)
  and `.plan/prex-sandbox-fix-tmpdir-migration/` (rounds `prex-sandbox-fix` /
  `remaining-skills-dctl`), and writing a findings report
  `cross-repo-verification-report.md` into this plan directory.
- **OUT of scope:** modifying anything in `~/.claude/skills`, `~/.dotfiles`, or
  `~/DocsNNotes`; any in-repo code change.

## Current State

### Key Files (authoritative claim sources — read these first)

- `/workspaces/codex-session/.plan/configs-rename-split-profiles/dotfiles-propagation-and-cross-repo-sync.md`
  — the dotfiles/cross-repo claims for the configs-rename plan.
- `/workspaces/codex-session/.plan/prex-sandbox-fix-tmpdir-migration/prex-sandbox-fix.md`
  and `remaining-skills-dctl.md` — the prex/skills/dctl claims for the prex-sandbox plan.

### External paths to audit (read-only)

- `~/.dotfiles/` — codex-session config tree, skills, dctl devcontainer.
- `~/.claude/skills/` — prex, prex-resume, and the other migrated skills.
- `~/DocsNNotes/` — `codex-conventions.md`, `AGENTS.md`.

### Existing Patterns

- This round is read-only against external repos. Use `rg`, `ls`, `cat`,
  `git -C <path> log`. Do not edit, stage, or commit anything outside this repo.
- If a path is absent or not accessible from the execution sandbox, that is a
  **finding** ("not present / not accessible"), not a failure — degrade
  gracefully and continue.
- Markdown obeys MD040 (fenced blocks need a language) and even-number indents.

## Implementation Steps

### Step 1: Re-derive the cross-repo checklist from the plan files

Read the three authoritative plan files listed above and extract every concrete
cross-repo claim into a checklist. Expect at least:

Plan 05 round 05 (dotfiles + DocsNNotes):

- `~/.dotfiles` codex-session config tree is in the **sibling** layout:
  `configs/{base,plugins,projects}.toml` present; a sibling `profiles/`
  directory; NO `settings/` directory; NO `settings.bak.*` backup dir.
- The recipe manifest (`default.yaml` or equivalent) uses `config-layers:`,
  not `settings-layers:`.
- `config.toml` references sibling `profiles/` (not `configs/profiles/`).
- No residual `settings-layers`, `configs/profiles`, or `[profiles.*]` tables
  anywhere in the dotfiles codex-session tree.
- `~/DocsNNotes` `codex-conventions.md` references the sibling `profiles/`
  layout with no residual legacy references.

Plan 06 rounds 02–03 (prex/skills/dctl):

- prex and prex-resume `SKILL.md` use the unified
  `--dangerously-bypass-approvals-and-sandbox` approach (no sandbox-mismatch on
  resume).
- Skill run directories use
  `${XDG_STATE_HOME:-$HOME/.local/state}/claude-session/skill-runs/` — no
  hardcoded `/tmp` run dirs across the migrated skills.
- `codex-conventions.md` carries the Resume Constraint, the `--full-auto`
  deprecation, the Skill Run Directories section, and the safety matrix.
- `AGENTS.md` `last-synced` was bumped.
- The dctl `devcontainer.json` no longer bind-mounts `/tmp`.

### Step 2: Verify each claim read-only

For each checklist item, run a read-only check against the external path and
record: the exact command run, the evidence (matching/empty output, file:line),
and a verdict — `confirmed`, `diverged` (exists but differs; describe), `missing`
(claimed but absent), or `not-accessible` (path/sandbox prevents verification).
Do not guess; if you cannot see it, mark `not-accessible`.

### Step 3: Write the findings report

Create `/workspaces/codex-session/.plan/done-plans-fidelity-cleanup/cross-repo-verification-report.md`
(named distinctly from this round file `cross-repo-verification.md`) with:

- A header (`# Cross-Repo Verification — 2026-06-02`) and the audited repos.
- One table per plan (configs-rename dotfiles round; prex-sandbox skills/dctl
  rounds) with columns `Claim | Verdict | Evidence`.
- A short "Follow-ups" section listing any `diverged`/`missing` items that need a
  fix in their home repo (these are NOT fixed here).
- An honest confidence note, including which items were `not-accessible` from the
  execution sandbox.

If everything is `confirmed`, say so plainly; if not, the report must make the
gaps unmistakable rather than rounding up to "done".

### Final Step: Update the queue and close the plan

Record completion in the queue files — status lives in YAML; nothing moves on
disk:

1. In this plan's `_QUEUE.yaml`, set the `cross-repo-verification` round's
   `status` to `done`.
2. All rounds are now done, so in the top-level `.plan/_QUEUE.yaml` set this
   plan's `status` to `done`. Leave the plan directory in place.

## Acceptance Criteria

- [ ] The cross-repo checklist was re-derived from the three authoritative plan
      files (configs-rename dotfiles round; prex-sandbox skills/dctl rounds).
- [ ] Every checklist item has a verdict (`confirmed`/`diverged`/`missing`/
      `not-accessible`) backed by an explicit read-only command and evidence.
- [ ] `cross-repo-verification-report.md` exists in the plan directory with
      per-plan tables and a Follow-ups section.
- [ ] No file outside `/workspaces/codex-session` was modified.
- [ ] This plan's `_QUEUE.yaml` shows the `cross-repo-verification` round as
      `done`.
- [ ] The top-level `.plan/_QUEUE.yaml` shows this plan as `done`.

## Next Round

This is the final round.
