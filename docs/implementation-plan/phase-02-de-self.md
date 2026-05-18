# Phase 02 — De-`self`: lift wrapper verbs to top level + introduce `config` subtree

**One-line summary.** Remove the `self` namespace from the public CLI
surface. Promote `version`, `help`, and config-related verbs to the
top level. Introduce a `config` subtree
(`status` / `merge` / `show-local`). Rename every `self_*` source
module, struct, test, and help-text reference.

## Prerequisites

Phase 01 must be complete. There must already be a real root `Cli` in
`src/cli/mod.rs` with the temporary `Commands::Self_(SelfArgs)`
variant.

## Goal

After this phase:

- The CLI surface is:

  ```
  codex-session                                  # → wrapper help
  codex-session --help                           # → wrapper help
  codex-session --version                        # → wrapper + child version (dual-line)
  codex-session version [--format text|json]    # same dual-line output
  codex-session help                             # → wrapper help (alias for --help)
  codex-session config status [--format text|json]
  codex-session config merge
  codex-session config show-local [--format text|json]
  codex-session <anything else>                  # → passthrough to codex
  codex-session -- [anything]                    # → passthrough, POSIX end-of-options
  ```

- No `self` subcommand exists. `codex-session self <anything>`
  returns a clap "unknown subcommand" error (exit 64, sysexits
  `EX_USAGE`).
- Every `self_*` source file is renamed (or merged into a `config`
  module). The `Commands::Self_` temporary variant from Phase 01 is
  deleted.
- Every `tests/cmd_self_*.rs` file is renamed.
- `src/ui/self_help.txt` is renamed to `src/ui/help.txt` and updated
  to describe the new top-level surface. Help text strings inside no
  longer contain `self`.
- `--version` (the flag) and `version` (the subcommand) both print
  the dual-line wrapper-version + child path + child version output.

## Spec rationale

- `self` namespace is reserved for self-modifying ops only
  (`update`, `uninstall`); not for introspection or config —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:196-203`
  (after the in-flight correction; treat `rustup self update` and
  `uv self update` as the only valid pattern, and reject the prior
  `cargo metadata` example as factually wrong — Cargo has no
  `cargo self`).
- Wrapper-owned introspection (`version`, `help`, `completion`,
  `config`) lives at top level by **semantic uniqueness**, not by
  namespace — see the real-world survey of `rustup`, `cargo`, `uv`,
  `gh`, `kubectl`, `git`, `gcloud`, `op`, `flyctl` in
  `docs/implementation-plan/README.md` §1.
- Four-edit rule per subcommand — `rust/cli-spec/02-subcommand-pattern.md:8-15`.
- `version` should print **both** wrapper version and resolved
  child path + version — `process-and-posix.md:248-260`.

## Current state (verify before planning)

After Phase 01, the repo has:

- `src/cli/mod.rs` — root `Cli`, with a temporary
  `Commands::Self_(SelfArgs)` variant.
- `src/cli/self_args.rs` (or equivalent) — holds the old `SelfCommand`
  enum.
- `src/cli/self_help.rs`, `src/cli/self_version.rs`,
  `src/cli/self_config_status.rs`, `src/cli/self_config_merge.rs`,
  `src/cli/self_show_local.rs` — per-verb parse-shape structs
  (`SelfHelpArgs`, `SelfVersionArgs`, etc.).
- `src/commands/self_help.rs`, `src/commands/self_version.rs`,
  `src/commands/self_config_status.rs`,
  `src/commands/self_config_merge.rs`,
  `src/commands/self_show_local.rs` — handlers.
- `tests/cmd_self_help.rs`, `tests/cmd_self_version.rs`,
  `tests/cmd_self_config_status.rs`, `tests/cmd_self_config_merge.rs`,
  `tests/cmd_self_show_local.rs`, `tests/cmd_self_dispatch.rs`,
  `tests/cmd_self_symlink.rs` — integration tests.
- `src/ui/self_help.txt` — curated help text containing
  `codex-session self <verb>` examples.

## Target state

### Source layout after this phase

```
src/cli/
├── mod.rs                  # root Cli + GlobalArgs + Commands enum (no `Self_`)
├── version.rs              # VersionArgs
├── config.rs               # ConfigArgs + ConfigCommand { Status, Merge, ShowLocal }
└── (no other files)

src/commands/
├── mod.rs
├── pass_through.rs
├── version.rs              # commands::version::run
├── config_status.rs        # commands::config_status::run
├── config_merge.rs         # commands::config_merge::run
└── config_show_local.rs    # commands::config_show_local::run

src/ui/
├── mod.rs
└── help.txt                # wrapper help; no `self` references

