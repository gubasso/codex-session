# Phase 12 — Profile architecture: wrapper-owned layers + per-terminal sessions

**One-line summary.** Replace the legacy merge-to-`~/.codex` model with a
wrapper-owned profile/layer/session-dir architecture that composes TOML layers
into per-terminal `CODEX_HOME` directories.

## Prerequisites

Phases 01–11 must be complete. In particular:

- Phase 03 established the layered wrapper config loader.
- Phase 04/05 established typed child invocation, recursion guard, and env
  scrubbing.
- Phase 09 locked in the snapshot/integration-test conventions this phase
  extends.

## Goal

After this phase:

- `src/services/profile/` owns manifest parsing, TOML layer parsing,
  deep-merge, `[env]` extraction, and compose-sidecar writing.
- `src/services/session/` owns terminal-id derivation, secure runtime/state
  session-root selection, session-dir creation, and `session-meta.json`.
- Wrapper config no longer contains `paths.base_config`, `paths.target_config`,
  or `stamp_file()`.
- The root CLI has a wrapper-owned global `--profile <NAME>` plus a new
  `profile list|show|compose` subtree.
- Pass-through always resolves a per-terminal session dir, writes
  `config.toml`, `.codex-session-compose.json`, and `session-meta.json`, then
  exports `CODEX_HOME=<session-dir>` into the child env.
- `src/services/merge.rs`, `src/commands/config_merge.rs`,
  `src/commands/config_show_local.rs`, and `src/domain/config_merge.rs` are
  gone.

## Spec Rationale

- Phase 03 established the repo’s layered wrapper config conventions and XDG
  path handling:
  [`phase-03-config-layer.md`](phase-03-config-layer.md).
- This phase ports the reviewed `claude-session` profile/session model from the
  external task brief into the Rust tree while preserving the repo’s existing
  typed config, command, and testing patterns.

## Current State

Before this phase the repo merged a wrapper-managed base config into the child
tool’s native config tree and tracked freshness with a cache stamp. The
reviewed task explicitly replaces that behavior with wrapper-owned profiles and
per-terminal session directories.

## Target State

- Wrapper config lives under `~/.config/codex-session/` and selects profiles
  from `profiles/*.yaml` plus `settings/*.toml`.
- Session roots resolve under `$XDG_RUNTIME_DIR/codex-session/` first, then
  `$XDG_STATE_HOME/codex-session/`, enforcing owner-match, non-symlink, and
  `0700` permissions.
- Profile `[env]` tables are validated, stripped from the child-facing TOML,
  recorded in the compose sidecar, and exported into the child env only when
  they do not collide with wrapper-private `CODEX_SESSION_*` keys.

## Tasks

1. Add YAML/TOML dependencies required for manifest parsing and ordered TOML
    merge (`serde_yaml_ng` fallback, `toml` with `preserve_order`, runtime
    `libc` promotion if safe to use).
2. Introduce `src/services/profile/` and `src/services/session/`.
3. Extend `ConfigError` and `error.rs` to surface profile/session failures with
    stable machine-readable `kind()` strings.
4. Rework `Config` / `PathsConfig` / `ProfileConfig` and active-profile
    resolution.
5. Add wrapper-owned `--profile` plus `profile list|show|compose`.
6. Rewrite `config status` and pass-through around session-dir composition.
7. Delete merge-era modules and tests.
8. Add composition/session/profile integration coverage and update snapshots.

## Tests

- `cargo test`
- `tests/profile_composition.rs`
- `tests/profile_errors.rs`
- `tests/cmd_profile_list.rs`
- `tests/cmd_profile_show.rs`
- `tests/cmd_profile_compose.rs`
- `tests/cmd_passthrough_env.rs`
- rewritten `tests/cmd_config_status.rs`, `tests/cmd_config_precedence.rs`,
  `tests/cmd_dry_run.rs`, `tests/cmd_passthrough.rs`

## Acceptance Criteria

- `cargo test`, `just check`, `just test`, and
  `cargo clippy --all-targets -- -D warnings` pass.
- Pass-through writes session artifacts and exports `CODEX_HOME`.
- `profile list|show|compose` are implemented and covered by snapshots.
- Merge-era modules and tests are removed.

## Out Of Scope

- Secret helpers
- hook execution
- file sync/symlink machinery
- auto-trust or post-exit behavior
