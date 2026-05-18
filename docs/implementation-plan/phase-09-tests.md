# Phase 09 — Tests: `insta` snapshots, signal/`128+N` matrix, golden argv, config precedence

**One-line summary.** Fill the spec-required testing gaps:
`--help`/`--version` snapshots via `insta`, signal-forwarding matrix
asserting `128+N` propagation, golden-argv snapshot tests against a
stub child, and a config-precedence matrix.

## Prerequisites

Phases 01–08 must be complete. The CLI surface, config layer, typed
child invocation, recursion guard, env scrubbing, logging, output
discipline, and error model must all be in place. Test infrastructure
(`tests/support/`, `tests/fixtures/`) must exist.

## Goal

After this phase the test suite covers:

### A. Snapshot tests via `insta`

- `tests/cmd_root_help.rs` — snapshot of `codex-session --help` and
  `codex-session help` (must be identical).
- `tests/cmd_version_snapshot.rs` — snapshot of
  `codex-session version` (text and JSON), using a stub child via
  `CODEX_SESSION_CHILD_BIN`.
- `tests/cmd_config_status.rs` — snapshot of
  `codex-session config status` (text and JSON), in a hermetic
  tempdir.
- `tests/cmd_config_show_local.rs` — snapshot of show-local output.

All `insta` snapshots use deterministic env (NO_COLOR, HOME,
XDG_*) set via the support helpers.

### B. Signal-forwarding matrix

- `tests/cmd_signal_passthrough.rs`:
  - Spawn `codex-session foo` where the child is a sleep stub that
    blocks for 30 seconds.
  - Send SIGINT to the wrapper.
  - Assert wrapper exits with code `130` (`128 + 2`).
  - Repeat for SIGTERM → 143, SIGHUP → 129.
  - This exercises the `exec()` model: because the wrapper calls
    `exec()` to replace itself with the child, signals naturally
    deliver to the child. The wrapper's own PID becomes the
    child's PID after `exec`, so its exit code is the child's.
    No active signal-forwarding logic is needed when `exec` is
    used, which is the case in our spawner.

### C. Golden argv (already started in Phase 04)

- `tests/cmd_passthrough_argv.rs`: assert the stub child sees the
  exact argv we expect.
- `tests/cmd_passthrough_dashdash.rs`: assert
  `codex-session -- --help foo` invokes the child with
  `--help foo` (the `--` is consumed by the wrapper, not forwarded).

### D. Config-precedence matrix (already started in Phase 03)

- `tests/cmd_config_precedence.rs`: table-driven across the five
  layers (defaults / user / project / env / CLI), asserting the
  resolved value wins per the spec.
- Add one row per overridable knob: `log.verbose`,
  `log.mirror_stderr`, `child.bin`.

### E. Recursion and env scrub (already in Phase 05)

- `tests/cmd_recursion_guard.rs` (exists).
- `tests/cmd_env_scrub.rs` (exists).

### F. Color policy (already in Phase 07)

- `tests/cmd_color_policy.rs` (exists).

## Spec rationale

- Snapshot `--help` and `--version` —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:308-310`.
- Signal/exit-code matrix —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:313-315`.
- Golden argv via stub child —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:304-308`.
- Config-precedence tests —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:316`,
  `rust/cli-spec/06-testing.md:119-175`.
- `assert_cmd`, `insta`, `tempfile`, `predicates` — already in
  `Cargo.toml` dev-dependencies (verified in
  `rust/cli-spec/07-dependencies.md:29-34`).

## Current state (verify before planning)

After Phases 01–08, the test directory should contain at least:

```
tests/
├── support/
│   ├── mod.rs            # env-clear helpers
│   └── color.rs          # Phase 07 helper
├── fixtures/
│   ├── echo-argv.sh      # Phase 04 stub
│   └── echo-env.sh       # Phase 05 stub
├── snapshots/            # insta snapshot dir
├── cmd_root_help.rs      # Phase 01
├── cmd_version.rs        # Phase 02 rename
├── cmd_config_status.rs  # Phase 02 rename
├── cmd_config_merge.rs   # Phase 02 rename
├── cmd_config_show_local.rs # Phase 02 rename
├── cmd_dispatch.rs       # Phase 02 rename
├── cmd_symlink.rs        # Phase 02 rename
├── cmd_self_rejected.rs  # Phase 02
├── cmd_passthrough.rs    # pre-existing
├── cmd_passthrough_argv.rs # Phase 04
├── cmd_dry_run.rs        # Phase 04
├── cmd_recursion_guard.rs # Phase 05
├── cmd_env_scrub.rs      # Phase 05
├── cmd_logging.rs        # pre-existing
├── cmd_child_resolution.rs # pre-existing
├── cmd_color_policy.rs   # Phase 07
├── cmd_version_parity.rs # Phase 07
├── cmd_config_precedence.rs # Phase 03
├── cmd_config_error.rs   # Phase 08
```

