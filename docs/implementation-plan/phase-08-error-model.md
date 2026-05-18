# Phase 08 — Error model: `Usage(clap::Error)`, `ConfigError` rung, structured fields

**One-line summary.** Replace `AppError::Usage(String)` with
`Usage(clap::Error)`. Add `Config(#[from] ConfigError)` as a real
error rung. Expand structured logging fields with `err.path` and
`err.line` from `figment` provenance where available.

## Prerequisites

Phases 01–07 must be complete. `ConfigError` already exists (Phase
03); `AppError` already aggregates `FsError`, `ProcessError`/
`SpawnerError`, `MergeError`.

## Goal

After this phase:

- `AppError::Usage` carries a real `clap::Error` (not a `String`).
  Rendering uses `clap::Error::render()`. Exit code: 64 (`EX_USAGE`)
  — same as today, but the message reproduction is now lossless
  and clap-styled.
- `AppError::Config(#[from] crate::config::ConfigError)` exists.
  Exit code: 78 (`EX_CONFIG`).
- The "temporary bridge" in Phase 03 (`AppError::Other(anyhow!(err))`
  when `Config::load` fails) is replaced by the direct
  `AppError::Config(err)` path.
- Structured logging fields emitted by `error::log_error`:
  - `err.kind` (existing, stable strings)
  - `err.msg` (existing)
  - `err.path` (NEW; populated for `Config`, `Fs`, `Merge`,
    `ChildNotFound`/`ChildNotExecutable`, `ChildRecursion` —
    the offending file or binary path)
  - `err.line` (NEW; populated where `figment` provides line/col
    metadata — primarily `ConfigError::Parse`)
  - `err.hint` (existing, where present)
- The error renderer (`src/error.rs::render`) emits a structured
  multi-line message that includes the path and line when present:

  ```
  error: config: parse error in /home/user/.config/codex-session/config.toml
    at line 12, column 5
    unknown field `bogus_key`
  hint: see codex-session config show-local for the schema.
  ```

- `AppError::kind()` returns a stable string per spec
  (`cli-design/02-error-messages.md:160-189`) — see the table below.
- Every variant of `AppError` has a deterministic `exit_code()`
  mapping per BSD sysexits.
- `AppError` is exhaustively pattern-matched everywhere — no `_ =>`
  fallbacks. (Clippy's `non_exhaustive_omitted_patterns` lint can
  help; or just grep `rg 'match.*AppError' src/`.)

## Spec rationale

- Typed errors per layer with `#[from]` aggregation —
  `cli-design/02-error-messages.md:56-72`,
  `rust/cli-spec/03-error-handling.md:103-214`.
- Stable `err.kind` for LLM consumption —
  `cli-design/02-error-messages.md:160-189`.
- Structured fields (`err.path`, `err.line`) —
  `cli-design/02-error-messages.md:76-101`.
- Exit codes (sysexits) —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:206-260`.
- `clap::Error::render()` for usage errors —
  `clap` docs (do not stringify clap errors; let clap render them).

## Current state (verify before planning)

- `src/error.rs::AppError::Usage(String)` (current shape from
  pre-refactor tree, kept through Phases 01–07 because the prior
  signature was already a `String`). Verify.
- `Config::load` in `src/config/mod.rs` returns `ConfigError`. The
  caller in `main.rs` wraps it as `AppError::Other(anyhow!(err))`.
- `AppError` already has `Process(#[from] SpawnerError)` (Phase 04
  renamed from `ProcessError`).
- Structured fields currently emitted (verify by reading
  `src/error.rs::log_error`): `err.kind`, `err.msg`,
  `error.where`, `error.hint`. The names `error.where`/`error.hint`
  should be normalized to `err.path`/`err.hint` per the spec.

## Target state

### `src/error.rs`

