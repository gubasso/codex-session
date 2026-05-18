# ADR-0004 — Codex resolution via which crate

**Status:** Accepted
**Date:** 2026-05-17

## Context

The legacy bash wrapper resolved `codex` with `command -v codex`. The Rust port
already uses the `which` crate to locate executables on PATH. The migration goal
is to preserve observable behavior, not the shell implementation detail.

## Decision

The Rust wrapper first honors `CODEX_SESSION_CHILD_BIN` when it is set and
non-empty. When no explicit override is present, it resolves `codex` with
`which::which("codex")`. This remains the supported replacement for the bash
`command -v codex` implementation.

## Consequences

The observable behavior remains aligned with the bash contract for PATH-based
lookup: first PATH match wins, and non-executable files are skipped. The wrapper
also gains an explicit child override for CI, debugging, and deterministic test
setups. These invariants are locked by integration tests for PATH ordering,
skipping non-executable PATH entries, and the override precedence.

## Alternatives considered

- Reimplement shell-style PATH lookup manually.
  Rejected because `which` already provides the required semantics.
- Treat `command -v` as a source-level parity requirement.
  Rejected because only runtime behavior matters in the Rust port.