This phase **adds**:

- `tests/cmd_signal_passthrough.rs`
- `tests/cmd_passthrough_dashdash.rs`
- `tests/cmd_version_snapshot.rs` (insta-driven; the existing
  `cmd_version.rs` may already snapshot, in which case consolidate)
- `tests/fixtures/sleep.sh`

This phase also **tightens** existing tests:

- Replace exact-string compares with `insta` snapshots wherever a
  test does `assert_eq!(stdout, expected_str)` on long output.
- Add the missing precedence matrix rows.
- Audit every test for env-leakage (must use the support helper).

## Target state

### `tests/support/mod.rs` (consolidate)

```rust
//! Shared test helpers.

use assert_cmd::Command;
use tempfile::TempDir;

/// Build a hermetic command with no host env leaking in.
pub(crate) fn hermetic(td: &TempDir) -> Command {
    let mut cmd = Command::cargo_bin("codex-session").unwrap();
    cmd.env_clear()
        .env("HOME", td.path())
        .env("XDG_CONFIG_HOME", td.path().join("config"))
        .env("XDG_STATE_HOME", td.path().join("state"))
        .env("XDG_CACHE_HOME", td.path().join("cache"))
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("NO_COLOR", "1");                  // stable snapshots
    cmd
}

/// Set CODEX_SESSION_CHILD_BIN to the given fixture script (absolute path).
pub(crate) fn with_stub_child(cmd: &mut Command, fixture: &str) {
    let path = std::env::current_dir().unwrap()
        .join("tests/fixtures").join(fixture);
    assert!(path.is_file(), "fixture missing: {}", path.display());
    cmd.env("CODEX_SESSION_CHILD_BIN", path);
}
```

### `tests/cmd_signal_passthrough.rs`

```rust
mod support;

use std::os::unix::process::ExitStatusExt as _;
use std::process::Command;
use std::time::Duration;
use tempfile::TempDir;

#[test]
fn sigint_propagates_as_130() {
    let td = TempDir::new().unwrap();
    let bin = assert_cmd::cargo::cargo_bin("codex-session");
    let fixture = std::env::current_dir().unwrap()
        .join("tests/fixtures/sleep.sh");
    assert!(fixture.is_file());

    let mut child = Command::new(&bin)
        .arg("just-pass-through")
        .env_clear()
        .env("HOME", td.path())
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("CODEX_SESSION_CHILD_BIN", fixture)
        .spawn().unwrap();

    std::thread::sleep(Duration::from_millis(300));   // let exec() land
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    let status = child.wait().unwrap();
    assert_eq!(status.code(), Some(130), "expected 128+SIGINT=130, got {status:?}");
}

// repeat for SIGTERM (143), SIGHUP (129)
```

Note: `assert_cmd` does not expose signal-send helpers; drop to
`std::process::Command` here. The `libc::kill` call needs `libc` as
a dev-dependency — add it to `Cargo.toml [dev-dependencies]`.

### `tests/fixtures/sleep.sh`

```bash
#!/usr/bin/env bash
# Stub child: sleep until killed.
sleep 30
```

Make executable.

### `tests/cmd_passthrough_dashdash.rs`

```rust
mod support;

use assert_cmd::prelude::*;
use insta::assert_snapshot;
use tempfile::TempDir;

#[test]
fn double_dash_is_consumed_by_wrapper() {
    let td = TempDir::new().unwrap();
    let mut cmd = support::hermetic(&td);
    support::with_stub_child(&mut cmd, "echo-argv.sh");
    let out = cmd.args(["--", "--help", "foo"]).output().unwrap();
    assert!(out.status.success());
    let body = String::from_utf8(out.stdout).unwrap();
    // The stub echoes its argv as JSON. Expect ["--help","foo"].
    assert_snapshot!("argv_after_dashdash", body);
}
```

### `tests/cmd_version_snapshot.rs`

```rust
mod support;

use insta::assert_snapshot;
use tempfile::TempDir;

#[test]
fn version_text_snapshot() {
    let td = TempDir::new().unwrap();
    let mut cmd = support::hermetic(&td);
    support::with_stub_child(&mut cmd, "echo-version.sh");
    let out = cmd.arg("version").output().unwrap();
    assert!(out.status.success());
    let body = String::from_utf8(out.stdout).unwrap();
    let body = redact_wrapper_version(&body);
    assert_snapshot!("version_text", body);
}

#[test]
fn version_json_snapshot() {
    // analogous, args = ["version", "--format", "json"]
    // redact the wrapper version field before snapshotting.
}

fn redact_wrapper_version(s: &str) -> String {
    // Replace "codex-session X.Y.Z" with "codex-session <VERSION>" so
    // bumping Cargo.toml does not break the snapshot.
    regex::Regex::new(r"codex-session \d+\.\d+\.\d+").unwrap()
        .replace_all(s, "codex-session <VERSION>").into_owned()
}
```

