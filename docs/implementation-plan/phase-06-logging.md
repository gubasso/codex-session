# Phase 06 — Logging: `tracing-appender`, `--quiet` / `--silent`, `--log-format`

**One-line summary.** Replace the synchronous `Arc<Mutex<File>>` log
sink with a non-blocking `tracing-appender` rolling writer. Add
`-q` / `--quiet` / `--silent` to suppress stderr output, and
`--log-format text|json` to control the stderr mirror's shape.

## Prerequisites

Phases 01–03 must be complete (`Config` available so log settings can
be read from the config layer).

## Goal

After this phase:

- `Cargo.toml` lists `tracing-appender = "0.2"`.
- `src/logging.rs` uses `tracing_appender::rolling::daily(dir, "codex-session.log")`
  wrapped in `tracing_appender::non_blocking(...)`. The
  `Arc<Mutex<File>>` writer and the custom `JsonLogWriter` /
  `LockedFileWriter` are deleted.
- `tracing_appender::non_blocking::WorkerGuard` is held on `LogInit`
  and kept alive for the process lifetime (must drop on shutdown
  so the writer thread flushes).
- Daily log rotation is enabled. The active log file is named
  `codex-session.log.<YYYY-MM-DD>` (per `tracing-appender`'s
  rolling format). The "current" symlink-style behavior is **not**
  required.
- `GlobalArgs` gains three new flags:
  - `-q` / `--quiet`: suppress all stderr **except** errors (i.e.
    suppress info/debug/trace stderr mirror; the file sink is
    unchanged).
  - `--silent`: suppress all stderr (including errors). The wrapper
    still exits with the error's exit code, but emits nothing on
    stderr.
  - `--log-format text|json`: control the stderr mirror's format.
    Default `text` (pretty) when stderr is a TTY; default `json`
    when it isn't. The file sink is **always** JSON regardless
    (per `rust/cli-spec/04-logging.md:85-94`: file is JSON for
    LLM-friendliness, stderr mirror is pretty by default).
- The existing `-v` / `--verbose` (counted) and `--log-stderr` flags
  stay. `--log-stderr` is now equivalent to `--log-format text` when
  stderr is non-TTY, and is a no-op otherwise.
- `--quiet` and `--silent` are mutually exclusive (`#[arg(conflicts_with)]`).
- `--quiet` + `-v` is allowed but `-v` wins for log filter
  (verbose overrides quiet for the FILE sink; quiet still
  suppresses the STDERR sink). Document this in the flag help text.
- `RUST_LOG` env var still overrides `-v` per the existing behavior
  (`src/logging.rs:23-24`: `EnvFilter::try_from_default_env`).
- The `tests/cmd_logging.rs` integration test passes with the new
  appender. Snapshots for log records may need regeneration with
  `cargo insta review`.

## Spec rationale

- `tracing-subscriber` + `tracing-appender` are mandatory for
  binaries — `rust/cli-spec/07-dependencies.md:21-22`.
- Non-blocking writer + rotation —
  `rust/cli-spec/04-logging.md:21-25, 40-80`.
- `--quiet` / `--silent` / log-format control —
  `cli-design/01-logging-and-output.md:101-152`.
- File sink JSON + pretty stderr mirror is the Rust-specific variant
  — `rust/cli-spec/04-logging.md:85-94`. (The general doc allows
  identical formats; the Rust doc prefers split. We chose the split
  variant in the consolidated decisions doc.)
- `EnvFilter` from `RUST_LOG` — `rust/cli-spec/04-logging.md:60-78`.

## Current state (verify before planning)

- `src/logging.rs:1-99` opens the log file with `OpenOptions`, wraps
  in `Arc<Mutex<File>>`, and implements `MakeWriter` with a custom
  `JsonLogWriter` (returns `LockedFileWriter` from `lock()`).
- No rotation.
- No `tracing-appender` in `Cargo.toml`.
- `GlobalArgs` has only `-v/--verbose` and `--log-stderr` (Phase
  01–04 may have added `--dry-run` and `--config` but no quiet /
  silent / format).
- The stderr mirror is unconditionally pretty + colored (the only
  color knob is `NO_COLOR`, fixed in Phase 07).
- `Config::log.format` was added in Phase 03 with values
  `Json | Pretty`. Defaults to `Json` for the file sink (already
  enforced); the stderr mirror format defaults are set by this
  phase.

## Target state

### `Cargo.toml`

```toml
tracing-appender = "0.2"
```

### `src/logging.rs` (replacement skeleton)

