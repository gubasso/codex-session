# codex-session

`codex-session` is a bash wrapper around the real `codex` CLI. It keeps `~/.codex/config.toml` in sync with the stow-managed `~/.codex/config.base.toml` while preserving machine-local TOML sections that Codex writes at runtime, such as project trust entries.

## Behavior Contract

- Resolves `BASE` as `$HOME/.codex/config.base.toml`.
- Resolves `TARGET` as `$HOME/.codex/config.toml`.
- Resolves `CACHE_DIR` as `${XDG_CACHE_HOME:-$HOME/.cache}/codex-session`.
- Resolves `STAMP` as `$CACHE_DIR/last-merge`.
- If `BASE` is missing, skips merge logic and `exec`s the real `codex` unchanged.
- If merge is needed, rewrites `TARGET` as base config plus preserved local-only sections.
- Uses `${TARGET}.tmp.$$` plus `mv -f` for the config rewrite.
- Uses the exact missing-binary error string: `ERROR: codex binary not found in PATH`.
- Passes all non-`self` argv through verbatim to the real `codex`, including `--help`, `--version`, `exec`, `resume`, and future verbs.

## Wrapper Verbs

- `codex-session self help`
- `codex-session self version`
- `codex-session self config-status`
- `codex-session self config-merge`
- `codex-session self show-local`

Everything outside `self` is pass-through to the real `codex`.

## Install

Build and install the binary into `~/.cargo/bin` via Cargo:

- `just install` &mdash; `cargo install --path . --force`
- `just uninstall` &mdash; `cargo uninstall codex-session`

## Development

- `just check` &mdash; `cargo fmt --check` + `cargo clippy -D warnings` + `cargo nextest run`
- `just fix` &mdash; auto-apply `cargo fmt` and `cargo clippy --fix`
- `just precommit` / `just precommit-all` &mdash; run pre-commit hooks (the `-all` form also runs the pre-push stage: bats, cargo-audit, cargo-machete, gitleaks)

Run `just` with no arguments to see every recipe.

## Architecture

`bin/codex-session` is a thin entrypoint that sets strict mode, resolves the project root, and sources the libraries in `lib/`. Runtime behavior lives in small library functions, and wrapper-only verbs live in one-file-per-command modules under `commands/`.
