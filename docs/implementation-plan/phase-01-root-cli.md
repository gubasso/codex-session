# Phase 01 — Root `Cli` parser + wrapper-owned `--help` / `--version`

**One-line summary.** Introduce a real root `clap::Parser` so the
wrapper owns top-level help/version and so the manual argv-bypass
machinery in `main.rs` can be deleted. After this phase, every later
phase has a normal CLI surface to extend.

## Prerequisites

None. This is the foundation of the refactor.

## Goal

After this phase:

- `src/cli/mod.rs` exposes a single root `Cli` struct
  (`#[derive(clap::Parser)]`) with a `GlobalArgs` (`#[command(flatten)]`)
  and a `Commands` enum (`#[derive(clap::Subcommand)]`).
- `codex-session --help` prints **wrapper** help (not the child's).
- `codex-session --version` prints `codex-session <wrapper-version>`
  on the first line and the resolved child path + child version on
  the second line.
- `codex-session <anything-not-a-wrapper-verb>` still forwards to the
  child via the passthrough variant.
- `codex-session -- --help` stops wrapper parsing at `--` and
  forwards `--help` to the child.
- The existing `self <verb>` invocations **still work**, dispatched
  via a temporary `Commands::Self_(SelfArgs)` variant that delegates
  to the current handlers. (They are removed in Phase 02; keeping
  them here makes this phase a pure-additive structural change.)
- `src/main.rs` is ≤120 LOC and does only:
  `parse → init logging → build AppContext → dispatch → exit-code`.
- `maybe_parse_self_cli`, `scan_self_globals`, `DeferredEarlyExit`,
  and `SelfCliParse` are deleted.

## Spec rationale

- Root `Cli`, `GlobalArgs`, `Commands` layout — `rust/cli-spec/02-subcommand-pattern.md:47-77`.
- Canonical directory tree — `rust/cli-spec/00-directory-tree.md:13-33, 48-49`.
- `main.rs` size budget ≤120 LOC — `rust/cli-spec/00-directory-tree.md:48`.
- Wrapper grammar `mywrap [WRAPPER-OPTS] <verb|positional> [--] [CHILD-ARGS...]` —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:24-45`.
- Wrapper-owned `--help` / `--version`, with `--version` printing
  wrapper + resolved child — `process-and-posix.md:248-260`.
- POSIX `--` end-of-options sentinel —
  `process-and-posix.md:32-38` (and POSIX Utility Syntax Guideline 10).

## Current state (verify before planning)

- `src/cli.rs:13-40` declares **only** `SelfCli`. There is no root
  `Cli`. `disable_help_subcommand = true` and
  `disable_version_flag = true` are set because the parser is only
  invoked as a subtree.
- `src/main.rs:32-225` does manual argv inspection:
  - `argv[0] == "self"` → parse `SelfCli`.
  - otherwise → `commands::pass_through::run(&ctx, &argv)`.
- `maybe_parse_self_cli` (`src/main.rs:117-142`), `scan_self_globals`
  (`src/main.rs:144-184`), and `DeferredEarlyExit`
  (`src/main.rs:106-115`) implement the parser-bypass logic.
- `src/ui/self_help.txt:5-35` documents the current grammar and
  explicitly says "Anything not under `self` is forwarded verbatim".
  Top-level `--help` / `--version` are not wrapper-owned today.
- `src/commands/self_*.rs` and `src/cli/self_*.rs` exist for every
  current wrapper verb. They stay in place this phase; Phase 02
  renames them.

## Target state

### `src/cli/mod.rs` (replaces `src/cli.rs`)

```rust
//! Root CLI parser.
//!
//! What this is: clap derive structs for the wrapper's command tree.
//! What this is not: business logic — that lives in `commands/`.

pub(crate) mod self_args;   // temporary; deleted in Phase 02

use clap::{ArgAction, Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(
    name = "codex-session",
    version = env!("CARGO_PKG_VERSION"),
    about = "Wrapper around `codex` with config-merge and machine-local preservation.",
    long_about = None,
    disable_help_subcommand = false,
)]
pub(crate) struct Cli {
    #[command(flatten)]
    pub(crate) global: GlobalArgs,

    #[command(subcommand)]
    pub(crate) command: Option<Commands>,

    /// Positional args after a `--` (or any non-wrapper-verb
    /// positional) — forwarded verbatim to the child binary.
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub(crate) passthrough: Vec<std::ffi::OsString>,
}

#[derive(Debug, Default, Clone, Copy, clap::Args)]
pub(crate) struct GlobalArgs {
    /// Increase wrapper log verbosity (-v info, -vv debug, -vvv trace).
    #[arg(short, long, action = ArgAction::Count, global = true)]
    pub(crate) verbose: u8,

    /// Mirror wrapper logs to stderr in addition to the log file.
    #[arg(long, global = true)]
    pub(crate) log_stderr: bool,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// TEMPORARY — `self <verb>` namespace. Removed in Phase 02.
    #[command(name = "self")]
    Self_(self_args::SelfArgs),
}

#[derive(Debug, Clone, Copy, Default, Eq, PartialEq, ValueEnum)]
pub(crate) enum OutputFormat {
    #[default]
    Text,
    Json,
}
```

The temporary `Commands::Self_` variant keeps every current
`codex-session self <verb>` invocation working until Phase 02 lifts
the verbs to top level. Move the existing `SelfCommand` enum +
its leaf modules under `src/cli/self_args.rs` unchanged — Phase 02
will then delete that file entirely.

### `src/main.rs` shape (≤120 LOC)

```rust
fn main() -> ExitCode {
    let cli = Cli::parse();                      // clap owns help/version
    let global = cli.global;
    let paths = domain::paths::CodexPaths::from_env();
    let mirror_stderr = global.log_stderr || global.verbose > 0;
    let _log = match logging::init(global.verbose, &paths.log_file, mirror_stderr) {
        Ok(handle) => handle,
        Err(err) => return print_and_exit(&AppError::Other(anyhow::anyhow!(err))),
    };
    let ctx = context::AppContext::new(paths);
    let result = dispatch(&ctx, cli);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => print_and_exit(&e),
    }
}

