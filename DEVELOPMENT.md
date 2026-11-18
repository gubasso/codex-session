# Development Guide

Workflow guide for hacking on `codex-session`: setup, common tasks,
quality gates, repository layout, and the rules contributors are
expected to follow.

## TL;DR

Every command below runs in the devshell: in a shell that direnv
activated, or with `direnv exec . CMD` or `nix develop --command CMD`.

```bash
# One-time setup
git clone <repo> && cd codex-session
direnv allow                   # load the devshell of flake.nix
prek install --prepare-hooks
cargo build

# Inner loop
just fix                       # auto-format + clippy autofix
just test                      # nextest
just check                     # lint + tests (must pass before pushing)
cargo nextest run <pattern>    # filter tests
cargo insta review             # review snapshot diffs

# Run the binary interactively (smoke test / poke at it)
just run -- help                            # wrapper's own help
just run -- version --format json           # JSON output
just run -- config status                   # built-in subcommand
just run -- --dry-run foo bar               # show resolved child cmd, don't exec
cargo run --release -- <codex args>         # passthrough → real codex REPL
RUST_LOG=debug just run -- config status    # verbose tracing

# Commit & push (main/master/develop are protected; use a branch)
git checkout -b feat/my-change
git add -p && git commit -m "feat(cli): ..."
git push -u origin HEAD        # pre-push: integration tests, gitleaks, deny, nix build

# Build the package that nix-secrets installs on the host
nix build .#default
```

## Prerequisites

