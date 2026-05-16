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

Use `make install` to stage the runtime under `~/.local/share/codex-session` and create `~/.local/bin/codex-session` as a symlink to the staged entrypoint.

`make install` refuses to overwrite an existing unmanaged `~/.local/bin/codex-session`. This is intentional because that path is currently provided by the dotfiles wrapper and must not be replaced implicitly.

- `make install`
- `make relink`
- `make uninstall`

`make relink` is the explicit opt-in path that backs up the existing `~/.local/bin/codex-session` to a timestamped `.bak.<ts>` name before replacing it.

## Development

- `make lint`
- `make test`
- `make smoke`

## Architecture

`bin/codex-session` is a thin entrypoint that sets strict mode, resolves the project root, and sources the libraries in `lib/`. Runtime behavior lives in small library functions, and wrapper-only verbs live in one-file-per-command modules under `commands/`.
