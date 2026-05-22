# Round 5 — Reintegrate Claude skills with the new `codex-session` wrapper API

This file is the **prex input** for Round 5. Pass its contents verbatim to `/prex -ar` after the
refactor (R1 → R4) has fully landed.

---

## Prerequisite

Rounds 1, 2, 3, 4 have shipped. The wrapper now exposes:

- Stable, persistent `CODEX_HOME` at `<state>/accounts/<account>/groups/<group-id>/` (R1).
- Top-level `account add|list|current|use|remove` subcommand + `--account <name>` global flag (R2).
- `--account auto` proactive quota-aware selection (R3).
- Reactive 429 failover, per-account cooldown, AuthBridge fully demoted to one-shot importer with
  no native write-back required (R4).

At least one account is registered (`codex-session account add default`) and has a valid
`auth.json` under `<state>/accounts/default/auth.json`. The native `~/.codex/auth.json` is no
longer the source of truth for the wrapper's workflows.

## Goal

Reverse the temporary stock-codex mode that was applied to the user's Claude skills while the
refactor was in flight, and switch every skill back to calling `codex-session exec` (with the new
account-aware API). Drop the per-call `-m <model> -c model_reasoning_effort="medium"` pins in
favor of profile composition where possible, otherwise keep them as explicit flags but route them
through the wrapper.

This round is **strictly a dotfiles change**. No code changes in `codex-session` itself.

## Background (read these before planning)

- `.plan/multi-account-refactor/00-overview.md` — north star, success criteria.
- `.plan/multi-account-refactor/01-architecture.md` — final wrapper layout, account/group axes,
  AuthBridge state after R4.
- `.plan/multi-account-refactor/05-cli-design.md` — final CLI surface, including `--account` and
  any new global flags introduced by R2/R3.