- [Nix](https://nixos.org/download/) with flakes enabled.
- [direnv](https://direnv.net/) with
  [nix-direnv](https://github.com/nix-community/nix-direnv).

`flake.nix` defines the devshell, and `.envrc` loads it. The devshell
holds the Rust **stable** toolchain from `rust-toolchain.toml` (MSRV
`1.89`, edition 2024), with `rustfmt` and `clippy`. `flake.lock` fixes
its version, so a local run and CI get the same compiler. Each
dependency has one owner, the most specific file that declares it
natively: `Cargo.toml` for the crate, then `.pre-commit-config.yaml`,
then the devshell, then the host. The devshell also holds
`just`, the tools of the hooks that keep `language: system` (`taplo`,
`dprint`, `lychee`, `cargo-nextest`, `cargo-deny`, `nixfmt`, `statix`), the
tools that recipes and tests call (`cargo-watch`, `cargo-insta`,
`python3`), and `nodejs`, on which the markdownlint-cli2
hook builds. Each system hook names its owner and class in an
`# owner:` comment. Run `just --list` to see the recipes.

The hook runner prek installs and manages the tools of the hooks whose
language gets an environment that it builds: `gitleaks`, `ripsecrets`,
`markdownlint-cli2`, `typos`, `committed`, `md-toc`,
`editorconfig-checker`, `cargo-machete`, and `deadnix`. The host supplies the tools
that [host-requirements.toml](./host-requirements.toml) lists, and
`direnv`, `bash`, and the coreutils. When a project tool is missing, add
it to its owner, never to the host.

### Compile cache and dev profile

`.envrc` sets `RUSTC_WRAPPER=kache`, so each rustc call goes through
[kache](https://github.com/kunobi-ninja/kache), the compile cache of the
host. The host configuration owns the one kache version. CI enters the
devshell without direnv, so CI builds without kache. To run one command
without kache, prefix it with `KACHE_DISABLED=1`, for example
`KACHE_DISABLED=1 cargo build`.

The dev profile in `Cargo.toml` keeps line tables for this crate and no
debug information for the dependencies. A backtrace keeps its files and
lines, but a debugger loses the variable values. If you need a debugger,
build with full debug information for one command:

```bash
cargo test --config 'profile.dev.debug="full"' --config 'profile.dev.package."*".debug="full"'
```

The command rebuilds every crate.

## First-time setup

```bash
git clone <repo> && cd codex-session
direnv allow                          # load the devshell of flake.nix
prek install --prepare-hooks          # pre-commit, commit-msg, pre-push
cargo build                           # warm the toolchain + deps
just check                            # confirm lint + tests pass
```

Outside a shell that direnv activated, run each command with
`direnv exec . CMD` or `nix develop --command CMD`, for example
`direnv exec . prek install --prepare-hooks`.

`prek install` registers hooks for the `pre-commit`, `commit-msg`, and
`pre-push` stages, so commits and pushes are gated locally. Commit and
push in the devshell, for example `direnv exec . git commit`: the hooks
find their tools on the `PATH` of the commit.

## Daily loop

| Goal                        | Command                                             |
| --------------------------- | --------------------------------------------------- |
| Build (debug)               | `cargo build`                                       |
| Build (release)             | `just build`                                        |
| Run with args               | `just run -- <args>` (e.g. `just run -- self help`) |
| Watch & re-test on change   | `just watch`                                        |
| Format                      | `just fmt`                                          |
| Auto-fix fmt + clippy       | `just fix`                                          |
| Local quality gates         | `just lint`                                         |
| Tests (nextest)             | `just test`                                         |
| Full pre-push check         | `just check`                                        |
| Pre-commit hooks (manual)   | `just precommit`                                    |
| Pre-commit + pre-push hooks | `just precommit-all`                                |
| Build the Nix package       | `nix build .#default`                               |
| Supply-chain audit          | `just deny` / `just machete`                        |

`just check` (= `lint` + `test`) is the fast gate to run before
pushing. The pre-push hook and CI run the full gate, both hook stages,
which `just precommit-all` runs too.

## CI

`.github/workflows/ci.yml` repeats the two hook stages of
`.pre-commit-config.yaml` in the devshell, with
`nix develop --command prek run`. CI uses the toolchain and the clippy
of `flake.lock`, and holds no check of its own, so a push that passed
its hooks passes CI. The job installs the host tools that the hooks
need: Nix, prek, and the `exobrain` CLI of the pinned exobrain-tech tag.

CI skips two hooks:

- `no-commit-to-branch`: the hook guards a local commit. CI makes no
  commit, and a push to `develop`, which a merge makes, checks out
  `develop`.
- `cargo-nextest-live`: the live tests call real endpoints with the
  OAuth credentials of a local account. CI has no such account.

The other lists of CI jobs are empty:

- CI-only checks: none.
- Report-only jobs: none.
- Scheduled jobs: none.

## Running the binary interactively

`codex-session` is not itself a REPL — it's a wrapper that either
handles one of its own subcommands or passes through to the real
`codex` binary (which provides the interactive session). Bare
invocation (no subcommand) is a pass-through with empty child argv,
so `codex-session` on its own launches the Codex TUI. To smoke-test
the wrapper from a source checkout:

```bash
# Wrapper's own verbs (handled in-process)
cargo run -- help
cargo run -- version
cargo run -- version --format json
cargo run -- config status

# Show what would be exec'd, without actually launching codex
cargo run -- --dry-run some passthrough args
cargo run -- --dry-run -- --flag-starting-with-dash
cargo run -- account add work
cargo run -- account list --format json
cargo run -- --group stable exec hi   # no --account => quota-aware auto-select
cargo run -- --max-retries 2 exec hi  # cap auto failover attempts

# Real interactive codex session via the wrapper (release build is snappier)
cargo run --release -- <codex args>

# Bare invocation: same as `codex` with no args (launches the TUI)
cargo run --release --

# Or through the just recipe, from the working tree
just run -- <codex args>
```

No recipe installs the binary. On the operator's hosts, nix-secrets
installs `codex-session` from `nix/package.nix` through Home Manager, so
a debug build stays in `target/` and never shadows the host copy.

Useful env knobs while poking at it:

```bash
RUST_LOG=debug cargo run -- config status     # verbose tracing on stderr
NO_COLOR=1 cargo run -- help                  # disable ANSI
CODEX_SESSION_CHILD_BIN=/path/to/codex \
    cargo run -- --dry-run foo                # point at a specific codex binary
```

## Quality gates

`just lint` runs three gates:

1. `cargo fmt --check` — formatting must be clean.
2. `cargo clippy --all-targets --all-features -- -D warnings` — clippy
   warnings are denied. The crate also enables the `pedantic` and
   `nursery` groups (see `[lints.clippy]` in `Cargo.toml`).
3. `just lint-print` — runs the `lint-print` hook, which enforces the
   **stdout/stderr ownership rule** (see below) at every commit.

Additional crate-level lints worth knowing:

- `unsafe_code = "forbid"`
- `missing_docs = "warn"` (public items need doc comments)
- `unwrap_used` / `expect_used` / `panic` are warned

### Stdout/stderr ownership rule

Only the `src/ui/` module is allowed to write to the terminal. The
`lint-print` hook of `.pre-commit-config.yaml`, which `just lint-print`
runs, fails the commit if it finds `println!`,
`print!`, `eprint!`, `eprintln!`, or direct `stdout()` / `stderr()`
handles anywhere in `src/` outside the small set of exempt modules
(`src/ui/`, `src/error.rs`, `src/logging.rs`).

Rationale:

- **stdout** carries program output meant for piping/parsing (data,
  JSON, completion scripts).
- **stderr** carries diagnostics meant for humans (logs, errors, help
  text written on failure).
- Mixing the two breaks scripting use cases and makes output
  contract-testing impossible.

If you need to surface information from deep in the code, return a
typed value or emit a `tracing` event; the `ui` layer decides how to
present it.

### Logging vs output

- User-facing output → goes through `src/ui/` (formatted text or JSON).
- Diagnostic logging → `tracing` macros (`info!`, `warn!`, `error!`,
  …). `src/logging.rs` configures a two-layer subscriber: a stderr
  layer for the user, plus a rotating file layer when
  `CODEX_SESSION_LOG_FILE` / `CODEX_SESSION_LOG_DIR` is set.
- `RUST_LOG` controls the filter; `NO_COLOR` / `FORCE_COLOR` control
  ANSI output (precedence: `NO_COLOR` > `FORCE_COLOR` > isatty).

## Error handling

The crate layers `thiserror` (typed, library-style errors) under
`anyhow` (application-level error chain). When adding errors:

- Define a typed variant in `src/error.rs` (or a domain-specific
  error enum) instead of returning a free-form `anyhow!` string.
- Each variant maps to a stable exit code. The exit code table lives
  in `README.md` and is part of the user-facing contract — do not
  change it without a deliberate version bump.
- Use `anyhow::Context` at boundaries (e.g. when crossing into
  `commands/`) to attach human-readable context without losing the
  typed root cause.

## Configuration

Configuration follows a layered precedence:

```text
CLI flags > environment variables > project config > user config > defaults
```

Implemented with [`figment`](https://docs.rs/figment) plus
[`directories`](https://docs.rs/directories) for XDG paths. All
config structs use `#[serde(deny_unknown_fields)]` so typos surface
as errors instead of being silently ignored.

All `CODEX_SESSION_*` environment variables are handled in
`src/config/mod.rs::apply_env_layer()`. Run `codex-session help` for the
most common ones.

## Testing

Tests live under `tests/` (integration) and use:

- [`assert_cmd`](https://docs.rs/assert_cmd) — spawn the CLI binary.
- [`insta`](https://insta.rs/) (with the `json` feature) — snapshot
  testing. Snapshots are committed under `tests/snapshots/`.
- [`predicates`](https://docs.rs/predicates), `tempfile`, `libc` —
  assertions, sandboxing, signal tests.

Useful commands:

```bash
just test                              # full suite via nextest
cargo nextest run <pattern>            # filter by test name
cargo test --test cmd_dispatch         # run a single integration file
INSTA_UPDATE=always cargo nextest run  # refresh snapshots
cargo insta review                     # interactively review snapshots
```

When changing user-visible output (help text, error messages, JSON
shapes), expect snapshot churn and review the diff carefully — these
snapshots **are** the contract for output.

Test conventions:

- One integration file per surface area (`cmd_<area>.rs`).
- Fixtures live in `tests/fixtures/`; shared helpers in `tests/support/`.
- Snapshot any output that's part of the user contract (help text,
  errors, JSON payloads, completion scripts).

## Repository layout

```text
src/
  main.rs       # entry point
  cli/          # parse-shape: clap definitions, argv handling, exit codes
  commands/     # runtime-shape: dispatch + per-verb command modules
  config/       # XDG layering via figment (cli > env > project > user > defaults)
  domain/       # plain data types shared across layers
  services/     # business logic (merge, child invocation, account resolver/retry/failover, …)
  adapters/     # IO boundaries (filesystem, process spawn, env)
  ui/           # the ONLY place allowed to write to stdout/stderr
  context.rs    # AppContext — wires services + config + IO together
  error.rs      # typed error layering (thiserror + anyhow)
  logging.rs    # tracing-subscriber + tracing-appender setup
tests/          # integration tests + snapshots + fixtures + helpers
docs/           # in-repo reference docs (e.g. upstream codex behavior)
```

See [`docs/upstream-codex.md`](./docs/upstream-codex.md) for verified
behavior of the wrapped `codex` binary — config paths, trust schema,
`CODEX_HOME` semantics, write model. Read it before making changes that
depend on how upstream codex behaves.

Two architectural patterns to keep in mind:

- **Parse-shape vs runtime-shape** — `src/cli/` only deals with
  parsing argv (clap types, validation). It converts parsed input into
  the typed value passed to `commands/`. No business logic in `cli/`,
  no clap types past `commands/`.
- **AppContext** — services receive an `AppContext` rather than
  reaching for globals (filesystem, env, clock, …). This is what makes
  the integration tests deterministic.

## Commits and branches

- Conventional Commits are enforced by the `committed` hook on
  `commit-msg`.
- Direct commits to `main`, `master`, and `develop` are blocked by
  `no-commit-to-branch`. Work on a feature branch and open a PR.
- Run `just check` before pushing; `pre-push` additionally runs
  `gitleaks`, `cargo-machete`, `cargo-deny`, and the integration tests.

## Troubleshooting

- **`just lint-print` fails** — a `println!` / `eprintln!` / `stdout()`
  call escaped into a non-`ui` module. Move it into `src/ui/` or route
  it through `tracing`.
- **Snapshot mismatches** — inspect with `cargo insta review`. If the
  new output is correct, accept; otherwise revert the code change.
- **`pre-push` slow** — `cargo-deny` fetches the advisory DB. Run
  `just deny` ahead of time to warm the cache.
- **Clippy denies a lint you disagree with** — prefer fixing the code.
  `#[allow(...)]` attributes need a one-line justification and should
  be local (function- or block-scoped), never crate-wide.
