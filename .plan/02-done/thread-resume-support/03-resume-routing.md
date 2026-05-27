# Round 03: Resume Routing

> Plan: thread-resume-support | Round: 03 of 04 | Complexity: L
> Generated: 2026-05-26T00:00:00Z | Repo: /workspaces/codex-session

## Context

codex-session wraps OpenAI's Codex CLI with multi-account support by setting
`CODEX_HOME` to a per-account session directory. It maintains a cross-account
JSONL thread index mapping thread IDs to the accounts that created them.

Upstream Codex supports thread resume via:
- `codex exec resume <SESSION_ID>` — resume a specific thread by UUID.
- `codex exec resume --last` — resume the most recent thread.
- `codex resume [--last|--all|<ID>]` — interactive resume variants.

codex-session must intercept these commands, resolve the correct account from
the thread index, and forward to Codex with that account's `CODEX_HOME`. If
the thread is not found in the index, fall back to the current account
(which will either succeed if the thread happens to be there, or Codex will
fail and the user gets an error).

## Previous Rounds

Round 01 created:
- `src/services/session/thread_index.rs` — `ThreadEntry` struct, `append()`,
  `lookup()`, `last_for_group()`, `last_any()` functions.
- Thread index at `<state_dir>/thread-index.jsonl`.

Round 02 wired:
- `--json` detection in child argv.
- Stdout capture (tee) for `--json` runs.
- Thread start event parsing from JSONL output.
- Automatic index writes after each `--json` exec run.
- `PreparedInvocation` now includes `group_id` and `cwd` fields.

## Scope of This Round

**IN scope:**
- Detect `exec resume` subcommand pattern in the pass-through argv.
- Parse resume arguments: `<SESSION_ID>`, `--last`, `--all-groups`.
- Look up the target account from the thread index.
- Override the account selection for this invocation (bypass normal
  account resolution for resume requests when the thread is found).
- Forward the `exec resume <ID>` argv to Codex with the correct
  `CODEX_HOME`.
- For `--last`: resolve the thread_id via `last_for_group()` (or
  `last_any()` with `--all-groups`), then route to that account.
- Graceful fallback when thread is not in the index.
- Also handle non-exec `resume` variants (`codex resume [--last|--all|<ID>]`)
  since these also pass through to Codex.
- Unit and integration tests.

**OUT of scope:**
- Documentation and dotfiles updates (Round 04).
- Index rotation/pruning (future follow-up).
- Interactive resume picker UI (Codex handles this natively).

## Current State

### Key Files

- `/workspaces/codex-session/src/commands/pass_through.rs` — the main exec
  path. `run()` handles routing:

  ```rust
  pub(crate) fn run(
      ctx: &crate::context::AppContext,
      argv: &[std::ffi::OsString],
  ) -> Result<i32, crate::error::AppError> {
      let first_arg = argv.first().and_then(|a| a.to_str()).unwrap_or("");
      match first_arg {
          "login" => { /* login handler */ }
          "logout" => { /* logout handler */ }
          _ => {}
      }
      let gated = crate::services::account::gate::ensure(ctx)?;
      // ... dry_run or run_with_retry
  }
  ```

  Resume routing should be added as a new branch in this match, after login/
  logout but before the default path.

- `/workspaces/codex-session/src/commands/dispatch.rs` — routes
  `Commands::External(argv)` to `pass_through::run()`. The external argv
  arrives as a `Vec<OsString>` with the first element being the codex
  subcommand (e.g., `"exec"`).

- `/workspaces/codex-session/src/services/account/retry.rs` —
  `run_with_retry()` resolves the account via `resolver::resolve()`. For
  resume routing, we need to override this resolution with the account from
  the thread index.

- `/workspaces/codex-session/src/services/account/resolver.rs` —
  `ResolvedAccount` struct and `resolve()` function. The resume path needs
  to bypass normal resolution and use a specific account.

- `/workspaces/codex-session/src/services/session/thread_index.rs` — (from
  Rounds 01-02) `lookup()`, `last_for_group()`, `last_any()`.

