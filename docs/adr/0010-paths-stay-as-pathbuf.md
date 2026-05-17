# ADR-0010 — Paths stay as PathBuf

**Status:** Accepted
**Date:** 2026-05-17

## Context

The Rust CLI spec encourages newtypes for domain primitives, and `camino` can
make UTF-8 path handling more explicit. In this wrapper, `CodexPaths` is a small
value object holding four derived filesystem paths that are never mutated or
round-tripped through user-visible string parsing.

## Decision

`CodexPaths` continues to store its fields as `PathBuf`. No `camino` dependency
or path newtype layer is added.

## Consequences

The code stays smaller and avoids a new dependency for marginal benefit. The
existing `CodexPaths` type already centralizes path derivation without exposing
raw string manipulation throughout the codebase.

## Alternatives considered

- Introduce UTF-8 path newtypes via `camino`.
  Rejected because the wrapper does not need the extra abstraction today.
- Wrap each path in a bespoke domain newtype.
  Rejected because the current value object is already sufficiently constrained.