```rust
//! tracing-subscriber installation. Called once from `main`.
//!
//! What this is: non-blocking, rolling file sink + optional stderr
//! mirror with format control.
//! What this is not: a logging API — emit `tracing::info!` etc.
//! everywhere; this module only installs the subscriber.

use crate::config::{Config, LogFormat};
use camino::Utf8Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

#[must_use = "WorkerGuard must be kept alive for log flushing"]
pub(crate) struct LogInit {
    _file_guard: WorkerGuard,
}

pub(crate) struct LogOptions<'a> {
    pub(crate) verbose: u8,
    pub(crate) log_file: &'a Utf8Path,
    pub(crate) mirror: StderrMirror,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum StderrMirror {
    Off,
    Pretty,
    Json,
}

impl StderrMirror {
    pub(crate) fn from_cli(quiet: bool, silent: bool, log_stderr: bool,
                            format: Option<LogFormat>, verbose: u8) -> Self {
        if silent { return Self::Off; }
        if quiet { return Self::Off; }    // quiet suppresses stderr mirror; file sink unaffected
        let want = log_stderr || verbose > 0;
        if !want { return Self::Off; }
        let fmt = format.unwrap_or_else(|| {
            use std::io::IsTerminal as _;
            if std::io::stderr().is_terminal() { LogFormat::Pretty } else { LogFormat::Json }
        });
        match fmt {
            LogFormat::Pretty => Self::Pretty,
            LogFormat::Json   => Self::Json,
        }
    }
}

pub(crate) fn init(opts: LogOptions<'_>) -> anyhow::Result<LogInit> {
    let default_directive = match opts.verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(default_directive));

    let dir = opts.log_file.parent()
        .ok_or_else(|| anyhow::anyhow!("log file has no parent: {}", opts.log_file))?;
    std::fs::create_dir_all(dir.as_std_path())?;

    let file_name = opts.log_file.file_name()
        .unwrap_or("codex-session.log");
    let appender = rolling::daily(dir.as_std_path(), file_name);
    let (file_writer, file_guard) = tracing_appender::non_blocking(appender);

    let file_layer = fmt::layer()
        .with_writer(file_writer)
        .with_ansi(false)
        .with_target(true)
        .json();

    let registry = tracing_subscriber::registry().with(filter).with(file_layer);

    match opts.mirror {
        StderrMirror::Off => registry.try_init()?,
        StderrMirror::Pretty => {
            let want_color = crate::ui::color::stderr_color();   // Phase 07 helper
            registry.with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(false)
                    .with_ansi(want_color),
            ).try_init()?;
        }
        StderrMirror::Json => {
            registry.with(
                fmt::layer()
                    .with_writer(std::io::stderr)
                    .with_target(true)
                    .json(),
            ).try_init()?;
        }
    }

    Ok(LogInit { _file_guard: file_guard })
}
```

NOTE: `tracing-appender`'s rolling file uses the supplied `file_name`
as a **prefix**; the actual file becomes `<file_name>.<YYYY-MM-DD>`.
This is acceptable; users may set `Config::log.file =
"<state_dir>/codex-session.log"` and find
`codex-session.log.2026-05-18` etc. in `<state_dir>`. Document this
in the `--help` text and config schema.

If you must keep a single non-rotated file (e.g. legacy compatibility),
use `tracing_appender::rolling::never(dir, file_name)` instead of
`daily(...)`. The default in this phase is `daily`; allow override
via `Config::log.rotation = never|daily|hourly` if useful (but defer
that unless trivial).

### `GlobalArgs` additions

```rust
#[derive(Debug, Default, Clone, Copy, clap::Args)]
pub(crate) struct GlobalArgs {
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub(crate) verbose: u8,
    #[arg(long, global = true)]
    pub(crate) log_stderr: bool,
    #[arg(long, global = true)]
    pub(crate) dry_run: bool,

    /// Suppress non-error stderr output. The log file is unaffected.
    #[arg(short = 'q', long, global = true, conflicts_with = "silent")]
    pub(crate) quiet: bool,

    /// Suppress ALL stderr including errors. The log file is unaffected.
    #[arg(long, global = true)]
    pub(crate) silent: bool,

    /// Format for the stderr mirror. Default: pretty if stderr is a TTY,
    /// json otherwise. The log file is always JSON.
    #[arg(long, value_enum, global = true, value_name = "FMT")]
    pub(crate) log_format: Option<crate::config::LogFormat>,
}
```

(`LogFormat` already exists in `src/config/mod.rs` from Phase 03;
re-derive `clap::ValueEnum` on it.)

### `main.rs` wiring

```rust
let opts = logging::LogOptions {
    verbose:  cli.global.verbose.max(config.log.verbose),
    log_file: &config.log.file_or_default(),
    mirror:   logging::StderrMirror::from_cli(
        cli.global.quiet, cli.global.silent, cli.global.log_stderr,
        cli.global.log_format, cli.global.verbose,
    ),
};
let _log = logging::init(opts)?;
```

(Phase 03 stored `log.file` as `Option<Utf8PathBuf>` on `LogConfig`;
provide a `file_or_default()` method that returns the configured
path or the XDG default.)

### Error-rendering and `--silent`

`src/error.rs::render` must check `ctx.global.silent` (or whatever
mechanism passes the silent flag down) and emit **nothing** on
stderr when silent is set. The exit code is still the error's
`exit_code()` so scripts can still detect failure via `$?`.

Currently `print_and_exit` does not have access to `global`. Two
options:

1. (Cleaner) Thread `&GlobalArgs` through `print_and_exit`. The
    call site has `cli.global` in scope in `main`.
