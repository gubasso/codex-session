# codex-session

`codex-session` is a Rust CLI wrapper around the real `codex` binary. It keeps `~/.codex/config.toml` in sync with the stow-managed `~/.codex/config.base.toml` while preserving machine-local TOML sections that Codex writes at runtime, such as project trust entries.

## Behavior Contract

- Resolves `BASE` as `$HOME/.codex/config.base.toml`.
- Resolves `TARGET` as `$HOME/.codex/config.toml`.
- Resolves `CACHE_DIR` as `${XDG_CACHE_HOME:-$HOME/.cache}/codex-session`.
- Resolves `STAMP` as `$CACHE_DIR/last-merge`.
- If `BASE` is missing, skips merge logic and `exec`s the real `codex` unchanged.
- If merge is needed, rewrites `TARGET` as base config plus preserved local-only sections.
- Uses a same-directory temporary file plus atomic rename for config rewrites.
- Passes all non-`self` argv through verbatim to the real `codex`, including `--help`, `--version`, `exec`, `resume`, and future verbs.

## `self` Verbs

- `codex-session self help`
- `codex-session self version [--format text|json]`
- `codex-session self config-status [--format text|json]`
- `codex-session self config-merge`
- `codex-session self show-local [--format text|json]`

Everything outside `self` is pass-through to the real `codex`. Wrapper-owned flags live only under `self` so the top-level argv contract stays transparent.

## Install

Install or reinstall the binary into `~/.cargo/bin`:

```bash
cargo install --path . --force
```

`just install` wraps the same command.

This deliberately differs from the legacy bash project's `make install` /
`make relink` / `make uninstall` flow. Cargo owns `~/.cargo/bin/codex-session`,
so there is no managed-versus-unmanaged symlink ambiguity to protect. If you
still have a hand-managed `~/.local/bin/codex-session` symlink from the bash
wrapper, remove it before installing the Rust binary so `PATH` resolution is
unambiguous.

## Design decisions

The migration-specific design decisions for this wrapper are recorded under
[`docs/adr/`](docs/adr/):

- [ADR-0001: top-level pass-through bypasses clap](docs/adr/0001-top-level-passthrough-bypasses-clap.md)
- [ADR-0003: install via cargo install](docs/adr/0003-install-via-cargo-install.md)
- [ADR-0004: codex resolution via which crate](docs/adr/0004-codex-resolution-via-which-crate.md)
- [ADR-0005: atomic write via named tempfile](docs/adr/0005-atomic-write-via-named-tempfile.md)
- [ADR-0006: unset HOME falls back to root](docs/adr/0006-unset-home-falls-back-to-root.md)
- [ADR-0008: skip directories and figment](docs/adr/0008-skip-directories-and-figment.md)
- [ADR-0009: foo.rs over mod.rs](docs/adr/0009-foo-dot-rs-over-mod-rs.md)
- [ADR-0010: paths stay as PathBuf](docs/adr/0010-paths-stay-as-pathbuf.md)
- [ADR-0011: wrapper env vars and exit codes](docs/adr/0011-wrapper-env-vars-and-exit-codes.md)
- [ADR-0012: two-layer logging and JSON output](docs/adr/0012-two-layer-logging-and-json-output.md)

## Environment

- `HOME` is used to resolve `~/.codex/config.base.toml` and `~/.codex/config.toml`.
- `XDG_CACHE_HOME` overrides the cache root for `codex-session/last-merge`.
- `PATH` is used to resolve the real `codex` binary when `CODEX_SESSION_CHILD_BIN` is unset.
- `RUST_LOG` overrides the wrapper's default verbosity mapping.

Wrapper-specific environment variables:

- `CODEX_SESSION_CHILD_BIN` points at the wrapped `codex` binary explicitly.
- `CODEX_SESSION_LOG_FILE` sets the full path for the program log file.
- `CODEX_SESSION_LOG_DIR` sets only the log directory; the file name stays `codex-session.log`.

## Logging & Diagnostics

`codex-session` always writes structured JSON logs to a file. The default path is:

- `${XDG_STATE_HOME}/codex-session/codex-session.log`
- or `${HOME}/.local/state/codex-session/codex-session.log`
- or a degraded fallback under `/tmp/codex-session/codex-session.log` when neither state root is usable

Wrapper-owned verbosity is exposed only under `self`:

- `codex-session self -v ...`
- `codex-session self -vv ...`
- `codex-session self -vvv ...`
- `codex-session self --log-stderr ...`

`-v` levels widen the wrapper log filter, `--log-stderr` mirrors wrapper logs to stderr for interactive debugging, and `RUST_LOG` still takes precedence over the default verbosity-derived filter.

## Exit Codes

`codex-session` uses BSD `sysexits` plus the standard shell child-resolution codes:

- `0` success
- `64` usage / clap parse failure
- `66` missing input such as a forced merge with no base config
- `70` internal software error
- `74` generic I/O failure or failed `exec` handoff
- `77` permission denied
- `126` child resolved but is not executable
- `127` child not found

## Machine-readable Output

These wrapper-owned read paths support `--format json`:

- `codex-session self version --format json`
- `codex-session self config-status --format json`
- `codex-session self show-local --format json`

The structured program log is JSON-per-line and includes stable fields such as `op`, `status`, `err.kind`, `err.msg`, and `bin.resolved`. Stdout remains reserved for command results; wrapper diagnostics go to the log file and optionally to stderr when stderr mirroring is enabled.

## Unix-only

`codex-session` is Unix-only. It relies on Unix `exec` replacement semantics via `std::os::unix::process::CommandExt::exec()`.

## Development

- `just check` runs `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, `just lint-print`, and `cargo nextest run`.
- `just fix` runs `cargo fmt --all` and `cargo clippy --fix --allow-dirty --allow-staged --all-features -- -W clippy::all`.
- `just precommit` and `just precommit-all` run the configured pre-commit hooks.
