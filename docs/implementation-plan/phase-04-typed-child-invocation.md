# Phase 04 — Typed `ChildInvocation` + `Spawner` trait + `--dry-run`

**One-line summary.** Replace the stringly `Vec<OsString> → exec()`
passthrough with a typed `ChildInvocation` value, a `Spawner` trait
that hides `fork/exec/wait`, and a `--dry-run` flag that prints the
resolved invocation instead of executing it.

## Prerequisites

Phases 01–03 must be complete. `Config` must be on `AppContext` so
the child binary can be resolved once at startup and stored.

## Goal

After this phase:

- `src/domain/child_invocation.rs` defines:

  ```rust
  pub(crate) struct ChildInvocation {
      pub(crate) binary: camino::Utf8PathBuf,
      pub(crate) args: Vec<std::ffi::OsString>,
      pub(crate) env: ChildEnv,
  }
  pub(crate) struct ChildEnv {
      pub(crate) inherit: bool,           // start from parent env
      pub(crate) remove: Vec<String>,     // env_remove these
      pub(crate) set: Vec<(String, std::ffi::OsString)>,
  }
  impl ChildInvocation {
      pub(crate) fn into_command(self) -> std::process::Command { ... }
      pub(crate) fn dry_run_report(&self) -> String { ... }
  }
  ```

- `src/adapters/spawner.rs` defines the `Spawner` trait (replacing
  the old `Process` trait):

  ```rust
  pub(crate) trait Spawner {
      /// Replace the current process. Returns only on failure.
      fn exec(&self, inv: ChildInvocation) -> SpawnerError;

      /// Resolve the child binary path, using config + override + PATH.
      fn resolve_child(&self, cfg: &ChildConfig) -> Result<Utf8PathBuf, SpawnerError>;

      /// First non-empty line of `<child> --version`. Best-effort.
      fn child_version_line(&self, child: &Utf8Path) -> Option<String>;
  }
  ```

  The default implementation is `StdSpawner`.

- `AppContext` holds:

  ```rust
  pub(crate) spawner: StdSpawner,
  pub(crate) resolved_child: camino::Utf8PathBuf,  // resolved once in main
  ```

  (The pre-Phase-04 `process: StdProcess` field is removed.)

- `commands::pass_through::run` no longer builds `Command` directly;
  it builds a `ChildInvocation` and delegates to `ctx.spawner.exec`.

- A new top-level flag `--dry-run` causes `pass_through::run` to
  print `ChildInvocation::dry_run_report()` to stdout and exit 0
  **without** spawning the child.

- The dry-run report includes: resolved binary path, full argv
  (one per line, JSON-safe), and the scrubbed env diff (which env
  vars are inherited, which are removed, which are explicitly set).
  This is the pure-`to_args()` surface the spec mandates.

## Spec rationale

- Typed model with pure `to_args()`/`into_command()` boundary —
  `cli-design/06-cli-wrapper-design/typing-and-validation.md:9-25, 57-155`.
- `Spawner` trait + golden-argv tests + `--dry-run` —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:301-321`,
  `checklist.md:55-69`.
- Resolve the child **once** —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:180-182`.

## Current state (verify before planning)

- `src/adapters/process.rs::Process` trait has three methods:
  `resolve_codex`, `child_version_line`, `exec_replace`. Default
  impl is `StdProcess`.
- `src/adapters/process.rs:68-72` (`exec_replace`) calls
  `std::process::Command::new(program).args(args).exec()` with raw
  `&[OsString]`. No typed model.
- `src/commands/pass_through.rs` builds the argv `Vec<OsString>`
  from clap's `passthrough` field and hands it to
  `ctx.process.exec_replace(&child, &argv)`.
- No `--dry-run` flag exists anywhere.

## Target state

### `src/domain/child_invocation.rs`

The full skeleton:

