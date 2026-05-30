# Round 03: Documentation API sweep — repo, ~/DocsNNotes, ~/.dotfiles

> Plan: account-auto-default-selection | Round: 03 of 03 | Complexity: L Generated: 2026-05-29 |
> Repo: /workspaces/codex-session

## Context

`codex-session` wraps OpenAI's `codex` CLI and multiplexes accounts. Rounds 01–02 changed the
account-selection API:

- **No account argument now means auto** (quota-aware selection with failover). `--account <name>` /
  `CODEX_SESSION_ACCOUNT=<name>` pins per-invocation. `--account auto` / `=auto` are KEPT as explicit
  aliases of the default.
- Failover rotation is on by default for auto (no cycling, stderr warnings, explanatory exhaustion
  error). The scoring selector fires only on the positive exec path.
- The `account use` command, `config.account.pinned`, and `CODEX_SESSION_ACCOUNT_PINNED` were
  removed. The `NoneResolved` error is gone.

Documentation across three locations still describes the OLD model (mandating `--account auto`,
documenting the LRU→`config.account.pinned`→`NoneResolved` chain, listing `account use`). This round
brings all prose and examples to the new API. Because `--account auto` remains valid, these edits are
non-breaking — the goal is to **prioritize the no-arg form in examples** and **stop mandating
`--account auto`**, while mentioning that `auto`/no-arg is the default and `--account <name>` pins.

## Previous Rounds

Round 01 delivered the core selection engine (auto-by-default, exec-only picking, no-cycling
failover, `AutoExhausted` errors, display callers migrated). Round 02 removed `account use`,
`config.account.pinned`, `CODEX_SESSION_ACCOUNT_PINNED`, reworded the `--account` help text, and
refreshed the root-help snapshot. The code now matches the target API; only documentation remains.

## Scope of This Round

IN scope — documentation only, in three locations:

- Repo docs: `README.md`, `DEVELOPMENT.md`, `docs/account-auto-selector.md`,
  `docs/multi-account-architecture.md`, `docs/auth-gate-spec.md`, `docs/upstream-codex.md`,
  `CLAUDE.md`, `docs/design/cli-style-guide.md`. Update `.plan/` references opportunistically.
- `~/DocsNNotes`: `tech/tools/claude-code/codex-conventions.md`, `tech/tools/claude-code/AGENTS.md`.
- `~/.dotfiles`: `claude/.claude/skills/{ask,prex,review-loop,project-preflight}/SKILL.md`.

OUT of scope:

- Any source-code change (delivered in Rounds 01–02).
- Rewriting the orchestration LOGIC of the dotfiles skills — only update the documented
  invocations/prose to the new API (the commands still function because `auto` stays valid).

## Current State

### Key Files (with the specific prose to fix)

- `/workspaces/codex-session/docs/upstream-codex.md` — already documents resume/cross-account
  constraints (F14–F16); the F16 "failure signature" mentions `--account auto` drift. Keep technical
  accuracy; reframe any "must pass `--account auto`" phrasing toward "auto is the default".

- `/home/gu/DocsNNotes/tech/tools/claude-code/codex-conventions.md` — the most important prose to
  correct. Current text:

  ```text
  **Account resolution:** `--account <name>` flag > `CODEX_SESSION_ACCOUNT` env > LRU pointer (from
  `account use`, stored in `state/last-account`) > `config.account.pinned` > `NoneResolved` error.
  There is no implicit `"default"` fallback. `--account auto` triggers the R3 quota-aware selector
  (non-interactive). … All skill and workflow invocations must pass `--account auto` so account
  selection is always quota-aware.
  ```

  Also: `**Wrapper-owned verbs:**` line lists `account add|list|current|use|remove|refresh` (drop
  `use`); the `**Argv pattern:**` block and the many example invocations (lines ~49-273, 352,
  429-430) use `codex-session --account auto exec …`.

- `/workspaces/codex-session/README.md` — line ~29 example
  `codex-session --account auto --max-retries 2 exec hi # quota-aware rotation`; the
  `## Multi-account management` section (~115-118): "When multiple accounts are registered,
  `--account auto` picks the best one … Combined with `--max-retries`, the wrapper automatically
  fails over …".

