# codex-session

## Overview

`codex-session` is a Unix-only Rust CLI wrapper around the `codex` binary.
Instead of mutating `~/.codex`, it composes wrapper-owned profile layers into a
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

## Filesystem layout

Run `codex-session config status` or `codex-session doctor` to see resolved
paths for the current environment. The general structure:

```text
$XDG_CONFIG_HOME/codex-session/
  config.toml                           wrapper config
  profiles/*.yaml                       profile manifests
  settings/*.toml                       settings layers

$XDG_CACHE_HOME/codex-session/
  settings.toml                         trust / cache-layer writes
  quota/<account>.json                  cached quota responses

$XDG_STATE_HOME/codex-session/
  state/last-account                    LRU pointer (plain text)
  accounts/<account>/
    auth.json                           account seed (auth source of truth)
    cooldown.json                       failover cooldown state
    groups/<group-id>/
      auth.json                         session copy (synced back on exit)
      config.toml                       composed codex config
      .codex-session-compose.json       composition metadata
      session-meta.json                 session metadata
```

Profile manifests list ordered `settings-layers`. Each layer is parsed from
`settings/<name>.toml`, deep-merged in order, stripped of its optional `[env]`
table, then written into the session directory. Stock mode still creates a
session directory with an empty `config.toml`.

## Multi-account management

When multiple accounts are registered, `--account auto` picks the best one
using quota-weighted scoring (see [`docs/account-auto-selector.md`](./docs/account-auto-selector.md)).
Combined with `--max-retries`, the wrapper automatically fails over to the
next account on 429 detection. See [`docs/auth-gate-spec.md`](./docs/auth-gate-spec.md)
for the full authentication model.

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
| `75` | All accounts exhausted (failover) |
| `77` | Permission denied |
| `78` | Configuration error |
| `126` | Child resolved but is not executable |
| `127` | Child not found |

Exit codes are a stable user-facing contract (SoT: `src/error.rs`). Child
exit codes are passed through as-is.
