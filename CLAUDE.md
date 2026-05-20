# codex-session — agent guide

## Quality gates: prefer `just` recipes over raw `cargo`

This project's quality gates are defined in [`justfile`](./justfile), which is a
thin wrapper over the pre-commit hooks in `.pre-commit-config.yaml` (the source
of truth). The recipes encode project-specific settings the agents don't see
otherwise — most notably `cargo-nextest` profiles tuned for coding-agent output,
`clippy-strict`, and a local stdout/stderr-ownership lint.

**Prefer the recipes below over running raw `cargo` for verification.** Raw
`cargo test` / `cargo clippy` / `cargo build --release` will appear to work but
skip the project's configuration and produce different output than CI.

| Intent | Use | Not |
|---|---|---|
| Run unit tests | `just test-unit` | `cargo test --lib` |
| Run integration tests | `just test-integration` | `cargo test --test '*'` |
| Run all tests | `just test` | `cargo test --workspace` |
| Lint (fmt-check + clippy-strict + print-ownership) | `just lint` | `cargo clippy -- -D warnings` |
| Full gate before pushing | `just check` | hand-rolled combo |
| Security audit / unused deps / license | `just audit`, `just machete`, `just deny` | — |
| Run every pre-commit + pre-push hook (mirrors CI) | `just precommit-all` | — |

Inner-loop recipes that are intentionally direct cargo (fast iteration, not
gates) are fine to invoke as-is: `just build`, `just run -- <args>`,
`just fmt`, `just fix`, `just watch`, `just clean`.

If you need a check the justfile doesn't cover, run raw `cargo` — but say so,
and consider whether the recipe should be extended.
