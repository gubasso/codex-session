# Phase 07 — Output discipline + color policy

**One-line summary.** Route all human-visible output through `src/ui/`.
Implement the full color-precedence policy
(`NO_COLOR > FORCE_COLOR/CLICOLOR_FORCE > isatty > CLICOLOR`).
Remove the last raw-stdout write from `src/main.rs`.

## Prerequisites

Phase 06 should be complete (this phase wires the real
`crate::ui::color::stderr_color()` helper that Phase 06 stubbed).
Phase 01–04 must be complete (root `Cli`, `Ui` already in
`AppContext`).

## Goal

After this phase:

- **Zero `println!` / `eprintln!` / `stdout().write_all` / direct
  `io::stderr()` writes outside `src/ui/` and `src/error.rs::render`.**
  Specifically:
  - The stdout write at `src/main.rs:80-83` (in pre-Phase-01 code)
    or wherever it landed after Phase 01 moves into a `Ui` method.
  - The `commands::pass_through::dry_run` print goes through
    `ctx.ui.write_dry_run` (already done in Phase 04 — verify).
  - The `commands::version::run` text rendering goes through
    `ctx.ui.write_version` (refactor if it currently uses
    `println!`).
  - Every config / show-local handler renders via `ctx.ui.*`.

- `src/ui/color.rs` exists, exposing:

  ```rust
  pub(crate) enum Stream { Stdout, Stderr }
  pub(crate) fn should_color(stream: Stream) -> bool;
  pub(crate) fn stderr_color() -> bool;   // shorthand for should_color(Stream::Stderr)
  pub(crate) fn stdout_color() -> bool;   // shorthand for should_color(Stream::Stdout)
  ```

  Precedence in `should_color`:
  1. If `NO_COLOR` env var is set (any non-empty value), return
      `false`. This is unconditional and overrides everything.
  2. Else if `FORCE_COLOR` or `CLICOLOR_FORCE` is set to a non-empty
      non-"0" value, return `true`.
  3. Else if the target stream is not a TTY (`!stream.is_terminal()`),
      return `false`.
  4. Else if `CLICOLOR=0`, return `false`.
  5. Else return `true`.

- `Ui` exposes the rendering methods callers need:

  ```rust
  impl Ui {
      pub(crate) fn write_help(&self, body: &str) -> std::io::Result<()>;
      pub(crate) fn write_version(&self, w: &VersionView, fmt: OutputFormat) -> std::io::Result<()>;
      pub(crate) fn write_config_status(&self, s: &ConfigStatusView, fmt: OutputFormat) -> std::io::Result<()>;
      pub(crate) fn write_show_local(&self, s: &ShowLocalView, fmt: OutputFormat) -> std::io::Result<()>;
      pub(crate) fn write_dry_run(&self, body: &str) -> std::io::Result<()>;
      // ... etc.
  }
  ```

  All write to `stdout`. Color is conditionally enabled via
  `crate::ui::color::stdout_color()`.

- Domain "view" types (`VersionView`, `ConfigStatusView`,
  `ShowLocalView`, ...) live in `src/domain/` or
  `src/commands/<name>.rs`. They are pure data — the projection
  from `Config`+`adapters` into "what to render" — and they have
  no I/O. The `Ui` methods turn them into bytes.

- The error renderer (`src/error.rs::render`) is the **one
  permitted exception** to "no writes outside `ui/`" because errors
  are written to stderr in a controlled way that does not need
  domain-aware formatting. It must:
  - Honor `NO_COLOR` etc. via `crate::ui::color::stderr_color()`.
  - Honor `--silent` (Phase 06 already wired this).

## Spec rationale

- Stdout = data, stderr = diagnostics —
  `cli-design/01-logging-and-output.md:25-29, 53-66`.
- No prints outside `ui/` —
  `rust/cli-spec/09-coding-style.md:80-89`,
  `rust/cli-spec/00-directory-tree.md:30`.
- Color precedence —
  `cli-design/01-logging-and-output.md:80-97`,
  `no-color.org`.
- `IsTerminal` is the stable stdlib API since Rust 1.70 (we're on
  MSRV 1.85): <https://doc.rust-lang.org/std/io/trait.IsTerminal.html>.

## Current state (verify before planning)

- `src/main.rs` (after Phase 01) still calls
  `std::io::stdout().lock().write_all(...)` for at least one
  early-exit help path. Verify by `rg -n 'stdout|stderr|println|eprintln' src/main.rs`.
