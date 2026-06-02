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

| Intent                                             | Use                                       | Not                           |
| -------------------------------------------------- | ----------------------------------------- | ----------------------------- |
| Run unit tests                                     | `just test-unit`                          | `cargo test --lib`            |
| Run integration tests                              | `just test-integration`                   | `cargo test --test '*'`       |
| Run all tests                                      | `just test`                               | `cargo test --workspace`      |
| Lint (fmt-check + clippy-strict + print-ownership) | `just lint`                               | `cargo clippy -- -D warnings` |
| Full gate before pushing                           | `just check`                              | hand-rolled combo             |
| Security audit / unused deps / license             | `just audit`, `just machete`, `just deny` | —                             |
| Run every pre-commit + pre-push hook (mirrors CI)  | `just precommit-all`                      | —                             |

Inner-loop recipes that are intentionally direct cargo (fast iteration, not
gates) are fine to invoke as-is: `just build`, `just run -- <args>`,
`just fmt`, `just fix`, `just watch`, `just clean`.

If you need a check the justfile doesn't cover, run raw `cargo` — but say so,
and consider whether the recipe should be extended.

## CLI Design System

User-facing CLI output is governed by
[`docs/design/cli-style-guide.md`](./docs/design/cli-style-guide.md).
Consult it before changing any `Ui::write_*` renderer, error rendering,
warning, prompt, runtime narration, or wrapper-owned command output. The
guide is the source of truth for color semantics, table layout, JSON/text
separation, and stdout/stderr ownership.

## Breaking Changes Policy

codex-session is pre-v1.0. Breaking changes are expected when they simplify
the wrapper or prevent long-term CLI ambiguity; do not add compatibility
layers for obsolete wrapper behavior. Prefer direct migrations to the
intended interface.

Wrapper-owned machine-readable output uses `--format json`, never `--json`.
Avoid introducing wrapper flags that overlap with native `codex` flags unless
the forwarding behavior is explicitly designed and documented.

## Codex config compatibility

`codex-session` is a composer, not a config dialect. The `$CODEX_HOME/` tree it
emits (under `<state>/accounts/<acct>/groups/<group>/`) MUST be byte-for-byte
structurally compatible with what upstream `codex` accepts as input. Composability
lives at the input layer (`configs/` directory + recipe manifests), never at the
output layer.

Concretely:

- If upstream codex rejects a key shape (e.g. legacy `profile = "..."` selector
  or `[profiles.*]` tables in `config.toml` since v0.134.0), the wrapper MUST
  reject it too — at both input layers (`configs/*.toml`, plus sibling
  `profiles/*.config.toml` for per-profile overrides) and emitted output.
  No compat shim, no alias, no auto-migration. (Enforcement lands in the later
  rounds of `.plan/configs-rename-split-profiles/`; the first round only
  codifies the contract.)
- Profile overrides emit as sibling files `$CODEX_HOME/<name>.config.toml` with
  bare top-level keys. Source-of-truth: [docs/upstream-codex.md](./docs/upstream-codex.md)
  §F6b.
- When upstream codex changes its config contract, update `docs/upstream-codex.md`
  first, then mirror the change in the composer.

## Upstream codex behavior reference

When answering a question or making a change that depends on how upstream
`codex` behaves (config paths, trust schema, `CODEX_HOME` semantics, write
model, prompt gating), **consult [`docs/upstream-codex.md`](./docs/upstream-codex.md)
before guessing or re-researching.** That file records the verified facts
and links to source-of-truth issues/PRs in `openai/codex`.

Keep `docs/upstream-codex.md` up to date — if you confirm or refute a fact
during a session, update the file (and bump the `Last verified` date).
