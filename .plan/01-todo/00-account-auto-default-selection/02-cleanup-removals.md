# Round 02: Remove `account use` and the dead config-pinned surface

> Plan: account-auto-default-selection | Round: 02 of 03 | Complexity: L Generated: 2026-05-29 |
> Repo: /workspaces/codex-session

## Context

`codex-session` wraps OpenAI's `codex` CLI and multiplexes accounts. Round 01 changed account
resolution so that no account argument auto-selects an enabled account (the quota-aware selector),
`--account <name>` pins per-invocation, and the scoring selector fires only on the positive exec
path. The legacy default-selection fallbacks were dropped from the resolver.

Two pieces of "past logic" are now orphaned and must be removed to keep the implementation lean with
no leftover:

1. **`account use <name>`** — set a persistent "current" account. In the new model there is no
   persistent resolution pin (you pin per-invocation with `--account <name>`), and the same
   `state/last-account` file is now read by the auto-selector only as a recency _penalty_ — so
   `account use X` would perversely make X _less_ likely to be picked next. The command no longer has
   coherent semantics.
2. **`config.account.pinned`** and the **`CODEX_SESSION_ACCOUNT_PINNED`** env var — a config-level
   default-selection mechanism the resolver no longer consults after Round 01. The field is dead.

No back-compat: delete both outright (the user explicitly accepted breaking existing configs).

## Previous Rounds

Round 01 delivered the core selection engine: `resolver.rs` split into `intent` /
`resolve_for_exec(exclude)` / `resolve_for_display`; `selector::pick(ctx, exclude)` with the
`set_current` side effect removed; `retry::run_auto` (no-cycling failover) + `single_attempt`;
`gate::ensure` returning `GateOutcome { Resolved | AutoDeferred }`; `AccountError::AutoExhausted`
replacing `NoneResolved`; and all display callers (`version`, `config_status`, `doctor`,
`account/list`, `account/current`, `context::get_or_resolve`) migrated to `resolve_for_display`.
After Round 01 the resolver no longer reads `config.account.pinned`, and `selector::pick` no longer
writes `set_current`; `account add` still seeds the first account as current, and the interactive
gate prompt still writes `set_current` on manual selection.

## Scope of This Round

IN scope:

- Delete the `account use` command end-to-end (CLI variant + args, dispatch arm, handler file).
- Remove `config.account.pinned` from the config struct, file layer, and `Default`.
- Remove the `CODEX_SESSION_ACCOUNT_PINNED` env parsing and its `help_extras.txt` entry.
- Remove now-unused config helpers/errors left by the above.
- Reword the `--account` help text to describe the new default.
- Refresh the root-help snapshot and any affected tests.

OUT of scope:

- Any change to resolver/selector/gate/retry/pass_through behavior (done in Round 01).
- Documentation prose/examples in the repo, `~/DocsNNotes`, `~/.dotfiles` (Round 03).

## Current State

### Key Files

- `/workspaces/codex-session/src/cli/account.rs` — defines `AccountCommand` (incl. a `Use(AccountUseArgs)`
  variant with doc text) and the `AccountUseArgs` struct. Also defines `AccountSelector` (keep) — do
  NOT remove the `Auto` variant.

- `/workspaces/codex-session/src/commands/account/mod.rs` — module list includes
  `pub(crate) mod use_;` and the dispatch arm:

  ```rust
  AccountCommand::Use(args) => use_::run(ctx, &args),
  ```

- `/workspaces/codex-session/src/commands/account/use_.rs` — the handler (calls
  `registry.set_current`). Delete this file.

- `/workspaces/codex-session/src/config/mod.rs` — `AccountConfig` with a `pinned: Option<AccountId>`
  field (and its `Default`); `FileAccountConfig` with a `pinned` field; a file-layer apply for
  `account.pinned`; an env arm matching `"ACCOUNT_PINNED"` (→ `CODEX_SESSION_ACCOUNT_PINNED`); and
  helpers `parse_account_id_field` / `parse_account_id_env` used only by those. Inline tests
  `invalid_account_pinned_in_file_layer_errors` and `invalid_account_pinned_in_env_layer_errors`.

- `/workspaces/codex-session/src/config/error.rs` — may define `ConfigError::AccountConfigParse`
  used only by the pinned parsing.

- `/workspaces/codex-session/src/ui/help_extras.txt` — documents env vars including
  `CODEX_SESSION_ACCOUNT_PINNED` (around line 37).

- `/workspaces/codex-session/tests/snapshots/cmd_root_help__root_help.snap` — root-help snapshot;
  must be regenerated after the `--account` help text changes and the `account use` removal.

### Existing Patterns