tests/
├── cmd_version.rs
├── cmd_config_status.rs
├── cmd_config_merge.rs
├── cmd_config_show_local.rs
├── cmd_dispatch.rs
├── cmd_symlink.rs
├── cmd_passthrough.rs      # already exists
├── cmd_child_resolution.rs # already exists
├── cmd_logging.rs          # already exists
├── cmd_root_help.rs        # from Phase 01
└── support/mod.rs
```

`cmd_self_help.rs` is **deleted**, not renamed — `--help` is now
clap-derived from the root `Cli` and is covered by
`cmd_root_help.rs` (Phase 01).

### `Cli` / `Commands` enum

```rust
#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Print wrapper and child version details.
    Version(version::VersionArgs),

    /// Print wrapper help (alias for `--help`).
    #[command(name = "help")]
    Help,

    /// Operate on the merged ~/.codex/config.toml.
    Config(config::ConfigArgs),
}

// src/cli/config.rs
#[derive(Debug, clap::Args)]
pub(crate) struct ConfigArgs {
    #[command(subcommand)]
    pub(crate) command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
pub(crate) enum ConfigCommand {
    /// Print base/target/stamp paths and whether a merge is needed.
    Status(ConfigStatusArgs),
    /// Force re-merge regardless of stamp freshness.
    Merge(ConfigMergeArgs),
    /// Print machine-local TOML sections preserved by merges.
    ShowLocal(ShowLocalArgs),
}
```

### Dual-line `--version`

Both `--version` (the flag) and `version` (the subcommand) print
exactly the same output. The simplest path: override clap's default
`--version` behavior by setting
`#[arg(long = "version", action = clap::ArgAction::SetTrue)]` on
`GlobalArgs` and handling the flag in `main` — OR keep clap's default
`--version` and have `commands::version::run` print the same text.
Choose whichever is cleaner; the acceptance test must accept either
path-of-implementation.

Required output (text format):

```
codex-session 0.1.0
codex /usr/local/bin/codex 1.2.3
```

(One line `codex-session <version>`; one line
`codex <resolved-child-path> <child --version first non-empty line>`.)

JSON format (when `--format json`):

```json
{
  "wrapper": { "name": "codex-session", "version": "0.1.0" },
  "child":   { "name": "codex", "path": "/usr/local/bin/codex", "version": "1.2.3" }
}
```

## Tasks

1. **Rename `cli/self_version.rs` → `cli/version.rs`.** Rename
    `SelfVersionArgs` → `VersionArgs`. Update `clap` attributes (e.g.
    `--format` doc text — drop "Honored by: self version" mentions).
2. **Rename `commands/self_version.rs` → `commands/version.rs`.**
    Function `commands::self_version::run` → `commands::version::run`.
    Update import path inside.
3. **Create `cli/config.rs`** with `ConfigArgs` and `ConfigCommand`
    (Subcommand enum). Move the parse-shape from
    `cli/self_config_status.rs`, `cli/self_config_merge.rs`,
    `cli/self_show_local.rs` into this single file as `ConfigStatusArgs`,
    `ConfigMergeArgs`, `ShowLocalArgs`. Delete the three old files.
4. **Rename handler modules** `commands/self_config_status.rs` →
    `commands/config_status.rs`; `commands/self_config_merge.rs` →
    `commands/config_merge.rs`; `commands/self_show_local.rs` →
    `commands/config_show_local.rs`. Update each `run()` to take the
    newly-named args struct. Update `commands/mod.rs` exports.
5. **Delete `cli/self_help.rs` and `commands/self_help.rs`.**
    `help` is now clap-derived. The optional `Commands::Help`
    variant prints the same text clap prints for `--help` — easiest
    approach is to dispatch by calling
    `Cli::command().print_help()`.
6. **Delete `cli/self_args.rs` (or whatever held the temporary
    `Commands::Self_` variant)** and remove the variant from
    `Commands`. The dispatch arm in `main.rs` goes with it.
7. **Update `dispatch` in `main.rs`** to handle the new variants:

    ```rust
    match cli.command {
        Some(Commands::Version(args))  => commands::version::run(ctx, args),
        Some(Commands::Help)           => print_root_help(),
        Some(Commands::Config(args))   => dispatch_config(ctx, args),
        None                           => commands::pass_through::run(ctx, &cli.passthrough),
    }

    fn dispatch_config(ctx, args: ConfigArgs) -> Result<(), AppError> {
        match args.command {
            ConfigCommand::Status(a)    => commands::config_status::run(ctx, a),
            ConfigCommand::Merge(a)     => commands::config_merge::run(ctx, a),
            ConfigCommand::ShowLocal(a) => commands::config_show_local::run(ctx, a),
        }
    }
    ```

8. **Wire dual-line `--version`.** Pick one of the two paths from
    the target-state section. The simplest: keep clap's default
    `--version` action but include the resolved child line by
    constructing the version string at `Cli` build-time — OR
    suppress clap's `--version` and intercept the `--version` flag
    manually in `main` to call `commands::version::run` with a
    default `OutputFormat::Text`. Either is acceptable.

9. **Rename `src/ui/self_help.txt` → `src/ui/help.txt`.** Replace
    all `codex-session self <verb>` references with the new
    top-level forms. Drop the line "Anything not under `self` is
    forwarded verbatim" and replace with "Any subcommand not
    recognized by the wrapper is forwarded verbatim to the real
    `codex`. Use `--` to force passthrough of `-`-prefixed args."