- `/workspaces/codex-session/src/services/session/group_id.rs` — `current()`
  resolves the group_id. Needed for `--last` (group-scoped).

### Existing Patterns

- **Argv pattern matching:** `pass_through::run()` already matches on the
  first argv element for `"login"` and `"logout"`. Resume detection extends
  this pattern.

- **Account override:** The retry loop in `retry.rs` has a `retry_same`
  mechanism that forces a specific `ResolvedAccount`. A similar pattern can
  be used for resume: resolve the account from the thread index and pass it
  directly, bypassing the normal resolution chain.

- **Error handling:** The project uses `tracing::warn!` for non-fatal
  failures and returns `AppError` for fatal ones. Thread-not-found in the
  index is non-fatal (fall back to normal resolution).

## Implementation Steps

### Step 1: Add resume argv detection

Create a new function in `src/commands/pass_through.rs` (or a new helper
module) that parses the resume intent from the child argv:

```rust
#[derive(Debug)]
enum ResumeIntent {
    ById(String),
    Last { all_groups: bool },
}

fn detect_resume(argv: &[std::ffi::OsString]) -> Option<ResumeIntent> {
    // Pattern 1: "exec" "resume" <SESSION_ID>
    // Pattern 2: "exec" "resume" "--last"
    // Pattern 3: "exec" "resume" "--last" "--all-groups"
    // Pattern 4: "resume" <SESSION_ID>
    // Pattern 5: "resume" "--last"
    // Pattern 6: "resume" "--all"  (codex's --all flag)
    // Return None if no resume pattern detected
}
```

Handle both `exec resume ...` and bare `resume ...` patterns, since both
are valid Codex commands that pass through `Commands::External`.

### Step 2: Add account resolution from thread index

Create a function that resolves the target account for a resume intent:

```rust
fn resolve_resume_account(
    ctx: &crate::context::AppContext,
    intent: &ResumeIntent,
) -> Option<(crate::services::account::resolver::ResolvedAccount, String)> {
    // Returns (resolved_account, thread_id) if found in index
    // Returns None if thread not found (fall back to normal resolution)
}
```

For `ResumeIntent::ById(id)`: call `thread_index::lookup()`.
For `ResumeIntent::Last { all_groups: false }`: resolve group_id via
`group_id::current()`, then call `thread_index::last_for_group()`.
For `ResumeIntent::Last { all_groups: true }`: call `thread_index::last_any()`.

When a `ThreadEntry` is found, construct a `ResolvedAccount` with the
entry's account and a new source variant (e.g., `AccountResolutionSource::ThreadIndex`).

### Step 3: Add `ThreadIndex` account resolution source

In `/workspaces/codex-session/src/services/account/resolver.rs`, add a new
variant to `AccountResolutionSource`:

```rust
#[derive(Debug, Clone, Copy)]
pub(crate) enum AccountResolutionSource {
    Flag,
    Env,
    Auto,
    Lru,
    ConfigPinned,
    Interactive,
    ThreadIndex,  // NEW
}
```

Update `source_label()`:

```rust
AccountResolutionSource::ThreadIndex => "thread-index",
```

### Step 4: Wire resume routing into `pass_through::run()`

Add resume detection after the login/logout match, before the default
`run_with_retry` path:

```rust
pub(crate) fn run(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
) -> Result<i32, crate::error::AppError> {
    let first_arg = argv.first().and_then(|a| a.to_str()).unwrap_or("");
    match first_arg {
        "login" => { /* ... */ }
        "logout" => { /* ... */ }
        _ => {}
    }

    // Resume routing: intercept exec resume / resume commands
    if let Some(intent) = detect_resume(argv) {
        return run_resume(ctx, argv, &intent);
    }

    let gated = crate::services::account::gate::ensure(ctx)?;
    // ... existing path
}
```

### Step 5: Implement `run_resume()`

The resume handler:

