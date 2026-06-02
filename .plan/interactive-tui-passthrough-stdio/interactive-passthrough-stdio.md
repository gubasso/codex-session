# Round 01: Interactive passthrough inherits stdio (auto + resume)

> Plan: interactive-tui-passthrough-stdio | Round: 01 of 01 | Complexity: S
> Generated: 2026-06-02 | Repo: /workspaces/codex-session

## Context

Launching the interactive codex TUI through this wrapper fails:

```text
[codex-session] auto-selection enabled (2 candidate(s)).
Error: stdout is not a terminal
```

`"stdout is not a terminal"` is emitted by **upstream codex**, not the wrapper: the codex TUI
checks `isatty(stdout)` on startup and refuses to run when its stdout is a pipe.

Root cause: with no pinned account and ≥2 eligible accounts, `gate::ensure` returns `AutoDeferred`,
so `pass_through::run` routes to `retry::run_auto`, which always runs the child with `capture=true`
to scan its stdout/stderr for `401`/`429` failover patterns. `capture=true` makes the spawner
**pipe** the child's stdio instead of inheriting the terminal, breaking the TUI. The identical bug
exists in `run_resume` for interactive `resume <id>`, which also forces `capture=true`.

Reactive output-scan failover cannot help an interactive TUI anyway (codex owns the terminal; the
wrapper cannot transparently rotate accounts and replay the session, and piping to observe output
is what breaks the TUI). The correct defense is the **pre-flight** account selection
`resolver::resolve_for_exec` already performs. Pinned accounts already prove the inherited-stdio
path works: `Resolved` → `single_attempt` → `run_once(capture=false)`.

This round makes interactive TUI launches (auto and resume) inherit stdio, while keeping
`exec`/`--json`/piped invocations on the unchanged capture + failover path.

## Previous Rounds

This is the first round — no prior rounds.

## Scope of This Round

IN scope:

- Add a shared predicate `is_interactive_passthrough(argv)` (plus a pure, unit-testable
  `argv_is_interactive_shape(argv)`) in `src/commands/pass_through.rs`.
- Add `run_auto_interactive` to `src/services/account/retry.rs` (inherited-stdio auto launch with
  best-effort `set_current`).
- Branch the `AutoDeferred` arm in `pass_through::run` to use the interactive path when applicable.
- Gate `run_resume`'s by-id spawn (capture flag + live-rate-limit post-scan) on the same predicate.
- Unit tests for `argv_is_interactive_shape`.
- Update `docs/design/cli-style-guide.md` and `docs/upstream-codex.md`.

OUT of scope:

- Any change to argv forwarding, env scrubbing, or emitted `$CODEX_HOME` config.
- The dead `spawner::exec()` (execvp) path — not used.
- Pinned-account path (already inherits stdio; untouched).
- Mid-session account rotation for interactive sessions (intentionally dropped).

## Current State

### Key Files

- `/workspaces/codex-session/src/commands/pass_through.rs` — passthrough entry, capture decision,
  resume handling, `has_json_flag`.

  The `AutoDeferred` routing arm in `run`:

  ```rust
  match outcome {
      crate::services::account::gate::GateOutcome::Resolved(resolved) => {
          crate::services::account::retry::single_attempt(ctx, argv, &resolved)
      }
      crate::services::account::gate::GateOutcome::AutoDeferred => {
          crate::services::account::retry::run_auto(ctx, argv)
      }
  }
  ```

  Existing json detection (insert the new predicate next to it):

  ```rust
  fn has_json_flag(argv: &[std::ffi::OsString]) -> bool {
      argv.iter().any(|arg| arg.to_str() == Some("--json"))
  }
  ```

  The resume by-id spawn inside `run_resume` (note `true` = capture):

  ```rust
  let session = SignalSession::install()?;
  let (exit_code, stdout, stderr) = run_once(
      ctx,
      &effective_argv,
      &resolved,
      &session,
      true,
      gid_override,
  )?;
  if let Some(err) = resume_blocked_from_live_rate_limit(
      ctx, &registry, &resolved, &thread_id, &stdout, &stderr,
  )? {
      return Err(err.into());
  }
  Ok(exit_code)
  ```

  `run_once` signature and the `effective_capture` rule (do NOT change this — `--json` keeps forcing
  capture as it must, for thread-id extraction):

  ```rust
  pub(crate) fn run_once(
      ctx: &crate::context::AppContext,
      argv: &[std::ffi::OsString],
      resolved: &crate::services::account::resolver::ResolvedAccount,
      session: &SignalSession,
      capture: bool,
      group_id_override: Option<&str>,
  ) -> Result<(i32, Vec<u8>, Vec<u8>), crate::error::AppError> {
      let json_mode = has_json_flag(argv);
      let effective_capture = capture || json_mode;
      // ...
  }
  ```

