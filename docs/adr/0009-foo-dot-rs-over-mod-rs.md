# ADR-0009 — foo.rs over mod.rs

**Status:** Accepted
**Date:** 2026-05-17

## Context

The upstream Rust CLI spec has an internal conflict: chapter 00's tree diagram
shows `src/cli/mod.rs` and `src/commands/mod.rs`, while chapter 06 explicitly
mandates the post-2018 `foo.rs + foo/` form over `mod.rs`. This repository
already uses `src/cli.rs`, `src/commands.rs`, `src/domain.rs`, `src/services.rs`,
and `src/adapters.rs`.

## Decision

This repository treats chapter 06 as authoritative and keeps the post-2018
`foo.rs + foo/` module form.

## Consequences

No churn is introduced solely to match chapter 00's diagram. The repository
records the upstream spec conflict locally and keeps the layout that is already
consistent with the stronger naming-and-visibility rule.

## Alternatives considered

- Rename module roots to `mod.rs` everywhere.
  Rejected because it would add noise without improving the crate.
- Ignore the conflict without documenting it.
  Rejected because future reviewers would keep rediscovering the mismatch.