- `/workspaces/codex-session/DEVELOPMENT.md` — the "Running the binary interactively" block
  (lines ~114-117) contains `cargo run -- account use work` (line 116, a now-removed command) and
  `cargo run -- --account auto --max-retries 2 --group stable` (line 117). (The line-28
  `RUST_LOG=debug just run -- config status` example is unrelated and stays.)

- `/home/gu/DocsNNotes/tech/tools/claude-code/AGENTS.md` — line ~20 references `--account auto` for
  quota-aware selection.

- `~/.dotfiles` skills — bash invocation examples using `codex-session --account auto exec …`:
  `ask/SKILL.md` (~35, 202, 211, 222), `prex/SKILL.md` (~342, 347, 365, 493, 498, 529, 800),
  `review-loop/SKILL.md` (~22, 69-70, 78, 176, 248, 259, 269, 401, 404),
  `project-preflight/SKILL.md` (~175).

### Existing Patterns

- The new mental model to state consistently everywhere:
  - **No `--account` (or `--account auto`) ⇒ quota-aware auto-selection with failover** (the
    default).
  - **`--account <name>` ⇒ pin** that account for the invocation (no rotation).
  - `CODEX_SESSION_ACCOUNT=<name>|auto` mirrors the flag.
  - Failover is automatic for auto; `--max-retries` is an optional cap, not the on-switch.
- Examples should LEAD with the no-arg form, then mention `auto`/pin as alternatives. Do not delete
  `--account auto` examples wholesale — reframe them.
- `~/DocsNNotes` is the `$DOCS_NOTES_REPO`; edits there are first-class (consulted by other skills).

## Implementation Steps

### Step 1: Repo prose — README / DEVELOPMENT / CLAUDE

- `/workspaces/codex-session/README.md`: change the quick-start example to lead with no-arg, e.g.
  `codex-session exec hi                                # auto-selects + fails over`, and add a pin
  example `codex-session --account work exec hi          # pin a specific account`. In
  `## Multi-account management`, restate: no-arg/`auto` auto-selects with automatic failover;
  `--account <name>` pins; `--max-retries` caps failover attempts.
- `/workspaces/codex-session/DEVELOPMENT.md`: in the "Running the binary interactively" block, drop
  the `cargo run -- account use work` line (line ~116; the command is removed in Round 02) and
  simplify the `--account auto` example (line ~117) to no-arg (keep `--max-retries` only if
  illustrating the cap), e.g. `cargo run -- --group stable exec hi`.
- `/workspaces/codex-session/CLAUDE.md`: if it mandates `--account auto` anywhere, reframe to "auto
  is the default; pin with `--account <name>`".

### Step 2: Repo docs/ — selector, architecture, auth-gate, style guide, upstream

- `docs/account-auto-selector.md`: note that the selector is what runs on the default (no-arg) path,
  not only under explicit `--account auto`; keep `account quota` / `account health` references.
- `docs/multi-account-architecture.md` and `docs/auth-gate-spec.md`: replace the old resolution
  chain (LRU → `config.account.pinned` → `NoneResolved`) with the new one (flag-name / env-name =
  pin; otherwise auto; failover-by-default; `AutoExhausted` on exhaustion). Remove `account use` and
  `config.account.pinned` references.
- `docs/design/cli-style-guide.md`: document that the rotation/switch warning is a stderr warning
  (default-on, suppressed by `--quiet`/`--silent`), consistent with the warning conventions.
- `docs/upstream-codex.md`: reframe "must pass `--account auto`" phrasing to "auto is the default";
  keep F14–F16 technical content intact.

### Step 3: ~/DocsNNotes