2. (Pragmatic) Read `cli.global.silent` once in `main` and stash
    it as a `static` `OnceLock<bool>`. The error renderer consults
    it.

Choose option 1 — it's a small signature change and avoids global
state.

## Tasks

1. **Add `Cargo.toml` dep** `tracing-appender = "0.2"`.

2. **Rewrite `src/logging.rs`** per the target above. Delete
    `JsonLogWriter` and `LockedFileWriter`. The `init` signature
    changes; update the one call site in `main.rs`.

3. **Add `-q`/`--quiet`, `--silent`, `--log-format`** to
    `GlobalArgs`. Mutual exclusion via `conflicts_with`.

4. **Re-derive `clap::ValueEnum` on `LogFormat`** in
    `src/config/mod.rs` so the `--log-format` flag accepts `text`
    (renamed alias?) and `json`. Provide a `serde(rename_all =
    "lowercase")` AND `clap(rename_all = "lowercase")` so both file
    and CLI use the same string.

    Decision: the CLI value names are `pretty` and `json` (not
    `text` and `json`). This matches `LogFormat::Pretty` and
    matches tracing's nomenclature. Update `GlobalArgs` doc text
    accordingly.

5. **Update `Config::log` defaults** so `LogConfig::format` defaults
    to `LogFormat::Json` (file sink) and add `LogConfig::stderr_format:
    Option<LogFormat>` for the stderr-mirror override (`None` =
    auto-detect from TTY). Wire through `CliOverrides::log.format`
    to override `stderr_format`.

6. **Thread `silent` into error rendering.** Change `print_and_exit`
    to accept `&GlobalArgs` (or just a `bool silent`). When
    `silent`, skip the `stderr.write` step but still log the error
    to the file via `error::log_error(e)`.

7. **Replace the `paths.log_path_degraded` warning** (from the old
    `domain/paths.rs`) — since `directories::ProjectDirs` is now the
    source of truth, the "degraded path" condition is rarer (only
    `ProjectDirs::from` returns `None`). Drop the warning emit unless
    the loader explicitly couldn't find a state dir; in that case,
    surface `ConfigError::NoXdg` at startup and exit.

8. **Update `tests/cmd_logging.rs`.** The test currently checks that
    tracing output reaches the log file. With rotation, the file
    name now includes a date suffix; adjust the assertion to find
    `*.log.*` under the log dir, or set
    `Config::log.rotation = "never"` in a test helper if you
    exposed that knob.

9. **Snapshot the new help output.** `cargo insta review` after
    running the test suite. The `--help` snapshot now includes the
    three new flags.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes (with `cargo insta review` for any
  regenerated snapshots).
- [ ] `Cargo.toml` lists `tracing-appender`.
- [ ] `src/logging.rs` no longer contains `Arc<Mutex<File>>` or
  `MakeWriter` impls.
- [ ] `LogInit` holds a `WorkerGuard` and dropping it flushes the
  appender.
- [ ] `codex-session -q version 2>&1 >/dev/null` produces no
  stderr output when `version` succeeds.
- [ ] `codex-session --silent self version 2>&1` (note: `self` is
  rejected post-Phase-02; substitute a real failing command, e.g.
  `codex-session --silent --config /nonexistent/x.toml version`)
  produces empty stderr and exits non-zero.
- [ ] `codex-session -v --log-format json` writes JSON lines to
  stderr.
- [ ] `codex-session -v` (no `--log-format`) writes pretty lines to
  stderr when stderr is a TTY (manual check) and JSON when stderr
  is piped.
- [ ] Log files appear under `<state_dir>/codex-session.log.<DATE>`.

## Tests

- Update `tests/cmd_logging.rs` to:
  - assert a log file matching `codex-session.log.*` is created
    under `<state_dir>`.
  - assert `-q` suppresses stderr while preserving the file sink.
  - assert `--silent` produces empty stderr even on error.
  - assert `--log-format json` produces parseable JSON lines on
    stderr.

## Out of scope

- Color policy (`NO_COLOR`/`FORCE_COLOR`/isatty precedence) — that
  is Phase 07. This phase uses a stub `crate::ui::color::stderr_color()`
  helper that may be a placeholder returning `true`; Phase 07
  implements the real precedence.
- Moving the `main.rs:80-83` stdout write into `Ui`. (Phase 07.)
- Log-format selection from project TOML (already wired via Phase
  03's `Config::log.format` for the file sink).

## References

- `rust/cli-spec/04-logging.md` — full Rust logging chapter.
- `rust/cli-spec/07-dependencies.md:21-22` — required crates.
- `cli-design/01-logging-and-output.md:101-152` — quiet/silent/format conventions, two-layer logging.
- `tracing-appender` docs: <https://docs.rs/tracing-appender/latest/tracing_appender/>.
- `tracing-subscriber` `EnvFilter`: <https://docs.rs/tracing-subscriber/latest/tracing_subscriber/filter/struct.EnvFilter.html>.
- `RUST_LOG` directive syntax: <https://docs.rs/env_logger/latest/env_logger/#enabling-logging>.