- `/workspaces/codex-session/src/services/account/retry.rs` — auto failover loop and
  `single_attempt`. Imports already in scope at the top:

  ```rust
  use std::collections::{HashMap, HashSet};
  use std::ffi::OsString;
  use crate::context::AppContext;
  use crate::error::AppError;
  use super::{
      AccountError, AccountId, cooldown,
      error::{AccountOutcomeLine, OutcomeState},
      failover, quota,
      registry::{AccountEntry, Registry},
      resolver::{self, ResolvedAccount},
      selector, token_refresh,
  };
  ```

  `single_attempt` (the inherited-stdio template — `run_auto_interactive` mirrors it but pre-selects
  the account and records `set_current`):

  ```rust
  pub(crate) fn single_attempt(
      ctx: &AppContext,
      argv: &[OsString],
      resolved: &ResolvedAccount,
  ) -> Result<i32, AppError> {
      let signal_session = crate::commands::pass_through::SignalSession::install()?;
      let (exit_code, _stdout_buf, _stderr_buf) =
          crate::commands::pass_through::run_once(ctx, argv, resolved, &signal_session, false, None)?;
      Ok(exit_code)
  }
  ```

  `run_auto`'s success path shows the best-effort `set_current` bookkeeping to mirror:

  ```rust
  if let Err(err) = registry.set_current(&resolved.id) {
      tracing::warn!(
          op = "last_account.write_failed",
          account = %resolved.id,
          error = %err,
      );
  }
  ```

  `resolve_for_exec` is the pre-flight selector (same one dry-run uses):
  `resolver::resolve_for_exec(ctx, &HashSet::new())` returns `Result<ResolvedAccount, AppError>`.

- `/workspaces/codex-session/src/adapters/spawner.rs` — `spawn_and_wait` (inherited stdio, used when
  `capture=false`) vs `spawn_and_wait_output` (`Stdio::piped()` on stdout/stderr, used when
  `capture=true`). No change needed here; the capture flag already selects the right path.

- `/workspaces/codex-session/docs/design/cli-style-guide.md` — source of truth for stdout/stderr
  ownership (per CLAUDE.md). Must document the interactive-passthrough stdio rule.

- `/workspaces/codex-session/docs/upstream-codex.md` — verified upstream-codex facts. Add the
  TUI `isatty` requirement and bump `Last verified`.

### Existing Patterns

- Terminal detection uses `std::io::IsTerminal` (already used in `gate.rs` via
  `std::io::stdin().is_terminal()`, and in `ui/spinner.rs`, `ui/color.rs`). Match this — do not add
  `atty`/`is-terminal` crates.
- argv inspection compares `arg.to_str()` against literals (see `has_json_flag` and `detect_resume`).
- Best-effort bookkeeping writes log via `tracing::warn!` and never convert to a hard error after
  the child has run.

## Implementation Steps

### Step 1: Add the interactive-passthrough predicate

In `/workspaces/codex-session/src/commands/pass_through.rs`, next to `has_json_flag`, add a pure
shape check plus a terminal-aware wrapper:

