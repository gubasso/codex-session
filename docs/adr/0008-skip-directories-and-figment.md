# ADR-0008 — Skip directories and figment

**Status:** Accepted
**Date:** 2026-05-17

## Context

The Rust CLI spec recommends `directories` for XDG path resolution and `figment`
for layered config loading. `codex-session` does not have a layered user config
model: its core contract is a bash-compatible merge between
`~/.codex/config.base.toml` and `~/.codex/config.toml`, with explicit
`${XDG_CACHE_HOME:-$HOME/.cache}` cache resolution and the documented unset-HOME
fallback from ADR-0006.

## Decision

The wrapper keeps its hand-rolled path resolution and does not adopt
`directories` or `figment`.

## Consequences

The code stays aligned with the legacy bash contract, especially for XDG cache
resolution and the unset-HOME edge case. This is an intentional exception to the
spec's default config/path guidance.

## Alternatives considered

- Adopt `directories` for path resolution only.
  Rejected because it would change the unset-HOME behavior and add churn.
- Adopt `figment` for a config model the wrapper does not need.
  Rejected because there is no layered runtime config surface to merge.