```rust
//! Crate-level error type and exit-code mapping.

use std::ffi::OsString;
use camino::Utf8PathBuf;

#[derive(Debug, thiserror::Error)]
pub(crate) enum AppError {
    /// CLI parse failure (from clap).
    #[error("{0}")]
    Usage(#[from] clap::Error),

    /// Config layer (load / parse / unknown key).
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),

    /// Missing `codex` on PATH or via override.
    #[error("failed to resolve wrapped codex binary")]
    ChildNotFound { tried: Utf8PathBuf, path_searched: Option<OsString> },

    /// Resolved child path is not executable.
    #[error("wrapped codex binary is not executable")]
    ChildNotExecutable { path: Utf8PathBuf },

    /// Child resolves to the wrapper itself.
    #[error("child binary resolves to the wrapper itself")]
    ChildRecursion { path: Utf8PathBuf },

    /// Missing base config for forced merge.
    #[error("failed to load base config")]
    BaseMissing(Utf8PathBuf),

    /// Process-layer failure after dispatch.
    #[error(transparent)]
    Process(#[from] crate::adapters::spawner::SpawnerError),

    /// Filesystem adapter failure.
    #[error(transparent)]
    Fs(#[from] crate::adapters::fs::FsError),

    /// Merge service failure.
    #[error(transparent)]
    Merge(#[from] crate::services::merge::MergeError),

    /// Unexpected I/O failure (last resort).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    /// Opaque application-edge failure.
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl AppError {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::Usage(_)              => "usage",
            Self::Config(e)             => e.kind(),     // delegated
            Self::ChildNotFound { .. }  => "child-not-found",
            Self::ChildNotExecutable { .. } => "child-not-executable",
            Self::ChildRecursion { .. } => "child-recursion",
            Self::BaseMissing(_)        => "base-missing",
            Self::Process(e)            => spawner_kind(e),
            Self::Fs(e)                 => fs_kind(e),
            Self::Merge(_)              => "merge-failed",
            Self::Io(e) if e.kind() == std::io::ErrorKind::NotFound => "io-not-found",
            Self::Io(e) if e.kind() == std::io::ErrorKind::PermissionDenied => "io-permission-denied",
            Self::Io(_)                 => "io-other",
            Self::Other(_)              => "internal",
        }
    }

    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(e)              => clap_exit_code(e),    // 64 typically; 0 for --help
            Self::Config(_)             => 78,
            Self::ChildNotFound { .. }  => 127,
            Self::ChildNotExecutable { .. } => 126,
            Self::ChildRecursion { .. } => 70,
            Self::BaseMissing(_)        => 66,
            Self::Process(_)            => 74,
            Self::Fs(_)                 => 74,
            Self::Merge(_)              => 70,
            Self::Io(_)                 => 74,
            Self::Other(_)              => 70,
        }
    }
}

fn clap_exit_code(e: &clap::Error) -> u8 {
    use clap::error::ErrorKind;
    match e.kind() {
        // Help / version are not errors.
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion => 0,
        _ => 64,
    }
}
```

### Structured logging

`error::log_error(e: &AppError)` should emit:

```rust
let path = error_path(e);     // Option<Utf8PathBuf>
let line = error_line(e);     // Option<usize>  (figment col/line)
let hint = error_hint(e);     // Option<&'static str>

tracing::error!(
    op = "app.error",
    err.kind = e.kind(),
    err.msg = %e,
    err.path = path.as_ref().map(camino::Utf8PathBuf::as_str),
    err.line = line,
    err.hint = hint,
    "{}", e
);
```

Where:

- `error_path`:
  - `ChildNotFound { tried, .. }` / `ChildNotExecutable { path }` /
    `ChildRecursion { path }` → `Some(path.clone())`.
  - `BaseMissing(p)` → `Some(p.clone())`.
  - `Config(ConfigError::Parse { path, .. })` / `UnknownKey { path, .. }`
    → `Some(path.clone())`.
  - `Fs(FsError::*)` → the offending path if the variant carries one.
  - `Merge(_)` → the offending path.
  - else → `None`.

- `error_line`:
  - `Config(ConfigError::Parse { source, .. })`: parse `source`
    (a `figment::Error`) and extract a line/column if present.
    `figment::Error` carries metadata via `metadata` and a path
    via `path`. See <https://docs.rs/figment/latest/figment/struct.Error.html>.
  - else → `None`.

- `error_hint`:
  - `ChildNotFound { .. }` → `Some("Set CODEX_SESSION_CHILD_BIN or install `codex` on PATH.")`.
  - `ChildRecursion { .. }` → `Some("Unset CODEX_SESSION_CHILD_BIN or point it at the real `codex`.")`.
  - `Config(ConfigError::UnknownKey { .. })` → `Some("Run `codex-session config status` to see the schema.")`.
  - else → `None`.

### `src/config/error.rs`

Extend `ConfigError` with a `kind()` method:

```rust
impl ConfigError {
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Self::NoXdg => "config-no-xdg",
            Self::Parse { .. } => "config-parse",
            Self::UnknownKey { .. } => "config-unknown-key",
            Self::Io(_) => "config-io",
            Self::Figment(_) => "config-figment",
            Self::NonUtf8Path(_) => "config-non-utf8-path",
        }
    }
}
```

### `print_and_exit`

```rust
fn print_and_exit(global: &GlobalArgs, e: &AppError) -> ExitCode {
    error::log_error(e);
    if !global.silent {
        if let AppError::Usage(clap_err) = e {
            // clap renders its own colored, multi-line message
            let _ = clap_err.print();
        } else {
            let mut stderr = std::io::stderr().lock();
            let _ = error::render(&mut stderr, e);
        }
    }
    ExitCode::from(e.exit_code())
}
```

