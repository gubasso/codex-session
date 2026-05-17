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
unchanged to the real `codex` process. The `self` subtree is dispatched by a
hand-rolled matcher that reproduces bash semantics exactly.

## Consequences

Pass-through compatibility is protected from clap parse drift. The wrapper does
not gain a top-level clap-driven help surface, and the `self` subtree keeps
legacy semantics such as `self --help` being an unknown verb. This is a
deliberate exception to the spec's clap-first parser shape.

## Alternatives considered

- Parse the full top-level CLI with clap.
  Rejected because new codex flags or verbs could start failing in the wrapper.
- Keep clap inside `dispatch_self`.
  Rejected because it diverged from bash for `self --help` and trailing args.
