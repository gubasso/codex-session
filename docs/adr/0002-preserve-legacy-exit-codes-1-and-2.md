# ADR-0002 — Preserve legacy exit codes 1 and 2

**Status:** Superseded by ADR-0011
**Date:** 2026-05-17

> Superseded on 2026-05-18 by [ADR-0011](0011-wrapper-env-vars-and-exit-codes.md).
> Kept for history only.

## Context

The Rust CLI spec prefers BSD `sysexits`, but the legacy bash wrapper exposed
two compatibility-sensitive exit codes: `1` for missing codex / missing base
config, and `2` for unknown `self` verbs. Downstream scripts may already depend
on those exact values.

## Decision

`AppError::CodexNotFound` and `AppError::BaseMissing` remain mapped to exit code
`1`. `AppError::UnknownSelfVerb` remains mapped to exit code `2`. All other
error arms use BSD `sysexits` mappings where appropriate.

## Consequences

The wrapper keeps its established shell-facing API for the legacy cases while
still adopting typed `AppError` mappings for the rest of the crate. This is an
intentional compatibility carve-out from the spec's preference to avoid `1` and
`2`.

## Alternatives considered

- Migrate all cases to `sysexits`.
  Rejected because it would be a breaking change for downstream scripts.
- Preserve code `1` only.
  Rejected because the unknown-verb behavior is part of the wrapper contract.
