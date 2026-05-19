# ADR-0005 — Atomic write via named tempfile

**Status:** Accepted
**Date:** 2026-05-17

## Context

The bash wrapper rewrote `config.toml` using `${TARGET}.tmp.$$` followed by
`mv -f`. The Rust port uses `tempfile::Builder::permissions(...).tempfile_in`
and `persist(target)` instead so the temporary file still lands in the target
directory while honoring the caller's umask. The user-facing requirement is
atomic replacement in the target directory without leaving temp-file debris on
success.

## Decision

Config rewrites use a same-directory `NamedTempFile` and an atomic persist into
the target path. The exact bash temp-file naming convention is not preserved.

## Consequences

The observable guarantees remain intact: temp files are created in the target
directory, the final replacement is atomic, and successful merges do not leave
behind `*.tmp.*` artifacts. The integration test
`merge_replaces_target_atomically_with_no_intermediate_state` locks the
no-leftover invariant.

## Alternatives considered

- Emulate `${TARGET}.tmp.$$` exactly.
  Rejected because `tempfile` gives safer lifecycle management.
- Write directly to the target file.
  Rejected because it would lose atomic replacement semantics.
