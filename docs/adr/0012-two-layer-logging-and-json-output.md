# ADR-0012 — Two-layer logging and JSON output

**Status:** Accepted
**Date:** 2026-05-18

## Context

The CLI design specs require stdout to stay reserved for command results, a
machine-greppable program log, and a machine-readable mode for wrapper-owned
read commands. `codex-session` is also an argv-transparent wrapper, so any
logging surface must avoid claiming new top-level flags that could collide with
future `codex` arguments.

## Decision

`codex-session` now uses a two-layer logging model:

- a structured JSON file sink written on every run
- an optional human-readable stderr mirror enabled only for wrapper-owned `self`
  invocations via `-v/-vv/-vvv` and `--log-stderr`

The log file resolves by this precedence order:

1. `CODEX_SESSION_LOG_FILE`
2. `CODEX_SESSION_LOG_DIR/codex-session.log`
3. `${XDG_STATE_HOME}/codex-session/codex-session.log`
4. `${HOME}/.local/state/codex-session/codex-session.log`
5. degraded fallback under `/tmp/codex-session/codex-session.log`

Wrapper-owned read commands also expose `--format json`:

- `self version`
- `self config-status`
- `self show-local`

## Consequences

Wrapper diagnostics are deterministic and machine-readable without polluting
stdout. The wrapper still preserves top-level argv transparency because the
verbosity controls live only under `self` and in `CODEX_SESSION_*` environment
variables.

## Alternatives considered

- Top-level `-v` or `--log-stderr`.
  Rejected because top-level argv must remain pass-through to the child.
- Human-readable logs only.
  Rejected because the spec requires machine-greppable records for agent and CI
  use.
- Add `directories` or `figment`.
  Rejected because ADR-0008 keeps path/config resolution hand-rolled for this
  wrapper's small fixed contract.
