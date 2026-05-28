# codex-session

## Overview

`codex-session` is a Unix-only Rust CLI wrapper around the `codex` binary.
Instead of mutating `~/.codex`, it composes wrapper-owned config-recipe layers into a
per-terminal session directory and exports `CODEX_HOME=<session-dir>` to the
wrapped child process.

## Install

```bash
cargo install --path .
```

`just install` wraps the same command.

## Usage

Run `codex-session help` for the full verb, flag, and environment variable
reference. A few common patterns:

```bash
codex-session                                        # bare → launches codex TUI
codex-session exec "hello"                           # passthrough verb
codex-session --dry-run exec "hello"                 # show what would run
codex-session account add work                       # register an account
codex-session account list --format json             # inspect accounts
codex-session --account auto --max-retries 2 exec hi # quota-aware rotation
codex-session login                                  # verify / re-authenticate
codex-session doctor                                 # full setup validation
```

Any verb not owned by the wrapper is forwarded verbatim to `codex`.

### Thread resume

Resume a previous `exec --json` session, routing to the correct account
automatically:

```bash
codex-session exec resume <SESSION_ID>       # resume by ID
codex-session exec resume --last             # most recent in current terminal group
codex-session exec resume --last --all-groups # most recent across all groups
codex-session resume --last                  # wrapper intercepts for cross-account routing
```

The wrapper resolves account and group from its thread index when
possible, then falls back to normal account resolution.

## Filesystem layout

Run `codex-session config status` or `codex-session doctor` to see resolved
paths for the current environment. The general structure:

```text
$XDG_CONFIG_HOME/codex-session/
  config.toml                           wrapper config
  config-recipes/*.yaml                 config-recipe manifests
  configs/                              composable config layers
    *.toml                              base config layers
    profiles/
      <name>.config.toml                profile overrides (one file per profile)

$XDG_CACHE_HOME/codex-session/
  configs.toml                          trust / cache-layer writes
  quota/<account>.json                  cached quota responses

$XDG_STATE_HOME/codex-session/
  state/last-account                    LRU pointer (plain text)
  thread-index.jsonl                    cross-account session resume index (JSONL)
  accounts/<account>/
    auth.json                           account seed (auth source of truth)
    cooldown.json                       failover cooldown state
    groups/<group-id>/
      auth.json                         session copy (synced back on exit)
      config.toml                       composed codex base config (no profile keys)
      <name>.config.toml                emitted per-profile sibling files
      .codex-session-compose.json       composition metadata
      session-meta.json                 session metadata
```

ConfigRecipe manifests list an ordered `config-layers:` array. Each layer is
parsed from `configs/<name>.toml`, deep-merged in order, stripped of its
optional `[env]` table, then written into the session directory as `config.toml`.
Profile overrides are emitted as sibling `<name>.config.toml` files, copied
1:1 from `configs/profiles/<name>.config.toml`. Stock mode still creates a
session directory with an empty `config.toml`.

## Composability contract

codex-session's emitted `$CODEX_HOME/` tree is byte-for-byte structurally
compatible with upstream codex's native input contract. Composability is
layered on top, never instead of:

- The emitted `config.toml` matches upstream codex's expected base config —
  it MUST contain no legacy `profile = "..."` selector and no `[profiles.*]`
  tables (rejected by codex v0.134+). The composer enforcement that
  guarantees this lands in rounds 02–03 of
  `.plan/01-todo/configs-rename-split-profiles/`; round 01 codifies the
  contract.
- Profile overrides emit as sibling `<name>.config.toml` files, selected by
  `codex --profile <name>` at invocation time.
- The wrapper never injects `--profile` for user-facing pass-through calls.
  Users pass it on the CLI and it flows to codex unchanged. Wrapper-owned
  health probes (e.g. heartbeat in `account health`) may pass `--profile ping`
  internally; see `docs/upstream-codex.md` §F6b for details.

If you want a layer to apply only when a specific profile is active, put it
under `configs/profiles/<name>.config.toml`. If you want it to apply
unconditionally, put it under `configs/<layer>.toml`.

## Multi-account management

When multiple accounts are registered, `--account auto` picks the best one
using quota-weighted scoring (see [`docs/account-auto-selector.md`](./docs/account-auto-selector.md)).
Combined with `--max-retries`, the wrapper automatically fails over to the
next account on 429 detection. See [`docs/auth-gate-spec.md`](./docs/auth-gate-spec.md)
for the full authentication model.

### Observability

```bash
codex-session account quota               # all accounts, ranked by composite score
codex-session account quota --detail       # verbose: scoring breakdown per account
codex-session account health               # live auth probe + quota + cooldown status
codex-session account health --fast        # local-only: JWT expiry + cached quota (no network)
codex-session account health --format json # structured output for scripts
```

`account quota` shows rank position and composite score for each account, sorted
by score descending. `account health` combines token validity, plan tier,
cooldown state, and eligibility into a single table. Both support `--format json`
and `--detail` for verbose text output.

## Skills

codex-session does **not** manage personal Codex skills. Codex CLI discovers
skills at these locations (all are scanned; duplicate `name`s are not merged
— both can appear in skill selectors):

- **Repo scopes.** Codex walks up from the current working directory to the
  repo root, looking for `.agents/skills/` at every level (so a project can
  pin a skill at `./.agents/skills/` and a workspace can share one further
  up the tree).
- **User scope.** `~/.agents/skills/` — recommended for personal skills.
- **Admin scope.** `/etc/codex/skills/`.
- **System scope.** Bundled with Codex itself.

Place each personal skill at `~/.agents/skills/<name>/SKILL.md`
(case-sensitive filename). Dotfiles users can stow a `.agents/` tree from
their dotfiles repo to deploy skills as symlinks under
`~/.agents/skills/`.

Reference: <https://developers.openai.com/codex/skills>

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `64` | Usage / clap parse failure |
| `65` | Data error |
| `66` | Missing required input |
| `69` | Service unavailable |
| `70` | Internal software error |
| `74` | Generic I/O failure or exec handoff failure |
| `75` | Auth failure / all accounts exhausted |
| `77` | Permission denied |
| `78` | Configuration error |
| `126` | Child resolved but is not executable |
| `127` | Child not found |

Exit codes are a stable user-facing contract (SoT: `src/error.rs`). Child
exit codes are passed through as-is.