```rust
//! Typed child invocation.
//!
//! What this is: the pure data describing how to invoke the child —
//! binary path, argv, env diff. Has no I/O.
//! What this is not: an executor. Use `Spawner` to actually run it.

use camino::Utf8PathBuf;
use std::ffi::OsString;

#[derive(Debug, Clone)]
pub(crate) struct ChildInvocation {
    pub(crate) binary: Utf8PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) env: ChildEnv,
}

#[derive(Debug, Clone)]
pub(crate) struct ChildEnv {
    pub(crate) inherit: bool,
    pub(crate) remove: Vec<String>,
    pub(crate) set: Vec<(String, OsString)>,
}

impl ChildEnv {
    /// The wrapper's private env keys that must never reach the child.
    pub(crate) fn scrubbed_default() -> Self {
        Self {
            inherit: true,
            remove: vec![
                "CODEX_SESSION_CHILD_BIN".into(),
                "CODEX_SESSION_LOG_FILE".into(),
                "CODEX_SESSION_LOG_DIR".into(),
                "CODEX_SESSION_REENTRY".into(),
            ],
            set: vec![],
        }
    }
}

impl ChildInvocation {
    /// Pure projection from typed invocation to std::process::Command.
    /// Tests snapshot the result of this function.
    pub(crate) fn into_command(self) -> std::process::Command {
        let mut cmd = std::process::Command::new(self.binary.as_std_path());
        cmd.args(&self.args);
        if !self.env.inherit {
            cmd.env_clear();
        }
        for key in &self.env.remove {
            cmd.env_remove(key);
        }
        for (k, v) in &self.env.set {
            cmd.env(k, v);
        }
        cmd
    }

    /// Human-readable report for --dry-run. Newline-terminated.
    pub(crate) fn dry_run_report(&self) -> String {
        // Format: binary line, argv lines (numbered or JSON-array),
        // env section (inherit y/n, remove list, set list).
        // Keep deterministic for snapshots.
        let mut out = String::new();
        out.push_str(&format!("binary: {}\n", self.binary));
        out.push_str("argv:\n");
        for (i, a) in self.args.iter().enumerate() {
            out.push_str(&format!("  [{}] {}\n", i, a.to_string_lossy()));
        }
        out.push_str(&format!("env.inherit: {}\n", self.env.inherit));
        if !self.env.remove.is_empty() {
            out.push_str("env.remove:\n");
            for k in &self.env.remove {
                out.push_str(&format!("  {}\n", k));
            }
        }
        if !self.env.set.is_empty() {
            out.push_str("env.set:\n");
            for (k, v) in &self.env.set {
                out.push_str(&format!("  {}={}\n", k, v.to_string_lossy()));
            }
        }
        out
    }
}
```

### `src/adapters/spawner.rs`

```rust
//! Process spawner adapter.
//!
//! What this is: the hexagonal port for fork/exec/wait.
//! What this is not: the typed invocation model — that's in
//! `domain/child_invocation.rs`.

use crate::config::ChildConfig;
use crate::domain::child_invocation::ChildInvocation;
use camino::{Utf8Path, Utf8PathBuf};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SpawnerError {
    #[error("child binary not found: tried {tried}")]
    NotFound { tried: Utf8PathBuf, path_searched: Option<std::ffi::OsString> },
    #[error("child binary not executable: {path}")]
    NotExecutable { path: Utf8PathBuf },
    #[error("exec failed: {0}")]
    Exec(#[from] std::io::Error),
    #[error("recursion guard tripped: child resolves to wrapper itself ({path})")]
    Recursion { path: Utf8PathBuf },
    #[error("non-utf8 path")]
    NonUtf8Path(#[from] camino::FromPathBufError),
}

pub(crate) trait Spawner {
    fn resolve_child(&self, cfg: &ChildConfig) -> Result<Utf8PathBuf, SpawnerError>;
    fn child_version_line(&self, child: &Utf8Path) -> Option<String>;
    fn exec(&self, inv: ChildInvocation) -> SpawnerError;
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct StdSpawner;

impl Spawner for StdSpawner {
    fn resolve_child(&self, cfg: &ChildConfig) -> Result<Utf8PathBuf, SpawnerError> {
        if let Some(p) = &cfg.bin {
            // metadata + executable bit check (existing logic from
            // adapters::process.rs:47-58).
            // No recursion check here — that's Phase 05.
            return Ok(p.clone());
        }
        let raw = which::which("codex").map_err(|_| SpawnerError::NotFound {
            tried: Utf8PathBuf::from("codex"),
            path_searched: std::env::var_os("PATH"),
        })?;
        Utf8PathBuf::try_from(raw).map_err(SpawnerError::from)
    }

    fn child_version_line(&self, child: &Utf8Path) -> Option<String> {
        // Keep the existing logic from adapters/process.rs:74-103
        // (best-effort, first non-empty line). The recursion check
        // currently in that function moves to Phase 05.
        ...
    }

    fn exec(&self, inv: ChildInvocation) -> SpawnerError {
        use std::os::unix::process::CommandExt as _;
        let mut cmd = inv.into_command();
        SpawnerError::from(cmd.exec())   // exec() only returns on failure
    }
}
```

