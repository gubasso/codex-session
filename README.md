# codex-session

## Overview

`codex-session` is a Unix-only Rust CLI wrapper around the `codex` binary.
Instead of mutating `~/.codex`, it composes wrapper-owned profile layers into a
per-terminal session directory and exports `CODEX_HOME=<session-dir>` to the
wrapped child process.

## Specs

Canonical source-of-truth documents live outside this repo:

- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/`
- `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/`

Repo-local docs should point to those trees rather than restating them.

## Install

```bash
cargo install --path .
```

`just install` wraps the same command.

## Usage

Wrapper-owned verbs:

- `codex-session version [--format text|json]`
- `codex-session completion <shell>`
- `codex-session config status [--format text|json]`
- `codex-session profile list [--format text|json]`
- `codex-session profile show [NAME] [--format text|json]`
- `codex-session profile compose [NAME]`

Wrapper-owned global flags:

- `--profile <NAME>` selects the wrapper profile before pass-through begins.
- `--dry-run` prints the resolved child invocation, including `CODEX_HOME`.

Anything else is forwarded verbatim to the real `codex`.

## Profile Layout

Wrapper config lives under XDG paths:

```text
$XDG_CONFIG_HOME/codex-session/
  config.toml
  profiles/*.yaml
  settings/*.toml

$XDG_CACHE_HOME/codex-session/settings.toml

$XDG_RUNTIME_DIR/codex-session/sessions/<terminal-id>/
  config.toml
  .codex-session-compose.json
  session-meta.json
```

Profile manifests list ordered `settings-layers`. Each layer is parsed from
`settings/<name>.toml`, deep-merged in order, stripped of its optional `[env]`
table, then written into the session directory. Stock mode still creates a
session directory with an empty `config.toml`.

## Environment

- `CODEX_SESSION_CHILD_BIN`: explicit path to the wrapped `codex` binary.
- `CODEX_SESSION_PROFILE`: active wrapper profile when CLI `--profile` is absent.
- `CODEX_SESSION_LOG_FILE`: directory hint for wrapper log rotation.
- `CODEX_SESSION_LOG_DIR`: legacy directory hint when `LOG_FILE` is unset.
- `CODEX_SESSION_REENTRY`: wrapper-set recursion guard.
- `NO_COLOR`, `FORCE_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `RUST_LOG`.

Wrapper-private `CODEX_SESSION_*` variables are scrubbed from the child
environment. Profile `[env]` tables may not reintroduce them.

## Exit Codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `64` | Usage / clap parse failure |
| `66` | Missing required input |
| `70` | Internal software error |
| `74` | Generic I/O failure or exec handoff failure |
| `77` | Permission denied |
| `78` | Configuration error |
| `126` | Child resolved but is not executable |
| `127` | Child not found |

## Implementation Plan

Implementation-phase tracking for this repo lives in
[`docs/implementation-plan/README.md`](docs/implementation-plan/README.md).