    Update the path in `src/ui/mod.rs` / wherever the text is
    `include_str!`'d.

10. **Rename all `tests/cmd_self_*.rs` files.** The file mapping:

    | Old | New | Notes |
    |---|---|---|
    | `cmd_self_version.rs` | `cmd_version.rs` | Update `cargo_bin` invocations: drop `self`. |
    | `cmd_self_config_status.rs` | `cmd_config_status.rs` | Invoke as `config status`. |
    | `cmd_self_config_merge.rs` | `cmd_config_merge.rs` | Invoke as `config merge`. |
    | `cmd_self_show_local.rs` | `cmd_config_show_local.rs` | Invoke as `config show-local`. |
    | `cmd_self_dispatch.rs` | `cmd_dispatch.rs` | Update dispatch assertions. |
    | `cmd_self_symlink.rs` | `cmd_symlink.rs` | Same. |
    | `cmd_self_help.rs` | (deleted) | Covered by `cmd_root_help.rs`. |

    Update every existing snapshot under `tests/snapshots/` to match
    the new command invocations (run `cargo insta review` after the
    rename to accept the regenerated snapshots).

11. **Search for stragglers.** Run:

    ```bash
    rg -n '\bself[ _-]' src/ tests/ docs/implementation-plan/ Cargo.toml justfile
    ```

    Anything still referencing `self` as a CLI verb or module name
    (excluding intentional Rust `self` keyword uses, and the
    `OutputFormat` ValueEnum if any) must be cleaned up.

12. **Add a guard test** `tests/cmd_self_rejected.rs`:

    ```rust
    #[test]
    fn self_subcommand_is_unknown() {
        let out = Command::cargo_bin("codex-session").unwrap()
            .arg("self").arg("version")
            .output().unwrap();
        assert!(!out.status.success());
        assert_eq!(out.status.code(), Some(64), "EX_USAGE expected");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("unrecognized subcommand") || stderr.contains("error"));
    }
    ```

    This is the regression test for "no `self` namespace exists yet".

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes (after `cargo insta review` accepts updated
  snapshots).
- [ ] `rg -n 'self[ _-]' src/cli/ src/commands/ tests/` returns nothing
  (modulo the Rust `self` keyword and any intentional matches).
- [ ] `codex-session version` prints two lines (wrapper + child).
- [ ] `codex-session --version` prints the same two lines.
- [ ] `codex-session config status` works (same behavior as the
  old `self config-status`).
- [ ] `codex-session config merge` works.
- [ ] `codex-session config show-local` works.
- [ ] `codex-session help` and `codex-session --help` both print
  wrapper help.
- [ ] `codex-session self version` fails with exit code 64 and
  emits a clap "unrecognized subcommand" error.
- [ ] `tests/cmd_self_*.rs` files are all renamed or deleted.
- [ ] `src/ui/help.txt` exists; `src/ui/self_help.txt` does not.

## Tests

- Add `tests/cmd_self_rejected.rs` (above).
- Update every existing wrapper-verb integration test to use the new
  invocation. Snapshot files under `tests/snapshots/` need a
  one-time `cargo insta review` after the file renames.

## Out of scope

- Adding `clap_complete` / `completion` subcommand. (Phase 10.)
- Introducing the `Config` struct / `figment`. (Phase 03.)
- Touching the typed child invocation, recursion guard, env
  scrubbing, logging, color policy. (Phases 04–07.)
- Expanding the `--version` JSON schema beyond the shape above.

## References

- `cli-design/06-cli-wrapper-design/process-and-posix.md:196-203, 248-260` (corrected `self`-namespace rule).
- `rust/cli-spec/02-subcommand-pattern.md:8-15, 47-77` (four-edit rule, root `Cli`).
- `rust/cli-spec/00-directory-tree.md:17-22, 33-37` (`cli/` and `commands/` layouts).
- `docs/implementation-plan/README.md` (§ Key decisions, item 1 & 2).
- Real-world reference CLIs (consulted; not authoritative): rustup, uv, cargo, gh, kubectl, git, gcloud, op, flyctl.
