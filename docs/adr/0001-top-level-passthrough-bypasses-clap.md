# ADR-0001 — Top-level pass-through bypasses clap

**Status:** Accepted
**Date:** 2026-05-17

## Context

`codex-session` is an argv-transparent wrapper around the real `codex` binary.
The legacy bash wrapper only intercepted the `self` subtree and forwarded every
other token sequence verbatim, including unknown future verbs. The Rust CLI
spec assumes the CLI owns its argv surface, but this wrapper must tolerate
`codex` flag and verb drift without requiring wrapper releases.

## Decision

The top-level command path bypasses clap entirely. `main.rs` will inspect only
the first token for the literal `self`; every other argv sequence is forwarded
unchanged to the real `codex` process. The `self` subtree is reserved for
wrapper-owned behavior and is parsed independently with clap.

## Consequences

Pass-through compatibility is protected from clap parse drift. The wrapper does
not gain a top-level clap-driven help surface, but wrapper-owned `self`
subcommands now get clap-generated validation, help text, and usage errors.
This keeps the top-level wrapper/child boundary explicit while still aligning
the owned subtree with the spec's clap-first parser shape.

## Alternatives considered

- Parse the full top-level CLI with clap.
  Rejected because new codex flags or verbs could start failing in the wrapper.
- Keep the `self` subtree hand-rolled forever.
  Rejected because clap gives stable help and usage behavior without touching
  top-level pass-through.
