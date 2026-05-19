# codex-session

## Overview

`codex-session` is a Unix-only Rust CLI wrapper around the `codex` binary.
It keeps `~/.codex/config.toml` in sync with the stow-managed
`~/.codex/config.base.toml` while preserving machine-local sections that Codex
writes at runtime.

## Specs

Canonical source-of-truth documents live outside this repo:

- `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/`
- `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/`

Repo-local docs should point to those trees rather than restating them.

## Install

```bash
cargo install --path .
```

`just install` wraps the same command. This replaces the legacy bash
project's staged install and relink flow.

## Usage

Top-level wrapper verbs:

- `codex-session help`
- `codex-session version [--format text|json]`
- `codex-session completion <shell>`
- `codex-session config status [--format text|json]`
- `codex-session config merge`
- `codex-session config show-local [--format text|json]`

Passthrough behavior:

- Any unrecognized top-level verb is forwarded verbatim to the real `codex`
  binary.
- `codex-session -- <codex args...>` forces pass-through of `-`-prefixed child
  arguments.
- `codex-session --dry-run <codex args...>` prints the resolved child
  invocation without executing it.

## Environment

- `CODEX_SESSION_CHILD_BIN`: explicit path to the wrapped `codex` binary.
- `CODEX_SESSION_LOG_FILE`: directory hint for the wrapper log file rotation.
- `CODEX_SESSION_LOG_DIR`: directory hint used when `CODEX_SESSION_LOG_FILE` is
  unset.
- `CODEX_SESSION_REENTRY`: wrapper-set marker used to detect recursion.
- `NO_COLOR`: disable ANSI colors.
- `FORCE_COLOR`: force ANSI colors.
- `RUST_LOG`: override the wrapper's default tracing filter.

## Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `64` | Usage / clap parse failure |
| `66` | Missing required input such as the base config for forced merge |
| `70` | Internal software error |
| `74` | Generic I/O failure or exec handoff failure |
| `77` | Permission denied |
| `78` | Configuration error |
| `126` | Child resolved but is not executable |
| `127` | Child not found |

See `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/06-cli-wrapper-design/process-and-posix.md`
for the canonical process and exit-code guidance.

## Configuration

Configuration follows XDG layering plus explicit environment and CLI overrides.
See
`/home/gu/Projects/docs-n-notes/tech/programming/cli-design/03-config-precedence.md`
for the canonical precedence rules.

## Implementation plan

Implementation-phase tracking for this repo lives in
[`docs/implementation-plan/README.md`](docs/implementation-plan/README.md).