```rust
fn run_resume(
    ctx: &crate::context::AppContext,
    argv: &[std::ffi::OsString],
    intent: &ResumeIntent,
) -> Result<i32, crate::error::AppError> {
    // 1. Ensure auth gate passes (same as normal path)
    let _gated = crate::services::account::gate::ensure(ctx)?;

    // 2. Try to resolve account from thread index
    if let Some((resolved, thread_id)) = resolve_resume_account(ctx, intent) {
        tracing::info!(
            op = "resume.routed",
            thread_id = %thread_id,
            account = %resolved.id,
            "routing resume to thread-index account"
        );
        // 3. Run with the specific account (bypass normal resolution)
        let signal_session = SignalSession::install()?;
        let (exit_code, _, _) = run_once(ctx, argv, &resolved, &signal_session, false)?;
        return Ok(exit_code);
    }

    // 4. Fallback: thread not in index, proceed with normal resolution
    tracing::info!(
        op = "resume.fallback",
        "thread not found in index; using normal account resolution"
    );
    crate::services::account::retry::run_with_retry(ctx, argv)
}
```

For `ResumeIntent::Last` when the thread IS found in the index but the
original argv was `exec resume --last`, we need to rewrite the argv to
`exec resume <thread_id>` since the `--last` flag is group-scoped in
codex-session but Codex's `--last` would look at its own local session
history (which may differ per `CODEX_HOME`). The thread_id from the index
is the correct one to forward.

### Step 6: Handle `--last` argv rewriting

When resume intent is `Last` and we found the thread in the index, rewrite
the child argv to use the concrete thread_id instead of `--last`:

```rust
fn rewrite_last_to_id(argv: &[OsString], thread_id: &str) -> Vec<OsString> {
    // Replace "--last" with the concrete thread_id
    // "exec" "resume" "--last" -> "exec" "resume" "<thread_id>"
    // "resume" "--last" -> "resume" "<thread_id>"
    // Remove "--all-groups" (codex-session flag, not forwarded)
}
```

This is necessary because each account's `CODEX_HOME` has its own
`state_5.sqlite`, so Codex's `--last` inside Account B's home would pick
Account B's last thread, not the one from the cross-account index.

### Step 7: Add integration test

Create `/workspaces/codex-session/tests/thread_resume_routing.rs`:

1. **Route by ID:** Pre-populate the thread index with a known entry. Run
    `codex-session exec resume <thread_id>`. Verify the child was invoked
    with the correct account's `CODEX_HOME`.

2. **Route by --last:** Pre-populate the index. Run
    `codex-session exec resume --last`. Verify correct routing.

3. **Fallback on miss:** Run `codex-session exec resume <unknown_id>`.
    Verify normal account resolution is used.

4. **--all-groups:** Pre-populate with entries from different groups. Run
    `codex-session exec resume --last --all-groups`. Verify the global
    most-recent is picked.

5. **Bare resume:** Test `codex-session resume <thread_id>` (without `exec`
    prefix) routes correctly.

Follow the mock-child-binary pattern from existing integration tests.

## Acceptance Criteria

- [ ] `detect_resume()` correctly identifies all resume argv patterns.
- [ ] `resolve_resume_account()` looks up thread→account from the index.
- [ ] `AccountResolutionSource::ThreadIndex` variant added and labeled.
- [ ] `exec resume <SESSION_ID>` routes to the correct account when the
      thread_id is in the index.
- [ ] `exec resume --last` resolves the group-scoped last thread and routes
      to its account.
- [ ] `exec resume --last --all-groups` resolves the global last thread.
- [ ] `--last` is rewritten to concrete thread_id before forwarding to Codex.
- [ ] `--all-groups` is stripped from the forwarded argv (codex-session flag).
- [ ] Unknown thread IDs fall back to normal account resolution.
- [ ] Bare `resume` (without `exec`) is also handled.
- [ ] `just test` passes (unit + integration).
- [ ] `just lint` passes.

## Next Round

Round 04 updates documentation: `docs/upstream-codex.md` (new F14 section
for thread resume scoping), `docs/multi-account-architecture.md` (resume
support section), project README, and syncs relevant dotfiles.
