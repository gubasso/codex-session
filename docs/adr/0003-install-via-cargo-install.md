# ADR-0003 — Install via cargo install

**Status:** Accepted
**Date:** 2026-05-17

## Context

The bash project staged payload files under `~/.local/share/codex-session` and
managed a symlink at `~/.local/bin/codex-session` via `make install`, `make
relink`, and `make uninstall`. The Rust crate is a single compiled binary, and
its supported project tooling documents `cargo install --path .` as the
installation path.

## Decision

Installation is owned by Cargo. The supported install flow is
`cargo install --path .`, wrapped by `just install`. The bash staging and
relink flow is superseded and is not reimplemented in Rust.

## Consequences

The runtime no longer manages a share-directory payload or symlink takeover
logic. Users who still have an old hand-managed `~/.local/bin/codex-session`
symlink should remove it so PATH resolution is unambiguous. This preserves the
Rust binary's simpler deployment model at the cost of diverging from the bash
project's Makefile behavior.

## Alternatives considered

- Recreate the bash install/relink/uninstall flow in Rust.
  Rejected because the compiled binary does not need staged payload files.
- Keep the current README note without an ADR.
  Rejected because install behavior is a deliberate migration decision.