### `GlobalArgs` gets a `--dry-run` flag

```rust
#[derive(Debug, Default, Clone, Copy, clap::Args)]
pub(crate) struct GlobalArgs {
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub(crate) verbose: u8,
    #[arg(long, global = true)]
    pub(crate) log_stderr: bool,
    /// Print resolved child invocation and exit 0 without running it.
    #[arg(long, global = true)]
    pub(crate) dry_run: bool,
    // (config file flag added in Phase 03)
}
```

### `commands::pass_through::run`

```rust
pub(crate) fn run(ctx: &AppContext, argv: &[OsString]) -> Result<(), AppError> {
    let inv = ChildInvocation {
        binary: ctx.resolved_child.clone(),
        args:   argv.to_vec(),
        env:    ChildEnv::scrubbed_default(),
    };

    if ctx.global.dry_run {
        ctx.ui.write_dry_run(&inv.dry_run_report())?;
        return Ok(());
    }

    let err = ctx.spawner.exec(inv);
    Err(AppError::from_spawner_error(err))
}
```

(`GlobalArgs` lives in `ctx.global` — thread the value through
`AppContext` as `pub(crate) global: GlobalArgs`. Take the copy at
context construction; `GlobalArgs` is `Copy`.)

### `Ui` adds `write_dry_run`

`src/ui/mod.rs` exposes:

```rust
pub(crate) fn write_dry_run(&self, body: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    let mut out = std::io::stdout().lock();
    out.write_all(body.as_bytes())?;
    out.flush()
}
```

