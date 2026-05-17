# ADR-0006 — Unset HOME falls back to root

**Status:** Accepted
**Date:** 2026-05-17

## Context

The bash wrapper ran under `set -u`, so an unset `HOME` caused shell expansion
failure before normal runtime error handling. The Rust port instead resolves a
coherent fallback path and continues running, which allows the program to return
normal errors instead of panicking.

## Decision

When `HOME` is unset or empty, the wrapper falls back to `/` when constructing
its derived codex paths. This behavior is preserved and documented.

## Consequences

The wrapper is more robust than the bash script in malformed environments and
does not crash solely because `HOME` is missing. The behavior is covered by the
existing integration test `missing_home_does_not_panic`.

## Alternatives considered

- Error immediately when `HOME` is absent.
  Rejected because the current Rust behavior is already more stable and tested.
- Try to infer a user home via platform-specific APIs.
  Rejected because it would diverge from the bash path contract.
