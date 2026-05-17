# ADR-0007 — No global verbosity flag

**Status:** Accepted
**Date:** 2026-05-17

## Context

The Rust CLI spec recommends `-v/-vv/-vvv` as a standard logging surface.
`codex-session` is an argv-transparent wrapper for everything outside `self`, so
claiming top-level `-v` would collide with `codex`'s own flags or swallow future
arguments that are meant for the wrapped binary.

## Decision

The wrapper does not implement a global verbosity flag. `RUST_LOG` remains the
only supported logging control.

## Consequences

The wrapper preserves argv transparency and avoids inventing a top-level parser
surface that could break pass-through behavior. This is an intentional exception
to chapter 04 of the Rust CLI spec.

## Alternatives considered

- Add `-v/-vv/-vvv` only for `self`.
  Rejected because it would create inconsistent semantics inside the wrapper.
- Add top-level verbosity and require `--` before codex args.
  Rejected because it would break the legacy pass-through contract.