`clap::Error::print()` returns the help/version body to stdout for
`DisplayHelp`/`DisplayVersion` kinds (exit 0) and to stderr for real
errors (exit 64). The exit code in those two cases is 0 — handled
by `clap_exit_code`.

## Tasks

1. **Change `AppError::Usage`** from `Usage(String)` to
    `Usage(#[from] clap::Error)`. Update every call site that
    constructs `AppError::Usage(...)`. After Phase 01 deleted the
    manual clap-parse machinery, the only construction sites should
    be the `#[from]` conversions and possibly one or two explicit
    wraps in `main.rs`.

2. **Add `AppError::Config(#[from] ConfigError)`.** Remove the
    temporary `AppError::Other(anyhow!(err))` bridge from Phase 03.
    `main.rs::Config::load()` failures now propagate via `?`.

3. **Add `AppError::ChildRecursion { path }`** if not already added
    in Phase 04/05. Wire to `exit_code()` (70) and `kind()`
    (`"child-recursion"`).

4. **Add `kind()` to `ConfigError`** per the target above.

5. **Update `kind()` and `exit_code()`** on `AppError` per the
    target. Make sure the `match` is exhaustive (no `_ =>`).

6. **Rewrite `error::log_error`** to use the new structured fields
    (`err.path`, `err.line`, `err.hint`). Rename `error.where` →
    `err.path` and `error.hint` → `err.hint` to match the spec.

7. **Rewrite `error::render`** to render the multi-line shape shown
    above. Color via `crate::ui::color::stderr_color()`. The path
    line uses bold/cyan if colored. Always include `hint:` line when
    the variant has one.

8. **Update `print_and_exit`** per the target. Move it from
    `main.rs` into `src/error.rs` and rename to
    `error::print_and_exit` (cleanup task from Phase 01).

9. **Add `figment` metadata extraction.** `ConfigError::Parse`
    should carry the `figment::Error` source. When extracting
    line/col, walk `source.metadata` for the topmost provider with
    path metadata. Sketch:

    ```rust
    fn figment_line(err: &figment::Error) -> Option<usize> {
        err.metadata.as_ref()
            .and_then(|m| m.source.as_ref())
            .and_then(|src| src.path.as_ref())
            .and_then(|_| err.path.last())  // rough; refine
            .and_then(|frag| frag.parse::<usize>().ok())
    }
    ```

    (The real extraction may need to traverse `figment::Error` more
    carefully; consult the docs link in References.)

10. **Update snapshots and existing tests.** Snapshots for usage
    errors now include clap's full rendered output (already done if
    Phase 02 regenerated them post-rename, but verify). Snapshots
    for config errors should include path and line where present.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes (including
  `match` exhaustiveness — no `_ =>`).
- [ ] `cargo test` passes.
- [ ] `rg -n 'AppError::Usage\(' src/` returns only `#[from]` impl
  and the print site — no string construction sites.
- [ ] `rg -n 'AppError::Other\(anyhow' src/` returns nothing for
  the `ConfigError` path (the bridge is removed).
- [ ] A malformed user TOML produces an error with `err.path` and
  `err.line` populated (verify by setting up a fixture and
  inspecting the log file).
- [ ] `codex-session --bogus-flag` produces clap's colored usage
  error on stderr with exit 64.
- [ ] `codex-session --help` exits 0 (clap's help kind).

## Tests

- Add `tests/cmd_config_error.rs`:
  - Set up `~/.config/codex-session/config.toml` with an unknown
    key and a syntax error.
  - Assert exit 78, stderr contains the file path, and (for the
    syntax error) the line number.
  - Inspect the log file (parse JSON lines) and assert the
    `err.kind`, `err.path`, `err.line` fields exist.

- Update `tests/cmd_self_rejected.rs` (from Phase 02) to assert
  exit 64 and a stderr containing clap's
  "unrecognized subcommand" wording.

## Out of scope

- Replacing `anyhow` entirely. `AppError::Other(#[from] anyhow::Error)`
  stays as the last-resort variant for genuinely opaque failures.
- Re-doing the entire `merge` service error story. `MergeError`
  stays as-is; its `kind()` is wired through `AppError::Merge` →
  `"merge-failed"` (unchanged from today).
- Adding I18n / translation infrastructure.

## References

- `cli-design/02-error-messages.md` — typed errors, structured fields, hint format.
- `rust/cli-spec/03-error-handling.md` — layered errors, `#[from]`, exit-code policy.
- `cli-design/06-cli-wrapper-design/process-and-posix.md:206-260` — exit-code policy.
- `clap` errors: <https://docs.rs/clap/latest/clap/error/struct.Error.html>.
- `figment` errors: <https://docs.rs/figment/latest/figment/struct.Error.html>.
