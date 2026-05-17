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
- Uses the exact missing-binary error string: `ERROR: codex binary not found in PATH`.
- Passes all non-`self` argv through verbatim to the real `codex`, including `--help`, `--version`, `exec`, `resume`, and future verbs.

## `self` Verbs

- `codex-session self help`
- `codex-session self version`
- `codex-session self config-status`
- `codex-session self config-merge`
- `codex-session self show-local`

Everything outside `self` is pass-through to the real `codex`.

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
- [ADR-0002: preserve legacy exit codes 1 and 2](docs/adr/0002-preserve-legacy-exit-codes-1-and-2.md)
- [ADR-0003: install via cargo install](docs/adr/0003-install-via-cargo-install.md)
- [ADR-0004: codex resolution via which crate](docs/adr/0004-codex-resolution-via-which-crate.md)
- [ADR-0005: atomic write via named tempfile](docs/adr/0005-atomic-write-via-named-tempfile.md)
- [ADR-0006: unset HOME falls back to root](docs/adr/0006-unset-home-falls-back-to-root.md)
- [ADR-0007: no global verbosity flag](docs/adr/0007-no-global-verbosity-flag.md)
- [ADR-0008: skip directories and figment](docs/adr/0008-skip-directories-and-figment.md)
- [ADR-0009: foo.rs over mod.rs](docs/adr/0009-foo-dot-rs-over-mod-rs.md)
- [ADR-0010: paths stay as PathBuf](docs/adr/0010-paths-stay-as-pathbuf.md)

## Environment

- `HOME` is used to resolve `~/.codex/config.base.toml` and `~/.codex/config.toml`.
- `XDG_CACHE_HOME` overrides the cache root for `codex-session/last-merge`.
- `PATH` must contain the real `codex` binary for pass-through mode.
- `RUST_LOG` controls wrapper logging. Logging is silent by default; set
  `RUST_LOG=info` or `RUST_LOG=trace` to inspect merge and pass-through
  decisions.

Example:

```bash
RUST_LOG=info cargo run -- self config-status
```

## Unix-only

`codex-session` is Unix-only. It relies on Unix `exec` replacement semantics via `std::os::unix::process::CommandExt::exec()`.

## Development

- `just check` runs `cargo fmt --check`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo nextest run`.
- `just fix` runs `cargo fmt --all` and `cargo clippy --fix --allow-dirty --allow-staged --all-features -- -W clippy::all`.
- `just precommit` and `just precommit-all` run the configured pre-commit hooks.