- `.plan/multi-account-refactor/10-phase-foundation.md` — describes the temporary banner that R5
  removes (the "Temporary stock-codex mode" note in the user's `codex-conventions.md`).
- `~/.dotfiles/claude/.claude/skills/prex/references/codex-conventions.md` — the canonical skill
  reference that currently documents stock-codex mode; R5 rewrites this back to the wrapper form.

## Numbered implementation steps

1. **Rewrite `~/.dotfiles/claude/.claude/skills/prex/references/codex-conventions.md`.**
    - Remove the "Temporary stock-codex mode" banner at the top of the file.
    - Restore the **Wrapper: `codex-session`** section. Update its contents to reflect the
      post-R4 wrapper:
      - Per-call lifecycle: pick or create
        `<state>/accounts/<account>/groups/<group-id>/`, NOT
        `$XDG_RUNTIME_DIR/codex-session/sessions/<terminal-id>/`.
        Group-id resolution follows the 5-step chain from R1 (flag → env → TTY → PPID+starttime
        → warned `pid-N`); document the warning explicitly.
      - Account resolution: `--account <name>` global flag, `$CODEX_SESSION_ACCOUNT` env, or the
        default from `account use`. Document `--account auto` and that it is non-interactive.
      - `auth.json` lives per-account at `<state>/accounts/<account>/auth.json` and is owned by
        the wrapper; codex writes refresh rotations in place. No write-back to `~/.codex/`.
      - Wrapper-owned verbs: `version`, `completion`, `config status`,
        `profile list|show|compose`, `doctor`, plus the new `account` verb tree from R2 and the
        new `account cooldown` verb tree from R4.
      - Wrapper-owned global flags: existing list plus `--account` (from R2).
    - Update **Version Baseline** to the post-R4 wrapper version (whatever ships from R4) and bump
      `Last verified` to the date R5 is implemented.
    - Replace every `codex exec` snippet in the document with `codex-session exec`. Drop the
      per-call `-m gpt-5.4 -c model_reasoning_effort="medium"` flags **only if** a profile is
      established to pin model + effort (see step 2); otherwise leave them inline and call them out.
    - Drop the "Trade-offs vs. `codex-session`" section — it is no longer relevant once the
      wrapper is back in use.
    - Restore the **Wrapper Exit Codes** table (sysexits-aligned).

2. **Decide on profile vs. inline pinning for model + reasoning effort.**
    - Option A (preferred if R2's profile design supports it): create
      `~/.config/codex-session/profiles/skills.yaml` that composes a `settings/<layer>.toml` with
      `model = "gpt-5.4"` and `model_reasoning_effort = "medium"`. Every skill invocation then
      uses `codex-session --profile skills exec ...` and drops the `-m`/`-c model_reasoning_effort`
      flags entirely.
    - Option B (fallback if profile composition does not cover these keys cleanly): keep
      `-m gpt-5.4 -c model_reasoning_effort="medium"` inline at each call site, exactly as the
      stock-codex mode does today, but on top of `codex-session exec`.
    - Pick one and apply it consistently across all skills below. Document the chosen path in the
      conventions file.

3. **Update `~/.dotfiles/claude/.claude/skills/prex/SKILL.md`.**
    - Replace the "Temporary stock-codex mode" wording in the **Inputs** section with the original
      wrapper-based wording (the install requirement is `codex-session` on `PATH`).
    - Replace every `codex exec ...` and `codex exec resume ...` call with the wrapper form
      chosen in step 2 (e.g. `codex-session --profile skills exec ...` or
      `codex-session exec -m gpt-5.4 -c model_reasoning_effort="medium" ...`).
    - Update the sandbox-detection probe identically.
    - Restore the **Guardrails** bullet that requires `codex-session exec`, never bare
      `codex exec`. Word it to match the post-R4 wrapper rationale (per-account isolation,
      profile composition, account-aware failover) rather than the pre-R1 reasons.
    - Remove the link to `14-phase-skills-reintegration.md` once R5 is complete — replace it with
      a one-line history note in a `<!-- -->` comment at the bottom of the file documenting the
      migration date.

4. **Update `~/.dotfiles/claude/.claude/skills/ask/SKILL.md`.**
    - Same treatment as step 3, for both `Phase A` (sandbox probe) and `Phase B` (native +
      fallback codex calls).
    - The `--codex` flag description should be rewritten to reference `codex-session exec` again.

5. **Update `~/.dotfiles/claude/.claude/skills/prex-resume/SKILL.md`.**
    - Same treatment as step 3, with extra attention to the `description:` frontmatter line, which
      currently mentions "stock `codex exec resume` calls".
    - Restore the sandbox probe and the two `codex-session exec resume` snippets.

6. **Update `~/.dotfiles/claude/.claude/skills/plan-exec/SKILL.md`.**
    - Same treatment as step 3, including the **Guardrails** bullet that names the verb.
    - The "fresh `codex exec` (not resume)" wording in the **Stage 3** and **Stage 4** sections
      gets renamed back to "fresh `codex-session exec` (not resume)".

7. **Update `~/.dotfiles/claude/.claude/skills/review-loop/SKILL.md`.**
    - Restore the round-1 and round-2+ wrapper snippets.
    - Restore the guardrail lines that name `codex-session exec` and forbid
      `codex-session review --uncommitted`.

8. **Smoke-test each skill against the new wrapper.**
    - Hand-run `/ask -c "ping"` in a Claude Code session. Verify the codex side-call lands under
      `<state>/accounts/default/groups/<group-id>/` and does NOT touch `~/.codex/`.
    - Hand-run a single-stage `/plan-exec` against a trivial throwaway plan. Verify session dir,
      auth file, and rollout location.
    - Hand-run a two-round `/review-loop` (round 1 fresh, round 2 resume). Verify resume succeeds.
    - For each smoke test, run `codex-session doctor` afterwards and confirm the reported
      `account`, `group-id`, and `group-id-source` match expectations (no `pid-N` fallback in a
      TTY-attached terminal).

9. **Remove the temporary phase from the in-flight plan.**
    - In `codex-session/.plan/multi-account-refactor/`, archive
      `14-phase-skills-reintegration.md` (this file) by moving it to
      `99-execution-plan.md` as a completed-phases note, OR leave it in place but prepend a
      `> COMPLETED <date>` banner. Pick whichever matches the project's house convention from R4.
    - Update `99-execution-plan.md` to mark this round as the final round of the refactor.

## Files touched (representative)

- `~/.dotfiles/claude/.claude/skills/prex/references/codex-conventions.md` (REWRITE — drop the
  banner, restore the wrapper section, refresh version baseline)
- `~/.dotfiles/claude/.claude/skills/prex/SKILL.md` (update inputs section, snippets, guardrails)
- `~/.dotfiles/claude/.claude/skills/ask/SKILL.md` (Phase A/B snippets, `--codex` flag description)
- `~/.dotfiles/claude/.claude/skills/prex-resume/SKILL.md` (frontmatter description, snippets)
- `~/.dotfiles/claude/.claude/skills/plan-exec/SKILL.md` (snippets, stage 4 wording, guardrails)
- `~/.dotfiles/claude/.claude/skills/review-loop/SKILL.md` (snippets, guardrails)
- `~/.config/codex-session/profiles/skills.yaml` (NEW, if option A in step 2 is chosen)
- `~/.config/codex-session/settings/<layer>.toml` (NEW or extended, if option A)
- `codex-session/.plan/multi-account-refactor/99-execution-plan.md` (mark R5 complete)
- `codex-session/.plan/multi-account-refactor/14-phase-skills-reintegration.md` (this file —
  archived per step 9)

**Net dotfiles diff estimate:** ~200–300 lines changed (mostly s/`codex exec`/`codex-session exec`/,
plus banner removal and section restoration). **Net codex-session repo diff:** ~10 lines (plan
status updates only).

## Done criteria

```sh
# 1. Sandbox probe still succeeds under the wrapper.
codex-session exec --sandbox read-only --json "echo probe" > /tmp/r5-probe.jsonl
jq -r 'select(.type == "thread.started") | .thread_id' /tmp/r5-probe.jsonl

# 2. Resume across invocations works (R1 success criterion, re-validated).
codex-session exec --json "first" > /tmp/r5-a.jsonl
THREAD=$(jq -r 'select(.type=="thread.started") | .thread_id' /tmp/r5-a.jsonl | head -1)
codex-session exec resume "$THREAD" --json "second"

# 3. doctor reports the expected layout.
codex-session doctor | grep -E '^(account|group-id|group-id-source|codex_home):'

# 4. No stock-codex artifacts remain in the skills.
! grep -rn '^[[:space:]]*codex exec' ~/.dotfiles/claude/.claude/skills/ \
    | grep -v 'codex-session exec'
```

Plus the smoke tests in step 8 above.

## Out of scope for Round 5

- Any further changes to `codex-session` source code. R5 is dotfiles-only.
- Adding new skills or new account/profile features. R5 only restores parity.
- Migrating the user's interactive `~/.codex/config.toml` defaults. The user's bare-terminal
  `codex` invocations stay independent of the wrapper.

## Constraints / project conventions (must follow)

- One change per skill file; do not bundle unrelated dotfiles edits into the same commit.
- Preserve the existing markdown formatting (line widths, fenced-code-block languages, table
  alignment) — these dotfiles have an editorconfig that enforces even-numbered indents and a
  120-char soft wrap.
- The user's dotfiles live in a separate git repo (`~/.dotfiles/`); commit those changes there with
  conventional-commit prefixes (`feat(claude/skills): …` or `fix(claude/skills): …`) and push
  separately from any `codex-session` PR.
- After R5 lands, re-run `git log --oneline -3` in `~/.dotfiles/` and capture the commit hash in
  the archived phase note for future cross-reference.
