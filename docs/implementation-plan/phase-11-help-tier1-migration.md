# Phase 11 — Help-text Tier 1 migration: clap-driven help with narrative addendum

**One-line summary.** Replace the current hand-authored `src/ui/help.txt` +
custom `help` subcommand (Tier 3) with the canonical Tier 1 pattern: clap
generates USAGE / flags / subcommands from the derive structs, and a small
`src/ui/help_extras.txt` is wired in via `after_long_help = include_str!(...)`.

## Prerequisites

Phases 01–10 must be complete. The wrapper-owned verbs (`version`,
`config …`, `completion`), `GlobalArgs`, and the dual-line `version`
output already exist; this phase only changes how `--help` / `help` is
*rendered*, not the command surface itself.

## Goal

After this phase:

- The root `Cli` in `src/cli/mod.rs` uses **Tier 1** help rendering:

    - `disable_help_subcommand = true` is **removed** so clap auto-generates
      a `help` subcommand and the `--help` / `-h` flags.
    - `#[command(...)]` gains
      `after_long_help = include_str!("../ui/help_extras.txt")`.
    - Every `pub(crate)` field on `GlobalArgs` already has a `///` doc
      comment (Phase 10 work) — clap now renders those as the
      OPTIONS table; the hand-written "Wrapper options" block goes away.
    - Each variant of `Commands` already has a `///` doc comment — clap
      now renders those as the SUBCOMMANDS / "Commands" block; the
      hand-written "Wrapper verbs" block goes away.