```rust
/// Shape-only: argv looks like an interactive TUI launch (not exec, not --json).
/// Pure and deterministic so it can be unit-tested without a real terminal.
fn argv_is_interactive_shape(argv: &[std::ffi::OsString]) -> bool {
    if has_json_flag(argv) {
        return false;
    }
    let first = argv.first().and_then(|a| a.to_str()).unwrap_or("");
    // `codex exec ...` and `codex exec resume <id>` are non-interactive streaming runs.
    first != "exec"
}

/// True when this launch is an interactive TUI attached to a real terminal, so the
/// child must inherit stdio (codex checks isatty on stdout/stdin and refuses a pipe).
fn is_interactive_passthrough(argv: &[std::ffi::OsString]) -> bool {
    use std::io::IsTerminal;
    argv_is_interactive_shape(argv)
        && std::io::stdout().is_terminal()
        && std::io::stdin().is_terminal()
}
```

### Step 2: Add `run_auto_interactive` to retry.rs

In `/workspaces/codex-session/src/services/account/retry.rs`, add a minimal sibling of
`single_attempt` that pre-selects the best eligible account and runs it with inherited stdio,
preserving the auto path's `set_current` bookkeeping:

```rust
/// Auto-selection for an interactive TUI launch: pick the best eligible account
/// up front (pre-flight, same selector dry-run uses) and run it with inherited
/// stdio. No output capture and no reactive 401/429 failover — codex owns the
/// terminal, so mid-session rotation is impossible and capture would break the
/// TUI's isatty check. Pre-flight selection (skips cooled-down/exhausted accounts,
/// scores by quota) is the defense.
pub(crate) fn run_auto_interactive(
    ctx: &AppContext,
    argv: &[OsString],
) -> Result<i32, AppError> {
    let resolved = resolver::resolve_for_exec(ctx, &HashSet::new())?;
    tracing::info!(
        op = "retry.interactive",
        account = %resolved.id,
        "interactive passthrough: failover disabled, stdio inherited"
    );
    let signal_session = crate::commands::pass_through::SignalSession::install()?;
    let (exit_code, _stdout_buf, _stderr_buf) =
        crate::commands::pass_through::run_once(ctx, argv, &resolved, &signal_session, false, None)?;
    // Best-effort recency bookkeeping, mirroring run_auto's success path. A write
    // failure must not turn a completed launch into an error.
    let registry = Registry::from_config(&ctx.config);
    if let Err(err) = registry.set_current(&resolved.id) {
        tracing::warn!(
            op = "last_account.write_failed",
            account = %resolved.id,
            error = %err,
        );
    }
    Ok(exit_code)
}
```

### Step 3: Branch the AutoDeferred arm

In `/workspaces/codex-session/src/commands/pass_through.rs`, in `run`, change the `AutoDeferred`
arm to choose the interactive path when applicable:

```rust
crate::services::account::gate::GateOutcome::AutoDeferred => {
    if is_interactive_passthrough(argv) {
        crate::services::account::retry::run_auto_interactive(ctx, argv)
    } else {
        crate::services::account::retry::run_auto(ctx, argv)
    }
}
```

Leave the `Resolved(resolved)` arm unchanged (pinned accounts already inherit stdio via
`single_attempt`).

### Step 4: Gate run_resume on the same predicate

In `/workspaces/codex-session/src/commands/pass_through.rs`, in `run_resume`'s by-id spawn, compute
the predicate once, pass `!interactive` as the capture flag, and skip the reactive live-rate-limit
scan when inheriting stdio. Keep the earlier `resume_preflight_block` call unchanged.

```rust
let session = SignalSession::install()?;
let interactive = is_interactive_passthrough(argv);
let (exit_code, stdout, stderr) = run_once(
    ctx,
    &effective_argv,
    &resolved,
    &session,
    !interactive,
    gid_override,
)?;
if !interactive
    && let Some(err) = resume_blocked_from_live_rate_limit(
        ctx, &registry, &resolved, &thread_id, &stdout, &stderr,
    )?
{
    return Err(err.into());
}
Ok(exit_code)
```

Note: pass the original `argv` (the user-facing invocation) to `is_interactive_passthrough`, not
`effective_argv` (post strip/rewrite), so `exec resume <id>` correctly classifies as
non-interactive via its `exec` first arg. Confirm `argv` is in scope at this point in `run_resume`;
if only `effective_argv` is available, verify its first element still reflects `exec` vs `resume`
before choosing which to pass — the classification must treat `exec resume <id>` as non-interactive
and bare/`resume <id>` as interactive.