- `src/logging.rs:55-59` checks **only** `NO_COLOR`. No
  `FORCE_COLOR`, no `isatty`. (Phase 06 may have added a stub call
  to `crate::ui::color::stderr_color()` that this phase implements.)
- `src/ui/mod.rs` (or `src/ui.rs`) exposes `Ui::new()` and a small
  set of write methods; verify which ones already exist.
- `src/commands/self_version.rs` (now `commands::version`) likely
  uses `ctx.ui.print_version(...)` or similar — verify and unify
  signatures.

## Target state

### `src/ui/color.rs`

```rust
//! Color policy.
//!
//! What this is: the single source of truth for "should we emit ANSI
//! color codes to this stream?".
//! What this is not: a color renderer. Callers ask `should_color`
//! and disable ANSI in their writer if the answer is `false`.

use std::io::IsTerminal as _;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Stream { Stdout, Stderr }

impl Stream {
    fn is_tty(self) -> bool {
        match self {
            Self::Stdout => std::io::stdout().is_terminal(),
            Self::Stderr => std::io::stderr().is_terminal(),
        }
    }
}

fn truthy_env(key: &str) -> bool {
    match std::env::var(key) {
        Ok(v) => !v.is_empty() && v != "0",
        Err(_) => false,
    }
}

pub(crate) fn should_color(stream: Stream) -> bool {
    // 1. NO_COLOR wins absolutely (any non-empty value).
    if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    // 2. Force flags win over isatty.
    if truthy_env("FORCE_COLOR") || truthy_env("CLICOLOR_FORCE") {
        return true;
    }
    // 3. Not a TTY → no color.
    if !stream.is_tty() {
        return false;
    }
    // 4. CLICOLOR=0 disables even on TTY.
    if std::env::var("CLICOLOR").as_deref() == Ok("0") {
        return false;
    }
    // 5. Default for TTY.
    true
}

pub(crate) fn stdout_color() -> bool { should_color(Stream::Stdout) }
pub(crate) fn stderr_color() -> bool { should_color(Stream::Stderr) }
```

### `src/ui/mod.rs` consolidation

```rust
//! Human-facing terminal output. The only module allowed to write to
//! stdout. Errors go through `src/error.rs::render` to stderr.

pub(crate) mod color;

use crate::cli::OutputFormat;
use std::io::Write as _;

pub(crate) struct Ui;

impl Ui {
    pub(crate) const fn new() -> Self { Self }

    pub(crate) fn write_help(&self, body: &str) -> std::io::Result<()> {
        let mut out = std::io::stdout().lock();
        out.write_all(body.as_bytes())?;
        if !body.ends_with('\n') { writeln!(out)?; }
        out.flush()
    }

    pub(crate) fn write_dry_run(&self, body: &str) -> std::io::Result<()> {
        // (Phase 04 may have a version of this; consolidate.)
        let mut out = std::io::stdout().lock();
        out.write_all(body.as_bytes())?;
        out.flush()
    }

    // Domain-specific renderers below. Each takes a pure view + a
    // format and prints to stdout. Color enable is conditional on
    // crate::ui::color::stdout_color().

    pub(crate) fn write_version(&self, v: &VersionView, fmt: OutputFormat) -> std::io::Result<()> {
        let mut out = std::io::stdout().lock();
        match fmt {
            OutputFormat::Text => {
                writeln!(out, "{} {}", v.wrapper.name, v.wrapper.version)?;
                writeln!(out, "{} {} {}", v.child.name, v.child.path, v.child.version.as_deref().unwrap_or("(unknown)"))?;
            }
            OutputFormat::Json => {
                serde_json::to_writer_pretty(&mut out, v)?;
                writeln!(out)?;
            }
        }
        out.flush()
    }

    // ... write_config_status, write_show_local, ...
}

#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct VersionView {
    pub(crate) wrapper: BinaryView,
    pub(crate) child:   ChildBinaryView,
}
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct BinaryView { pub(crate) name: String, pub(crate) version: String }
#[derive(Debug, Clone, serde::Serialize)]
pub(crate) struct ChildBinaryView { pub(crate) name: String, pub(crate) path: camino::Utf8PathBuf, pub(crate) version: Option<String> }
```

Move existing `VersionView`-like types into this module (or
`src/domain/` if they fit better there — both are spec-acceptable;
keep the projection close to the renderer).

### `src/error.rs::render`

```rust
pub(crate) fn render<W: std::io::Write>(w: &mut W, e: &AppError) -> std::io::Result<()> {
    let use_color = crate::ui::color::stderr_color();
    // ... build the message with or without ANSI.
}
```

The current renderer is already in `src/error.rs`; adjust the color
gating to consult the new helper.

