# ADR-0004 — Codex resolution via which crate

**Status:** Accepted
**Date:** 2026-05-17

## Context

The legacy bash wrapper resolved `codex` with `command -v codex`. The Rust port
already uses the `which` crate to locate executables on PATH. The migration goal
is to preserve observable behavior, not the shell implementation detail.

## Decision

The Rust wrapper resolves `codex` with `which::which("codex")`. This is the
supported replacement for the bash `command -v codex` implementation.

## Consequences

The observable behavior remains aligned with the bash contract: first PATH match
wins, and non-executable files are skipped. Those invariants are locked by the
existing integration tests `path_order_first_match_wins` and
`resolve_skips_non_executable_files`.

## Alternatives considered

- Reimplement shell-style PATH lookup manually.
  Rejected because `which` already provides the required semantics.
- Treat `command -v` as a source-level parity requirement.
  Rejected because only runtime behavior matters in the Rust port.