- Wrapper-owned account subcommands after this round: `add | list | current | remove | refresh |
  quota | health | cooldown` (no `use`).
- The `--account` arg lives in `GlobalArgs` (`src/cli/mod.rs`) with doc comment
  "Select the account whose `CODEX_HOME` the child uses. Use `auto` for R3+ selector." — reword.
- Snapshot tests use `insta`; regenerate via the project's accept flow (`cargo insta accept` or the
  `just` equivalent) rather than hand-editing the `.snap`.

## Implementation Steps

### Step 1: Delete the `account use` command

- In `/workspaces/codex-session/src/cli/account.rs`: remove the `Use` variant from `AccountCommand`
  and delete the `AccountUseArgs` struct. Leave `AccountSelector` (and its `Auto` variant) intact.
- In `/workspaces/codex-session/src/commands/account/mod.rs`: remove `pub(crate) mod use_;` and the
  `AccountCommand::Use(args) => use_::run(ctx, &args),` dispatch arm.
- Delete `/workspaces/codex-session/src/commands/account/use_.rs`.
- Grep for any other references to `use_`/`Use(`/`account use` in `src/` and remove/adjust (e.g.
  help text strings) so the crate compiles.

### Step 2: Remove `config.account.pinned` + `CODEX_SESSION_ACCOUNT_PINNED`

In `/workspaces/codex-session/src/config/mod.rs`:

- Delete the `pinned` field from `AccountConfig` and from its `Default` impl.
- Delete the `pinned` field from `FileAccountConfig`.
- Delete the file-layer apply that copies `account.pinned` into the resolved config.
- Delete the env arm matching `"ACCOUNT_PINNED"`.
- Delete `parse_account_id_field` and `parse_account_id_env` if they have no remaining callers
  (confirm via grep — only the pinned paths used them) and remove now-unused imports.
- Delete the inline tests `invalid_account_pinned_in_file_layer_errors` and
  `invalid_account_pinned_in_env_layer_errors`. Keep `account_config_round_trips` (it exercises
  `AccountId` serde generally) — adjust it if it referenced `pinned`.

In `/workspaces/codex-session/src/config/error.rs`:

- Remove `ConfigError::AccountConfigParse` if it is now unused (grep first).

### Step 3: Remove the env from help_extras

- In `/workspaces/codex-session/src/ui/help_extras.txt`: remove the `CODEX_SESSION_ACCOUNT_PINNED`
  line. Keep `CODEX_SESSION_ACCOUNT` (still valid; documents pin-or-`auto`).

### Step 4: Reword the `--account` help text

In `/workspaces/codex-session/src/cli/mod.rs`, change the `--account` doc comment to describe the new
model, e.g.:

```rust
/// Account to use. Omit (or `auto`) for quota-aware auto-selection with
/// failover; pass a name to pin a specific account for this invocation.
```

### Step 5: Refresh snapshots and run gates

- Regenerate `tests/snapshots/cmd_root_help__root_help.snap` (and any account-help snapshot) via the
  insta accept flow so the removed `use` subcommand and the new `--account` help text are reflected.
- Run `just lint` and `just test` until green.

### Final Step: Update plan index

Update the plan's `README.md` (same directory as this round file):

1. In the `## Execution Order` table, find the row for round 02.
2. Change `Status` from `todo` to `done`.
3. Change `Completed` from `--` to today's date (`YYYY-MM-DD`).

## Acceptance Criteria

- [ ] `codex-session account use …` is gone (unknown subcommand); `account --help` lists no `use`.
- [ ] `config.account.pinned` and `CODEX_SESSION_ACCOUNT_PINNED` are removed from the code and no
      longer parsed; a config containing `[account] pinned = …` is simply ignored (no resolution
      effect; no parse error for the removed field path).
- [ ] `CODEX_SESSION_ACCOUNT_PINNED` no longer appears in `help_extras.txt`.
- [ ] `--account` help text describes omit/`auto` ⇒ auto-selection, name ⇒ pin.
- [ ] Root-help (and account-help) snapshots regenerated; no stale `use` entry.
- [ ] `just lint` and `just test` pass.
- [ ] Plan `README.md` execution order table shows round 02 as `done` with today's date.

## Next Round

Round 03 sweeps documentation to the new API across the repo (`README.md`, `DEVELOPMENT.md`,
`docs/*`, `CLAUDE.md`, `docs/design/cli-style-guide.md`, `docs/upstream-codex.md`), `~/DocsNNotes`
(`codex-conventions.md`, `AGENTS.md`), and `~/.dotfiles` (skills `ask`, `prex`, `review-loop`,
`project-preflight`) — prioritizing the no-arg form and removing `account use` / "must pass
`--account auto`" references.
