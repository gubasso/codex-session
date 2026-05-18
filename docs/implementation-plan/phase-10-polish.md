# Phase 10 — Polish: `clap_complete`, `missing_docs = "warn"`, module headers, ADR sweep

**One-line summary.** Ship a `codex-session completion <shell>`
subcommand via `clap_complete`. Flip `missing_docs` from `allow` to
`warn` and add module headers ("what it is / what it isn't") to every
`src/*.rs`. Update or remove now-stale ADRs.

## Prerequisites

Phases 01–09 must be complete. CLI surface, config, errors, logging,
tests must all be in place.

## Goal

After this phase:

- `codex-session completion <shell>` writes the completion script for
  the requested shell to stdout. Supported shells: `bash`, `zsh`,
  `fish`, `elvish`, `powershell` (clap's full list). Exit 0 on
  success. Bad shell name → clap usage error.
- `Cargo.toml` lists `clap_complete = "4"` (matching `clap`'s major
  version).
- `Cargo.toml [lints.rust] missing_docs = "warn"`. Every `pub(crate)`
  item has a doc comment.
- Every `src/**/*.rs` starts with a module header of the form:

  ```rust
  //! <short purpose>.
  //!
  //! What this is: <one short sentence>.
  //! What this is not: <one short sentence>.
  ```

  Per `rust/cli-spec/08-naming-and-visibility.md:106-145`.
- The existing ADRs under `docs/adr/` are either updated to reflect
  the post-refactor reality or moved to `docs/adr/superseded/` if
  they describe a design that no longer holds.
- `README.md` at the repo root reflects the new CLI surface
  (`codex-session version`, `codex-session config status`, etc.) and
  no longer mentions `codex-session self ...` except in a "deferred
  / future" note.
- `justfile` tasks (`check`, `lint`, `test`, `fmt`) all green.
- The repo passes `cargo deny check` if a `deny.toml` exists (and is
  added if not — minimal config sufficient).

## Spec rationale

- Static completions for the wrapper's own flags —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:260-262`,
  `rust/cli-spec/07-dependencies.md:17`.
- `missing_docs` / module headers —
  `rust/cli-spec/08-naming-and-visibility.md:106-145`,
  `rust/cli-spec/09-coding-style.md`.
- Doc maintenance hygiene — `cli-design/00-architecture.md` (every
  directory has a single responsibility + "does NOT belong here").

## Current state (verify before planning)

- `Cargo.toml:40` sets `missing_docs = "allow"`.
- No `clap_complete` dependency.
- `docs/adr/` contains entries from earlier in the migration:
  - `0001-top-level-passthrough-bypasses-clap.md` — **stale** (Phase 01
    fixed this; `codex-session` no longer bypasses clap).
  - `0002-preserve-legacy-exit-codes-1-and-2.md` — verify; may still
    apply.
  - `0004-codex-resolution-via-which-crate.md` — partially stale
    (Phase 03 wraps `which::which` behind config layer; Phase 05
    adds inode guard).
  - `0007-no-global-verbosity-flag.md` — **stale** (Phase 01 makes
    `-v` a root global; verify).
  - `0008-skip-directories-and-figment.md` — **stale** (Phase 03
    introduces both `directories` and `figment`).
  - `0011-wrapper-env-vars-and-exit-codes.md` — verify; may need a
    new entry for `CODEX_SESSION_REENTRY`.
  - `0012-two-layer-logging-and-json-output.md` — verify against
    Phase 06's `tracing-appender` implementation.
- Module headers in `src/` are inconsistent (`src/context.rs:1-3`,
  `src/error.rs:1-2`, `src/logging.rs:1` are partial).
- `README.md` and `AGENTS.md` reference the old `self` surface.

## Target state

### `src/cli/completion.rs`

```rust
//! `completion` subcommand: parse-shape.
//!
//! What this is: clap derive struct for `codex-session completion <SHELL>`.
//! What this is not: the generator — that lives in
//! `src/commands/completion.rs`.

#[derive(Debug, clap::Args)]
pub(crate) struct CompletionArgs {
    /// Target shell.
    #[arg(value_enum)]
    pub(crate) shell: clap_complete::Shell,
}
```

### `src/commands/completion.rs`

```rust
//! `completion` handler.

use crate::cli::Cli;
use crate::context::AppContext;
use crate::error::AppError;
use clap::CommandFactory as _;

pub(crate) fn run(_ctx: &AppContext, args: crate::cli::completion::CompletionArgs)
    -> Result<(), AppError>
{
    let mut cmd = Cli::command();
    let bin = cmd.get_name().to_string();
    clap_complete::generate(args.shell, &mut cmd, bin, &mut std::io::stdout());
    Ok(())
}
```

### `Cli::Commands` enum gains a variant

```rust
Completion(completion::CompletionArgs),
```

### `Cargo.toml`

```toml
clap_complete = "4"
```

Update `[lints.rust]`:

```toml
unsafe_code     = "forbid"
missing_docs    = "warn"               # CHANGED from "allow"
unreachable_pub = "warn"
unused_must_use = "deny"
```

## Tasks

1. **Add `clap_complete = "4"` to `Cargo.toml`.**

2. **Add `Commands::Completion(completion::CompletionArgs)`** to
    `src/cli/mod.rs`. Create `src/cli/completion.rs` and
    `src/commands/completion.rs` per the targets above. Add the
    dispatch arm in `main.rs::dispatch`.

3. **Flip `missing_docs = "warn"`.** Compile; expect many warnings.

4. **Add doc comments** to every `pub(crate)` item that lacks one.
    The most common offenders are:
    - Struct fields on `GlobalArgs`, `VersionArgs`, `ConfigArgs`,
      etc.
    - Enum variants on `Commands`, `ConfigCommand`, `AppError`,
      `ConfigError`, `SpawnerError`, `FsError`, `MergeError`,
      `LogFormat`, `OutputFormat`.
    - Public functions on `Ui`, `Spawner`, helpers in
      `src/error.rs`, `src/logging.rs`.

    A doc comment should be a real description, not a tautology
    ("the foo bar" for field `foo_bar`). One-liner is fine:

    ```rust
    /// Increase wrapper log verbosity (-v info, -vv debug, -vvv trace).
    pub(crate) verbose: u8,
    ```

5. **Add module headers** ("what it is / what it isn't") to every
    `src/**/*.rs`. Use the template:

    ```rust
    //! <one-sentence purpose>.
    //!
    //! What this is: <short>.
    //! What this is not: <short — name the closest thing it isn't>.
    ```

    Example for `src/context.rs`:

    ```rust
    //! Application context built once in `main`.
    //!
    //! What this is: the single value threaded by reference to every
    //! handler. Holds `Config`, paths, resolved child, UI, and adapters.
    //! What this is not: business logic — handlers in `commands/` do
    //! that. Mutable state — `AppContext` is immutable after `new`.
    ```

    Run `rg -L '^//!' src/**/*.rs` to find files missing the header.

6. **ADR sweep — already done at plan-writing time.** The ten ADRs
    whose content is now covered by the canonical specs OR reversed
    by this refactor were deleted in the same commit that introduced
    `docs/implementation-plan/`:
    - `0001-top-level-passthrough-bypasses-clap.md` (reversed by Phase 01)
    - `0002-preserve-legacy-exit-codes-1-and-2.md` (already superseded)
    - `0004-codex-resolution-via-which-crate.md` (canonical spec prescribes it; Phase 03/05 layer on top)
    - `0006-unset-home-falls-back-to-root.md` (reversed by Phase 03)
    - `0007-no-global-verbosity-flag.md` (already superseded; Phase 01 adds `-v`)
    - `0008-skip-directories-and-figment.md` (reversed by Phase 03)
    - `0009-foo-dot-rs-over-mod-rs.md` (reversed by Phase 01)
    - `0010-paths-stay-as-pathbuf.md` (reversed by Phase 03)
    - `0011-wrapper-env-vars-and-exit-codes.md` (env vars become `Config` schema; sysexits in canonical spec)
    - `0012-two-layer-logging-and-json-output.md` (replaced by Phase 06)

    The two surviving ADRs are project-specific decisions **not**
    covered by the canonical specs:
    - `0003-install-via-cargo-install.md` (deployment flow)
    - `0005-atomic-write-via-named-tempfile.md` (merge service internal)

    This phase's only ADR work is: **verify** the two survivors are
    still accurate after the refactor lands; update them in place if
    not. Do NOT re-introduce a `superseded/` subdirectory — the SoT
    for everything covered by canonical specs is the canonical spec
    tree at `/home/gu/Projects/docs-n-notes/tech/`.

7. **In-repo docs SoT sweep.** Every in-repo document must either
    contain project-specific content **not** in the canonical specs,
    or point to the canonical specs as the SoT. Files to audit:

    - `README.md` (root) — rewrite per task 8 below.
    - `docs/adr/*.md` — already pruned to 0003 + 0005 (project-only).
    - `docs/implementation-plan/*.md` — kept; this is the refactor
      plan, not the spec.
    - Any `AGENTS.md`, `CLAUDE.md`, or `CONTRIBUTING.md` at the
      repo root — if present, update to point at the canonical
      specs. At plan-writing time only `README.md` exists at root.
    - Inline `//!` module headers — handled by task 5 above.
    - Doc comments on `pub(crate)` items — handled by task 4.

    The rule: if a sentence in any in-repo doc duplicates content
    that lives in the canonical specs, **delete the sentence** and
    replace with a one-line "see `<canonical-path>`" pointer.
    In-repo docs may extend canonical specs with project-specific
    notes; they must not contradict, restate, or fork them.

8. **Rewrite `README.md` (root).** Replace any obsolete content
    with these sections:

    - **Overview** (≤5 lines): "`codex-session` is a Unix-only Rust
      CLI wrapper around the `codex` binary that keeps
      `~/.codex/config.toml` in sync with a stow-managed
      `~/.codex/config.base.toml` while preserving machine-local
      sections."
    - **Specs** (1 line each): point at the canonical spec trees
      under `/home/gu/Projects/docs-n-notes/tech/programming/cli-design/`
      and `/home/gu/Projects/docs-n-notes/tech/languages/rust/cli-spec/`.
      Make clear these are external SoT.
    - **Install** — `cargo install --path .` (per `docs/adr/0003`).
    - **Usage** — top-level CLI surface:

      ```
      codex-session [WRAPPER-OPTS] <verb|positional> [--] [CHILD-ARGS...]

      codex-session --help / help
      codex-session --version / version [--format text|json]
      codex-session config status [--format text|json]
      codex-session config merge
      codex-session config show-local [--format text|json]
      codex-session completion <shell>
      codex-session <anything else>       # forwarded to codex
      codex-session -- <child-args>       # POSIX end-of-options, forwarded
      ```

    - **Environment** — list `CODEX_SESSION_CHILD_BIN`,
      `CODEX_SESSION_LOG_FILE`, `CODEX_SESSION_LOG_DIR`,
      `CODEX_SESSION_REENTRY`, `NO_COLOR`, `FORCE_COLOR`,
      `RUST_LOG`. One line each.
    - **Exit codes** — link to the canonical spec
      (`cli-design/06-cli-wrapper-design/process-and-posix.md`) and
      give the project-specific table only as quick reference.
    - **Configuration** — short description of XDG layering;
      link to `cli-design/03-config-precedence.md`.
    - **Implementation plan** — pointer to
      `docs/implementation-plan/README.md` for refactor work.

    Drop everything else (FAQ-style content, migration notes from
    bash, old `self` examples).

9. **Update `justfile`.** Add a `completion-install` task that
    pipes `codex-session completion <shell>` to the appropriate
    user dir, e.g.:

    ```just
    completion-install shell:
        codex-session completion {{shell}} > $XDG_CONFIG_HOME/<...>
    ```

    (Optional polish.)

10. **Add `deny.toml`** if missing. Minimal config:

    ```toml
    [advisories]
    yanked = "warn"
    ignore = []

    [licenses]
    allow = ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-3-Clause", "ISC", "Unicode-DFS-2016"]
    confidence-threshold = 0.93

    [bans]
    multiple-versions = "warn"
    wildcards = "deny"
    ```

    Run `cargo deny check` if `cargo-deny` is installed.

11. **Smoke test the whole tree.** Run:

    ```bash
    cargo fmt --all -- --check
    cargo clippy --all-targets -- -D warnings
    cargo test
    cargo doc --no-deps --document-private-items 2>&1 | grep -i 'warning'
    ```

    No warnings from any of them.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes with
  `missing_docs = "warn"`.
- [ ] `cargo test` passes.
- [ ] `cargo doc --no-deps --document-private-items` produces no
  `missing_docs` warnings.
- [ ] `codex-session completion bash` prints a non-empty bash
  completion script.
- [ ] `codex-session completion zsh`, `fish`, `elvish`, `powershell`
  all work.
- [ ] `codex-session completion not-a-shell` exits 64 with a clap
  usage error.
- [ ] `Cargo.toml [lints.rust] missing_docs = "warn"`.
- [ ] `rg -L '^//!' src/` (find files NOT starting with `//!`)
  returns nothing.
- [ ] `docs/adr/` contains only `0003-install-via-cargo-install.md`
  and `0005-atomic-write-via-named-tempfile.md` (the only ADRs
  with content not in the canonical specs). No `superseded/`
  subdirectory exists.
- [ ] `README.md` documents the new top-level CLI surface; no
  `codex-session self ...` examples remain; the Specs section
  points at the canonical spec tree as SoT.
- [ ] `rg -n 'codex-session self' README.md docs/` returns nothing
  except the deferred-future note.

## Tests

- Add `tests/cmd_completion.rs`:
  - For each shell (`bash`, `zsh`, `fish`): `codex-session
    completion <shell>` exits 0 and stdout is non-empty.
  - For `not-a-shell`: exit 64.

## Out of scope

- Dynamic shell completion (querying runtime state). Static is
  sufficient.
- Manpage generation (`mywrap(1)`, `mywrap-VERB(1)`). The spec
  mentions it (`process-and-posix.md:263-264`) as nice-to-have;
  defer until users request it.
- Refactoring the `services/merge.rs` interior. The merge service
  is orthogonal; this whole refactor preserves its behavior.
- Workspace migration / second binary.

## References

- `clap_complete` docs: <https://docs.rs/clap_complete/latest/clap_complete/>.
- `cli-design/06-cli-wrapper-design/process-and-posix.md:260-262` — completions.
- `rust/cli-spec/08-naming-and-visibility.md:106-145` — module headers, doc coverage.
- `rust/cli-spec/09-coding-style.md` — coding-style rules.
- `cargo-deny`: <https://github.com/EmbarkStudios/cargo-deny>.
- ADR template: <https://adr.github.io/madr/>.