(This is one of the only stdout writes; it belongs in `ui/` per the
spec's "all human output through `ui/`" rule.)

## Tasks

1. **Add `--dry-run` to `GlobalArgs`** in `src/cli/mod.rs`.

2. **Thread `GlobalArgs` onto `AppContext`** so commands can read it
    without re-parsing argv. Pass by value (it's `Copy`).

3. **Create `src/domain/child_invocation.rs`** per the target above.
    Register the module in `src/domain/mod.rs`.

4. **Create `src/adapters/spawner.rs`** per the target above.
    Register in `src/adapters/mod.rs`. Keep the existing
    `src/adapters/process.rs` file alive **for one step** so the
    migration is bisectable.

5. **Resolve child once in `main`.** After `Config::load`, call
    `StdSpawner::default().resolve_child(&config.child)` and store
    the result on `AppContext` as `resolved_child:
    camino::Utf8PathBuf`. If resolution fails, surface the
    `SpawnerError` via the existing `print_and_exit` path.

    (Exception: if the only invocation is a wrapper-owned verb that
    does NOT need the child — e.g. `codex-session help`,
    `codex-session config show-local` — resolution failure should
    not crash. Implement lazy resolution: store
    `resolved_child: OnceCell<Utf8PathBuf>` on the context, eager-
    resolve only for `Commands::Version` and passthrough. The simplest
    pattern is a `LazyChild` newtype wrapping `OnceCell`.)

6. **Migrate `commands::pass_through::run`** to build a
    `ChildInvocation` and call `ctx.spawner.exec`. Honor
    `ctx.global.dry_run`.

7. **Migrate `commands::version::run`** to use
    `ctx.spawner.child_version_line(&ctx.resolved_child)`. (The
    recursion guard inside `child_version_line` stays as-is for
    this phase — Phase 05 unifies the guards.)

8. **Update `src/error.rs`.** Add a helper
    `AppError::from_spawner_error(SpawnerError) -> AppError` that
    maps:

    - `SpawnerError::NotFound { .. }` → `AppError::ChildNotFound { .. }`
    - `SpawnerError::NotExecutable { path }` → `AppError::ChildNotExecutable { path }`
    - `SpawnerError::Recursion { path }` → new variant
      `AppError::ChildRecursion { path }` with exit code 70 (`EX_SOFTWARE`)
    - `SpawnerError::Exec(io)` → wraps as `AppError::Process(...)` (the existing variant)
    - `SpawnerError::NonUtf8Path(_)` → `AppError::Other(anyhow!(...))`

9. **Delete `src/adapters/process.rs`.** After every caller is
    migrated, remove the file and the `process` module from
    `src/adapters/mod.rs`. `AppContext::process` is gone; replaced
    by `AppContext::spawner` and `AppContext::resolved_child`.

10. **Add `tests/cmd_dry_run.rs`** asserting:

    - `codex-session --dry-run exec foo bar` exits 0.
    - stdout contains `binary:`, `argv:`, `env.inherit: true`, and
      shows the four `CODEX_SESSION_*` keys in `env.remove`.
    - the child is **not** invoked (set `CODEX_SESSION_CHILD_BIN`
      to a fake stub script that writes a marker file; assert the
      marker is absent).

11. **Add `tests/cmd_passthrough_argv.rs`** (golden-argv snapshot,
    promoted from Phase 09 because it lives most naturally here):

    - Drop a stub `tests/fixtures/echo-argv.sh` that prints its argv
      as JSON.
    - Set `CODEX_SESSION_CHILD_BIN=<absolute-path-to-stub>`.
    - Invoke `codex-session exec foo --bar baz -- --child-flag`.
    - Snapshot the stub's stdout with `insta`.

    This locks the argv-translation contract that
    `ChildInvocation::into_command` represents.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes (snapshots may need `cargo insta review`).
- [ ] `src/adapters/process.rs` is deleted.
- [ ] `src/adapters/spawner.rs` exists with `Spawner` trait + `StdSpawner`.
- [ ] `src/domain/child_invocation.rs` exists.
- [ ] `AppContext` has `spawner` + `resolved_child` (or
  `LazyChild`); no `process` field.
- [ ] `codex-session --dry-run <anything>` exits 0 without
  spawning the child.
- [ ] The `--dry-run` report deterministically lists the four
  scrubbed env keys (`CODEX_SESSION_CHILD_BIN`,
  `CODEX_SESSION_LOG_FILE`, `CODEX_SESSION_LOG_DIR`,
  `CODEX_SESSION_REENTRY`).
- [ ] `tests/cmd_passthrough_argv.rs` snapshots a known-good argv
  translation.

## Tests

- `tests/cmd_dry_run.rs` (above).
- `tests/cmd_passthrough_argv.rs` (above).
- `tests/fixtures/echo-argv.sh` (stub child; make executable).

## Out of scope

- Adding the recursion guard on the exec path. (Phase 05 — though
  the `SpawnerError::Recursion` variant is added here so Phase 05
  has somewhere to map.)
- Removing the recursion check inside `child_version_line`
  (leave the existing one in place; Phase 05 unifies them).
- Signal-forwarding tests. (Phase 09.)
- Switching the log sink. (Phase 06.)

## References

- `cli-design/06-cli-wrapper-design/typing-and-validation.md` — typed model, `to_args()`/`into_command()`, `Executable` trait.
- `cli-design/06-cli-wrapper-design/process-and-posix.md:154-182, 301-321` — resolve once, golden-argv tests.
- `cli-design/06-cli-wrapper-design/checklist.md:25-39, 55-69` — typed builder, `--dry-run`.
- `insta` snapshot docs: <https://insta.rs/>.