- `src/ui/help.txt` is **renamed to `src/ui/help_extras.txt`** and shrunk
  to contain only what clap cannot derive:

    - Passthrough semantics (`--` end-of-options, "unrecognized
      subcommands are forwarded to `codex`").
    - Environment variables the parser does not see
      (`CODEX_SESSION_CHILD_BIN`, `CODEX_SESSION_LOG_FILE`,
      `CODEX_SESSION_LOG_DIR`, `CODEX_SESSION_REENTRY`, `NO_COLOR`,
      `FORCE_COLOR`, `CLICOLOR`, `CLICOLOR_FORCE`, `RUST_LOG`).
    - A short `EXAMPLES` section (1–3 invocations).
    - **No** USAGE line, **no** flag table, **no** subcommand list.

- The custom `help` handler is removed:

    - `Commands::Help` variant deleted from `src/cli/mod.rs`.
    - `src/commands/help.rs` deleted; `commands/mod.rs` no longer
      re-exports it.
    - The `Commands::Help | None => commands::help::run(ctx)` arm in
      `src/commands/dispatch.rs:23` is replaced by a single
      `None => commands::pass_through::run(ctx, &[])` (or whatever the
      no-args dispatch currently does — verify at planning time).
    - `src/cli/exit.rs` drops the `HELP_TEXT` constant and the
      `DisplayHelp` branch in `handle_clap_error` — clap prints help
      itself on its own writer; the wrapper does not intercept it.

- `codex-session --help`, `codex-session -h`, and
  `codex-session help` all print clap's auto-generated help, followed
  by the narrative addendum from `help_extras.txt`. All three are
  byte-equal modulo clap's `-h` vs `--help` length difference.

- Snapshot test `tests/cmd_root_help.rs` is updated and its
  `tests/snapshots/cmd_root_help__root_help.snap` regenerated.

## Spec rationale

The canonical CLI spec prescribes Tier 1 by default and forbids
hand-maintaining a parallel flag table:

- `rust/cli-spec/02-subcommand-pattern.md` §"Help rendering with clap"
  — Default recipe: `after_long_help = include_str!("../ui/help_extras.txt")`.
  "`src/ui/help_extras.txt` is the **only** authored help file. It must
  not contain a flag table, a usage line, or a subcommand list — clap
  generates those from the derive structs."
- Same file, "Escalation tiers": Tier 3 (custom `help` subcommand) is
  "Justified only when you have dynamically-discovered passthrough
  subcommands (cargo-plugin-style) clap can't enumerate, or other
  narrative the parser fundamentally can't express. Document the
  choice in an ADR — every new flag now has to be added in two
  places."
- `override_help` and full Tier 3 are explicitly called out as "almost
  always the wrong call".
- The wrapper has a fixed, known command tree (`version`, `config`,
  `completion`, plus external passthrough) — it fits Tier 1's profile
  exactly. The "Reading the table" note in the same section places
  `cargo`, `jj`, `rustup`, and clap's own examples in Tier 1.
- `cli-design/07-naming-and-docs.md` §"`--help` is generated, not
  authored" (the general prerequisite the Rust chapter inherits from)
  is the canonical rule.

The audit that motivated this phase observed that Phase 02 renamed
`src/ui/self_help.txt → src/ui/help.txt` and kept the Tier 3 shape
without an ADR justifying the escalation, and Phase 10 did not catch
the divergence. This phase closes that gap.

## Current state (verify before planning)

Confirmed at plan-writing time (line numbers will drift; re-read):

- `src/cli/mod.rs:18-28` — root `Cli` has
  `disable_help_subcommand = true` and **no** `after_help` /
  `after_long_help` wiring.
- `src/cli/mod.rs:86-88` — `Commands::Help` variant with
  `#[command(name = "help")]`.
- `src/cli/exit.rs:9-10` — `pub(crate) const HELP_TEXT: &str =
  include_str!("../ui/help.txt");`.
- `src/cli/exit.rs:17-31` — `handle_clap_error` intercepts
  `DisplayHelp` and routes through `Ui::write_help(HELP_TEXT)`.
- `src/commands/help.rs:1-16` — independent module that
  re-`include_str!`s the same file and prints via `ctx.ui.write_help`.
- `src/commands/dispatch.rs:23` —
  `Some(cli::Commands::Help) | None => commands::help::run(ctx)`.
- `src/ui/help.txt:1-64` — full hand-authored help including USAGE
  block, "Wrapper verbs" table, "Wrapper options" table, and
  Environment + passthrough narrative.
- `tests/cmd_root_help.rs` + `tests/snapshots/cmd_root_help__root_help.snap`
  — snapshot of the current Tier 3 output.

## Target state

### `src/cli/mod.rs` (root `Cli` attributes)

```rust
#[derive(Debug, Parser)]
#[command(
    name = "codex-session",
    bin_name = "codex-session",
    about = "Wrapper around `codex` with config-merge and machine-local preservation.",
    long_about = None,
    after_long_help = include_str!("../ui/help_extras.txt"),
    disable_version_flag = true,           // unchanged — version flag is intercepted in dispatch
    allow_external_subcommands = true,
    subcommand_negates_reqs = true,
    // disable_help_subcommand removed — clap owns `help`
)]
pub(crate) struct Cli { /* unchanged fields */ }
```

### `Commands` enum

```rust
#[derive(Debug, Subcommand)]
pub(crate) enum Commands {
    /// Print wrapper and child version details.
    Version(version::VersionArgs),

    // Help variant removed — clap auto-generates `help` and `--help`.

    /// Emit a shell-completion script.
    Completion(completion::CompletionArgs),

    /// Operate on config state managed by the wrapper.
    Config(config::ConfigArgs),

    /// Forward any unknown top-level verb to the wrapped `codex` binary.
    #[command(external_subcommand)]
    External(Vec<OsString>),
}
```

### `src/ui/help_extras.txt` (new; replaces `help.txt`)

```
Pass-through:
  Any subcommand not recognized by the wrapper is forwarded verbatim to the
  real `codex`. Use `--` to force pass-through of `-`-prefixed arguments.

Environment:
  CODEX_SESSION_CHILD_BIN   Explicit path to the inner `codex` binary.
                            Tried before PATH lookup.
  CODEX_SESSION_LOG_FILE    Explicit log file path. Overrides LOG_DIR.
  CODEX_SESSION_LOG_DIR     Directory holding `codex-session.log`.
                            Rotated daily as `codex-session.log.<YYYY-MM-DD>`.
  CODEX_SESSION_REENTRY     Re-entry marker set on the child env to detect
                            chained-wrapper loops. Refuses to start when set
                            to `1` in the parent env.
  RUST_LOG                  Standard `tracing` filter; overrides -v.
  NO_COLOR                  Disable ANSI colors absolutely.
  FORCE_COLOR               Force ANSI colors even when output is piped.
  CLICOLOR=0                Disable ANSI colors on TTY output.
  CLICOLOR_FORCE            Force ANSI colors even when output is piped.

Examples:
  codex-session version --format json
  codex-session config status
  codex-session --dry-run -- foo bar
```

No USAGE line, no flag table, no subcommand list — clap renders all of
those from the derive structs and field doc-comments.

### `src/cli/exit.rs`

`HELP_TEXT` constant deleted. `handle_clap_error` is shrunk:

```rust
pub(crate) fn handle_clap_error(err: clap::Error, _argv: &[OsString]) -> ExitCode {
    use clap::error::ErrorKind::{DisplayHelp, DisplayVersion};
    match err.kind() {
        // clap prints help / version itself on its own writer; we exit 0.
        DisplayHelp | DisplayVersion => {
            let _ = err.print();
            ExitCode::SUCCESS
        }
        _ => crate::error::print_and_exit(
            &crate::error::AppError::Usage(err),
            &crate::cli::GlobalArgs::default(),
        ),
    }
}
```

(`DisplayVersion` was previously `unreachable!()` because version was
intercepted via `GlobalArgs.version`; that interception is unchanged in
this phase, so the arm is defensive only. Codex may keep it as
`unreachable!()` if `disable_version_flag = true` stays on.)

### `src/commands/dispatch.rs`

```rust
match cli.command {
    Some(cli::Commands::Version(args))     => commands::version::run(ctx, args),
    Some(cli::Commands::Completion(args))  => commands::completion::run(ctx, args),
    Some(cli::Commands::Config(args))      => run_config(ctx, args),
    Some(cli::Commands::External(argv))    => commands::pass_through::run(ctx, &argv),
    None                                   => commands::pass_through::run(ctx, &[]),
}
```

The `None` arm is the only place that needs a decision: today
`Commands::Help | None` prints help. Spec-wise, a bare `codex-session`
with no args should **either** print help **or** forward to the child.
Phase 02 chose "print help"; the canonical wrapper spec
(`cli-design/06-cli-wrapper-design/process-and-posix.md`) is silent on
this specific case. Pick one and document it in the phase plan stage:

- **Option A (recommended).** `None` → print clap's auto-generated help
  (call `Cli::command().print_help()` or rely on a clap arg group with
  `arg_required_else_help = true` on the root). Matches Phase 02
  behavior; preserves the current snapshot's intent.
- **Option B.** `None` → forward bare invocation to the child. More
  POSIX-pure but changes user-visible behavior; out of scope for a
  Tier-1-help-only phase.

Default to Option A. If Codex picks Option B, it must add an explicit
note to this phase doc and update Phase 02's acceptance criteria.

### `src/commands/help.rs`

**Deleted.** Remove the `pub(crate) mod help;` line from
`src/commands/mod.rs`.

## Tasks

1. **Rename** `src/ui/help.txt` → `src/ui/help_extras.txt` and rewrite
    it to contain **only** the passthrough paragraph, environment
    table, and examples (template above). Delete the USAGE block,
    "Wrapper verbs" table, and "Wrapper options" table — clap renders
    those from the derive structs.

2. **Edit `src/cli/mod.rs`:**
    - Remove `disable_help_subcommand = true` from the root `#[command(...)]`.
    - Add `after_long_help = include_str!("../ui/help_extras.txt")` to
      the root `#[command(...)]`.
    - Delete the `Commands::Help` variant (and its `#[command(name =
      "help")]` attribute). Clap re-introduces its own auto-generated
      `help` subcommand.
    - Verify every `pub(crate)` field on `GlobalArgs` and every variant
      of `Commands` has a `///` doc comment good enough to read in the
      help table. They should already from Phase 10; tighten wording
      if any are tautological.

3. **Edit `src/cli/exit.rs`:**
    - Delete `pub(crate) const HELP_TEXT: &str = include_str!(...)`.
    - Replace the `DisplayHelp` arm body with `let _ = err.print();
      ExitCode::SUCCESS`. Leave `DisplayVersion` either as the
      existing `unreachable!()` or as a defensive `err.print()` call —
      either is acceptable.
    - Update the module-level `//!` header: drop the "plus the
      curated `HELP_TEXT` served for `--help`" sentence.

4. **Delete `src/commands/help.rs`** and remove its `pub(crate) mod
    help;` line from `src/commands/mod.rs`.

5. **Edit `src/commands/dispatch.rs:21-27`:**
    - Remove the `Some(cli::Commands::Help) | None => commands::help::run(ctx)`
      arm.
    - Add a `None` arm per Option A or B above (default: Option A —
      print clap's auto-generated help).

6. **Update snapshot test `tests/cmd_root_help.rs`** to assert the new
    clap-rendered output:
    - `codex-session --help` exits 0, stdout starts with
      `Wrapper around \`codex\``, contains an auto-rendered
      "Commands:" block listing `version`, `completion`, `config`,
      `help`, and includes the passthrough/environment narrative from
      `help_extras.txt`.
    - Re-snapshot via `cargo insta review`.
    - Add (if not already present) a parity assertion that
      `codex-session help` and `codex-session --help` produce
      byte-equal stdout.

7. **Add `tests/cmd_help_no_handauthored.rs`** as a guard rail:

    ```rust
    #[test]
    fn help_text_is_clap_generated() {
        let help = String::from_utf8(
            std::process::Command::new(env!("CARGO_BIN_EXE_codex-session"))
                .arg("--help")
                .output()
                .unwrap()
                .stdout,
        ).unwrap();
        // Sanity: clap-generated USAGE line is present.
        assert!(help.contains("Usage:"));
        // Sanity: every wrapper verb shows up in the auto-generated Commands block.
        for verb in ["version", "completion", "config", "help"] {
            assert!(help.contains(verb), "verb `{verb}` missing from help");
        }
        // Sanity: the addendum from help_extras.txt is appended.
        assert!(help.contains("CODEX_SESSION_CHILD_BIN"));
    }
    ```

8. **Search for stragglers:**

    ```bash
    rg -n 'help\.txt|HELP_TEXT|write_help' src/ tests/
    ```

    After this phase, the only matches should be inside `src/ui/`
    (a thin `Ui::write_help` wrapper *may* survive if other code paths
    use it — verify and delete the helper if it has zero remaining
    callers).

9. **ADR check.** Per the spec, Tier 3 escalations must be
    ADR-documented. This phase moves the wrapper *down* to Tier 1, so
    no ADR is needed. Confirm that `docs/adr/` still contains only
    `0003-install-via-cargo-install.md` and
    `0005-atomic-write-via-named-tempfile.md`; do **not** add a new
    ADR for "we chose Tier 1" — Tier 1 is the spec default.

10. **Refresh the implementation-plan README index** (this is housekeeping):
    add a row for Phase 11 in the phase table and append `11` to the
    dependency diagram. (Already done in the same commit as this phase
    file lands.)

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes.
- [ ] `cargo fmt --all -- --check` passes.
- [ ] `codex-session --help` and `codex-session help` produce
  byte-equal stdout (modulo trailing newline).
- [ ] `codex-session -h` produces a shorter form (clap's short-help
  convention) and still includes every wrapper verb name.
- [ ] `src/ui/help_extras.txt` exists and contains **no** USAGE line,
  **no** flag table, **no** subcommand list. `rg -n '(Usage|--help|--version)' src/ui/help_extras.txt`
  returns nothing (modulo the `-- [<child args>]` passthrough mention).
- [ ] `src/ui/help.txt` does not exist.
- [ ] `src/commands/help.rs` does not exist.
- [ ] `src/cli/exit.rs` no longer defines `HELP_TEXT`.
- [ ] `src/cli/mod.rs` does not contain `disable_help_subcommand` and
  the `Commands::Help` variant is gone.
- [ ] `tests/cmd_root_help.rs` snapshots clap's auto-generated output.
- [ ] `tests/cmd_help_no_handauthored.rs` exists and passes.
- [ ] `docs/adr/` still contains only the two surviving ADRs (no new
  ADR for this phase).

## Tests

- Update `tests/cmd_root_help.rs` and regenerate
  `tests/snapshots/cmd_root_help__root_help.snap`.
- Add `tests/cmd_help_no_handauthored.rs` (template above) as a
  regression guard so a future change cannot silently re-introduce a
  hand-authored USAGE block or flag table.
- Verify the existing `tests/cmd_self_rejected.rs`,
  `tests/cmd_version.rs`, `tests/cmd_config_status.rs` etc. still pass
  unchanged.

## Out of scope

- Switching to the `clap-help` crate for richer presentation
  (different motivation; would be a separate Tier 2 phase if ever
  pursued).
- Changing the `--version` flag interception
  (`GlobalArgs.version` stays).
- Per-subcommand `after_long_help` files (`version`, `config`,
  `completion`). The wrapper's command surface is small enough that
  the root-level addendum covers everything; revisit only if a
  subcommand grows substantial passthrough/env narrative of its own.
- Rewriting `Ui::write_help` to integrate with a pager. If the helper
  is removed because it has no callers after this phase, fine; if it
  survives for other reasons, leave it.
- Changing the bare-invocation behavior (Option B). Default to
  Option A.

## References

- `rust/cli-spec/02-subcommand-pattern.md` §"Help rendering with clap"
  — Default recipe, escalation tiers, prior-art table.
- `cli-design/07-naming-and-docs.md` §"`--help` is generated, not
  authored" — the canonical general rule the Rust chapter inherits.
- `cli-design/06-cli-wrapper-design/process-and-posix.md:248-260` —
  wrapper-owned `--help` / `--version` UX (already implemented in
  Phase 01–02; unchanged here).
- clap 4.x `Command::after_long_help`:
  <https://docs.rs/clap/latest/clap/struct.Command.html#method.after_long_help>.
- Prior-art models worth a quick look while planning: `rustup`'s
  per-command `after_help` + `include_str!` (closest analog for a
  wrapper); `jj`'s in-derive `long_about` (alternative if you ever
  want the prose next to the struct instead of in a text file).