(Add `regex` to dev-deps if not already; or do a string-replace.)

Add `tests/fixtures/echo-version.sh`:

```bash
#!/usr/bin/env bash
echo "codex 1.2.3 (stub)"
```

### Snapshot strategy

Use the `insta` setting:

```rust
// tests/snapshots/.config.yaml or via env
INSTA_OUTPUT=summary
INSTA_FORCE_PASS=1   // not in CI
```

Commit `*.snap` files to the repo. `cargo insta review` is the
maintainer workflow.

## Tasks

1. **Add `libc` to `[dev-dependencies]` in `Cargo.toml`** (needed by
    the signal test).

2. **Create `tests/fixtures/sleep.sh`** and `tests/fixtures/echo-version.sh`.
    Make both executable.

3. **Write `tests/cmd_signal_passthrough.rs`** with SIGINT, SIGTERM,
    SIGHUP cases.

4. **Write `tests/cmd_passthrough_dashdash.rs`** with the
    `-- --help foo` case.

5. **Write `tests/cmd_version_snapshot.rs`** with text and JSON
    variants, redacting the wrapper version.

6. **Audit existing tests** for non-hermetic env. Convert any
    direct `Command::cargo_bin("codex-session")` calls into
    `support::hermetic(&td)` to ensure NO_COLOR=1 and clean XDG
    paths.

7. **Tighten `tests/cmd_root_help.rs`** (Phase 01) to use `insta`.
    The current Phase 01 test just asserts non-empty stdout; promote
    to a real snapshot now that the surface is stable.

8. **Expand `tests/cmd_config_precedence.rs`** (Phase 03) with at
    least these rows:

    | knob | defaults | user | project | env | cli | expected |
    |---|---|---|---|---|---|---|
    | `log.verbose` | 0 | — | — | — | — | 0 |
    | `log.verbose` | 0 | 1 | — | — | — | 1 |
    | `log.verbose` | 0 | 1 | 2 | — | — | 2 |
    | `log.verbose` | 0 | 1 | 2 | 3 | — | 3 |
    | `log.verbose` | 0 | 1 | 2 | 3 | 4 (`-vvvv`) | 4 |
    | `child.bin` | none | — | — | `CODEX_SESSION_CHILD_BIN=/tmp/x` | — | `/tmp/x` |

9. **Run `cargo insta review`** and commit all `.snap` files.

10. **Verify CI green.** `cargo test` must pass on a fresh clone
    after this phase. No `INSTA_FORCE_PASS` in CI.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes.
- [ ] `tests/cmd_signal_passthrough.rs` exists; SIGINT case asserts
  exit 130.
- [ ] `tests/cmd_passthrough_dashdash.rs` exists; snapshot matches.
- [ ] `tests/cmd_version_snapshot.rs` exists; both text and JSON
  snapshots match.
- [ ] All `*.snap` files committed.
- [ ] `rg -n 'Command::cargo_bin' tests/ | rg -v 'support::hermetic'`
  returns nothing (every test goes through the helper).
- [ ] `cargo nextest run` (if installed) passes too — test
  parallelism does not introduce flakes.

## Tests

This phase **is** the test phase. No additional production tests.

## Out of scope

- Property-based tests / fuzzing. Defer.
- Benchmark suite (`benches/`). Defer until there's a measurable
  perf concern.
- Test on Windows / macOS. Crate is Unix-only
  (`#[cfg(not(unix))] compile_error!`).
- Replacing `assert_cmd` with `xtask`-style integration tests.

## References

- `cli-design/06-cli-wrapper-design/process-and-posix.md:301-321` — full testability checklist.
- `rust/cli-spec/06-testing.md:79-95, 119-175` — `assert_cmd`, `insta`, config-precedence matrix.
- `insta` docs: <https://insta.rs/docs/>.
- `assert_cmd` docs: <https://docs.rs/assert_cmd/latest/assert_cmd/>.
- POSIX `128 + signal` exit convention: <https://www.gnu.org/s/bash/manual/html_node/Exit-Status.html>.
- `std::os::unix::process::ExitStatusExt`: <https://doc.rust-lang.org/std/os/unix/process/trait.ExitStatusExt.html>.