## Tasks

1. **Create `src/ui/color.rs`** per the target above.

2. **Update `src/logging.rs`** so the `Pretty` stderr mirror's
    `with_ansi(...)` argument comes from
    `crate::ui::color::stderr_color()`. Phase 06 may have left a
    stub call; replace with the real implementation.

3. **Update `src/error.rs::render`** to consult
    `crate::ui::color::stderr_color()` for ANSI gating.

4. **Audit `src/` for direct stdout/stderr writes.** Run:

    ```bash
    rg -n '\b(println!|eprintln!|print!|eprint!|stdout\(|stderr\()' src/ \
      | grep -v 'src/ui/' | grep -v 'src/error.rs'
    ```

    Every remaining hit is a violation. Fix:
    - `src/main.rs`: move any help-text or version-text write into
      `Ui::write_help` / `Ui::write_version`.
    - `src/commands/*.rs`: every renderer call must go through
      `ctx.ui.*`. If a domain "view" type is missing, create it.

5. **Standardize the version-rendering path.** `--version` (the
    clap flag) and `version` (the subcommand) must call the SAME
    renderer. Recommended: clap's `--version` is suppressed via
    `disable_version_flag = true` on `Cli`, AND we add `-V/--version`
    manually as a `bool` flag on `GlobalArgs`. When that flag is
    set, the dispatch routes to `commands::version::run` with a
    default `OutputFormat::Text`.

    (Phase 02 deferred this decision; Phase 07 finalizes it.)

6. **Update help text.** `src/ui/help.txt` should document the
    color env vars (`NO_COLOR`, `FORCE_COLOR`, `CLICOLOR`,
    `CLICOLOR_FORCE`) in its Environment section.

7. **Snapshot helpers.** Add `tests/support/color.rs` exposing:

    ```rust
    pub(crate) fn clear_color_env(cmd: &mut Command) {
        cmd.env_remove("NO_COLOR")
          .env_remove("FORCE_COLOR")
          .env_remove("CLICOLOR")
          .env_remove("CLICOLOR_FORCE");
    }
    ```

    Existing `tests/support/mod.rs` should call this in its
    env-clear setup.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes.
- [ ] The audit grep
  `rg -n 'println!|eprintln!|stdout\(|stderr\(' src/ | grep -v 'src/ui/' | grep -v 'src/error.rs' | grep -v 'src/logging.rs'`
  returns nothing.
- [ ] `src/ui/color.rs` exists and implements the full precedence.
- [ ] `NO_COLOR=1 codex-session --help` produces no ANSI escapes
  in stdout.
- [ ] `FORCE_COLOR=1 codex-session --help | cat` (piped, no TTY)
  produces ANSI escapes in stdout.
- [ ] `CLICOLOR=0 codex-session --help` (on a TTY) produces no
  ANSI escapes.
- [ ] `codex-session --version` and `codex-session version`
  produce byte-identical output (both formats).
- [ ] `tests/cmd_root_help.rs` snapshot regenerated with
  no-color (env-clear ensures stable snapshots regardless of
  user env).

## Tests

- Add `tests/cmd_color_policy.rs`:
  - Table-driven: set each env-var combination, capture stderr
    output of a single tracing line, assert presence/absence of
    `\x1b[`.
- Update `tests/cmd_root_help.rs` to set `NO_COLOR=1` so the
  snapshot is stable.
- Add `tests/cmd_version_parity.rs`: asserts `codex-session
  --version` stdout == `codex-session version` stdout (text and
  JSON each).

## Out of scope

- Adding a `--no-color` CLI flag. `NO_COLOR=1` is the standard
  mechanism; a CLI flag is redundant. Skip unless a user requests it.
- A custom theme system. The `Ui` only needs `should_color: bool` —
  any further styling decisions are deferred.
- Removing `src/logging.rs`'s direct stderr layer write — that is
  the spec-prescribed location for the stderr mirror and is allowed
  per `rust/cli-spec/04-logging.md`. The audit grep above
  whitelists `src/logging.rs`.

## References

- `rust/cli-spec/09-coding-style.md:80-89` — no prints outside `ui/`.
- `cli-design/01-logging-and-output.md:25-29, 53-66, 80-97` — stdout/stderr discipline, color precedence.
- `no-color.org` — the NO_COLOR convention.
- BurntSushi's `bat` color matrix (reference impl): <https://github.com/sharkdp/bat/issues/1377>.
- `std::io::IsTerminal`: <https://doc.rust-lang.org/std/io/trait.IsTerminal.html>.