### Step 5: Unit tests for the shape check

Add unit tests (in `src/commands/pass_through.rs` under `#[cfg(test)]`, matching the file's existing
test conventions) for `argv_is_interactive_shape`. Build `OsString` argv slices and assert:

```rust
// helper: fn av(items: &[&str]) -> Vec<std::ffi::OsString>
assert!(argv_is_interactive_shape(&av(&[])));                       // bare TUI
assert!(argv_is_interactive_shape(&av(&["resume"])));               // interactive picker
assert!(argv_is_interactive_shape(&av(&["resume", "ID"])));         // interactive resume
assert!(argv_is_interactive_shape(&av(&["some prompt"])));          // prompt-only TUI
assert!(!argv_is_interactive_shape(&av(&["exec", "do"])));          // exec
assert!(!argv_is_interactive_shape(&av(&["exec", "resume", "ID"])));// exec resume
assert!(!argv_is_interactive_shape(&av(&["--json"])));              // json forces capture
```

The `is_terminal()` half is environment-dependent and is covered by the manual E2E check below, not
by unit tests.

### Step 6: Update docs

- `/workspaces/codex-session/docs/design/cli-style-guide.md` — in the stdout/stderr-ownership
  section, state that an interactive TUI passthrough (not `--json`, not `exec`, stdout+stdin are
  terminals) inherits the child's stdio and does not capture; reactive `401`/`429` failover applies
  only to non-interactive (`exec`/`--json`/piped) invocations, where pre-flight account selection is
  the defense for interactive launches.
- `/workspaces/codex-session/docs/upstream-codex.md` — add a short verified note: the codex TUI
  requires `isatty(stdout)` (and stdin); the wrapper must inherit, not pipe, for interactive
  launches. Bump the `Last verified` date to today.

### Final Step: Update the queue

Record completion in the queue files — status lives in YAML; nothing moves on
disk:

1. In this plan's `_QUEUE.yaml`, set this round's `status` to `done`.
2. All rounds are now done, so in the top-level `.plan/_QUEUE.yaml` set this
   plan's `status` to `done`. Leave the plan directory in place.

## Acceptance Criteria

- [ ] `argv_is_interactive_shape` and `is_interactive_passthrough` exist in `pass_through.rs`; the
      shape check is pure (no terminal access) and the wrapper uses `std::io::IsTerminal`.
- [ ] `AutoDeferred` arm routes interactive launches to `run_auto_interactive` and all other
      launches to `run_auto`.
- [ ] `run_auto_interactive` exists in `retry.rs`, pre-selects via `resolve_for_exec`, runs
      `run_once(..., false, ...)`, and records `set_current` best-effort.
- [ ] `run_resume`'s by-id spawn passes `!interactive` as the capture flag and skips
      `resume_blocked_from_live_rate_limit` when interactive; `resume_preflight_block` still runs.
      `exec resume <id>` remains non-interactive (capture + live scan intact).
- [ ] Unit tests for `argv_is_interactive_shape` cover empty / `resume` / `resume ID` / prompt /
      `exec` / `exec resume ID` / `--json` and pass under `just test-unit`.
- [ ] `docs/design/cli-style-guide.md` documents the interactive-passthrough stdio rule;
      `docs/upstream-codex.md` records the codex-TUI `isatty` requirement with a bumped date.
- [ ] `just test-integration` passes (existing passthrough/failover/resume suites run under a
      non-tty harness, exercising the unchanged capture path — no regression).
- [ ] `just lint` and `just check` pass.
- [ ] Manual E2E in a real terminal: with two eligible accounts and no pin, `codex-session` launches
      the TUI with no "stdout is not a terminal"; `codex-session resume <id>` launches; piped
      `codex-session ... | cat` and `codex-session exec --json "…"` still capture (failover intact).
- [ ] This plan's `_QUEUE.yaml` shows the round as `done`.
- [ ] The top-level `.plan/_QUEUE.yaml` shows this plan as `done`.

## Next Round

This is the final round.