- `/home/gu/DocsNNotes/tech/tools/claude-code/codex-conventions.md`:
  - Rewrite the **Account resolution** paragraph to: `--account <name>` flag / `CODEX_SESSION_ACCOUNT`
    name = pin; otherwise (no flag, or `auto`) = quota-aware auto-selection with automatic failover
    (no cycling); exhaustion → explanatory error. Remove the LRU/`account use`/`config.account.pinned`/
    `NoneResolved` chain.
  - In **Wrapper-owned verbs**, drop `use` from `account add|list|current|use|remove|refresh`.
  - Reframe the **Argv pattern** block and example invocations to lead with `codex-session exec …`
    (mention `--account auto` is the explicit/default form). Updating every example is optional
    cosmetic polish — at minimum fix the prose mandates ("must pass `--account auto`") and the verb
    list.
- `/home/gu/DocsNNotes/tech/tools/claude-code/AGENTS.md`: reframe the `--account auto` mandate (~line
  20) to "auto is the default".

### Step 4: ~/.dotfiles skills

For `ask`, `prex`, `review-loop`, `project-preflight` SKILL.md: the embedded
`codex-session --account auto exec …` commands still work (auto kept), so this is a non-breaking
prose/example refresh. Prefer to simplify the documented invocations to `codex-session exec …`
(dropping the now-redundant `--account auto`) and update any prose that says auto must be passed.
Keep behavior identical. If an executor prefers minimal churn, it MAY leave functional command
strings as-is and only correct prose mandates — but the recommended action is to drop the redundant
flag for cleanliness.

### Step 5: Verify

- Re-grep all three locations to confirm no stale references remain to `account use`,
  `config.account.pinned`, `CODEX_SESSION_ACCOUNT_PINNED`, `NoneResolved`, or "must pass
  `--account auto`":

  ```bash
  rg -n -e 'account use' -e 'account\.pinned' -e 'CODEX_SESSION_ACCOUNT_PINNED' \
        -e 'NoneResolved' -e 'must pass `?--account auto' \
        /workspaces/codex-session/README.md /workspaces/codex-session/DEVELOPMENT.md \
        /workspaces/codex-session/CLAUDE.md /workspaces/codex-session/docs \
        /home/gu/DocsNNotes/tech/tools/claude-code \
        /home/gu/.dotfiles/claude/.claude/skills
  ```

  Any remaining hit must be intentional (e.g. `docs/upstream-codex.md` describing historical
  behavior) and clearly framed as such.
- Repo markdown must satisfy the project's markdownlint (fenced code blocks need language
  specifiers). Run the repo's docs/lint gate if one exists (`just lint` covers Rust; for markdown use
  the pre-commit hook if configured).

### Final Step: Update plan index and complete the plan

Update the plan's `README.md` (same directory as this round file):

1. In the `## Execution Order` table, find the row for round 03.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

Because this is the final round, also:

4. In the README.md header blockquote, change `Status: todo` to `Status: done`.
5. Move the plan directory to done:

```bash
mkdir -p .plan/02-done && mv .plan/01-todo/00-account-auto-default-selection .plan/02-done/00-account-auto-default-selection
```

## Acceptance Criteria

- [ ] Repo docs (`README.md`, `DEVELOPMENT.md`, `docs/*`, `CLAUDE.md`, `cli-style-guide.md`) describe
      no-arg ⇒ auto, `--account <name>` ⇒ pin, automatic failover, and `AutoExhausted` — with no-arg
      examples leading.
- [ ] No repo/DocsNNotes/dotfiles doc mandates "must pass `--account auto`"; `account use`,
      `config.account.pinned`, `CODEX_SESSION_ACCOUNT_PINNED`, and `NoneResolved` are gone from prose
      (except intentional historical notes in `docs/upstream-codex.md`).
- [ ] `~/DocsNNotes/codex-conventions.md` resolution paragraph and verb list updated; `AGENTS.md`
      mandate reframed.
- [ ] `~/.dotfiles` skill invocations reflect the new API (functionally unchanged).
- [ ] The Step 5 grep returns only intentional hits.
- [ ] Plan `README.md` execution order table shows round 03 as `done` with today's date.
- [ ] Plan `README.md` header status is `done`.
- [ ] Plan directory moved from `.plan/01-todo/00-account-auto-default-selection` to
      `.plan/02-done/00-account-auto-default-selection`.

## Next Round

This is the final round.