fn dispatch(ctx: &context::AppContext, cli: cli::Cli) -> Result<(), error::AppError> {
    match cli.command {
        // Wrapper-owned verbs are dispatched here. In phase 01 the
        // only one is the temporary `Self_` variant.
        Some(cli::Commands::Self_(args)) => dispatch_self(ctx, args),
        // No verb (or only positional args) → passthrough.
        None => commands::pass_through::run(ctx, &cli.passthrough),
    }
}
```

The `print_and_exit` and `from_process_error` helpers stay (Phase 08
moves them into `src/error.rs`).

## Tasks

1. **Convert `src/cli.rs` to `src/cli/mod.rs`.** Move the file
    (Rust supports either form; `mod.rs` is what the canonical tree
    prescribes). Make sure `cargo check` passes after the move alone
    (no behavior change yet).

2. **Add the root `Cli` struct above.** Use `clap_derive`'s
    defaults — do NOT set `disable_help_subcommand` or
    `disable_version_flag` on `Cli`. Use
    `version = env!("CARGO_PKG_VERSION")` so `clap` derives a real
    `--version`.

3. **Add the `Commands::Self_(SelfArgs)` temporary variant.** Move
    the existing `SelfCli`, `SelfGlobalArgs`, `SelfCommand` types
    into a new `src/cli/self_args.rs` (renamed from the embedded
    types in the old `cli.rs`). The struct that flattens into
    `Commands::Self_` should accept the same subcommands the old
    `SelfCli::command` field did. Keep the existing module names
    `self_config_merge.rs`, `self_config_status.rs`, `self_help.rs`,
    `self_show_local.rs`, `self_version.rs` for this phase. The
    `--format` flag must continue to work.

4. **Implement passthrough via `trailing_var_arg + allow_hyphen_values`.**
    When `cli.command` is `None`, forward `cli.passthrough` to
    `commands::pass_through::run`. Verify
    `codex-session foo bar`, `codex-session exec something`, and
    `codex-session -- --help` all forward correctly.

5. **Rewrite `src/main.rs`.** Reduce to the shape above. Strip
    `maybe_parse_self_cli`, `scan_self_globals`, `DeferredEarlyExit`,
    `SelfCliParse`, and the deferred-help machinery. Keep the existing
    `print_and_exit` and the `impl AppError { fn from_process_error
    }` block; they're orthogonal to this phase.

6. **Handle `clap`'s early-exit (help/version) cleanly.** `Cli::parse()`
    already terminates the process on `--help` / `--version` /
    parse errors; we no longer need to defer those. Verify no
    tracing call fires before `logging::init` runs in the normal
    path. If clap's help fires, the process exits before logging is
    installed — that is OK.

7. **Wire up the wrapper-owned `--version` body.** The default
    `clap`-derived `--version` prints only the crate version. The
    spec requires a second line with the resolved child path and
    child version. Two options:
    - (Preferred) Define a custom `--version` action via
      `#[arg(long, action = clap::ArgAction::Version)]` and replace
      the default `version` derive — but this is fiddly with clap
      derive. Simpler:
    - Set `version = env!("CARGO_PKG_VERSION")` for clap-driven
      `--version`, AND keep the existing `commands::self_version::run`
      handler (renamed in Phase 02 to `commands::version::run`).
      Users who want the dual-line output use `codex-session version`
      (the subcommand) which Phase 02 promotes to top level.

    Phase 01 ships with single-line clap `--version`; Phase 02 wires
    the dual-line `version` subcommand to top level. This is the only
    knowing compromise in this phase and is called out in the
    acceptance criteria.

