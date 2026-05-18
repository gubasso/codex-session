# ADR-0011 — Wrapper env vars and exit codes

**Status:** Accepted
**Date:** 2026-05-18

## Context

`codex-session` preserves verbatim top-level pass-through to the real `codex`
binary, so wrapper-owned controls must live either under the reserved `self`
subtree or in a wrapper-specific environment namespace. The reviewed
implementation plan also replaces the legacy bash-compatible exit codes `1` and
`2` with the CLI-spec contract of BSD `sysexits` plus wrapper-standard `126`
and `127` for child resolution failures.

## Decision

The wrapper reserves the `CODEX_SESSION_*` environment namespace and recognizes
these variables:

- `CODEX_SESSION_CHILD_BIN` — explicit path to the wrapped `codex` binary.
- `CODEX_SESSION_LOG_FILE` — full path override for the program log file.
- `CODEX_SESSION_LOG_DIR` — directory override for the program log file, which
  stays named `codex-session.log`.

The wrapper exit-code contract is:

- `0` — success
- `64` — usage / clap parse failures
- `66` — missing input such as a forced-merge base config
- `70` — internal software error
- `74` — generic I/O or exec handoff failure
- `77` — permission denied
- `126` — child resolved but is not executable
- `127` — child not found

## Consequences

The wrapper now follows the CLI design spec's stable, machine-parseable
exit-code rules instead of preserving the earlier bash-specific `1` / `2`
contract. Wrapper-owned controls remain out of the top-level pass-through argv
surface, which keeps the wrapper future-proof against new upstream `codex`
flags.

## Alternatives considered

- Keep the legacy `1` / `2` exit codes from ADR-0002.
  Rejected because the reviewed plan explicitly standardizes on BSD sysexits and
  shell-convention `126` / `127`.
- Use a shorter env namespace such as `CS_*`.
  Rejected because the wrapper spec calls for a single explicit app namespace
  and `CODEX_SESSION_*` is unambiguous.