8. **Update `src/ui/self_help.txt`.** Drop the line "Anything not
    under `self` is forwarded verbatim". Replace with a short note
    that wrapper-owned verbs (currently still under `self`) sit
    alongside passthrough, and document `--`. Keep this file's name
    (`self_help.txt`) for this phase; Phase 02 renames it.

9. **Update existing tests** so they pass through the new root
    parser. `tests/cmd_self_*.rs` tests that invoke
    `codex-session self version` etc. should still work because the
    `Self_` variant preserves the old grammar. Add at most one new
    test asserting `codex-session --help` prints non-empty stdout
    and exits 0.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` — every existing test still passes (no regressions).
- [ ] `src/cli/mod.rs` exists; `src/cli.rs` is gone.
- [ ] `src/main.rs` ≤ 120 LOC (`wc -l src/main.rs`).
- [ ] `maybe_parse_self_cli`, `scan_self_globals`, `DeferredEarlyExit`,
  `SelfCliParse` removed (`rg 'maybe_parse_self_cli|scan_self_globals|DeferredEarlyExit|SelfCliParse' src/` returns nothing).
- [ ] `codex-session --help` prints wrapper help on stdout, exit 0
  (manual check: build, run, eyeball).
- [ ] `codex-session --version` prints `codex-session 0.1.0` (or
  current crate version) and exits 0. Dual-line output (wrapper +
  child) is **not** required this phase.
- [ ] `codex-session self version --format json` continues to print
  the JSON shape it printed before (regression check via the
  existing `tests/cmd_self_version.rs` snapshot).
- [ ] `codex-session foo bar` forwards to the child (test via stub
  child if necessary; otherwise eyeball with `CODEX_SESSION_CHILD_BIN`).
- [ ] `codex-session -- --help` forwards `--help` to the child
  (regression check).

## Tests

- Keep all existing `tests/cmd_self_*.rs` green. They are deleted in
  Phase 02; do not touch them yet.
- Add `tests/cmd_root_help.rs`:
  - Asserts `codex-session --help` exit 0 and stdout starts with
    `codex-session`.
  - Asserts `codex-session --version` exit 0 and stdout matches
    `^codex-session \d+\.\d+\.\d+`.
- (Optional) Add `tests/cmd_passthrough_double_dash.rs` asserting
  that `codex-session -- --help` invokes the child with `--help` as
  argv[1].

## Out of scope

- Renaming any `self_*` modules, structs, tests, or help text.
  (Phase 02.)
- Introducing `clap_complete` or a `completion` subcommand.
  (Phase 10.)
- Restructuring `src/commands/pass_through.rs` (the current
  implementation continues to work; Phase 04 layers `ChildInvocation`
  on top).
- Touching `src/logging.rs`, `src/error.rs`, `src/domain/paths.rs`
  beyond what the new `main.rs` shape requires.
- Adding new dependencies. (`Cargo.toml` is unchanged in this phase.)

## References

- `rust/cli-spec/00-directory-tree.md` — `src/main.rs` budget, `cli/mod.rs` layout.
- `rust/cli-spec/02-subcommand-pattern.md:47-77` — root `Cli` shape with `clap`.
- `cli-design/00-architecture.md` — `main` orchestration, single `AppContext`.
- `cli-design/06-cli-wrapper-design/process-and-posix.md:24-45, 248-260` — argv layout, `--`, wrapper-owned help/version.
- clap derive docs on `trailing_var_arg`, `allow_hyphen_values`, `external_subcommand`: <https://docs.rs/clap/latest/clap/_derive/index.html>.
